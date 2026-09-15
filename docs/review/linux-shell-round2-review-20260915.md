# 青简 Linux 壳 第二轮独立巡检报告

**本文地位**:这是一份**发现记录**,不是定稿设计。它只记录"哪行代码在什么条件下会出什么问题",修法与优先级以甲方裁决为准;与 `docs/design/linux-fcitx5.md`(定稿设计)冲突时,设计稿说了算,本文只提供事实。**所有结论都带复现证据**,探针代码见附录 A,可直接粘回去重跑。

**一句话结论**:本轮 8 个提交的 1488 行增量里逮到 **7 处问题**(1 处可达 panic、1 处漏事件给应用),**核实后判"不是问题"的 14 处**(含上一轮已裁的 3 条);重点区①键路由、②模型状态机、④升级路径全部实测过,重点区③的"定时器重新武装"与"`lastIc_` 生命周期"经源码 + C 探针证成**成立**。

巡检时间:2026-09-15。巡检范围:`73bf650..612b363`(实际 **8** 个提交,委托写的是 7 个,`612b363` 是注释修正那条)。巡检用的临时探针跑完已全部还原,仓库里只多了本文这一份报告(未提交,`git status` 里显示为未跟踪)。

---

## 一、范围与方法

**精读的增量**(1488 行):

```
apps/linux/fcitx5-shim/addon.cpp        229 行(全文)
apps/linux/fcitx5-shim/qingjian.h        43
apps/linux/host/src/bridge.rs           242
apps/linux/host/src/host/keys.rs        343
apps/linux/host/src/host/model.rs       139
apps/linux/host/src/host/mod.rs         209
apps/linux/host/src/host/session.rs     198
apps/linux/host/src/host/config_watch.rs106
apps/linux/host/src/logging/{mod,log_file}.rs  220
apps/linux/host/src/host/tests.rs       755(含既有断言)
apps/linux/{install.sh,pack.sh}         230
crates/qingjian-platform/src/config/{apps,general,mod}.rs
docs/notes/crate-notes.md
```

**方法**(照上一轮):不采信提交信息,整量精读 → 可疑点写临时探针 → 在**真代码/真数据/真模型**上跑出证据 → 逐条判真假。

**方法上的两点说明**:

1. **对照 macOS 壳逐行核了 4 处"对齐 macOS"的声明**(英文模式标点、译词快捷键吞键、组句吞键、模型 attach),4 处声明**都是真的**,注释没撒谎。
2. **读了 fcitx5 5.1.22 源码**核对框架契约:`eventloopinterface.h/cpp`、`event_sdevent.cpp`、`event_libuv.cpp`、`ui/classic/inputwindow.cpp`、`lib/fcitx/candidatelist.{h,cpp}`、`fcitx-utils/trackableobject.h`。

**基线**:`cargo test -p qingjian-linux-host --release` → **49 passed / 0 failed**;`cargo clippy -p qingjian-linux-host --release` → **0 告警**。

**复现命令**(探针需要真数据与真模型):

```bash
cd /vol1/1000/docker/qingjian
export RUSTUP_HOME=$PWD/.toolchain/rustup CARGO_HOME=$PWD/.toolchain/cargo PATH=$PWD/.toolchain/cargo/bin:$PATH
# 1) 把附录 A 的探针追加到 apps/linux/host/src/host/tests.rs
cat >> apps/linux/host/src/host/tests.rs <<'EOF'
... 附录 A ...
EOF
# 2) 跑(注意:libtest 只吃一个过滤词,--nocapture 让 println 出来)
cargo test -p qingjian-linux-host --release probe_ -- --nocapture --test-threads=1 2>&1 | grep -oE "PROBE_[A-I] .*"
# 3) 还原(探针会往 tests.rs 里加东西,必须还回去)
git checkout -- apps/linux/host/src/host/tests.rs
```

真数据位置:`data/generated/dict.qj`(3.5MB,92810 词条)、`data/model/model.qjm`(56MB);探针用临时目录 + symlink 装配,不碰仓库。

---

## 二、问题清单(按严重度)

### 【中】F1. 结束组句的键,松键会漏给应用(上下不对称)

**位置**:`apps/linux/host/src/host/keys.rs:99-102`

```rust
if release {
    // 组句期间吞掉普通键的松键,免得应用收到无头的 release;修饰键已在上面放行。
    return self.composing() && !(0xffe1..=0xffee).contains(&keyval);
}
```

