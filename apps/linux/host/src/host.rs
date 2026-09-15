//! Linux 壳的业务侧:引擎装配 + 输入会话状态。照搬 macOS host 的结构
//! (init.rs 的装配、session.rs 的分页会话),砍掉本刀还不做的部分(云/模型/统计)。

use std::path::{Path, PathBuf};

use qingjian_core::{CandidateLayout, Engine, Language};
use qingjian_dictionary::{Dictionary, WordList};
use qingjian_learning::FrequencyLearner;
use qingjian_platform::Config;
use qingjian_translate::Glossary;

/// 云端词占位数:Linux 首版无云联想,不留位。
const CLOUD_SLOTS: usize = 0;
/// 学习数据落盘的最小间隔(上屏路径上顺带检查,焦点切换仍即时落)。
const FLUSH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

pub struct Host {
    pub engine: Engine,
    /// 当前查询的候选排布与 UI 状态。
    pub layout: CandidateLayout,
    pub highlighted: usize,
    pub page: usize,
    /// 候选窗顶部的拼音行(带音节分隔)与光标(字节偏移)。
    pub preedit: String,
    pub preedit_cursor: usize,
    /// 待上屏文本:按键处理里攒,shim 每个事件后取走。
    pub pending_commit: Option<String>,
    /// Shift 轻点检测:按下 Shift 后没夹别的键,松开才算「轻点」,切中英。
    shift_armed: bool,
    /// 每页候选数(配置 1–9)。
    page_size: usize,
    last_flush: std::time::Instant,
    /// 配置热加载:watch 的文件、上次见到的修改时间、上次探测时刻。
    config_file: Option<PathBuf>,
    config_mtime: Option<std::time::SystemTime>,
    last_config_check: std::time::Instant,
    /// 当前生效的学习语言(换语言要重载释义表,记着才能比对)。
    learning_language: Option<Language>,
    data_dir: PathBuf,
}

/// 配置探测间隔:按键路径上顺带查 mtime,改完配置敲下一个键就生效。
const CONFIG_CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

/// 配置文件:$XDG_CONFIG_HOME/qingjian/config.toml,缺省 ~/.config/qingjian/config.toml。
pub fn config_path() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME")
        && !dir.is_empty()
    {
        return Some(PathBuf::from(dir).join("qingjian/config.toml"));
    }
    std::env::var("HOME")
        .ok()
        .map(|home| PathBuf::from(home).join(".config/qingjian/config.toml"))
}

/// 数据目录:$XDG_DATA_HOME/qingjian,缺省 ~/.local/share/qingjian。
pub fn data_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("XDG_DATA_HOME")
        && !dir.is_empty()
    {
        return Some(PathBuf::from(dir).join("qingjian"));
    }
    std::env::var("HOME")
        .ok()
        .map(|home| PathBuf::from(home).join(".local/share/qingjian"))
}

