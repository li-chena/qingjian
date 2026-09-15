//! 按键处理:把 X keysym 翻译成引擎动作。返回 true = 吞掉。

use qingjian_platform::Modifiers;

use super::Host;

/// fcitx5(X11)修饰键位:Shift / Ctrl / Alt(Mod1)/ Super(Mod4)。
const SHIFT: u32 = 1 << 0;
const CTRL: u32 = 1 << 2;
const ALT: u32 = 1 << 3;
const SUPER: u32 = 1 << 6;

/// 配置里的修饰键组合(macOS 词汇)翻成 X 修饰位:option→Alt,command→Super。
fn mods_mask(m: Modifiers) -> u32 {
    (if m.shift { SHIFT } else { 0 })
        | (if m.control { CTRL } else { 0 })
        | (if m.option { ALT } else { 0 })
        | (if m.command { SUPER } else { 0 })
}

/// keysym 是主键区或小键盘的数字 1-9 吗?是则给出 0 起的页内序号。
fn digit_offset(keyval: u32) -> Option<usize> {
    match keyval {
        0x31..=0x39 => Some((keyval - 0x31) as usize),
        0xffb1..=0xffb9 => Some((keyval - 0xffb1) as usize),
        _ => None,
    }
}

/// Shift 按着时数字键在 X 下出的是符号 keysym(美式布局 `!` `@` `#` …):映射回 0 起的页内序号。
/// 只给修饰键快捷键(译词第二组、删候选缺省带 Shift)用,普通标点路径不受影响。
fn shifted_digit_offset(keyval: u32) -> Option<usize> {
    match keyval {
        0x21 => Some(0), // !
        0x40 => Some(1), // @
        0x23 => Some(2), // #
        0x24 => Some(3), // $
        0x25 => Some(4), // %
        0x5e => Some(5), // ^
        0x26 => Some(6), // &
        0x2a => Some(7), // *
        0x28 => Some(8), // (
        _ => None,
    }
}

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
        // 修饰键+数字快捷键(只在组句中认):译词上屏(缺省 Alt=第一个,Alt+Shift=第二个)、删候选(缺省 Shift)。
        if !release
            && self.composing()
            && let Some(offset) = digit_offset(keyval).or_else(|| shifted_digit_offset(keyval))
        {
            let pressed = state & (SHIFT | CTRL | ALT | SUPER);
            if pressed != 0 {
                let (first, second) = self.translation_mods;
                if pressed == mods_mask(first) {
                    self.commit_translation_on_page(offset, 0);
                    return true;
                }
                if pressed == mods_mask(second) {
                    self.commit_translation_on_page(offset, 1);
                    return true;
                }
                if pressed == mods_mask(self.delete_mods) {
                    self.forget_on_page(offset);
                    return true;
                }
            }
        }
        if state & CTRL_ALT_SUPER != 0 {
            return false;
        }
        if release {
            // 组句期间吞掉普通键的松键,免得应用收到无头的 release;修饰键已在上面放行。
            return self.composing() && !(0xffe1..=0xffee).contains(&keyval);
        }
        let composing = self.composing();
        // 英文直输段(缓冲区里已有 `-` 这类字符):可见字符一律追加,空格/回车整段原样上屏。
        let raw = composing && self.engine.raw_mode();
        match keyval {
            // a-z:进缓冲区。英文候选关着时的英文模式是纯直通,字母不进缓冲区。
            0x61..=0x7a => {
                if self.engine.english_mode() && !self.english_candidates && !composing {
                    self.engine.note_passthrough(keyval as u8 as char);
                    return false;
                }
                self.engine.push(keyval as u8 as char);
                self.refresh();
                true
            }
            // 直输段里的可见字符(数字、标点,翻页键字符也算):字面追加。
            _ if raw && (0x21..=0x7e).contains(&keyval) => {
                self.engine.push(keyval as u8 as char);
                self.refresh();
                true
            }
            // 数字 1-9(含小键盘):组句中按页内序号上屏。
            _ if composing && digit_offset(keyval).is_some() => {
                let offset = digit_offset(keyval).expect("刚匹配过");
                let index = self.page * self.layout.page_size() + offset;
                if self.layout.candidate(index).is_some() {
                    self.commit_index(index);
                }
                true
            }
            // 空格:中文模式总是上屏高亮候选;英文模式只在动过高亮后才选,
            // 没动过就把敲的字母原样上屏(词表里没有的词不被补全替换)。
            // 直输段整段原样上屏,空格本身也交给应用(`hello, world` 里的空格要在)。
            0x20 if composing => {
                if raw {
                    self.commit_index(self.highlighted);
                    self.engine.note_passthrough(' ');
                    return false;
                }
                if self.engine.english_mode() && !self.navigated {
                    self.commit_raw();
                } else {
                    self.commit_index(self.highlighted);
                }
                true
            }
            // 回车:拼音原文上屏。
            0xff0d | 0xff8d if composing => {
                self.commit_raw();
                true
            }
            // 退格:组句中删一个字符。
            0xff08 if composing => {
                self.engine.backspace();
                self.refresh();
                true
            }
            // 不组句时的退格删的是已上屏的词:记「选错了」信号供纠错学习,按键仍透传给应用真删。
            0xff08 => {
                self.engine.note_backspace();
                false
            }
            // Delete / 小键盘 Delete:光标后向前删。
            0xffff | 0xff9f if composing => {
                self.engine.delete_forward();
                self.refresh();
                true
            }
            // Esc:放弃本次输入。
            0xff1b if composing => {
                self.engine.clear();
                self.refresh();
                true
            }
            // 组句中敲 `-`:进入英文直输段(`no-way`),不当翻页键——翻页键见配置 `[general] page_keys`。
            0x2d if composing => {
                self.engine.push('-');
                self.refresh();
                true
            }
            // 翻页:配置的键对(缺省 `[` `]`)与 PageUp / PageDown(含小键盘)、方向键上下。
            // 英文模式不认字符翻页键:标点一律半角透传(选词靠方向键,翻页还有 PageUp/Down)。
            _ if composing
                && !self.engine.english_mode()
                && keyval == u32::from(self.page_keys.0) =>
            {
                self.turn_page(-1);
                true
            }
            _ if composing
                && !self.engine.english_mode()
                && keyval == u32::from(self.page_keys.1) =>
            {
                self.turn_page(1);
                true
            }
            0xff55 | 0xff9a | 0xff52 | 0xff97 if composing => {
                self.turn_page(-1);
                true
            }
            0xff56 | 0xff9b | 0xff54 | 0xff99 if composing => {
                self.turn_page(1);
                true
            }
            // 方向键左右(含小键盘):移动高亮。
            0xff51 | 0xff96 if composing => {
                self.move_highlight(-1);
                true
            }
            0xff53 | 0xff98 if composing => {
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
                let c = keyval as u8 as char;
                // 英文模式标点一律半角(macOS 口径):不走全角转换,组句先把敲的字母原样上屏,
                // 标点本身交给应用。
                if self.engine.english_mode() {
                    if composing {
                        self.commit_raw();
                    }
                    self.engine.note_passthrough(c);
                    return false;
                }
                if composing {
                    self.commit_index(self.highlighted);
                }
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
            // 修饰键本身(Ctrl / Alt / Super / CapsLock …)按下永远透传,应用要看修饰状态。
            0xffe1..=0xffee => false,
            // 组句期间剩下的编辑键(Tab / Home / F 键…)一律接管吞掉,
            // 否则应用会动光标、丢焦点,组句跟着作废(macOS 同款口径)。
            _ if composing => true,
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
        // HOST 是进程级单例(不分 InputContext):按住 Shift 点击切窗后松开,会在新窗口
        // 静默切中英——切窗时必须撤销待兑现的 Shift 轻点。
        self.shift_armed = false;
        self.navigated = false;
        self.flush();
    }
}
