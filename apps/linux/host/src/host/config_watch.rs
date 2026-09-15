//! 配置热加载:按键路径上周期探测 mtime,变了就重载并热应用。

use std::path::PathBuf;

use qingjian_platform::Config;

use super::paths::{load_glossary, mtime_of};
use super::{CONFIG_CHECK_INTERVAL, Host};

impl Host {
    /// 开启配置热加载:记下文件与当前 mtime,之后按键路径上周期探测。
    pub fn watch_config(&mut self, path: PathBuf) {
        self.config_mtime = mtime_of(&path);
        self.config_file = Some(path);
    }

    /// 配置文件变了就重载并热应用(模糊音/每页候选数/学习语言)。
    pub(super) fn maybe_reload_config(&mut self) {
        if self.last_config_check.elapsed() < CONFIG_CHECK_INTERVAL {
            return;
        }
        self.last_config_check = std::time::Instant::now();
        let Some(path) = self.config_file.clone() else {
            return;
        };
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
        self.engine.set_fuzzy(config.fuzzy);
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
}
