# 青简 Linux 壳 第三轮独立巡检报告

**本文地位**:这是一份**发现记录**,不是定稿设计。它只记录"哪行代码在什么条件下会出什么问题",修法与优先级以甲方裁决为准;与 `docs/design/linux-fcitx5.md`(定稿设计)冲突时,设计稿说了算,本文只提供事实。**所有结论都带复现证据**,探针代码见附录 A,可直接粘回去重跑;install.sh 全部在沙箱(假 HOME + 假 XDG_DATA_HOME + fcitx5 空桩)里跑,未触碰真实用户目录与正在运行的 fcitx5。

**一句话结论**:第二轮的 F1/F2/F3/F5/F6 **确认已修掉**(全部实测复验),F4 的 dist 分层重设计方向正确、正常路径(全新装/同版重装/幂等/用户覆盖件)全过;但**迁移逻辑逮到 1 处中等缺陷**(数据资产升过级的旧布局永久盖住 dist,恰是 F4 想根治的病)与 2 处低危(legacy dicts/ 迁移后成死区、rm -rf dist 先删后验的撕裂窗口),键路由重构逮到 1 处低危 keysym 漂移(Shift+数字快捷键的松键既漏又误吞)+ 1 条设计备注;另有 **19 处核实后判"不是问题"**,逐条列出防来回纠结。

巡检时间:2026-09-15。巡检范围:`git diff 612b363..HEAD` 全部增量(5 个提交:c286c12 巡检二报告、a7738f6 F2、57b6233 F1/F3/F5/F6、3684833 F4 分层、dec1ac4 拍板文档;共 860 行增/46 行删)。巡检用的探针跑完已全部还原(`git checkout -- apps/linux/host/src/host/tests.rs`),仓库里只多本报告一份(未提交)。

---

## 一、范围与方法

**精读的增量与牵连上下文**:

```
apps/linux/host/src/host/keys.rs          全文 360 行(重构后)
apps/linux/host/src/host/mod.rs           全文(swallowed_presses 字段与装配)
apps/linux/host/src/host/model.rs         全文(F3 补查)
apps/linux/host/src/host/paths.rs         全文(data_layers/find_data/find_file)
apps/linux/host/src/host/session.rs       全文(refresh/note_displayed 与补查的交互)
apps/linux/host/src/host/config_watch.rs  全文(dist/dicts 换点)
apps/linux/host/src/logging/mod.rs        全文(try_init + XDG_STATE_HOME)
apps/linux/host/src/bridge.rs             全文(FFI 合同)
apps/linux/fcitx5-shim/addon.cpp          全文(keyEvent→sync 顺序合同、model timer)
apps/linux/install.sh + pack.sh           全文(分层、迁移、分发/开发两条路)
crates/qingjian-platform/src/config/apps.rs(F2 修复与测试)
apps/linux/host/src/host/tests.rs         新增 145 行回归测试
docs/{design/linux-fcitx5.md, notes/crate-notes.md, review/…round2…}
```

**方法**(照前两轮):不采信提交信息与注释,整量精读 → 可疑点写临时探针在真代码/真数据(data/generated/dict.qj 3.5MB、data/model/model.qjm 56MB,临时目录 + symlink 装配)上跑出证据 → 逐条判真假。install.sh 用假分发 payload(v1/v2 两版数据模拟资产升级)在沙箱里跑了 5 个场景。

**基线**(修复后代码):
- `cargo test -p qingjian-linux-host --release` → **54 passed / 0 failed**(含 4 组 F1 回归、分层查找、dist 单独装配;真数据条件测试 `model_attached_mid_composition_rescores_that_round` 实跑通过,0.15s,与「attach ~45ms + 防抖 80ms + 打分 ~25ms」时序吻合)。
- `cargo test -p qingjian-platform --release` → **26 passed / 0 failed**(含 F2 回归 `prefix_patterns_survive_multibyte_app_names`)。
- `cargo clippy -p qingjian-linux-host -p qingjian-platform --all-targets --release -- -D warnings` → **0 告警**。

