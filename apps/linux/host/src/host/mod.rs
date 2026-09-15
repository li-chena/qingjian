//! Linux 壳的业务侧:引擎装配 + 输入会话状态。照搬 macOS host 的结构
//! (init.rs 的装配、session.rs 的分页会话),砍掉首版不做的部分(云/模型/统计)。

mod config_watch;
mod keys;
mod paths;
mod session;
#[cfg(test)]
mod tests;

use std::path::PathBuf;

use qingjian_core::{CandidateLayout, Engine, Language};
use qingjian_dictionary::{Dictionary, WordList};
use qingjian_learning::FrequencyLearner;
use qingjian_platform::Config;
use qingjian_translate::Glossary;

pub use paths::{config_path, data_dir};
use paths::{find_data, load_glossary};

/// 云端词占位数:Linux 首版无云联想,不留位。
const CLOUD_SLOTS: usize = 0;
/// 学习数据落盘的最小间隔(上屏路径上顺带检查,焦点切换仍即时落)。
const FLUSH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);
/// 配置探测间隔:按键路径上顺带查 mtime,改完配置敲下一个键就生效。
const CONFIG_CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

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

impl Host {
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
        let dict_path = find_data(&dir, "dict")
            .ok_or_else(|| format!("{} 下没有 dict.qj/dict.tsv", dir.display()))?;
        let dictionary =
            Dictionary::from_path(&dict_path).map_err(|e| format!("词库加载失败:{e}"))?;
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
        engine.set_fuzzy(config.fuzzy);
        let learner = FrequencyLearner::from_path(dir.join("user.tsv")).unwrap_or_default();
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
}