fn mtime_of(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// 按配置的学习语言挑释义表并装载;没有对应文件依次退回英语、任一存在的。
fn load_glossary(dir: &Path, configured: &str) -> Option<(Language, Glossary)> {
    let configured = configured.parse::<Language>().ok();
    let language = [configured, Some(Language::English)]
        .into_iter()
        .flatten()
        .find(|l| find_data(dir, &format!("glossary-{}", l.code())).is_some())?;
    let path = find_data(dir, &format!("glossary-{}", language.code()))?;
    match Glossary::from_path(language, &path) {
        Ok(glossary) => {
            tracing::info!(language = language.code(), glosses = glossary.len(), "释义表已加载");
            Some((language, glossary))
        }
        Err(error) => {
            tracing::warn!(%error, "释义表加载失败,候选无译文");
            None
        }
    }
}

fn find_data(dir: &Path, stem: &str) -> Option<PathBuf> {
    // 同名 .qj 优先于 .tsv,与 extra_dictionaries 的约定一致。
    for ext in ["qj", "tsv"] {
        let path = dir.join(format!("{stem}.{ext}"));
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

impl Host {
    /// 装配引擎:词库必备,释义表与学习数据可选缺。
    /// `config` 传 None = 从标准路径读(测试传 Some 以隔离环境)。
    pub fn init(dir: PathBuf, config: Option<Config>) -> Result<Self, String> {
        let config = config.unwrap_or_else(|| match config_path() {
            Some(path) => Config::load(&path).unwrap_or_else(|error| {
                // 配置笔误不能让输入法起不来:报日志、按默认跑,用户修好重启即生效。
                tracing::error!(%error, "配置解析失败,本次按默认配置");
                Config::default()
            }),
            None => Config::default(),
        });
        let dict_path =
            find_data(&dir, "dict").ok_or_else(|| format!("{} 下没有 dict.qj/dict.tsv", dir.display()))?;
        let dictionary = Dictionary::from_path(&dict_path).map_err(|e| format!("词库加载失败:{e}"))?;
        let mut engine = Engine::new(dictionary);
        let learning_language = match load_glossary(&dir, &config.general.learning_language) {
            Some((language, glossary)) => {
                engine = engine.with_translator(Box::new(glossary));
                Some(language)
            }
            None => {
                tracing::info!("无释义表,候选无译文");
                None
            }
        };
        engine.set_fuzzy(config.fuzzy.clone());
        let learner = match FrequencyLearner::from_path(&dir.join("user.tsv")) {
            Ok(learner) => learner,
            Err(_) => FrequencyLearner::default(),
        };
        engine = engine.with_learner(Box::new(learner));
        // 英文模式的词表与英→中释义,都可选缺:缺了英文模式只是没候选。
        if let Some(path) = find_data(&dir, "english") {
            match WordList::from_path(&path) {
                Ok(words) => engine = engine.with_english(words),
                Err(error) => tracing::warn!(%error, "英文词表加载失败"),
            }
        }
        if let Some(path) = find_data(&dir, "glossary-zh") {
            match Glossary::from_path(Language::Chinese, &path) {
                Ok(glossary) => engine = engine.with_english_translator(Box::new(glossary)),
                Err(error) => tracing::warn!(%error, "英→中释义表加载失败"),
            }
        }
        let page_size = config.general.page_size.clamp(1, 9);
        Ok(Host {
            engine,
            layout: CandidateLayout::new(Vec::new(), page_size, CLOUD_SLOTS),
            highlighted: 0,
            page: 0,
            preedit: String::new(),
            preedit_cursor: 0,
            pending_commit: None,
            shift_armed: false,
            page_size,
            last_flush: std::time::Instant::now(),
            config_file: None,
            config_mtime: None,
            last_config_check: std::time::Instant::now(),
            learning_language,
            data_dir: dir,
        })
    }

    /// 开启配置热加载:记下文件与当前 mtime,之后按键路径上周期探测。
    pub fn watch_config(&mut self, path: PathBuf) {
        self.config_mtime = mtime_of(&path);
        self.config_file = Some(path);
    }

    /// 配置文件变了就重载并热应用(模糊音/每页候选数/学习语言)。
    fn maybe_reload_config(&mut self) {
        if self.last_config_check.elapsed() < CONFIG_CHECK_INTERVAL {
            return;
        }
        self.last_config_check = std::time::Instant::now();
        let Some(path) = self.config_file.clone() else { return };
        let mtime = mtime_of(&path);
        if mtime == self.config_mtime {
            return;
        }
        self.config_mtime = mtime;
        let config = match Config::load(&path) {
            Ok(config) => config,
            Err(error) => {
                // 笔误不打断输入:保留旧配置继续跑,等用户改对再生效。
                tracing::error!(%error, "配置重载失败,维持旧配置");
                return;
            }
        };
        self.engine.set_fuzzy(config.fuzzy.clone());
        self.page_size = config.general.page_size.clamp(1, 9);
        let wanted = load_glossary(&self.data_dir, &config.general.learning_language);
        if wanted.as_ref().map(|(l, _)| *l) != self.learning_language
            && let Some((language, glossary)) = wanted
        {
            self.engine.set_translator(Box::new(glossary));
            self.learning_language = Some(language);
        }
        tracing::info!("配置已热加载");
        if self.composing() {
            self.refresh();
        }
    }

    pub fn composing(&self) -> bool {
        !self.engine.composition().is_empty()
    }

    /// 重新查询并刷新候选排布(每次缓冲区变化后调)。
    pub fn refresh(&mut self) {
        if !self.composing() {
            self.clear_view();
            return;
        }
        match self.engine.query() {
            Ok(mut query) => {
                self.engine.annotate(&mut query.candidates);
                self.preedit = query.marked_text();
                self.preedit_cursor = query.marked_cursor();
                self.layout =
                    CandidateLayout::new(query.candidates.items, self.page_size, CLOUD_SLOTS);
                self.highlighted = (0..self.layout.len())
                    .find(|&i| self.layout.candidate(i).is_some())
                    .unwrap_or(0);
                self.page = self.highlighted / self.layout.page_size();
            }
            Err(error) => {
                // 解析不动(如纯辅音):preedit 原样显示缓冲区,不出候选。
                tracing::debug!(%error, "查询失败");
                self.preedit = self.engine.composition().text().to_owned();
                self.preedit_cursor = self.preedit.len();
                self.layout = CandidateLayout::new(Vec::new(), self.page_size, CLOUD_SLOTS);
                self.highlighted = 0;
                self.page = 0;
            }
        }
    }

    fn clear_view(&mut self) {
        self.layout = CandidateLayout::new(Vec::new(), self.page_size, CLOUD_SLOTS);
        self.highlighted = 0;
        self.page = 0;
        self.preedit.clear();
        self.preedit_cursor = 0;
    }

    /// 上屏第 `index` 格(排布内绝对下标)。
    fn commit_index(&mut self, index: usize) {
        let Some(candidate) = self.layout.candidate(index).cloned() else {
            return self.commit_raw();
        };
        let text = self.engine.commit(&candidate);
        self.push_commit(text);
        // 连续组句:上屏后缓冲区还有剩余拼音就接着查,否则收窗。
        self.refresh();
    }

    fn commit_raw(&mut self) {
        let raw = self.engine.take_raw();
        self.push_commit(raw);
        self.refresh();
    }

    fn push_commit(&mut self, text: String) {
        if text.is_empty() {
            return;
        }
        match &mut self.pending_commit {
            Some(pending) => pending.push_str(&text),
            None => self.pending_commit = Some(text),
        }
        // 学习数据周期落盘:焦点切换即时落之外的兜底,防长会话崩溃丢学习。
        if self.last_flush.elapsed() >= FLUSH_INTERVAL {
            self.engine.flush_learning();
            self.last_flush = std::time::Instant::now();
        }
    }

    fn page_count(&self) -> usize {
        self.layout.len().div_ceil(self.layout.page_size()).max(1)
    }

    fn turn_page(&mut self, delta: isize) {
        let pages = self.page_count() as isize;
        let next = (self.page as isize + delta).clamp(0, pages - 1);
        if next != self.page as isize {
            self.page = next as usize;
            self.highlighted = self.page * self.layout.page_size();
            self.engine.note_page_turn();
        }
    }

    fn move_highlight(&mut self, delta: isize) {
        let len = self.layout.len() as isize;
        if len == 0 {
            return;
        }
        let next = (self.highlighted as isize + delta).rem_euclid(len) as usize;
        self.highlighted = next;
        self.page = next / self.layout.page_size();
    }

    /// 落盘学习数据(焦点离开时调,与 macOS 的 flush 时机对齐)。
    pub fn flush(&mut self) {
        let _ = &self.data_dir; // user.tsv 路径在 learner 里;flush 由 Engine 统一发。
        self.engine.flush_learning();
    }

    /// 按键处理。返回 true = 吞掉。keyval 是 X keysym。
    pub fn key(&mut self, keyval: u32, state: u32, release: bool) -> bool {
        self.maybe_reload_config();
        const CTRL_ALT_SUPER: u32 = (1 << 2) | (1 << 3) | (1 << 6);
        const SHIFT_KEYS: std::ops::RangeInclusive<u32> = 0xffe1..=0xffe2;
        // Shift 轻点切中英:按下预备,中间没夹别的键、松开时兑现(macOS 同款手感)。
        if SHIFT_KEYS.contains(&keyval) {
            if release {
                if self.shift_armed {
                    self.shift_armed = false;
                    let on = !self.engine.english_mode();
                    self.engine.set_english_mode(on);
                    self.refresh();
                }
            } else {
                self.shift_armed = true;
            }
            return false; // 修饰键本身永远透传,应用要看 Shift 状态
        }
        if !release {
            self.shift_armed = false;
        }
        if state & CTRL_ALT_SUPER != 0 {
            return false;
        }
        if release {
            // 组句期间吞掉普通键的松键,免得应用收到无头的 release;修饰键已在上面放行。
            return self.composing() && !(0xffe1..=0xffee).contains(&keyval);
        }
        let composing = self.composing();
        match keyval {
            // a-z:进缓冲区。
            0x61..=0x7a => {
                self.engine.push(keyval as u8 as char);
                self.refresh();
                true
            }
            // 数字 1-9:组句中按页内序号上屏。
            0x31..=0x39 if composing => {
                let offset = (keyval - 0x31) as usize;
                let index = self.page * self.layout.page_size() + offset;
                if self.layout.candidate(index).is_some() {
                    self.commit_index(index);
                }
                true
            }
            // 空格:上屏高亮候选。
            0x20 if composing => {
                self.commit_index(self.highlighted);
                true
            }
            // 回车:拼音原文上屏。
            0xff0d | 0xff8d if composing => {
                self.commit_raw();
                true
            }
            // 退格。
            0xff08 if composing => {
                self.engine.backspace();
                self.refresh();
                true
            }
            // Esc:放弃本次输入。
            0xff1b if composing => {
                self.engine.clear();
                self.refresh();
                true
            }
            // 翻页:- / = 与 PageUp / PageDown、方向键上下。
            0x2d | 0xff55 | 0xff52 if composing => {
                self.turn_page(-1);
                true
            }
            0x3d | 0xff56 | 0xff54 if composing => {
                self.turn_page(1);
                true
            }
            // 方向键左右:移动高亮。
            0xff51 if composing => {
                self.move_highlight(-1);
                true
            }
            0xff53 if composing => {
                self.move_highlight(1);
                true
            }
            // Shift 按住打的大写字母:临时打英文——拼音原样上屏,字母本身透传给应用。
            0x41..=0x5a => {
                if composing {
                    self.commit_raw();
                }
                self.engine.note_passthrough(keyval as u8 as char);
                false
            }
            // 其余可打印键 = 标点/符号:组句中先上屏高亮候选,然后转全角;
            // 转不了的(半角规则如数字后的点)原样透传。空闲时同一条路。
            0x21..=0x7e => {
                if composing {
                    self.commit_index(self.highlighted);
                }
                let c = keyval as u8 as char;
                match self.engine.punctuate(c) {
                    Some(full_width) => {
                        self.push_commit(full_width.to_owned());
                        true
                    }
                    None => {
                        // 透传:字符本身不吞。若组句中已上屏候选,顺序由 shim 保证——
                        // 它对未吞掉的键也先取走 pending_commit 发出去,再放行按键。
                        self.engine.note_passthrough(c);
                        false
                    }
                }
            }
            _ => false,
        }
    }

    /// 私密输入(密码框):学习与输入日志静音,排序不变。
    pub fn set_private(&mut self, private: bool) {
        self.engine.set_private(private);
    }

    /// 上屏当前页第 `offset` 格(鼠标点选走这里)。
    pub fn select_on_page(&mut self, offset: usize) {
        let index = self.page * self.layout.page_size() + offset;
        if self.layout.candidate(index).is_some() {
            self.commit_index(index);
        }
    }

    /// 会话重置(切窗/切输入法):缓冲区未上屏内容直接丢弃,学习落盘。
    pub fn reset(&mut self) {
        self.engine.clear();
        self.clear_view();
        self.pending_commit = None;
        self.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_host() -> Host {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../assets/sample");
        Host::init(dir, Some(qingjian_platform::Config::default())).expect("样例数据应能装配")
    }

    fn type_str(h: &mut Host, s: &str) {
        for c in s.chars() {
            assert!(h.key(c as u32, 0, false), "字母键应被吞掉:{c}");
        }
    }

    #[test]
    fn nihao_out_candidates_and_space_commits() {
        let mut h = sample_host();
        type_str(&mut h, "nihao");
        assert!(h.composing());
        assert!(h.layout.len() > 0, "应有候选");
        assert!(!h.preedit.is_empty(), "preedit 应显示拼音");
        assert!(h.key(0x20, 0, false), "空格应被吞掉");
        let committed = h.pending_commit.take().expect("应有上屏文本");
        assert!(!committed.is_empty());
        assert!(!h.composing(), "nihao 应整体上屏、缓冲清空");
    }

    #[test]
    fn digit_selects_on_page() {
        let mut h = sample_host();
        type_str(&mut h, "ni");
        let second = h.layout.candidate(1).map(|c| c.text.clone());
        assert!(h.key(0x32, 0, false), "数字 2 应被吞掉");
        if let Some(expected) = second {
            assert_eq!(h.pending_commit.take().unwrap(), expected);
        }
    }

    #[test]
    fn backspace_then_escape_clears() {
        let mut h = sample_host();
        type_str(&mut h, "ni");
        assert!(h.key(0xff08, 0, false));
        assert!(h.composing(), "退一格还剩 n");
        assert!(h.key(0xff1b, 0, false));
        assert!(!h.composing(), "Esc 应清空");
        assert!(h.pending_commit.is_none());
    }

    #[test]
    fn passthrough_when_idle() {
        let mut h = sample_host();
        assert!(!h.key(0x20, 0, false), "空闲时空格透传");
        assert!(!h.key(0x31, 0, false), "空闲时数字透传");
        assert!(!h.key(0xff08, 0, false), "空闲时退格透传");
        assert!(!h.key(0x61, 1 << 2, false), "Ctrl+a 透传");
    }

    #[test]
    fn candidates_carry_translation() {
        let mut h = sample_host();
        type_str(&mut h, "ni");
        let translated = (0..h.layout.len())
            .filter_map(|i| h.layout.candidate(i))
            .any(|c| c.translation.is_some());
        assert!(translated, "样例释义表下应至少有一个候选带译文");
    }

    #[test]
    fn enter_commits_raw_pinyin() {
        let mut h = sample_host();
        type_str(&mut h, "nihao");
        assert!(h.key(0xff0d, 0, false));
        assert_eq!(h.pending_commit.take().unwrap(), "nihao");
        assert!(!h.composing());
    }


    #[test]
    fn config_hot_reload_applies_fuzzy() {
        let dir = std::env::temp_dir().join(format!("qj-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = dir.join("config.toml");
        std::fs::write(&cfg, "[general]\n").unwrap();
        let mut h = sample_host();
        h.watch_config(cfg.clone());
        // 未开模糊:si 出不了「是」(sh 声母)
        type_str(&mut h, "si");
        let has_shi = |h: &Host| (0..h.layout.len())
            .filter_map(|i| h.layout.candidate(i))
            .any(|c| c.text == "是");
        assert!(!has_shi(&h), "模糊未开时 si 不应出「是」");
        h.key(0xff1b, 0, false); // Esc 清空
        // 改配置开 s_sh,回拨探测节流与 mtime 后重新输入
        std::fs::write(&cfg, "[fuzzy]\ns_sh = true\n").unwrap();
        h.config_mtime = None; // 模拟 mtime 变化(文件系统秒级精度不可靠)
        h.last_config_check = std::time::Instant::now() - CONFIG_CHECK_INTERVAL;
        type_str(&mut h, "si");
        assert!(has_shi(&h), "热加载 s_sh 后 si 应出「是」");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn punctuation_idle_full_width() {
        let mut h = sample_host();
        assert!(h.key(0x2c, 0, false), "逗号应转全角并吞掉");
        assert_eq!(h.pending_commit.take().unwrap(), "\u{ff0c}");
    }

    #[test]
    fn punctuation_while_composing_commits_then_converts() {
        let mut h = sample_host();
        type_str(&mut h, "ni");
        let top = h.layout.candidate(h.highlighted).unwrap().text.clone();
        assert!(h.key(0x2c, 0, false));
        assert_eq!(h.pending_commit.take().unwrap(), format!("{top}\u{ff0c}"));
        assert!(!h.composing());
    }

    #[test]
    fn shift_tap_toggles_english_mode() {
        let mut h = sample_host();
        assert!(!h.engine.english_mode());
        assert!(!h.key(0xffe1, 0, false), "Shift 按下透传");
        assert!(!h.key(0xffe1, 1, true), "Shift 松开透传");
        assert!(h.engine.english_mode(), "轻点应切到英文模式");
        // 夹了别的键就不算轻点
        assert!(!h.key(0xffe1, 0, false));
        h.key(0x61, 1, false);
        assert!(!h.key(0xffe1, 1, true));
        assert!(h.engine.english_mode(), "夹键后松开不应再切换");
    }

    #[test]
    fn shifted_letter_flushes_raw_and_passes() {
        let mut h = sample_host();
        type_str(&mut h, "ni");
        assert!(!h.key(0x4e, 1, false), "大写 N 应透传");
        assert_eq!(h.pending_commit.take().unwrap(), "ni", "拼音应原样上屏");
        assert!(!h.composing());
    }

    #[test]
    fn private_mode_smoke() {
        let mut h = sample_host();
        h.set_private(true);
        type_str(&mut h, "nihao");
        assert!(h.key(0x20, 0, false));
        assert!(h.pending_commit.take().is_some(), "私密模式照常上屏,只是不学习");
        h.set_private(false);
    }
}