**第二轮五处修复的复验结论**(用附录 A 探针 + 随修提交的回归测试):

| 巡检二发现 | 复验结论 | 证据 |
|---|---|---|
| F1 松键漏无头 keyup | **已修**。空格/回车/Esc/数字/空闲逗号的松键全部对称吞;rollover、无对应按下的二次松键透传 | 随修回归测试 3 组全过 + 探针 R3A/R3D |
| F2 多字节应用名 panic | **已修**。`str::get` 边界安全;巡检二 P7 的三个 panic 输入全部收敛为不匹配,`jetbrains-idea` 仍命中 | 平台层回归测试过 |
| F3 模型接上不补当前轮 | **已修**。真模型实测:加载窗口里敲 `nihao`,attach@51ms 当次 poll 补查(pending=true、deadline 立起、since 不抢跑),repaint 位 @147ms 到位 | 随修条件测试 + 探针 R3G |
| F5 logging init 二次 panic | **已修**。同进程二次 `init()` 返回 None、不 panic | 探针 R3H/R3I |
| F6 XDG_STATE_HOME 空串 | **已修**。空串回退 `~/.local/state`,与 paths.rs 判法一致 | 探针 R3H |

---

## 二、问题清单(按严重度)

### 【中】R3-1. 迁移判据漏掉「数据资产已升级」的旧布局:旧随包件永久盖住 dist

**位置**:`apps/linux/install.sh:37-49`(`migrate_legacy`)、`:62`、`:89-92`(dicts)、`:104-108`(model)

迁移判据是「与**本次安装的来源**逐字节一致才删」。但旧安装器装下的文件对应的是**当时**的数据资产;只要数据资产在「旧布局最后一次安装」与「新安装器第一次运行」之间更新过(gh release `data` 资产升版、assets 样例改动),cmp 必然不一致 → 被判成「用户自有」永久保留在用户层 → **按分层语义永远盖住 dist 里的新数据**。这恰好复活了 F4 想根治的病(数据永不更新),而且不自愈:以后每次安装都拿更新的来源去比,旧文件永远比不上。

**证据(沙箱场景 3,v1 数据旧布局 + v2 payload 升级)**:

```
--- 用户层残留(应全被迁走才对,实际?):
dict.qj  dicts/01_idiom.qj  glossary-en.tsv  model/model.qjm     ← 四件全留下
--- dist 内容与根遮蔽对照:
dict.qj:        根=dict-v1      dist=dict-v2      ← host 读到的是 v1
glossary-en.tsv: 根=gloss-en-v1  dist=gloss-en-v2
model:          根=model-v1     dist=model-v2
```

对照场景 2(同版数据重装):迁移干净、用户自有覆盖件(`glossary-zh.tsv` 改过内容)正确保留、学习数据不碰——正常路径没毛病,缺陷只在「跨版本」这一条。

**现实面**:Linux 未发版,旧布局只存在于开发机(甲方本机);若在数据资产下一次升版**之前**跑一次新安装器,迁移即干净,病灶不落地。但这段迁移代码会随安装器活很久,谁的机器在窗口外跑第一次就中招,且症状(词库/模型旧)几乎不可能被用户报出来。

**建议修法**(任选其一,由轻到重):
1. 最低限度:迁移循环后检查用户层还剩哪些与 put_data 清单同名的文件,`echo` 出「以下文件将盖过随包数据:…(如非你自己放置,请删除)」——把静默变成可见。
2. 判据换向:随包件在 `dist/` 里留一份来源清单(文件名+SHA256);下次安装时与**上一次 dist 记录的哈希**一致的根文件也算「安装器装的」。首次迁移(没有记录)退回现判据。
3. 一次性迁移:旧布局文件一律挪到 `legacy-backup/`(不删),host 不读该目录;用户真有自有件自己挪回根。

