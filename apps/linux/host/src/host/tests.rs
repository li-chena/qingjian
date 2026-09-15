//! Host 行为测试:走 assets/sample 真数据。

use super::*;

fn sample_host() -> Host {
    host_with(qingjian_platform::Config::default())
}

/// 每个 Host 一个独立临时数据目录:init 之后引擎会往数据目录写统计/日志,
/// 绝不能拿 assets/sample 当数据目录(会把落盘文件写进仓库)。
fn host_with(config: qingjian_platform::Config) -> Host {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let dir = temp_data_dir(&format!("h{}", SEQ.fetch_add(1, Ordering::Relaxed)));
    Host::init(dir, Some(config)).expect("样例数据应能装配")
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
    assert!(!h.layout.is_empty(), "应有候选");
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
fn punctuation_while_composing_enters_raw_segment() {
    // 组句中敲半角标点(翻页键除外):进缓冲区成英文直输段(hello, dui'ma?),
    // 与 macOS 实现、docs/user/input/shortcuts.md 口径一致。
    let mut h = sample_host();
    type_str(&mut h, "ni");
    assert!(h.key(0x2c, 0, false), ", 应进缓冲区");
    assert!(h.composing());
    assert!(h.engine.raw_mode(), "标点应把整段变成直输段");
    assert_eq!(h.engine.composition().text(), "ni,");
    assert!(h.pending_commit.is_none(), "不应上屏候选或全角标点");
    assert!(h.key(0xff0d, 0, false));
    assert_eq!(h.pending_commit.take().unwrap(), "ni,");
}

#[test]
fn apostrophe_separates_syllables() {
    // 音节分隔符 ':xi'an 组句中进缓冲区,不当标点。
    let mut h = sample_host();
    type_str(&mut h, "xi");
    assert!(h.key(0x27, 0, false), "' 应进缓冲区");
    assert!(h.composing());
    assert!(h.pending_commit.is_none());
    type_str(&mut h, "an");
    assert_eq!(h.engine.composition().text(), "xi'an");
}

#[test]
fn shuangpin_semicolon_completes_syllable() {
    // 微软/搜狗双拼的 ; 是 ing 键:末尾落单声母时进缓冲区,不当标点。
    let mut config = qingjian_platform::Config::default();
    config.general.shuangpin = "microsoft".to_owned();
    let mut h = host_with(config);
    type_str(&mut h, "x");
    assert!(h.engine.takes_semicolon(), "落单声母 x 后 ; 应是 ing");
    assert!(h.key(0x3b, 0, false), "; 应进缓冲区");
    assert_eq!(h.engine.composition().text(), "x;");
    assert!(h.pending_commit.is_none());
}

#[test]
fn english_composing_takes_digits_uppercase_apostrophe() {
    // 英文组句中数字、大写、' 进缓冲区(win32 / McDonald / don't),不选词不打断。
    let mut h = sample_host();
    h.key(0xffe1, 0, false);
    h.key(0xffe1, 1, true);
    assert!(h.engine.english_mode());
    type_str(&mut h, "win");
    assert!(h.key(0x33, 0, false), "英文组句中 3 应进缓冲区");
    assert_eq!(h.engine.composition().text(), "win3");
    h.key(0xff1b, 0, false);
    type_str(&mut h, "don");
    assert!(h.key(0x27, 0, false), "英文组句中 ' 应进缓冲区");
    assert_eq!(h.engine.composition().text(), "don'");
    h.key(0xff1b, 0, false);
    type_str(&mut h, "mc");
    assert!(h.key(0x44, 1, false), "英文组句中大写 D 应进缓冲区");
    assert_eq!(h.engine.composition().text(), "mcD");
}

#[test]
fn page_keys_dash_equals_config_beats_raw_entry() {
    // 用户显式配回 page_keys = "-=":翻页优先于 - 的直输段入口。
    let mut config = qingjian_platform::Config::default();
    config.general.page_keys = "-=".to_owned();
    let mut h = host_with(config);
    type_str(&mut h, "s");
    assert!(h.page_count() > 1);
    assert!(h.key(0x3d, 0, false), "= 应翻下一页");
    assert_eq!(h.page, 1);
    assert!(h.key(0x2d, 0, false), "- 应翻回上一页");
    assert_eq!(h.page, 0);
    assert!(!h.engine.raw_mode(), "配置为翻页键的 - 不应进直输段");
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
fn page_indicator_multi_page() {
    let mut h = sample_host();
    // 找一个候选数超过一页的输入
    type_str(&mut h, "shi");
    if h.page_count() > 1 {
        assert_eq!(
            h.page_indicator(),
            format!("1/{}", h.page_count()),
            "首页应显示 1/N"
        );
        h.turn_page(1);
        assert_eq!(
            h.page_indicator(),
            format!("2/{}", h.page_count()),
            "翻页后应显示 2/N"
        );
    }
    // 清空后无候选:无页码
    h.reset();
    assert_eq!(h.page_indicator(), "", "无候选时不显示页码");
}

#[test]
fn default_page_keys_brackets_turn_pages() {
    // 缺省翻页键是配置的 `[` `]`(DEFAULT_PAGE_KEYS),不是 - =。
    let mut h = sample_host();
    type_str(&mut h, "s");
    assert!(h.page_count() > 1, "输入 s 应有多页候选");
    assert!(h.key(0x5d, 0, false), "] 应被吞掉");
    assert_eq!(h.page, 1, "] 应翻到下一页");
    assert!(h.pending_commit.is_none(), "翻页不应上屏任何东西");
    assert!(h.key(0x5b, 0, false), "[ 应被吞掉");
    assert_eq!(h.page, 0, "[ 应翻回上一页");
    assert!(h.composing());
}

#[test]
fn page_keys_config_comma_period() {
    // 配置 page_keys = ",." 后,逗号句号翻页而不再当标点。
    let mut config = qingjian_platform::Config::default();
    config.general.page_keys = ",.".to_owned();
    let mut h = host_with(config);
    type_str(&mut h, "s");
    assert!(h.page_count() > 1);
    assert!(h.key(0x2e, 0, false), ". 应被吞掉");
    assert_eq!(h.page, 1, ". 应翻到下一页");
    assert!(h.pending_commit.is_none());
    assert!(h.key(0x2c, 0, false), ", 应被吞掉");
    assert_eq!(h.page, 0, ", 应翻回上一页");
}

#[test]
fn hyphen_enters_raw_segment_space_commits() {
    // 组句中敲 `-`:进英文直输段(no-way),空格整段原样上屏、空格本身交给应用。
    let mut h = sample_host();
    type_str(&mut h, "ni");
    assert!(h.key(0x2d, 0, false), "- 应被吞掉(进缓冲区)");
    assert!(h.composing());
    assert!(h.engine.raw_mode(), "- 之后应是英文直输段");
    type_str(&mut h, "hao");
    assert!(
        !h.key(0x20, 0, false),
        "直输段的空格应透传(hello, world 的空格要在)"
    );
    assert_eq!(h.pending_commit.take().unwrap(), "ni-hao");
    assert!(!h.composing());
}

#[test]
fn raw_segment_takes_page_keys_and_punctuation() {
    // 直输段里可见字符一律追加:翻页键字符、标点都是字面。
    let mut h = sample_host();
    type_str(&mut h, "ni");
    assert!(h.key(0x2d, 0, false));
    assert!(h.key(0x5b, 0, false), "直输段里 [ 应进缓冲区而非翻页");
    assert!(h.key(0x2c, 0, false), "直输段里 , 应进缓冲区而非转全角");
    assert!(h.composing());
    assert!(h.pending_commit.is_none(), "追加过程不应上屏");
    assert!(h.key(0xff0d, 0, false), "回车整段原样上屏");
    assert_eq!(h.pending_commit.take().unwrap(), "ni-[,");
}

#[test]
fn full_width_punctuation_off_passes_half_width() {
    // 配置关掉全角标点:空闲时敲逗号不再转全角,原样透传。
    let mut config = qingjian_platform::Config::default();
    config.general.full_width_punctuation = false;
    let mut h = host_with(config);
    assert!(!h.key(0x2c, 0, false), "全角标点关掉后逗号应透传");
    assert!(h.pending_commit.is_none());
}

#[test]
fn custom_phrases_config_applies() {
    // 自定义短语:敲输入码,固定位置出短语。
    let config = qingjian_platform::Config {
        custom_phrases: vec![qingjian_core::CustomPhrase {
            code: "addr".to_owned(),
            text: "青简大道 1 号".to_owned(),
            position: 1,
            enabled: true,
        }],
        ..Default::default()
    };
    let mut h = host_with(config);
    type_str(&mut h, "addr");
    let first = h.layout.candidate(0).map(|c| c.text.clone());
    assert_eq!(
        first.as_deref(),
        Some("青简大道 1 号"),
        "自定义短语应出现在第 1 格"
    );
}

#[test]
fn shuangpin_config_applies() {
    // 双拼(小鹤):u=sh、i=i,敲 ui 应出「是」;全拼下 ui 不该出。
    let mut config = qingjian_platform::Config::default();
    config.general.shuangpin = "xiaohe".to_owned();
    let mut h = host_with(config);
    type_str(&mut h, "ui");
    let has_shi = (0..h.layout.len())
        .filter_map(|i| h.layout.candidate(i))
        .any(|c| c.text == "是");
    assert!(has_shi, "小鹤双拼下 ui 应出「是」");
}

#[test]
fn delete_shortcut_swallowed_while_composing() {
    // 删词键(缺省 Shift+数字):X 布局下 Shift+1 的 keysym 是 `!`,须映射回数字位。
    // 组句中按下:吞掉、不上屏、仍在组句(词库词没什么可删,但键不能漏给应用)。
    let mut h = sample_host();
    type_str(&mut h, "ni");
    assert!(h.key(0x21, 1, false), "组句中 Shift+1(!)应被删词键吞掉");
    assert!(h.pending_commit.is_none(), "删词不应上屏任何东西");
    assert!(h.composing(), "删词后仍在组句");
    h.key(0xff1b, 0, false);
    // 不组句时 Shift+1 还是标点:照走全角转换。
    assert!(h.key(0x21, 1, false), "空闲时 ! 应转全角并吞掉");
    assert_eq!(h.pending_commit.take().unwrap(), "\u{ff01}");
}

#[test]
fn alt_shift_digit_reaches_second_sense() {
    // 译词第二组(缺省 Alt+Shift+数字):X 布局下 keysym 是符号,也得映射回数字位。
    let mut h = sample_host();
    type_str(&mut h, "ni");
    let alt_shift = (1 << 0) | (1 << 3);
    assert!(
        h.key(0x21, alt_shift, false),
        "组句中 Alt+Shift+1(!)应被译词键吞掉"
    );
    assert!(h.composing() || h.pending_commit.is_some());
}

#[test]
fn composing_swallows_unknown_editing_keys() {
    // 组句期间所有编辑动作都由我们接管;不认识的一律吞掉(macOS 同款),按下与松开对称。
    let mut h = sample_host();
    type_str(&mut h, "ni");
    assert!(h.key(0xff09, 0, false), "组句中 Tab 按下应被吞掉");
    assert!(h.key(0xff09, 0, true), "组句中 Tab 松开应被吞掉");
    assert!(h.key(0xff50, 0, false), "组句中 Home 应被吞掉");
    assert!(h.key(0xffbe, 0, false), "组句中 F1 应被吞掉");
    assert!(h.composing(), "吞掉之后组句不受影响");
    assert!(!h.key(0xffe3, 0, false), "Ctrl 修饰键本身按下应透传");
    assert!(!h.key(0xfe03, 0, false), "AltGr(ISO_Level3_Shift)应透传");
    assert!(!h.key(0xff7f, 0, false), "Num_Lock 应透传");
    assert!(!h.key(0x1008ff13, 0, false), "XF86 媒体键应透传");
    assert!(!h.key(0xe9, 0, false), "AltGr 打出的非 ASCII 字符(é)应透传");
    h.key(0xff1b, 0, false);
    assert!(!h.key(0xff09, 0, false), "空闲时 Tab 应透传");
}

#[test]
fn english_candidates_off_is_pure_passthrough() {
    // [general] english_candidates = false:英文模式纯直通,字母不进缓冲区。
    let mut config = qingjian_platform::Config::default();
    config.general.english_candidates = false;
    let mut h = host_with(config);
    h.key(0xffe1, 0, false);
    h.key(0xffe1, 1, true);
    assert!(h.engine.english_mode(), "轻点仍切到英文模式");
    assert!(!h.key(0x6b, 0, false), "英文候选关着:字母应透传");
    assert!(!h.composing(), "纯直通不组句");
    assert!(h.pending_commit.is_none());
}

#[test]
fn english_mode_punctuation_stays_half_width() {
    // 英文模式标点一律半角(macOS 口径):不转全角,原样透传;[ ] 也不当翻页键。
    let mut h = sample_host();
    h.key(0xffe1, 0, false);
    h.key(0xffe1, 1, true);
    assert!(h.engine.english_mode());
    // 空闲:透传,不出「【」「,」
    assert!(!h.key(0x5b, 0, false), "英文模式空闲 [ 应透传半角");
    assert!(h.pending_commit.is_none(), "不应转出全角「【」");
    assert!(!h.key(0x2c, 0, false), "英文模式空闲 , 应透传半角");
    assert!(h.pending_commit.is_none());
    // 组句中:先把敲的字母原样上屏,标点本身交给应用;] 不翻页
    type_str(&mut h, "kubectl");
    assert!(!h.key(0x5d, 0, false), "英文模式组句中 ] 应透传而非翻页");
    assert_eq!(
        h.pending_commit.take().unwrap(),
        "kubectl",
        "敲的字母应原样上屏"
    );
    assert!(!h.composing());
}

#[test]
fn config_hot_reload_applies_new_fields() {
    // 热加载也要覆盖新接的字段:翻页键与全角标点开关。
    let dir = std::env::temp_dir().join(format!("qj-test-fields-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = dir.join("config.toml");
    std::fs::write(&cfg, "[general]\n").unwrap();
    let mut h = sample_host();
    h.watch_config(cfg.clone());
    std::fs::write(
        &cfg,
        "[general]\npage_keys = \",.\"\nfull_width_punctuation = false\n",
    )
    .unwrap();
    h.config_mtime = None;
    h.last_config_check = std::time::Instant::now() - CONFIG_CHECK_INTERVAL;
    type_str(&mut h, "s");
    assert!(h.page_count() > 1);
    assert!(h.key(0x2e, 0, false), "热加载后 . 应翻页");
    assert_eq!(h.page, 1);
    h.key(0xff1b, 0, false);
    assert!(!h.key(0x2c, 0, false), "热加载后空闲逗号应透传(全角已关)");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn expression_mode_takes_digits_and_operators() {
    // v1+2:表达式模式里数字与运算符进缓冲区,不当选词键/标点;候选出 3。
    let mut h = sample_host();
    type_str(&mut h, "v");
    assert!(h.engine.expression_mode(), "v 应进入表达式模式");
    assert!(h.key(0x31, 0, false), "表达式里 1 应进缓冲区");
    assert!(h.key(0x2b, 1, false), "+(Shift+=)应进缓冲区");
    assert!(h.key(0x32, 0, false), "表达式里 2 应进缓冲区");
    assert!(h.pending_commit.is_none(), "追加过程不上屏");
    let has_result = (0..h.layout.len())
        .filter_map(|i| h.layout.candidate(i))
        .any(|c| c.text == "3");
    assert!(has_result, "v1+2 的候选里应有 3");
    // 表达式里 Shift+( 是括号,不当删词/译词快捷键
    assert!(h.key(0x28, 1, false), "( 应进缓冲区");
    assert!(h.composing());
    assert!(h.pending_commit.is_none(), "( 不应触发删词或上屏");
}

#[test]
fn unicode_entry_takes_digits() {
    // u4e00:问字模式的码点输入,数字进缓冲区,候选出「一」。
    let mut h = sample_host();
    type_str(&mut h, "u");
    assert!(h.engine.question_mode(), "u 应进入问字模式");
    assert!(h.key(0x34, 0, false), "码点里 4 应进缓冲区");
    type_str(&mut h, "e");
    assert!(h.key(0x30, 0, false), "码点里 0 应进缓冲区");
    assert!(h.key(0x30, 0, false));
    let has_char = (0..h.layout.len())
        .filter_map(|i| h.layout.candidate(i))
        .any(|c| c.text == "一");
    assert!(has_char, "u4e00 的候选里应有「一」");
}

#[test]
fn correction_gives_intended_candidate() {
    // 拼写纠错(引擎侧,验证 Linux 按键路径畅通):nihoa 应仍给出「你好」。
    let mut h = sample_host();
    type_str(&mut h, "nihoa");
    let has = (0..h.layout.len())
        .filter_map(|i| h.layout.candidate(i))
        .any(|c| c.text == "你好");
    assert!(has, "nihoa 应纠错给出「你好」");
}

/// 建一个独立的临时数据目录(拷样例数据),测试写盘类功能不弄脏 assets/sample。
fn temp_data_dir(tag: &str) -> PathBuf {
    let sample = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../assets/sample");
    let dir = std::env::temp_dir().join(format!("qj-test-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for f in ["dict.tsv", "english.tsv", "glossary-en.tsv"] {
        std::fs::copy(sample.join(f), dir.join(f)).unwrap();
    }
    dir
}

#[test]
fn user_extra_dictionary_appears_in_candidates() {
    // 用户词库:user-dicts/ 下的 TSV 词条应进候选。
    let dir = temp_data_dir("extradict");
    std::fs::create_dir_all(dir.join("user-dicts")).unwrap();
    std::fs::write(
        dir.join("user-dicts/mine.tsv"),
        "青简验证\tqing jian yan zheng\t500\n",
    )
    .unwrap();
    let mut h = Host::init(dir.clone(), Some(qingjian_platform::Config::default()))
        .expect("样例数据应能装配");
    type_str(&mut h, "qingjianyanzheng");
    let has = (0..h.layout.len())
        .filter_map(|i| h.layout.candidate(i))
        .any(|c| c.text == "青简验证");
    assert!(has, "用户词库的词应出现在候选里");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn input_log_writes_when_enabled() {
    // 输入日志(缺省开):上屏后 input-log.jsonl 应落盘;配置关掉则不再新增。
    let dir = temp_data_dir("inputlog");
    let mut h = Host::init(dir.clone(), Some(qingjian_platform::Config::default()))
        .expect("样例数据应能装配");
    type_str(&mut h, "ni");
    h.key(0x20, 0, false);
    h.pending_commit.take();
    h.reset(); // 落盘时机与切窗对齐
    let log = dir.join("input-log.jsonl");
    assert!(log.exists(), "输入日志开着时应写 input-log.jsonl");
    assert!(
        std::fs::metadata(&log).unwrap().len() > 0,
        "日志文件应有内容"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn per_app_english_candidates_off() {
    // [apps] english_candidates_off 列出的应用里英文模式纯直通;别的应用不受影响。
    let config = qingjian_platform::Config {
        apps: qingjian_platform::AppsConfig::with_english_candidates_off(&["konsole"]),
        ..Default::default()
    };
    let mut h = host_with(config);
    h.key(0xffe1, 0, false);
    h.key(0xffe1, 1, true);
    assert!(h.engine.english_mode());
    h.set_program("konsole");
    assert!(!h.key(0x6b, 0, false), "名单内应用里字母应透传");
    assert!(!h.composing());
    h.set_program("kate");
    assert!(h.key(0x6b, 0, false), "名单外应用里应正常给英文候选");
    assert!(h.composing());
}

#[test]
fn preedit_display_follows_config() {
    // [general] preedit 决定拼音行显示位置(0=两处 1=只行内 2=只窗口),shim 按它画。
    let mut h = sample_host();
    assert_eq!(h.preedit_display(), 0, "缺省两处都显示");
    let mut config = qingjian_platform::Config::default();
    config.general.preedit = qingjian_platform::PreeditMode::Window;
    h.apply_config(&config);
    assert_eq!(h.preedit_display(), 2, "配置只在窗口后应返回 2");
}

/// 假打分器:偏爱指定句子,其余打大负分(引擎自己的重排测试同款思路)。
struct Prefers(&'static str);

impl qingjian_core::sentence::SentenceScorer for Prefers {
    fn score(&self, _context: &str, texts: &[&str]) -> Vec<f64> {
        texts
            .iter()
            .map(|t| if *t == self.0 { 0.0 } else { -100.0 })
            .collect()
    }
}

#[test]
fn model_poll_idle_is_inert() {
    // 没接模型:定时问一句立刻歇,不空转。
    let mut h = sample_host();
    assert_eq!(h.model_poll(), 0, "无模型无组句时应返回 0(停定时器)");
    type_str(&mut h, "ni");
    assert_eq!(h.model_poll(), 0, "无模型时组句中也没什么可等");
}

#[test]
fn model_rescoring_requests_after_debounce_and_repaints() {
    // 接上异步打分器 → 敲拼音攒下整句路径 → 防抖到点发请求 → 分回来要求重画。
    let mut h = sample_host();
    h.engine
        .set_async_sentence_scorer(Some(Box::new(Prefers("你好"))));
    type_str(&mut h, "nihao");
    assert!(h.engine.rescoring_pending(), "组句后应有整句路径等着打分");
    assert!(h.rescore_deadline.is_some(), "查询后应起防抖计时");
    // 把防抖截止拨到现在,循环 poll 等后台线程把分送回来。
    h.rescore_deadline = Some(std::time::Instant::now());
    let start = std::time::Instant::now();
    let mut repainted = false;
    while start.elapsed() < std::time::Duration::from_secs(2) {
        if h.model_poll() & 1 != 0 {
            repainted = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(repainted, "重排结果到了应要求重画");
    assert!(h.composing(), "重画只是换排序,组句不受影响");
}

#[test]
fn model_poll_keeps_polling_bit_while_work_pending() {
    // 重画那次返回值也要带「继续定时」位,否则 shim 的一次性定时器带着在途工作停摆。
    let mut h = sample_host();
    h.engine
        .set_async_sentence_scorer(Some(Box::new(Prefers("你好"))));
    type_str(&mut h, "nihao");
    h.rescore_deadline = Some(std::time::Instant::now());
    // 造一个「加载线程还挂着」的在途状态:重画后仍须继续轮询。
    let (_tx, rx) = std::sync::mpsc::channel();
    h.model_loader = Some(rx);
    let start = std::time::Instant::now();
    let mut repaint = None;
    while start.elapsed() < std::time::Duration::from_secs(2) {
        let poll = h.model_poll();
        if poll & 1 != 0 {
            repaint = Some(poll);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let poll = repaint.expect("应有一次重画");
    assert!(
        poll & 0b10 != 0,
        "加载线程仍在途:重画那次也应带继续定时位,实得 {poll:#b}"
    );
    drop(_tx);
}

#[test]
fn model_config_disable_unloads_scorer() {
    // 热加载 [model] enabled = false:卸掉打分器,不再重排。
    let mut h = sample_host();
    h.engine
        .set_async_sentence_scorer(Some(Box::new(Prefers("你好"))));
    assert!(h.engine.has_sentence_scorer());
    let mut config = qingjian_platform::Config::default();
    config.model.enabled = false;
    h.apply_config(&config);
    assert!(!h.engine.has_sentence_scorer(), "关掉配置应卸掉打分器");
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

#[test]
fn releases_swallowed_iff_press_was_swallowed() {
    // 上屏类的键按下就结束组句:松键仍须吞,否则应用收到无头 keyup(2026-09-15 巡检 F1)。
    for (name, keyval) in [
        ("空格", 0x20u32),
        ("回车", 0xff0d),
        ("Esc", 0xff1b),
        ("数字1", 0x31),
    ] {
        let mut h = sample_host();
        type_str(&mut h, "ni");
        assert!(h.key(keyval, 0, false), "{name} 按下应被吞");
        assert!(!h.composing(), "{name} 按下后组句应已结束");
        assert!(h.key(keyval, 0, true), "{name} 松键应与按下对称地被吞");
    }
    // 空闲时敲逗号转全角上屏:按下吞,松键同吞。
    let mut h = sample_host();
    assert!(h.key(0x2c, 0, false), "空闲逗号按下应被吞(转全角)");
    assert!(h.pending_commit.take().is_some());
    assert!(h.key(0x2c, 0, true), "空闲逗号松键应被吞");
}

#[test]
fn rollover_releases_after_commit_are_swallowed() {
    // 按住字母未松就敲空格上屏(rollover):字母的松键到达时组句已结束,按下是我们吞的,松键也吞。
    let mut h = sample_host();
    type_str(&mut h, "ni");
    assert!(h.key(0x20, 0, false));
    assert!(!h.composing());
    assert!(h.key(0x69, 0, true), "i 的按下被吞过,松键也吞");
    assert!(h.key(0x6e, 0, true), "n 同理");
    assert!(!h.key(0x78, 0, true), "x 没按过,松键透传");
    h.pending_commit.take();
}

#[test]
fn passthrough_presses_keep_their_releases_passthrough() {
    // 按下透传过的键,松键也透传(应用见过按下,吞掉松键同样制造不对称)。
    let mut h = sample_host();
    // 空闲敲 ]:punctuate 转不动,透传;随后组句中它的松键到达,也必须透传。
    let press = h.key(0x5d, 0, false);
    type_str(&mut h, "ni");
    if !press {
        assert!(!h.key(0x5d, 0, true), "按下透传过的 ] 组句中松键也透传");
    }
    // Ctrl+a:按下透传,松键透传。
    assert!(!h.key(0x61, 1 << 2, false), "Ctrl+a 按下透传");
    assert!(!h.key(0x61, 1 << 2, true), "Ctrl+a 松键透传");
    // 同一键的松键只吞一次:上屏空格的松键吞过之后,再来一次(没有对应按下)就透传。
    let mut h = sample_host();
    type_str(&mut h, "ni");
    assert!(h.key(0x20, 0, false));
    assert!(h.key(0x20, 0, true));
    assert!(!h.key(0x20, 0, true), "没有对应按下的第二次松键透传");
    h.pending_commit.take();
}

/// 真数据在才跑(data/generated/dict.qj + data/model/model.qjm,均不进 git):
/// 模型后台加载完成时,加载窗口里敲出的那一轮也要拿到整句重排(2026-09-15 巡检 F3)。
#[test]
fn model_attached_mid_composition_rescores_that_round() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let dict = repo.join("data/generated/dict.qj");
    let model = repo.join("data/model/model.qjm");
    if !dict.is_file() || !model.is_file() {
        eprintln!("跳过:没有真词库/真模型(CI 上属正常)");
        return;
    }
    let dir = temp_data_dir("lateattach");
    std::fs::remove_file(dir.join("dict.tsv")).unwrap();
    std::os::unix::fs::symlink(&dict, dir.join("dict.qj")).unwrap();
    std::fs::create_dir_all(dir.join("model")).unwrap();
    std::os::unix::fs::symlink(&model, dir.join("model/model.qjm")).unwrap();
    let mut h =
        Host::init(dir, Some(qingjian_platform::Config::default())).expect("真词库应能装配");
    // 模型还在后台加载时就把词敲出来(加载窗口里的那一轮)。
    type_str(&mut h, "nihao");
    assert!(h.model_loader.is_some(), "模型应还在后台加载");
    // 按 shim 的方式轮询:模型接上后,这一轮必须出现重画位(bit0 = 重排换了排序要重画)。
    let started = std::time::Instant::now();
    let mut repainted = false;
    while started.elapsed() < std::time::Duration::from_secs(30) {
        if h.model_poll() & 1 != 0 {
            repainted = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
        repainted,
        "模型接上后应补重排并给出重画位,而不是这一轮永远错过"
    );
}

#[test]
fn data_lookup_prefers_user_layer_over_dist() {
    // 分层查找(2026-09-15 巡检 F4):随包数据在 dist/,用户层(根)同名文件盖过它;
    // 层内 .qj 优先 .tsv,但用户层整层先于随包层——用户放个 tsv 也能盖过随包 qj。
    let dir = temp_data_dir("layers");
    std::fs::create_dir_all(dir.join("dist")).unwrap();
    std::fs::write(dir.join("dist/probe.qj"), "").unwrap();
    std::fs::write(dir.join("dist/probe.tsv"), "").unwrap();
    assert_eq!(
        find_data(&dir, "probe").unwrap(),
        dir.join("dist/probe.qj"),
        "只有随包层时取 dist,且 .qj 优先"
    );
    std::fs::write(dir.join("probe.tsv"), "").unwrap();
    assert_eq!(
        find_data(&dir, "probe").unwrap(),
        dir.join("probe.tsv"),
        "用户层整层先于随包层"
    );
    std::fs::write(dir.join("dist/emoji-probe.tsv"), "").unwrap();
    assert_eq!(
        find_file(&dir, "emoji-probe.tsv").unwrap(),
        dir.join("dist/emoji-probe.tsv")
    );
    std::fs::write(dir.join("emoji-probe.tsv"), "").unwrap();
    assert_eq!(
        find_file(&dir, "emoji-probe.tsv").unwrap(),
        dir.join("emoji-probe.tsv"),
        "find_file 同样用户层优先"
    );
}

#[test]
fn dist_layer_alone_assembles_host() {
    // 新装机形态:随包数据全在 dist/,用户层只有学习数据——必须装得起来。
    let dir = {
        let dir = std::env::temp_dir().join(format!("qj-test-distonly-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("dist")).unwrap();
        let sample = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../assets/sample");
        for f in ["dict.tsv", "english.tsv", "glossary-en.tsv"] {
            std::fs::copy(sample.join(f), dir.join("dist").join(f)).unwrap();
        }
        dir
    };
    let mut h = Host::init(dir, Some(qingjian_platform::Config::default()))
        .expect("随包层单独在场应能装配");
    type_str(&mut h, "ni");
    assert!(!h.layout.is_empty(), "dist 层词库应出候选");
}
