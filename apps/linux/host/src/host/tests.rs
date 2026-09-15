//! Host 行为测试:走 assets/sample 真数据。

use super::*;

fn sample_host() -> Host {
    host_with(qingjian_platform::Config::default())
}

fn host_with(config: qingjian_platform::Config) -> Host {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../assets/sample");
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