### 【低】R3-2. legacy `dicts/` 里迁不走的文件成「死区」:用户自放的领域词库静默失效

**位置**:`apps/linux/install.sh:82-97` + `apps/linux/host/src/host/config_watch.rs:43-44`

host 现在只读 `dist/dicts`(随包层)与 `user-dicts`(用户层),**`$qdata/dicts` 不再被任何代码读取**(全仓 grep 证实)。迁移只删「与来源逐字节一致」的,于是两类文件会留在 `dicts/` 里再无人问津:① R3-1 的跨版本旧随包件(死重,尚无功能损失,dist 有新版);② **用户按旧文档自己放进 `dicts/` 的词库**——旧布局里它确实生效,升级后静默失效,连日志都不会有一行。其他数据文件(dict/glossary/model)留在用户层至少还被当覆盖件读到,唯独 `dicts/` 的「用户层」换了名字(user-dicts),留守文件彻底不可达。场景 3 实测 `dicts/01_idiom.qj`(v1)留守。

**建议修法**:迁移时 cmp 不一致的 `dicts/*.qj` 挪进 `user-dicts/`(语义正确:用户自有词库本来就该在那),或至少 echo 警告。

### 【低】R3-3. `rm -rf dist` 先删后验:坏包一次跑把上一版随包层全删光

**位置**:`apps/linux/install.sh:36`(rm)对 `:79-80`(词库守卫)

`rm -rf "${qdata:?}/dist"` 在任何来源校验之前执行。payload 缺 `data/`(打包机没取回数据资产、包被裁剪/损坏)时:.so 已被换新(`:27`),dist 整层已删光(含 56MB 模型),put_data 一无所装,词库守卫 exit 1 —— 用户被留在「新 .so + 零随包数据 + 未重启」的撕裂态;正在跑的 fcitx5 靠 mmap 撑着,下次重启即装配失败。

**证据(沙箱场景 4)**:v2 全新装后 dist 6 个文件;跑缺数据 payload → `exit=1`,`dist 存在=no`,`.so` 已换成新包的。

**现实面**:pack.sh 的 put_first 兜底 assets 样例,正常产出的包必有 data/,触发要坏包;另外即使正常安装,rm 与重装之间也有个短窗口(新起的 fcitx5 会装配失败),但正常路径半秒级、可忽略。

**建议修法**:装到 `dist.new/` 再 `rm -rf dist && mv`,或 rm 之前先确认词库来源在场(把 `:79` 的守卫提前成对来源的检查)。

### 【低】R3-4. Shift+数字快捷键的 keysym 漂移:松键既漏无头 keyup、又留陈账误吞

**位置**:`apps/linux/host/src/host/keys.rs:70-85`(记账按 keysym)+ `:32-45`(shifted_digit_offset 的存在本身说明按下 keysym 是符号)

X 的 keysym 由**事件当时**的修饰状态决定。删候选快捷键(缺省 Shift+数字)按下时到达的是符号 keysym(`!`=0x21),记账记 0x21;若用户先松 Shift 再松数字(常见的松手顺序),松键到达的 keysym 是 0x31(`1`)——账上没有 → **透传**,应用收到无头 `1` keyup;同时 0x21 留在账上,**下一次没有对应按下的 0x21 松键会被误吞**(例如之后在别处 Shift+1 打 `!` 且松键顺序相反时)。译词快捷键(Alt+数字)不受影响:Alt 不改数字 keysym,实测对称。

**证据(探针 R3B,组句 "ni" 中)**:

```
PROBE_R3B Shift+1(!)按下吞=true
PROBE_R3B 松键 keysym=0x31(Shift 已松)吞=false   ← 漏无头 keyup
PROBE_R3B 松键 keysym=0x21(Shift 未松)吞=true    ← 这里演示的是陈账被(此处恰好正确地)消掉;
                                                     若这次 0x21 松键属于透传过的按下,就是误吞
PROBE_R3A Alt+1 按下吞=true;1 松键(Alt 仍按)吞=true  ← Alt 路径对称,无此问题
```