**根因**:判松键用的是**当前**是否组句,而"上屏类"的键按下之后就结束组句了(`composing()` 变 false),于是松键被判为"不该吞"、放给了应用。注释想防的正是"无头 release",但漏了这个镜像情况——**无头 keydown 配上漏出去的 keyup**。

**证据(探针 P1/P8)**:

| 键(组句 "ni" 中) | 按下吞 | 松键吞 | 按下后还在组句 |
|---|---|---|---|
| 空格 | true | **false** | false |
| 数字 1 | true | **false** | false |
| 回车 | true | **false** | false |
| Esc | true | **false** | false |
| 空闲时的逗号(转全角上屏) | true | **false** | false |
| 字母 n | true | true | true |
| 退格 | true | true | true |
| 翻页 `]` | true | true | true |

**后果**:应用收到一次没有 keydown 的 keyup(空格/回车/数字/Esc)。多数工具忽略,但 X11 上按状态机读键的客户端(游戏、部分 Qt 控件)会误判。macOS 壳没有这个问题——IMK 根本不把 keyUp 送给输入法,所以这不是"macOS 同款"能解释的差异。

**建议修法**:松键的判据改成"这个键的**按下**有没有被吞",而不是"现在还在不在组句"。最小改法:加一个 `swallowed_press: Option<u32>`(`keys.rs` 里按下吞掉时记 keyval、松键比对后清空),或在 `Host` 上记一个"上屏后仍在等的松键"集合。回归测试:按下空格/回车/Esc/数字后,其松键也必须返回 true。

---

### 【中】F2. 多字节应用名 + 带 `*` 的名单项 → panic

**位置**:`crates/qingjian-platform/src/config/apps.rs:130-137`

```rust
fn matches_app(pattern: &str, app: &str) -> bool {
    let pattern = pattern.trim();
    match pattern.strip_suffix('*') {
        Some(prefix) => {
            app.len() >= prefix.len() && app[..prefix.len()].eq_ignore_ascii_case(prefix)
        }
        None => pattern.eq_ignore_ascii_case(app),
    }
}
```

`app[..prefix.len()]` 按**字节**切片,切点不落在 UTF-8 字符边界就 panic(`&str` 索引的固有行为)。

**证据(探针 P7)**:

```
PROBE_H 缺省名单条数=23
PROBE_H app="日本語入力テスト"    **panic**
PROBE_H app="日本語"             命中=false    ← 9 字节 < 前缀 10 字节,被长度守卫挡下
PROBE_H app="abc日本語"           **panic**
PROBE_H app="jetbrains-idea"     命中=true
```

**触发条件**:① 英文模式开着;② 按 a-z 字母(唯一调用点是 `keys.rs:114`);③ 当前应用 fcitx5 报的 `program()` 是 ≥10 字节、且第 10 字节落在字符中间的非 ASCII 名。Linux 缺省名单里 `jetbrains-*`(前缀 `jetbrains-`,10 字节)就是那个 `*` 项;名单越大越容易踩。

`fcitx5` 的 `InputContext::program()` 文档原话:**"It can be empty depending on the application."** —— 不保证 ASCII,也不保证有值(空串由 `matches_app` 的长度守卫挡下,安全)。

**后果**:`with_host` 的 `catch_unwind` 兜住了(不会崩 fcitx5),当次按键退化成透传——碰巧正是该场景本来想要的行为。**但每敲一个键刷一条 ERROR 日志**(`bridge.rs:27`),而且真出现别的 panic 时会被这堆噪音淹掉。

**归属**:`matches_app` 是平台层共享代码,不是本分支新增(macOS 名单本来也有 `com.jetbrains.*`),但本分支新加的 Linux 名单(`DEFAULT_ENGLISH_CANDIDATES_OFF_LINUX`)把它带到了 Linux 上。修在平台层最干净,顺手可以随提库那批一起走。

**建议修法**:`app.get(..prefix.len()).is_some_and(|head| head.eq_ignore_ascii_case(prefix))`。回归测试:`english_candidates_off("日本語入力テスト")` 不 panic 且返回 false。

---

### 【低-中】F3. 模型后台加载完成时,不补当前这一轮的整句重排

**位置**:`apps/linux/host/src/host/model.rs:59-76`(`attach_loaded_model`)、`session.rs:32`(`schedule_rescoring`)

