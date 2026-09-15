//! 输入会话状态:查询刷新、候选排布、上屏、翻页与高亮、学习落盘。

use qingjian_core::CandidateLayout;

use super::{CLOUD_SLOTS, FLUSH_INTERVAL, Host};

impl Host {
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

    pub(super) fn clear_view(&mut self) {
        self.layout = CandidateLayout::new(Vec::new(), self.page_size, CLOUD_SLOTS);
        self.highlighted = 0;
        self.page = 0;
        self.preedit.clear();
        self.preedit_cursor = 0;
    }

    /// 上屏第 `index` 格(排布内绝对下标)。
    pub(super) fn commit_index(&mut self, index: usize) {
        let Some(candidate) = self.layout.candidate(index).cloned() else {
            return self.commit_raw();
        };
        let text = self.engine.commit(&candidate);
        self.push_commit(text);
        // 连续组句:上屏后缓冲区还有剩余拼音就接着查,否则收窗。
        self.refresh();
    }

    pub(super) fn commit_raw(&mut self) {
        let raw = self.engine.take_raw();
        self.push_commit(raw);
        self.refresh();
    }

    pub(super) fn push_commit(&mut self, text: String) {
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

    pub(super) fn page_count(&self) -> usize {
        self.layout.len().div_ceil(self.layout.page_size()).max(1)
    }

    pub(super) fn turn_page(&mut self, delta: isize) {
        let pages = self.page_count() as isize;
        let next = (self.page as isize + delta).clamp(0, pages - 1);
        if next != self.page as isize {
            self.page = next as usize;
            self.highlighted = self.page * self.layout.page_size();
            self.engine.note_page_turn();
        }
    }

    pub(super) fn move_highlight(&mut self, delta: isize) {
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
}