**后果与频度**:仅删候选/Shift 组合快捷键 + 特定松手顺序才触发,单次后果与 F1 同类(无头 keyup)但一次一发不成对;误吞需要更巧的序列。比 F1 低一到两档。

**根治方向**:按 **keycode** 记账(fcitx5 的 `Key` 有 `code()`,shim 现只传 `sym()`+`states()`,要动 `qingjian.h` 合同加一个参数);过渡方案是松键查账时把 shifted/unshifted 变体(`shifted_digit_offset` 的映射反查)一并算命中。

### 【备注】R3-5. `reset()` 不清 `swallowed_presses`:方向正确,留一个极端序列

**位置**:`apps/linux/host/src/host/keys.rs:350-359`(reset 清 shift_armed/navigated,不清账本)

实测(探针 R3C):组句中切焦点(reset)后,先前被吞按键的松键在新窗口仍被吞——这**正是想要的**(新窗口的应用没见过按下,放行就是无头 keyup),不清是对的。留的极端序列:按键被吞后其松键**没有**经过本引擎(用户切到别的输入法/键盘引擎期间松开),陈账留存;之后同 keysym 的按下会自愈(吞→dedup,透传→retain 清账),**唯一**坏序列是「按住该键时切回本输入法」——松键带着陈账被误吞,应用(经别的引擎)见过按下、收不到松开,卡键。需要跨输入法按住同一个曾欠账的键,概率极低,且下一次该键按下即自愈。挂备注不开单;若要保险,`reset()` 里只在**输入法自身被停用**(而非切窗)时清账——但 shim 现在两个入口(deactivate/reset)不可区分,代价大于收益。

---

## 三、核实后判「不是问题」的(防来回纠结,逐条留痕)