**根因**:`Engine::rescoring_pending()`(`crates/qingjian-core/src/engine/rescoring/mod.rs:93`)是

```rust
self.rescorer.is_some() && self.neural_cache.borrow().has_wanted()
```

**没接上打分器时它恒为 false**,所以"加载窗口里打的那个词"不会排进防抖;等模型接上,`attach_loaded_model` 只把打分器装上,既不重排也不设截止时间,于是这一轮永久错过。

**证据(探针 P5,真词库 + 真模型)**:

```
PROBE_F 加载中打字: pending=false deadline=false 载入线程挂着=true
PROBE_F 模型接上耗时=45.57785ms
PROBE_F 接上后 120ms: poll=0b0 pending=false deadline=false 组句=true   ← 这一轮永不重排
PROBE_F 期间出现过的位={"0b0", "0b10"}                                   ← 从未出现重画位 0b01
```

作为对照,正常时序(P6,同样真模型)完全正确:

```
PROBE_E 模型接上耗时=45.499319ms
PROBE_E 敲完 nihao: 候选前3=["你好", "你好好", "你好像"] pending=true deadline=true
PROBE_E 重画=true 到位耗时=102.97435ms 位序列=[...0b10 每 2ms 一次..., 0b1@102.97ms]
PROBE_E 重排后 pending=false 组句=true
PROBE_E 重画后再 poll=0b0
```

`0b10` 持续 → 防抖 80ms → 模型 ~23ms → **103ms 时重画位 0b01 到位** → 之后 `0b0` 停表,与 `model.rs:93-95` 的注释逐字一致。

**后果**:fcitx5 启动后头几十毫秒打的那一个词拿不到神经重排(之后每次按键又正常),`[model] enabled` 热开时正在组的那句同理。用户只会觉得"第一个词偶尔不太聪明",很难报成 bug。

**归属**:**这是从 macOS 壳整段复制过来的**——`apps/macos/src/host/model.rs:58-60` 在 attach 成功后也只补了一条 `log_session`,同样不补重排。建议随提库那批一起说,别只修 Linux 一边(改法:attach 成功后若 `composing()` 就 `refresh()`,或直接 `rescore_deadline = Some(now + DEBOUNCE)`)。

---

### 【低】F4. 升级路径:模型只装不更,旧 `.tsv` 不清理

**位置**:`apps/linux/install.sh:71-76`、`:42-56`

```bash
for model_src in "$src_data/model.qjm" "${repo:-/nonexistent}/data/model/model.qjm"; do
    if [[ -f $model_src && ! -f "$qdata/model/model.qjm" ]]; then
        install -Dm644 "$model_src" "$qdata/model/model.qjm"
        break
    fi
done
```

`! -f "$qdata/model/model.qjm"` 这条守卫决定**模型永不被覆盖**。

**证据(本机实测 `~/.local/share/qingjian`)**:

- `model/model.qjm` 时间戳 `Sep 15 18:54`,而 `dict.qj`/`glossary-*.qj` 等都是 `19:29` —— 说明后续几次重跑安装脚本,模型一直停在首次安装的那份。
- `dict.qj` + `dict.tsv`、`glossary-en.qj` + `glossary-en.tsv`、`glossary-ja.qj/tsv`、`glossary-zh.qj/tsv` 全部并存,旧 `.tsv` 合计约 **27MB 死重**。

**后果**:① 升级分发包后,模型静默停留在旧版本,想更新只能手删;注释写的意图是"用户自己的 .qjm 优先",但脚本分不清"用户自己放的"和"上次装的"。② 27MB 只是磁盘浪费(**功能不受影响**,见"不是问题"第 12 条)。

**建议**:模型落成带版本/哈希的文件名(`model-<hash>.qjm`,`find_model` 取目录里排序第一个,天然兼容),或安装时比对 `data/model/SHA256SUMS`。

---

### 【低】F5. `qj_init` 顶部新加的日志初始化在 `catch_unwind` 之外

**位置**:`apps/linux/host/src/bridge.rs:59-62`

```rust
static LOG_GUARD: std::sync::OnceLock<Option<tracing_appender::non_blocking::WorkerGuard>> =
    std::sync::OnceLock::new();
LOG_GUARD.get_or_init(crate::logging::init);          // ← 在 catch_unwind 之前
...
let built = std::panic::catch_unwind(AssertUnwindSafe(|| Host::init(dir, None)));   // ← 防线在这里
```

