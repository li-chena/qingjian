//! 路径约定(XDG)与数据文件探测、释义表装载。

use std::path::{Path, PathBuf};

use qingjian_core::Language;
use qingjian_translate::Glossary;

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

pub(super) fn mtime_of(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// 按配置的学习语言挑释义表并装载;没有对应文件依次退回英语、任一存在的。
pub(super) fn load_glossary(dir: &Path, configured: &str) -> Option<(Language, Glossary)> {
    let configured = configured.parse::<Language>().ok();
    let language = [configured, Some(Language::English)]
        .into_iter()
        .flatten()
        .find(|l| find_data(dir, &format!("glossary-{}", l.code())).is_some())?;
    let path = find_data(dir, &format!("glossary-{}", language.code()))?;
    match Glossary::from_path(language, &path) {
        Ok(glossary) => {
            tracing::info!(
                language = language.code(),
                glosses = glossary.len(),
                "释义表已加载"
            );
            Some((language, glossary))
        }
        Err(error) => {
            tracing::warn!(%error, "释义表加载失败,候选无译文");
            None
        }
    }
}

pub(super) fn find_data(dir: &Path, stem: &str) -> Option<PathBuf> {
    // 同名 .qj 优先于 .tsv,与 extra_dictionaries 的约定一致。
    for ext in ["qj", "tsv"] {
        let path = dir.join(format!("{stem}.{ext}"));
        if path.is_file() {
            return Some(path);
        }
    }
    None
}