1. **F1 修复本体**:上屏类键(空格/回车/Esc/数字)、空闲全角逗号、rollover 字母、无对应按下的二次松键、Ctrl+组合透传键——全部对称(随修回归测试 + 探针 R3A/R3D)。
2. **swallowed_presses 无界增长**:按下吞 → `contains` 去重;按下透传 → `retain` 清同 keysym 陈账。上限 = 同时欠着松键的不同 keysym 数(实际 ≤ 键盘 rollover),不会积累。
3. **auto-repeat**:press,press,release 序列账上只一条,首个 release 消账,再来的孤 release 透传(探针 R3D)。
4. **松键路径与 shim 的 pending_commit 顺序合同**:release 分支在 `press()` 之前返回,永不产生上屏文本(Shift 轻点的 release 走 refresh,也不产 commit);addon.cpp 对未吞键「先 sync 再放行」的合同不受影响。
5. **修饰键快捷键 Alt+数字的松键**:同 keysym,对称吞(探针 R3A)。keysym 漂移只在 Shift 组合上(见 R3-4)。
6. **F2 修复本体**:`app.get(..prefix.len())` 切不到字符边界即不匹配;既有 ASCII 前缀命中不变;`*` 单星全匹配是存量行为未动。
7. **F3 修复本体**:attach 当次 poll 内 refresh → schedule_rescoring 只立 deadline(80ms 防抖),**不会**在同一次 poll 里抢跑 request(探针 R3G:attach 时 `since=false`),时序与正常轮完全同构,repaint @147ms。
8. **attach 的双查/递归**:`refresh()` 开头的 `attach_loaded_model()` 在 model_poll 已消费 loader 后是空转(`model_loader=None` 直接返回),无递归无双查。
9. **navigated/page>0 时不补查**:克制守卫与 poll_rescoring 同款;实测该轮 poll 归 0、定时器停、不空转,下一键 deadline/pending 恢复(探针 R3E/R3F)。该轮放弃重排是拍过的取舍。
10. **补查 refresh 不带 bit0(不重画)**:此刻神经分尚无缓存,重查输出与上一次确定性一致(探针 R3G:前 3 不变),真正排序变化由随后的 poll_rescoring 带 bit0 重画;面板与 Host 状态不会失配。
11. **note_displayed 重复计数**:补查 refresh 多记一次「本页展示」,与每次按键 refresh、翻页、poll_rescoring 重画的既有语义同构(macOS render 亦然),词汇记录的「见过」本来就按展示次数计,不算问题。
12. **logging try_init 失败路径**:返回 None 时 guard/FILTER 句柄确实丢弃,但该分支意味着进程里已有别的全局 subscriber(qingjian 不接管日志),set_level 静默 no-op 是正确降级;bridge 的 OnceLock 不重试也无害。
13. **install.sh 的 set -e 短路**:`migrate_legacy` 尾部是 for+if 结构,cmp 全不中时 if(无 else)恒返回 0,不会炸掉调用链;沙箱 5 场景 exit 码全符合预期(成功 0 / 缺词库 1)。
14. **`rm -rf "${qdata:?}/dist"` 的目标安全**:qdata 恒以 `/qingjian` 结尾;`set -u` 下 HOME 缺失在更早的 `lib_dir` 处就炸;`${XDG_DATA_HOME:-…}` 空串回退与 Rust 侧 `!is_empty()` 判法一致。
15. **词库缺失守卫新判据**:dist 或用户层根任一有 dict 即过——合理:用户层可独立支撑装配(新增测试 `dist_layer_alone_assembles_host` 证反向也行);场景 2/3 均未误炸。
16. **`.tsv 已有同名 dist qj 就不装`的守卫**:put_data 顺序 qj 先于 tsv,守卫成立;glossary-es.tsv(无 qj 变体)不受影响;migrate 仍照跑,同版旧 tsv 死重被清掉(27MB 那批,同版场景实测)。
17. **分层查找无绕过点**:全仓 grep,`user.tsv`/`usage.tsv`/`user-vocab.tsv`/`input-log.jsonl`/`user-dicts` 直连根是设计如此(学习/用户数据);model.rs 手动两层、config_watch `dist/dicts`+`user-dicts` 即两层;再无第三处 join 数据目录。
18. **用户层 tsv 盖过 dist qj**:「层序先于扩展名序」是设计本意(用户放 tsv 也能覆盖),测试有断言,文档已写。
19. **docs/user 未随 F4 改**:Linux 未发版,`docs/user` 尚无 Linux 页面(数据文件页只列 macOS/Windows),不构成「同一提交改用户文档」的违约;Linux 发版前需补一页(含 dist 分层与卸载路径),挂到 todo 即可。

---

## 四、未覆盖面(如实交代)

- **真机 fcitx5 端到端**:按委托纪律未重启真 fcitx5、未碰真实 `~/.local/share/qingjian`,键路由与迁移全在 Host 单元层/沙箱验证;X11/Wayland 真实事件序列(尤其 keysym 漂移在真实合成器下的到达形态)未实测。
- **甲方本机的旧布局实况**:R3-1 是否已在本机落地(root 下 model.qjm 与当前 data/model/model.qjm 是否 cmp 一致)未验——那要读真实用户目录,留给甲方一条命令自查:`cmp ~/.local/share/qingjian/model/model.qjm /vol1/1000/docker/qingjian/data/model/model.qjm && echo 同版可迁 || echo 已漂移`(下次跑 install.sh 前看一眼)。
- **56MB model 的 cmp 成本**:迁移期每次安装多读两遍大文件,秒级,未计时。
- **全 workspace clippy/test**:macOS 专属 crate(objc2)在本机编译失败是环境固有,只跑了 linux-host 与 platform 两个 crate。
- **renderer-spike 分支、apps/cli**:不在本轮范围。

---

