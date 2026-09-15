//! Host 行为测试:走 assets/sample 真数据。

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
    let has_shi = |h: &Host| {
        (0..h.layout.len())
            .filter_map(|i| h.layout.candidate(i))
            .any(|c| c.text == "是")
    };
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
    assert!(
        h.pending_commit.take().is_some(),
        "私密模式照常上屏,只是不学习"
    );
    h.set_private(false);
}

#[test]
fn keypad_digit_selects() {
    let mut h = sample_host();
    type_str(&mut h, "ni");
    let second = h.layout.candidate(1).map(|c| c.text.clone());
    assert!(h.key(0xffb2, 0, false), "小键盘 2 应被吞掉");
    if let Some(expected) = second {
        assert_eq!(h.pending_commit.take().unwrap(), expected);
    }
}

#[test]
fn translation_shortcut_commits_gloss() {
    let mut h = sample_host();
    // 找到当前页第一个带译文的候选,用 Alt+对应数字上屏其译文。
    let mut target = None;
    type_str(&mut h, "ni");
    for off in 0..h.layout.page_size() {
        if let Some(c) = h.layout.candidate(off)
            && let Some(t) = &c.translation
            && let Some(sense) = t.senses().first()
        {
            target = Some((off, sense.text.clone()));
            break;
        }
    }
    let (off, gloss) = target.expect("样例里应有带译文的候选");
    // Alt = 1<<3 = 0x8;数字键 '1'+off。
    let keyval = 0x31 + off as u32;
    assert!(h.key(keyval, 1 << 3, false), "Alt+数字应被吞掉");
    assert_eq!(
        h.pending_commit.take().unwrap(),
        gloss,
        "应上屏译文而非中文"
    );
}

#[test]
fn reset_cancels_pending_shift_tap() {
    // 按住 Shift(未夹别键)→ 切窗(reset)→ 松开 Shift:不应静默切中英。
    let mut h = sample_host();
    assert!(!h.engine.english_mode());
    h.key(0xffe1, 0, false); // Shift 按下,shift_armed = true
    h.reset(); // 切窗
    h.key(0xffe1, 1, true); // 在别处松开 Shift
    assert!(!h.engine.english_mode(), "切窗后松开 Shift 不应切换模式");
}

#[test]
fn english_mode_space_without_nav_commits_raw() {
    let mut h = sample_host();
    // Shift 轻点进英文模式
    h.key(0xffe1, 0, false);
    h.key(0xffe1, 1, true);
    assert!(h.engine.english_mode());
    type_str(&mut h, "kubectl"); // 词表里多半没有的词
    // 没动过高亮:空格应原样上屏所敲字母,不被英文候选替换
    assert!(h.key(0x20, 0, false));
    assert_eq!(h.pending_commit.take().unwrap(), "kubectl");
    assert!(!h.composing());
}

#[test]
fn plain_digit_still_selects_chinese() {
    // 不带修饰键的数字仍选中文,不被译词快捷键抢走。
    let mut h = sample_host();
    type_str(&mut h, "ni");
    let first = h.layout.candidate(0).map(|c| c.text.clone());
    assert!(h.key(0x31, 0, false));
    if let Some(expected) = first {
        assert_eq!(h.pending_commit.take().unwrap(), expected);
    }
}