第一轮特意给 `Host::init` 裹了 `catch_unwind`(防"损坏词库 panic 越过 FFI 边界 abort 整个 fcitx5"),但这行新代码在它**外面**。`logging::init` 里的

```rust
tracing_subscriber::registry().with(filter).with(fmt::layer()...).init();   // logging/mod.rs:41-48
```

在"全局 subscriber 已被设过"时会 panic;那一刻 panic 会直接越过 `extern "C" fn qj_init` → abort 整个 fcitx5,**正是第一轮要防的那一类**。

**可达性低**:进程内只有这个 staticlib 用 tracing,且 `OnceLock` 保证只初始化一次。但"防线有缺口"这件事与防线本身的意图不符,建议顺手包进同一个 `catch_unwind`(或让 `logging::init` 内部吞掉 init 失败)。

---

### 【低】F6. `logging` 没判 `XDG_STATE_HOME` 空串(与 `paths.rs` 不一致)

**位置**:`apps/linux/host/src/logging/mod.rs:74-79`

```rust
pub fn log_dir() -> Option<PathBuf> {
    if let Some(state) = std::env::var_os("XDG_STATE_HOME") {
        return Some(PathBuf::from(state).join("qingjian/logs"));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state/qingjian/logs"))
}
```

**同一个分支里的** `apps/linux/host/src/host/paths.rs:10-13`(`config_path`)与 `:22-25`(`data_dir`)都多判了一句 `&& !dir.is_empty()`。

**后果**:`XDG_STATE_HOME=""` 时日志目录变成**相对路径** `qingjian/logs`,写到 fcitx5 进程的 cwd(实测 `PathBuf::from("").join("qingjian/logs")` 就是相对路径)。只在极端环境变量配置下触发。

**建议修法**:加 `&& !state.is_empty()`,与 `paths.rs` 对齐。

---

### 【提示·需拍板】F7. 中英切换只有 Shift 轻点,Caps Lock 完全不参与

- **macOS**:`apps/macos/src/imk/controller.rs:381` `let english = modifiers::caps_lock_on();`、`:420` `set_english_mode(english_candidates && !question)` —— **Caps Lock 就是英文模式**,每个键事件都按它重设。
- **Linux**:`keys.rs:54-66` 只有 Shift 轻点切;Caps Lock(keysym 0xffe5)走 `keys.rs:310` 的修饰键臂直接透传,**不参与模式**。

从 macOS 过来的用户按 Caps Lock 会一点反应没有。这是漏接还是平台适配(Shift 轻点是刻意的 Linux 手感),**需要拍板**;本轮不当作 bug 报,只列出来防漂移。

---

## 三、核实后判"不是问题"的(防来回纠结)