## 附录 A:探针代码(追加到 `apps/linux/host/src/host/tests.rs`,跑完 `git checkout --` 还原)

```bash
export RUSTUP_HOME=$PWD/.toolchain/rustup CARGO_HOME=$PWD/.toolchain/cargo PATH=$PWD/.toolchain/cargo/bin:$PATH
cargo test -p qingjian-linux-host --release probe_r3 -- --nocapture --test-threads=1 2>&1 | grep -E "PROBE_"
git checkout -- apps/linux/host/src/host/tests.rs
```

```rust
// R3-P1:修饰键+数字快捷键的松键——同 keysym 时对称吞;keysym 漂移(Shift 先松)时的行为。
#[test]
fn probe_r3_modifier_digit_release() {
    let mut h = sample_host();
    type_str(&mut h, "ni");
    let sw = h.key(0x31, 1 << 3, false);
    println!("PROBE_R3A Alt+1 按下吞={sw} 上屏={:?}", h.pending_commit.take());
    println!("PROBE_R3A 1 松键(Alt 仍按)吞={}", h.key(0x31, 1 << 3, true));
    let mut h = sample_host();
    type_str(&mut h, "ni");
    let sw = h.key(0x21, 1 << 0, false);
    println!("PROBE_R3B Shift+1(!)按下吞={sw} 组句={}", h.composing());
    println!("PROBE_R3B 松键 keysym=0x31(Shift 已松)吞={}", h.key(0x31, 0, true));
    println!("PROBE_R3B 松键 keysym=0x21(Shift 未松)吞={}", h.key(0x21, 0, true));
}

// R3-P2:reset()(切焦点)不清 swallowed_presses——切窗后到达的松键仍被吞。
#[test]
fn probe_r3_reset_keeps_ledger() {
    let mut h = sample_host();
    type_str(&mut h, "ni");
    h.reset();
    println!("PROBE_R3C reset 后 n 松键吞={}", h.key(0x6e, 0, true));
    println!("PROBE_R3C reset 后 i 松键吞={}", h.key(0x69, 0, true));
    println!("PROBE_R3C reset 后 x 松键吞={}", h.key(0x78, 0, true));
}

// R3-P3:auto-repeat(press,press,release,release)与账本上限。
#[test]
fn probe_r3_autorepeat_and_bound() {
    let mut h = sample_host();
    type_str(&mut h, "n");
    h.key(0x6e, 0, false);
    println!("PROBE_R3D 重复按下后账本额外条目不重复(release1 吞={})", h.key(0x6e, 0, true));
    println!("PROBE_R3D release2(无对应按下)吞={}", h.key(0x6e, 0, true));
}

// R3-P4:模型接上时 navigated=true → 不补查也不空转。
#[test]
fn probe_r3_attach_while_navigated() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let dict = repo.join("data/generated/dict.qj");
    let model = repo.join("data/model/model.qjm");
    if !dict.is_file() || !model.is_file() { eprintln!("跳过:无真数据"); return; }
    let dir = temp_data_dir("r3nav");
    std::fs::remove_file(dir.join("dict.tsv")).unwrap();
    std::os::unix::fs::symlink(&dict, dir.join("dict.qj")).unwrap();
    std::fs::create_dir_all(dir.join("model")).unwrap();
    std::os::unix::fs::symlink(&model, dir.join("model/model.qjm")).unwrap();
    let mut h = Host::init(dir, Some(qingjian_platform::Config::default())).unwrap();
    type_str(&mut h, "nihao");
    h.key(0xff53, 0, false); // 右方向键:navigated=true
    assert!(h.model_loader.is_some());
    let started = std::time::Instant::now();
    let mut last = 0u32;
    let mut repaint = false;
    while started.elapsed() < std::time::Duration::from_secs(10) {
        last = h.model_poll();
        if last & 1 != 0 { repaint = true; break; }
        if !h.engine.has_sentence_scorer() { std::thread::sleep(std::time::Duration::from_millis(5)); continue; }
        if last == 0 { break; }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    println!("PROBE_R3E navigated 下接上:重画={repaint} 末位={last:#b} 耗时={:?} pending={}",
        started.elapsed(), h.engine.rescoring_pending());
    h.key(0xff08, 0, false);
    println!("PROBE_R3F 接上后再敲键 deadline={:?} pending={}", h.rescore_deadline.is_some(), h.engine.rescoring_pending());
}

// R3-P5:F3 补查那次 refresh 是否在同一次 poll 里连带 request(错序检查)。
#[test]
fn probe_r3_attach_refresh_sequencing() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let dict = repo.join("data/generated/dict.qj");
    let model = repo.join("data/model/model.qjm");
    if !dict.is_file() || !model.is_file() { eprintln!("跳过"); return; }
    let dir = temp_data_dir("r3seq");
    std::fs::remove_file(dir.join("dict.tsv")).unwrap();
    std::os::unix::fs::symlink(&dict, dir.join("dict.qj")).unwrap();
    std::fs::create_dir_all(dir.join("model")).unwrap();
    std::os::unix::fs::symlink(&model, dir.join("model/model.qjm")).unwrap();
    let mut h = Host::init(dir, Some(qingjian_platform::Config::default())).unwrap();
    type_str(&mut h, "nihao");
    let before = (0..3).filter_map(|i| h.layout.candidate(i).map(|c| c.text.clone())).collect::<Vec<_>>();
    let started = std::time::Instant::now();
    let mut trace = Vec::new();
    while started.elapsed() < std::time::Duration::from_secs(10) {
        let attached_before = h.engine.has_sentence_scorer();
        let p = h.model_poll();
        if !attached_before && h.engine.has_sentence_scorer() {
            trace.push(format!("attach@{:?} poll={p:#b} pending={} deadline={} since={}",
                started.elapsed(), h.engine.rescoring_pending(), h.rescore_deadline.is_some(), h.rescore_since.is_some()));
        }
        if p & 1 != 0 { trace.push(format!("repaint@{:?}", started.elapsed())); break; }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let after = (0..3).filter_map(|i| h.layout.candidate(i).map(|c| c.text.clone())).collect::<Vec<_>>();
    println!("PROBE_R3G 轨迹={trace:?}");
    println!("PROBE_R3G 重排前后前3:{before:?} -> {after:?} 组句={}", h.composing());
}

// R3-P6:F5/F6——重复 init 不 panic;XDG_STATE_HOME 空串回退 HOME。
#[test]
fn probe_r3_logging_init_and_state_home() {
    let tmp = std::env::temp_dir().join(format!("qj-r3-log-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    unsafe { std::env::set_var("XDG_STATE_HOME", "") };
    println!("PROBE_R3H XDG_STATE_HOME=空串 log_dir={:?}", crate::logging::log_dir());
    unsafe { std::env::set_var("XDG_STATE_HOME", &tmp) };
    println!("PROBE_R3H XDG_STATE_HOME=临时 log_dir={:?}", crate::logging::log_dir());
    let first = std::panic::catch_unwind(crate::logging::init);
    let second = std::panic::catch_unwind(crate::logging::init);
    println!(
        "PROBE_R3I 首次 init panic={} 得 guard={};二次 init panic={} 得 guard={}",
        first.is_err(), matches!(&first, Ok(Some(_))),
        second.is_err(), matches!(&second, Ok(Some(_)))
    );
    unsafe { std::env::remove_var("XDG_STATE_HOME") };
}
```

