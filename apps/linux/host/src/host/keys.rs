//! 按键处理:把 X keysym 翻译成引擎动作。返回 true = 吞掉。

use super::Host;

impl Host {
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