1. **翻页键接线正确**:中文组句按 `]` 正确翻页(实测页 `0->1`),`page_keys=",."` 也生效。**上一轮那个 bug 真修掉了。**
2. **英文模式下 `]` 不翻页、反而把敲的字母原样上屏** —— 与 macOS 英文模式分支逐行同款(`controller.rs:437-448`:`commit_raw` + `note_passthrough` + `return false`),不是 bug。
3. **直输段里 `]` `,` 当字面字符** —— 与 match 臂顺序(直输段臂 `keys.rs:123` 在翻页臂 `:225` 之前)一致,`raw_segment_takes_page_keys_and_punctuation` 盯着,不是"翻页键在直输段失效"。
4. **译词快捷键无译文时吞键** —— 与 macOS `handle_translation_key` 同款(Some→insert,None→debug!,返回 true)。`session.rs:165` 那句"macOS 同款口径"是**真话**。
5. **键路由其余部分**:Tab/Home/F1 组句中按下+松键都吞;Ctrl/AltGr(0xfe03)/Num_Lock(0xff7f)/XF86 媒体键(0x1008ff13)/AltGr 打出的 é(0xe9)一律透传;空闲 Tab 透传。
6. **修饰键+数字 20 组矩阵全对**:无修饰选词、Alt=译词一、Alt+Shift=译词二、Shift=删候选、Ctrl 透传;Shift+9 的 `(` 正确映射回第 9 格。Alt+9 / Alt+( 落在空位时"吞掉不动作"(与删候选同口径,已裁决)。
7. **sd-event 一次性定时器"在回调里重新武装"成立** —— 本机 fcitx5 用 sd-event(`libFcitx5Utils.so` 只含 `sd-event`、链 libsystemd、无 libuv 依赖)。我写了个 C 探针复刻 shim 的用法(`set_time(now+2ms)` + `sd_event_source_set_enabled(SD_EVENT_ONESHOT)`),**连续触发 3 次** ✓;`return true` 在 fcitx5 的包装里映射成 `0`(`event_sdevent.cpp:296-308`),不会被禁用 ✓。**本轮最该验的假设,验成了"成立"。**
8. **位契约与轮询停表**:防抖窗口内持续 `0b10`、重画那次带 `0b01`、之后 `0b0` 停表,与 `model.rs:93-95` 的注释逐字一致(实测数据见 F3 的 PROBE_E)。
9. **防抖重推**:敲下一个键截止时间后推(实测 40ms 后敲键,截止后推 40ms)。
10. **`lastIc_` 生命周期安全**:`InputContext : public TrackableObject<InputContext>`(`/usr/include/Fcitx5/Core/fcitx/inputcontext.h:50`),`watch()` 给弱引用,`TrackableObjectReference::get()` 在对象析构后返回 nullptr(`fcitx-utils/trackableobject.h`),不悬垂。
11. **安装覆盖 `.so` 不会打死正在跑的 fcitx5**:实测 coreutils `install` 是**换 inode**(`unlink` + 新建),映射中的旧文件不受影响(我原本怀疑这条会触发 SIGBUS,验完不是)。
12. **老 `.tsv` + 新 `.qj` 并存**:实测装载的是 `.qj`(92810 词条),不是 1.9MB 的 `.tsv` —— 同目录 `.qj` 优先这条兜住了升级面(`paths.rs` 的 `find_data` 先查 `qj` 再查 `tsv`)。
13. **preedit 三档位置**:`行内+窗口 / 只行内 / 只窗口` 的映射与"应用不支持行内就退窗口"的回退都在 `addon.cpp:118-127`,与 `PreeditMode` 对得上。
14. **apps 名单匹配语义**:大小写不敏感、`*` 是前缀匹配(`konsole`/`ORG.KDE.Konsole`/`jetbrains-idea` 命中,空串不命中)。名单是 X11 形态与 Wayland 形态混编的,**实际 app_id 需要按机器逐个校准**(我没有在真机逐个验证哪些应用会漏,所以不列为 bug,只作提醒)。

**只记不动两条**:① 第一轮报过的"候选在自己的 `select()` 调用栈里被释放"仍在(`addon.cpp:211-216` 里 `sync()` → `panel.reset()`),当前 fcitx5 的 `InputWindow::click` 在 `select` 返回后立刻 `break`、不再碰列表,未观察到崩溃,仍建议加固;② 本机数据目录里 `user.tsv` 缺席不是缺陷迹象——学习确实在落盘(`user-choices.tsv`/`user-ngram.tsv`/`user-english.tsv` 三个 sidecar 都是新的),`user.tsv` 是基础频次表,`frequency_learner/learner_impl.rs:209` 有 `dirty` 守卫。

---

## 四、未覆盖面(诚实交代)

1. **没有在真 fcitx5 里做交互式测试**:候选鼠标点选、滚轮、`activate/deactivate` 的真实回调顺序,都是靠源码 + API 语义判断的,没做运行时实验。
2. **没审引擎内部**:只审"壳怎么用引擎",引擎自己的排序/学习/重排逻辑不在本轮范围。
3. **`lastIc_` 的"IC 中途销毁"路径**是靠弱引用语义推的,没构造运行时实验去触它。
4. **apps 缺省名单**没有在真机逐个核对实际 `program()` 值(见"不是问题"第 14 条)。

---

## 附录 A:探针代码(可直接粘回去重跑)

追加到 `apps/linux/host/src/host/tests.rs` 末尾即可(跑完 `git checkout --` 还原):

```rust
// ===== 第二轮巡检临时探针 =====
fn probe_data_dir(tag: &str, with_model: bool) -> PathBuf {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let dir = std::env::temp_dir().join(format!("qj-probe-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("model")).unwrap();
    let sample = repo.join("assets/sample");
    for f in ["english.tsv", "glossary-en.tsv"] {
        std::fs::copy(sample.join(f), dir.join(f)).unwrap();
    }
    let real = repo.join("data/generated/dict.qj");
    if real.is_file() {
        std::os::unix::fs::symlink(&real, dir.join("dict.qj")).unwrap();
    } else {
        std::fs::copy(sample.join("dict.tsv"), dir.join("dict.tsv")).unwrap();
    }
    if with_model {
        let model = repo.join("data/model/model.qjm");
        if model.is_file() {
            std::os::unix::fs::symlink(&model, dir.join("model/model.qjm")).unwrap();
        }
    }
    dir
}

// P1:松键对称性(→ F1)
#[test]
fn probe_release_symmetry() {
    for (name, keyval) in [("space", 0x20u32), ("digit1", 0x31), ("enter", 0xff0d), ("esc", 0xff1b), ("comma", 0x2c)] {
        let mut h = sample_host();
        type_str(&mut h, "ni");
        let press = h.key(keyval, 0, false);
        let after = h.composing();
        let release = h.key(keyval, 0, true);
        println!("PROBE_A {name:<7} 按下吞={press} 松键吞={release} 按下后组句={after}");
    }
    let mut h = sample_host();
    type_str(&mut h, "ni");
    println!("PROBE_A 字母n   松键吞={}", h.key(0x6e, 0, true));
    let mut h = sample_host();
    type_str(&mut h, "ni");
    let p = h.key(0x61, 1 << 2, false);
    let r = h.key(0x61, 1 << 2, true);
    println!("PROBE_A Ctrl+a  按下吞={p} 松键吞={r}");
    let mut h = sample_host();
    type_str(&mut h, "s");
    let p = h.key(0x5d, 0, false);
    let r = h.key(0x5d, 0, true);
    println!("PROBE_A 翻页]   按下吞={p} 松键吞={r} 仍在组句={}", h.composing());
}

// P8:松键漏出的范围(→ F1)
#[test]
fn probe_release_leak_scope() {
    let mut h = sample_host();
    let p = h.key(0x2c, 0, false);
    let r = h.key(0x2c, 0, true);
    println!("PROBE_I 空闲逗号 按下吞={p} 松键吞={r} 上屏={:?}", h.pending_commit.take());
    let mut h = sample_host();
    let p = h.key(0x61, 0, false);
    let r = h.key(0x61, 0, true);
    println!("PROBE_I 空闲字母a 按下吞={p} 松键吞={r}");
    let mut h = sample_host();
    type_str(&mut h, "ni");
    let p = h.key(0xff08, 0, false);
    let r = h.key(0xff08, 0, true);
    println!("PROBE_I 退格 按下吞={p} 松键吞={r} 仍在组句={}", h.composing());
}

// P7:多字节应用名(→ F2)
#[test]
fn probe_apps_matching_utf8() {
    let apps = qingjian_platform::AppsConfig::default();
    println!("PROBE_H 缺省名单条数={}", apps.english_candidates_off.len());
    println!("PROBE_H konsole 命中={}", apps.english_candidates_off("konsole"));
    println!("PROBE_H 空串命中={}", apps.english_candidates_off(""));
    for app in ["日本語入力テスト", "日本語", "abc日本語", "jetbrains-idea"] {
        let r = std::panic::catch_unwind(|| apps.english_candidates_off(app));
        match r {
            Ok(v) => println!("PROBE_H app={app:?} 命中={v}"),
            Err(_) => println!("PROBE_H app={app:?} **panic**"),
        }
    }
}

// P5:模型加载中的那一轮(→ F3)
#[test]
fn probe_model_late_load() {
    let dir = probe_data_dir("lateload", true);
    let mut h = Host::init(dir.clone(), Some(qingjian_platform::Config::default()))
        .expect("真词库应能装配");
    type_str(&mut h, "nihao");
    println!(
        "PROBE_F 加载中打字: pending={} deadline={:?} 载入线程挂着={}",
        h.engine.rescoring_pending(), h.rescore_deadline.is_some(), h.model_loader.is_some()
    );
    let t0 = std::time::Instant::now();
    let mut bits = Vec::new();
    while t0.elapsed() < std::time::Duration::from_secs(30) {
        let p = h.model_poll();
        bits.push(p);
        if h.engine.has_sentence_scorer() {
            println!("PROBE_F 模型接上耗时={:?}", t0.elapsed());
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    std::thread::sleep(std::time::Duration::from_millis(120));
    let p = h.model_poll();
    println!(
        "PROBE_F 接上后 120ms: poll={p:#b} pending={} deadline={:?} 组句={}",
        h.engine.rescoring_pending(), h.rescore_deadline.is_some(), h.composing()
    );
    println!("PROBE_F 期间出现过的位={:?}",
        bits.iter().map(|b| format!("{b:#b}")).collect::<std::collections::BTreeSet<_>>());
}

// P6:正常时序对照(→ F3 的反面)
#[test]
fn probe_model_real_load_and_rescore() {
    let dir = probe_data_dir("model", true);
    let mut h = Host::init(dir.clone(), Some(qingjian_platform::Config::default())).expect("真词库应能装配");
    let t0 = std::time::Instant::now();
    while t0.elapsed() < std::time::Duration::from_secs(30) && !h.engine.has_sentence_scorer() {
        h.model_poll();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    type_str(&mut h, "nihao");
    println!(
        "PROBE_E 敲完 nihao: 候选前3={:?} pending={} deadline={:?} 组句={}",
        (0..h.layout.len().min(3)).filter_map(|i| h.layout.candidate(i).map(|c| c.text.clone())).collect::<Vec<_>>(),
        h.engine.rescoring_pending(), h.rescore_deadline.is_some(), h.composing()
    );
    let t1 = std::time::Instant::now();
    let mut seen = Vec::new();
    let mut repainted = false;
    while t1.elapsed() < std::time::Duration::from_secs(5) {
        let p = h.model_poll();
        if p != 0 { seen.push(format!("{p:#b}@{:?}", t1.elapsed())); }
        if p & 1 != 0 { repainted = true; break; }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    println!("PROBE_E 重画={repainted} 到位耗时={:?} 位序列首尾={:?}/{:?}", t1.elapsed(), seen.first(), seen.last());
    println!("PROBE_E 重画后再 poll={:#b}", h.model_poll());
}

// P2/P3:翻页键按模式 + 修饰键×数字矩阵(→ 不是问题 1/2/6)
#[test]
fn probe_page_keys_by_mode() {
    let mut h = sample_host();
    type_str(&mut h, "s");
    let before = h.page;
    let sw = h.key(0x5d, 0, false);
    println!("PROBE_B 中文组句  ] 吞={sw} 页 {before}->{} 上屏={:?}", h.page, h.pending_commit.take());
    let mut h = sample_host();
    h.key(0xffe1, 0, false);
    h.key(0xffe1, 1, true);
    type_str(&mut h, "shi");
    let before = h.page;
    let sw = h.key(0x5d, 0, false);
    println!("PROBE_B 英文模式  ] 吞={sw} 页 {before}->{} 上屏={:?} 组句={}", h.page, h.pending_commit.take(), h.composing());
    let mut h = sample_host();
    type_str(&mut h, "ni");
    h.key(0x2d, 0, false);
    let sw = h.key(0x5d, 0, false);
    println!("PROBE_B 直输段    ] 吞={sw} 组句={}", h.composing());
    let mut h = sample_host();
    type_str(&mut h, "v");
    type_str(&mut h, "1");
    let sw = h.key(0x5d, 0, false);
    println!("PROBE_B 表达式    ] 吞={sw} 组句={}", h.composing());
    let mut h = sample_host();
    type_str(&mut h, "u");
    let sw = h.key(0x5d, 0, false);
    println!("PROBE_B 问字      ] 吞={sw} 组句={}", h.composing());
}

#[test]
fn probe_modifier_digit_matrix() {
    let combos: [(&str, u32); 5] = [
        ("无修饰", 0), ("Shift", 1 << 0), ("Alt", 1 << 3),
        ("Alt+Shift", (1 << 3) | (1 << 0)), ("Ctrl", 1 << 2),
    ];
    for (cname, state) in combos {
        for (kname, keyval) in [("1", 0x31u32), ("!", 0x21), ("9", 0x39), ("(", 0x28)] {
            let mut h = sample_host();
            type_str(&mut h, "ni");
            let sw = h.key(keyval, state, false);
            println!("PROBE_C {cname:<8} {kname:<2} 吞={sw} 上屏={:?} 组句={}",
                h.pending_commit.take(), h.composing());
        }
    }
}

// P4:老 tsv 与新 qj 并存(→ 不是问题 12)
#[test]
fn probe_real_dict_and_upgrade_precedence() {
    let dir = probe_data_dir("prec", false);
    let sample = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../assets/sample");
    std::fs::copy(sample.join("dict.tsv"), dir.join("dict.tsv")).unwrap();
    let h = Host::init(dir, Some(qingjian_platform::Config::default())).unwrap();
    println!("PROBE_D dict.tsv 与 dict.qj 并存 → 装载词条数={}", h.engine.dictionary().len());
}

// P9:防抖重推(→ 不是问题 9)
#[test]
fn probe_model_debounce_resets_on_typing() {
    let dir = probe_data_dir("debounce", true);
    let mut h = Host::init(dir.clone(), Some(qingjian_platform::Config::default())).expect("真词库应能装配");
    let t0 = std::time::Instant::now();
    while t0.elapsed() < std::time::Duration::from_secs(30) && !h.engine.has_sentence_scorer() {
        h.model_poll();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    type_str(&mut h, "nihao");
    let d1 = h.rescore_deadline;
    std::thread::sleep(std::time::Duration::from_millis(40));
    h.key(0x67, 0, false);
    let d2 = h.rescore_deadline;
    println!("PROBE_G 首次截止={d1:?} 敲键后截止={d2:?} 是否后推={} pending={}", d2 > d1, h.engine.rescoring_pending());
}
```

**sd-event 重新武装的 C 探针**(→ 不是问题 7,不进 Rust 测试套件):

```c
// cc -o sdprobe sdprobe.c $(pkg-config --cflags --libs libsystemd)
#include <systemd/sd-event.h>
#include <stdio.h>
#include <time.h>
#include <stdint.h>
static uint64_t now_us(void){struct timespec ts;clock_gettime(CLOCK_MONOTONIC,&ts);return (uint64_t)ts.tv_sec*1000000ull+ts.tv_nsec/1000;}
static int fired=0, rearm=0;
static int cb(sd_event_source *s, uint64_t usec, void *ud){
    (void)usec;(void)ud; fired++;
    if(rearm && fired<6){ sd_event_source_set_time(s, now_us()+2000); sd_event_source_set_enabled(s, SD_EVENT_ONESHOT); }
    return 0;
}
int main(int argc,char**argv){
    rearm=(argc>1);
    sd_event *e=NULL; sd_event_default(&e);
    sd_event_source *src=NULL;
    sd_event_add_time(e,&src,CLOCK_MONOTONIC,now_us()+2000,0,cb,NULL);
    sd_event_source_set_enabled(src,SD_EVENT_ONESHOT);
    int steps=0;
    while(fired<3 && steps<40){ int n=sd_event_run(e, now_us()+3000); steps++; if(n<0) break; }
    printf("rearm=%d fired=%d run次数=%d\n",rearm,fired,steps);
    sd_event_unref(e); return 0;
}
// 不重新武装:fired=1(链停);带 setOneShot 重新武装:fired=3(链续)← shim 的写法
```

---

## 处置记录(2026-09-15,甲方拍板后由 leader 落实)

| 发现 | 裁决 | 落点 |
|---|---|---|
| F1 松键漏无头 keyup | 修 | `swallowed_presses`:松键按「按下吞过没有」对称吞(keys.rs 重构 press/release 两路)+ 4 组回归测试 |
| F2 多字节应用名 panic | 修(平台层) | `matches_app` 改 `str::get` 字符边界安全比较 + 回归测试 |
| F3 模型接上不补当前轮 | 修 Linux 侧;macOS 同款缺口提上游 issue,随提库批次修 | `model_poll` 里刚接上且组句中即补查(带未翻页/未动高亮克制守卫)+ 真模型条件测试 |
| F4 模型只装不更、旧 tsv 死重 | 重设计:随包数据分层(甲方拍「参考官方」= fcitx5 StandardPaths 用户层盖随包层) | `dist/` 随包层(安装器独占、装机整个换新),用户层同名文件/`model/*.qjm` 盖过它;旧布局按「与来源逐字节一致则删」迁移(沙箱实测:迁移/幂等/用户覆盖件三场景过) |
| F5 日志初始化在防线外 | 修 | `logging::init` 改 `try_init`,重复初始化不再 panic 越 FFI |
| F6 XDG_STATE_HOME 空串 | 修 | 空串当没设,与 paths.rs 对齐 |
| F7 Caps Lock 不参与切换 | **不做,维持现状**(甲方拍) | 拍板记录入 `docs/design/linux-fcitx5.md` |

挂账不动(维持报告原判):候选 `select()` 调用栈内释放(建议加固,未观察到崩溃);apps 缺省名单真机 app_id 校准。