**探针实测输出**:

```
PROBE_R3A Alt+1 按下吞=true 上屏=Some("you")
PROBE_R3A 1 松键(Alt 仍按)吞=true
PROBE_R3B Shift+1(!)按下吞=true 组句=true
PROBE_R3B 松键 keysym=0x31(Shift 已松)吞=false      ← R3-4
PROBE_R3B 松键 keysym=0x21(Shift 未松)吞=true       ← 陈账
PROBE_R3C reset 后 n 松键吞=true / i 吞=true / x 吞=false
PROBE_R3D release1 吞=true;release2(无对应按下)吞=false
PROBE_R3E navigated 下接上:重画=false 末位=0b0 耗时=45.6ms pending=false
PROBE_R3F 接上后再敲键 deadline=true pending=true
PROBE_R3G 轨迹=["attach@50.8ms poll=0b10 pending=true deadline=true since=false", "repaint@147.0ms"]
PROBE_R3G 重排前后前3:["你好","你好好","你好像"] -> ["你好","你好好","你好像"] 组句=true
PROBE_R3H XDG_STATE_HOME=空串 log_dir=Some(".../.local/state/qingjian/logs")
PROBE_R3I 首次 init panic=false 得 guard=true;二次 init panic=false 得 guard=false
```

## 附录 B:install.sh 沙箱场景(假 HOME + 假 XDG_DATA_HOME + PATH 前置 fcitx5 空桩)

