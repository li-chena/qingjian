//! Linux 壳的业务侧:引擎装配 + 输入会话状态。照搬 macOS host 的结构
//! (init.rs 的装配、session.rs 的分页会话),砍掉本刀还不做的部分(云/模型/统计)。

use std::path::{Path, PathBuf};

use qingjian_core::{CandidateLayout, Engine, Language};
use qingjian_dictionary::Dictionary;
use qingjian_learning::FrequencyLearner;
use qingjian_translate::Glossary;

/// 每页候选数与云端词占位数:先取 macOS 的缺省(9 格、不留云端位)。
const PAGE_SIZE: usize = 9;
const CLOUD_SLOTS: usize = 0;

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
    data_dir: PathBuf,
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
    pub fn init(dir: PathBuf) -> Result<Self, String> {
        let dict_path =
            find_data(&dir, "dict").ok_or_else(|| format!("{} 下没有 dict.qj/dict.tsv", dir.display()))?;
        let dictionary = Dictionary::from_path(&dict_path).map_err(|e| format!("词库加载失败:{e}"))?;
        let mut engine = Engine::new(dictionary);
        // 释义表:先只装英语;多语言跟随配置文件在后续刀接入。
        match find_data(&dir, "glossary-en") {
            Some(path) => match Glossary::from_path(Language::English, &path) {
                Ok(glossary) => engine = engine.with_translator(Box::new(glossary)),
                Err(error) => tracing::warn!(%error, "释义表加载失败,候选无译文"),
            },
            None => tracing::info!("无释义表,候选无译文"),
        }
        let learner = match FrequencyLearner::from_path(&dir.join("user.tsv")) {
            Ok(learner) => learner,
            Err(_) => FrequencyLearner::default(),
        };
        engine = engine.with_learner(Box::new(learner));
        Ok(Host {
            engine,
            layout: CandidateLayout::new(Vec::new(), PAGE_SIZE, CLOUD_SLOTS),
            highlighted: 0,
            page: 0,
            preedit: String::new(),
            preedit_cursor: 0,
            pending_commit: None,
            data_dir: dir,
        })
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
                    CandidateLayout::new(query.candidates.items, PAGE_SIZE, CLOUD_SLOTS);
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
                self.layout = CandidateLayout::new(Vec::new(), PAGE_SIZE, CLOUD_SLOTS);
                self.highlighted = 0;
                self.page = 0;
            }
        }
    }

    fn clear_view(&mut self) {
        self.layout = CandidateLayout::new(Vec::new(), PAGE_SIZE, CLOUD_SLOTS);
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
        const CTRL_ALT_SUPER: u32 = (1 << 2) | (1 << 3) | (1 << 6);
        if state & CTRL_ALT_SUPER != 0 {
            return false;
        }
        if release {
            // 组句期间吞掉松键,免得应用收到无头的 release。
            return self.composing();
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
            // 组句中的其他可打印键:先吞掉不处理(标点等后续刀接)。
            0x21..=0x7e if composing => true,
            _ => false,
        }
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
        Host::init(dir).expect("样例数据应能装配")
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
}