伪 payload:分发模式目录(libqingjian.so 假文件 + 真 conf/theme + data/ 下 v1/v2 两套内容不同的假数据文件,含 dicts/01_idiom.qj 与 model.qjm),模拟数据资产升级。

| 场景 | 操作 | 结果 |
|---|---|---|
| 1 全新安装 | 跑 v1 | 全部落 `dist/`(含 dicts/、model/),exit 0 |
| 2 旧布局同版重装 | 根下预置 v1 四件 + 用户自有 glossary-zh.tsv + user.tsv,跑 v1 | 四件全迁走(dicts/、model/ 目录也 rmdir 掉),用户自有件与学习数据保留,exit 0 |
| 3 旧布局跨版升级 | 根下预置 v1 四件,跑 v2 | **四件全留守,根 v1 盖住 dist v2**(R3-1);dicts/01_idiom.qj 落死区(R3-2) |
| 4 缺数据的坏包 | 正常 v2 装好后跑无 data/ 的 payload | dist 整层被删光、.so 已换新、exit 1(R3-3) |
| 5 幂等 | v2 连跑两遍 | 结果一致,exit 0 |

---

## 处置记录(2026-09-15,甲方拍「直接动手」后由 leader 落实)

| 发现 | 裁决 | 落点 |
|---|---|---|
| R3-1 跨版旧随包件永久遮蔽 | 修,取「可见化」方案 | 安装收尾扫描 dist 内容,用户层同词干遮蔽文件逐个点名(自己放的属正常、残留请删);不做哈希清单——新安装器不往用户层写文件,首次迁移后根下同名文件按定义即用户自有 |
| R3-2 旧 dicts/ 死区 | 修,取警告方案(改自报告建议的「挪 user-dicts」) | 剩余文件点名列出、指路 user-dicts/;不自动挪——迁不走的多半是旧版随包件,自动挪进 user-dicts 会把陈旧数据复活成用户词库 |
| R3-3 rm -rf dist 先删后验 | 修 | 装进 dist.new 暂存层、就绪后原子换名;暂存层为空(坏包)不换、保留上一版并警告。沙箱复测:坏包两变体(守卫拦下 / 用户层放行)上一版 dist 均保留 |
| R3-4 Shift+数字 keysym 漂移 | 修过渡方案;keycode 根治记档 | `shift_counterpart`:松键查账与陈账清理认同一物理键的两个 keysym(符号表提成单一来源);根治(shim 合同加 keycode)记入设计稿「上游 PR 清单/待清理」 |
| R3-5 reset() 不清账本 | 同意报告结论,不动 | — |

复测:壳 55 / 平台层 23 测试全绿,clippy `-D warnings` 0 告警;install.sh 沙箱七场景(全新/幂等/同版迁移/跨版遮蔽警告/dicts 死区警告/坏包两变体)全过。
