# 青简 Linux 壳 第四轮独立巡检报告

**本文地位**:这是一份**发现记录**,不是定稿设计。它只记录"哪行代码在什么条件下会出什么问题",修法与优先级以甲方裁决为准;与 `docs/design/linux-fcitx5.md`(定稿设计)冲突时设计稿说了算,本文只提供事实。所有结论都带复现证据,探针代码见附录 A,install.sh 的沙箱场景与命令见附录 B。**不受 `docs/review/linux-shell-round3-review-20260915.md` 结论约束**,该报告的条目在第四节逐条交叉验证。

**一句话结论**:巡检三的 4 处修复(F1/F2/F3/F5/F6 里的 F3、R3-3、R3-4)**实测全部生效**,方向键/Alt/Super 四条新臂**行为正确**;但本轮的**新面里逮到 2 处中等缺陷**——① **英文模式下空格被吞**(macOS 与 Windows 两个参考实现都放行,Linux 独吞,后果是英文单词之间打不出空格);② **分发模式安装下,F4 的分层迁移判据永久失效**(新包只带 `.qj`,旧布局的 `.tsv` 永远匹配不上任何来源 → 永久遮蔽随包层;这与巡检三"同版重装迁移干净"的结论相反,我实测复现并给出包形状证据)。另有 3 处低危与 1 条潜伏备注,**19 条核实后判"不是问题"**逐条留痕防来回纠结。

巡检时间:2026-09-15。范围:`612b363..4626c1a`,**9 个提交**、1422 行增量。工作树跑完已还原(`git status` 只剩本轮开始前就有的两个未跟踪文件),基线 **57 tests 全绿**。

---

## 一、范围与方法

**精读的增量**(1422 行):

```
apps/linux/host/src/host/keys.rs         442(全文,+149)
apps/linux/host/src/host/paths.rs         86(全文,+27)
apps/linux/host/src/host/model.rs        154(+15)
apps/linux/host/src/host/mod.rs          213(+17)
apps/linux/host/src/host/config_watch.rs 106(1 行)
apps/linux/host/src/host/tests.rs        985(+230)
apps/linux/host/src/logging/mod.rs       147(+15)
apps/linux/install.sh                    215(全文,+106)
crates/qingjian-platform/src/config/apps.rs  
docs/design/linux-fcitx5.md / docs/notes/crate-notes.md / docs/plan/todo.md
docs/review/round2、round3 两份报告(交叉验证对象)
```

**方法**:不采信提交信息与注释 → 整量精读 → 可疑点写探针 → 在真代码/真数据(`data/generated/dict.qj` 92810 词条、`data/model/model.qjm` 56MB,临时目录 + symlink 装配)上跑出证据 → 逐条判真假。

**沙箱纪律**(委托明令):install.sh 只在**假 HOME + 假 XDG_DATA_HOME/XDG_CONFIG_HOME/XDG_STATE_HOME + PATH 前置 fcitx5 空桩**下跑,共 4 个场景(附录 B)。**实测未碰真实家目录、未重启真 fcitx5**:运行前后 `pgrep -a fcitx5` 进程集合完全一致(5 个现役进程 PID 不变),桩日志记录了每次被调用 ✓。

**基线**:`cargo test -p qingjian-linux-host --release` → **57 passed / 0 failed**。

---

## 二、问题清单(按严重度)

### 【中】F4-1. 英文模式下空格被吞:英文单词之间打不出空格

**位置**:`apps/linux/host/src/host/keys.rs:258-270`(空格臂)

```rust
0x20 if composing => {
    if raw {
        self.commit_index(self.highlighted);
        self.engine.note_passthrough(' ');
        return false;          // ← 直输段:空格交给应用(作者想到了)
    }
    if self.engine.english_mode() && !self.navigated {
        self.commit_raw();
    } else {
        self.commit_index(self.highlighted);
    }
    true                        // ← 英文模式:吞掉,空格丢了
}
```

**证据(探针 P4/P5)**:

```
PROBE_EN 不动     nav=false 空格吞=true 上屏=Some("helo") 组句=false
PROBE_EN ↑       nav=true  空格吞=true 上屏=Some("help") 组句=false
PROBE_EN ←       nav=false 空格吞=true 上屏=Some("helo") 组句=false
PROBE_EN ↑后←     nav=false 空格吞=true 上屏=Some("helo") 组句=false

PROBE_ES 进英文模式=true
PROBE_ES 空格:吞=true 上屏=Some("helo") | 空格2:吞=true 上屏=Some("wrld") | 空闲空格:吞=false 上屏=None
```

**三个壳逐行对照**(这是我判它"是缺陷"而不是"设计"的依据):

| 壳 | 英文模式组句中按空格 | 空格本体 |
|---|---|---|
| macOS `apps/macos/src/imk/controller.rs:422-460`(空格判定在 `:454`) | 动过高亮 → `commit_highlighted`,否则 `commit_raw` | `note_passthrough(' ')` + `return false` → **进应用** |
| Windows `apps/windows/server/src/dispatch/key/input.rs:181-205`(`apply_english`,空格判定在 `:199`) | 同上(`c == ' ' && navigated` 选词,否则 `take_raw`) | 落到 `apply_punctuation` → **Effect::Passthrough** → **进应用** |
| Linux(本仓) | 同上 | **`return true` 吞掉** |

**后果**:英文模式下每敲一个英文词,词末那个空格被吃掉。用户要打出 `hello world` 必须**连按两下空格**(第一下提交、第二下才进应用)。同一文件里直输段(`raw`)那条分支特意写了"空格本身也交给应用(`hello, world` 里的空格要在)"——说明作者知道空格该放行,英文模式这条是无意的遗漏。

**旁证**:回归测试 `english_mode_space_without_nav_commits_raw` 只断言了"上屏 = kubectl"(字母)与"键被吞",**没有断言空格该不该进应用**,所以这个行为被测试固化下来了。

**建议修法**:英文模式两个分支都改成 `note_passthrough(' ')` + `return false`(与直输段分支一致);回归测试补一条:英文模式空格提交后,空格必须进应用(吞 = false)。注意中文模式的空格仍应吞掉(那是选词键),别改过头。

---

### 【中】F4-2. 分发模式(.tsv→.qj 换代)让分层迁移判据永久失效 —— 与巡检三"同版重装迁移干净"的结论相反

**位置**:`apps/linux/install.sh:40-66`(`migrate_legacy` + `put_data` 的来源列表)、`:57`(同名 qj 在就不装 tsv 的守卫)

**根因**:迁移判据是"与**本次安装的来源**逐字节一致才删"。而新版 `pack.sh` 打包时 `.qj` 优先(`put_first "$gen/dict.qj" "$repo/assets/lexicon/dict.tsv"`),**包里根本不再带 `dict.tsv` / `glossary-{en,zh,ja}.tsv`**;`put_data` 又因为 `dist_new/dict.qj` 已存在而跳过 `.tsv`。于是这四个词干的 `.tsv` **在来源列表里一个都找不到** → 迁移永远删不掉 → 留在用户层 → 按分层语义("用户层整层先于随包层")**永久盖住随包的 `.qj`**。

**证据链(三步都是实测)**:

① **真实包形状**(既有产物 tar,不是我的假设):

```
612b363 包的 data/ 顶层:dict.qj  glossary-en.qj  glossary-zh.qj  glossary-ja.qj  lm.qj  …
                          ↑ 全是 .qj,没有同名 .tsv
5e41b35 包(F4 之前):    dict.tsv  glossary-en.tsv  glossary-zh.tsv  glossary-ja.tsv  …
                          ↑ 全是 .tsv —— 旧用户层的 .tsv 就是这么来的
```

② **分发模式沙箱**(假 HOME,旧布局 + 新包):

```
安装输出(警告确实打出来了,巡检三 R3-1 的可见化生效):
  | 注意:用户层下列文件将盖过随包同名数据(随包升级对它们不生效)。
  |     /tmp/.../qingjian/dict.tsv
  |     /tmp/.../qingjian/glossary-en.tsv
  |     /tmp/.../qingjian/glossary-zh.tsv
安装后用户层根: dict.tsv  dist  glossary-en.tsv  glossary-zh.tsv  user.tsv
dist 层:        dict.qj  glossary-en.qj  glossary-zh.qj  lm.qj  model/  dicts/  …
```

③ **实际装载哪个文件**(探针 P10,直接打印分层查找命中):

```
PROBE_SB find_data(dict)        = ".../qingjian/dict.tsv"          ← 命中用户层旧 tsv,不是 dist/dict.qj
PROBE_SB find_data(glossary-en) = ".../qingjian/glossary-en.tsv"
PROBE_SB find_data(glossary-zh) = ".../qingjian/glossary-zh.tsv"
PROBE_SB find_data(lm)          = ".../qingjian/dist/lm.qj"        ← 没有旧 lm.tsv,所以正常
PROBE_SB 分层目录装载:词条数=92810 耗时=145.175369ms
PROBE_SB 只 dist 层装载:词条数=92810 耗时=23.66121ms              ← 差 121ms(6 倍)
```

**当前实际损失**:今天两边内容等价(`dict.tsv` 92813 行 vs `.qj` 92810 词条,同一份数据),所以**没有数据错**,只有 ~121ms 的启动开销(每次 fcitx5 启动;`assets/lexicon/dict.tsv` 走 TSV 解析,`dist/dict.qj` 走 mmap)。

**真正的损失在未来**:`dist/` 层的意义就是"升级即更新"。这四个词干被旧 `.tsv` 占住以后,**以后任何随包数据升级(`gh release download data` 换新版)对它们永远不会生效** —— 用户拿到的永远是他机器上那份旧 `.tsv`,而且症状(词库/译文版本旧)几乎不可能被用户报出来。

**与巡检三的关系**(交叉验证,不是重复报):巡检三 R3-1 逮到的是"**数据资产升过版**时旧随包件比不中";它的沙箱场景 2 结论是"**同版数据重装:迁移干净**"。我这边证明的是:**在 `pack.sh` 真实产出的包形状下,连"同版重装"都迁不干净** —— 因为没有 `.tsv` 来源可比,cmp 恒不中。也就是说 R3-1 的触发条件比我原先理解的宽:**不需要数据升版,只要包是 `.qj` 形状就会中**;R3-1 选的"可见化"修复确实在按设计工作(警告打了),但它的前提假设("首次迁移后根下同名文件按定义即用户自有")在这四个词干上**不成立**——这些文件不是用户自有的,是安装器残留,警告文案却把它们归入"若非你放置…请删除",把判断责任推给了用户。

**建议修法**(按代价从小到大,择一):

1. **迁移判据加一条"格式换代"规则**:随包层已有 `X.qj` 时,用户层的 `X.tsv` 若**与任何已知数据集都不同源**就无法证明归属 —— 与其猜,不如在收尾把这类文件**改名留档**(`mv "$qdata/X.tsv" "$qdata/legacy-backup/X.tsv"`,不删、不读),分层立刻干净,用户想找回也找得到。
2. **判据换向**:`dist/` 里留一份来源清单(文件名 + SHA256),下次安装时把"与上一次 dist 记录一致"的根文件也算安装器所有 —— 首次迁移(无清单)退回现判据。
3. 最低限度:把警告文案里这类改名的文件单独成段("以下文件是旧版安装器的残留,建议删除;删除后随包数据即刻生效"),并在 `docs/user` 的 Linux 安装页写明。

---

### 【低-中】F4-3. 组句中 Tab 被吞却什么都不做;⇧+Tab 根本没进吞键面

**位置**:`keys.rs:410-413`(功能键区兜底吞)

```rust
_ if composing && (0xff00..=0xffff).contains(&keyval) => true,
```

**证据(探针 P6)**:

```
PROBE_LG 组句 Tab吞=true 之后(组句,页,有上屏)=(true, 0, false)
PROBE_LG ⇧+Tab(0xfe20)吞=false      ← 根本没被吞
PROBE_LG ⇧+Tab 松键吞=false
```

**两个子问题**:

① **Tab 是死键**:组句中按下 Tab → 被吞、页不变、无上屏、组句照旧 —— 什么都没发生。参考实现都不是这样:

| 壳 | 中文模式 Tab | 英文模式 Tab |
|---|---|---|
| macOS(`handle_command` insertTab) | 有整句补全就接受,否则 **`turn_page(1)` 翻页** | `commit_highlighted`(选高亮词) |
| Windows(`input.rs:105-113`) | 有补全就接受,否则 **Passthrough 交还应用**(缩进/跳焦点) | `commit_highlighted` |
| Linux | **吞掉 + 无事发生** | **吞掉 + 无事发生** |

也就是说 Linux 既没走 macOS 的"翻页",也没走 Windows 的"交还应用",而是第三种谁都没有的行为。用户文档 `docs/user/getting-started/keys.md:34` 记的就是 macOS/Windows 这两种(「`Tab`;无补全时翻页」/「`Tab`;无补全时交给应用」)。**Linux 该走哪条需要拍板**(Linux 没有云整句补全,所以"有补全就接受"那条用不上)。

② **⇧+Tab(ISO_Left_Tab 0xfe20)完全没进吞键面**:0xfe20 落在 `0xff00..=0xffff` **之外**,所以走最后一条 `_ => false` → **组句中漏给应用**。应用收到 ⇧+Tab 会反向移动焦点、组句跟着作废 —— 这正是那条兜底注释要防的事("否则应用会动光标、丢焦点,组句跟着作废"),只是它的键段没覆盖到 ⇧+Tab。同一份文档里 macOS 的 ⇧+Tab 是「上一页」。

> 口径依据:`rawKey()` 按 fcitx5 文档是"layout conversion 之后、**未归一化**"的键(`/usr/include/Fcitx5/Core/fcitx/event.h:335-342` 的 `KeyEventBase::rawKey` 注释:"Basically it is the unnormalized key event"),shim 传的正是 `event.rawKey().sym()`,所以 X11 下 ⇧+Tab 到达的就是 ISO_Left_Tab 0xfe20。**真机未逐帧验证**(见未覆盖面),但即便某些前端把它归一化成 `Tab + SHIFT` 态,子问题①(Tab 死键)仍然成立。

**建议修法**:兜底键段补上 `0xfe20`(或把判据从"键段"换成"非修饰键的功能键一律吞");Tab 的行为按拍板结果实现。

---

### 【低】F4-4. 词库守卫漏了随包层:坏包盖在新布局上会 exit 1,而数据其实在 `dist/`

**位置**:`install.sh:82-83`

```bash
[[ -f "$dist_new/dict.qj" || -f "$dist_new/dict.tsv" || -f "$qdata/dict.qj" || -f "$qdata/dict.tsv" ]] \
    || { echo "词库缺失:…(上一版随包数据未动)" >&2; exit 1; }
```

守卫只看暂存层与**用户层根**,**没有看 `$qdata/dist/`** —— 而这正是下一段(`:125-131`)承诺要保留的层。

**证据(沙箱场景 4)**:

```
先装好包(新布局):根=[dist]  dist/dict.qj 存在=有
再盖一个 data/ 为空的包:
  退出码=1
  词库缺失:分发包 data/ 或仓库里都找不到 dict.qj/dict.tsv(上一版随包数据未动)
  装后 dist/dict.qj 还在吗=有          ← 数据确实没丢(承诺做到了)
  残留:.../qingjian/dist.new            ← 中止在清理之前,留了个空暂存目录(下次运行自清)
```

**后果**:数据没丢 ✓,但①脚本以 **exit 1** 中止,`set -e` 的外层调用者/打包 CI 会当失败处理;②中止点在**重启之前**(`:214`),而 `.so`/conf 已经在 `:27-29` 换过了 —— 现役 fcitx5 继续跑旧 `.so`,用户下次重启才生效(无害,但"装了却没说重启"这件事与脚本末尾的提示矛盾);③提示文案说"词库缺失",实际词库在,容易把人带偏。触发要坏包(`pack.sh` 的 `put_first` 有 assets 兜底,正常产出的包必有 dict),所以是低危。

**建议修法**:守卫条件补 `|| -f "$qdata/dist/dict.qj" || -f "$qdata/dist/dict.tsv"`;顺手把 `dist.new` 的清理挪到守卫之前(或加 trap)。

---

### 【低】F4-5. 暂存层换名的注释与实现不符:仍是"先删旧、再挪新"

**位置**:`install.sh:123-131`

```bash
# 随包层全部就绪:原子换名,旧 dist 只在新层完整落地后才删(巡检三 R3-3)。
if [[ -n "$(ls -A "$dist_new")" ]]; then
    rm -rf "${qdata:?}/dist"        # ← 先删旧
    mv "$dist_new" "$qdata/dist"    # ← 再挪新
```

注释说"旧 dist 只在新层完整落地后才删",实际是**先把旧层整个删掉再挪新层**:两步之间掉电/被杀,新旧两层同时不存在(fcitx5 下次启动找不到任何数据,装配失败)。巡检三 R3-3 的处置记录写的是"装进 dist.new 暂存层、就绪后原子换名"——暂存层这一步 ✓ 做到了,但"原子"只到"挪新"为止。正常路径半秒级,风险很小,**问题主要是注释与实现不符**(与本仓判"注释不许写没核实的事实"的一贯口径同一类)。

**建议修法**(三段式,真正原子的写法):

```bash
mv "$qdata/dist" "$qdata/dist.old" 2>/dev/null || true
mv "$dist_new" "$qdata/dist"
rm -rf "$qdata/dist.old"
```

顺带一提:这一处与 F4-2 的警告一样,是"注释承诺 > 实现做到"的同一类,建议两条一起收。

---

### 【观察】F4-6. preedit 光标的单位:engine 说"字符数"、壳标"字节"、fcitx5 要"字节"

三处口径:

- engine 侧:`crates/qingjian-core/src/engine/marked/segment.rs:3` —「整段 preedit 是若干段按顺序拼起来,**光标位置按拼接后的字符数算**」
- 壳侧:`apps/linux/host/src/host/mod.rs:38` —「拼音行(带音节分隔)与光标(**字节偏移**)」;`qingjian.h:23` —「`int qj_preedit_cursor(); // 字节偏移`」
- fcitx5 侧:`/usr/include/Fcitx5/Core/fcitx/text.h:34-37` —「Get/Set cursor **by byte**」

**当前无差异**:preedit 只由 ASCII 构成(三种 `MarkedKind` 都是拼音/字母:Typed/Rest/Corrected),字符数 == 字节数。**但这是本轮新增能力第一次让它变得有意义**——在这轮之前光标永远在末尾,任何单位口径差都不可见;现在光标能停在中间,一旦将来 preedit 出现非 ASCII(比如纠错段带别的字符),客户端 preedit 里的光标就会偏 N 个字符。属于**潜伏项**,建议在设计稿里记一行口径(要么在壳侧做 `char→byte` 换算,要么把 engine 的注释改成字节)。

---

## 三、核实后判"不是问题"的(防来回纠结,逐条留痕)

1. **方向键四条新臂本体**:←/→ 移拼音光标、↑/↓ 移高亮、Home/End 到头尾,**在中文/英文/直输段/表达式/问字五种模式下逐键实测全部生效**:
   ```
   PROBE_MS 中文 cur=6 | ←5 | ←4 | ←2      （4→2 是音节边界跳法,见下条）
   PROBE_MS 英文 cur=4 | ←3 | ←2
   PROBE_MS 表达式 cur=5 | ←4 | ←3 | ←2
   PROBE_MS 问字 cur=5 | ←4 | ←3 | ←2
   PROBE_AR Home cur=0 / End cur=6 / 已在头再按 ← 吞掉且不动
   ```
2. **光标移动后 `refresh()` 重置 `navigated`/`page` 与英文模式空格语义的交互**:**与 macOS 同构,不是缺陷**。macOS 的 `moveLeft:`/`moveRight:` 同样走 `refresh` → `Session::reset` → `navigated=false`(macOS `session.rs:44`)。实测:
   ```
   PROBE_AR ↑ hl=1 nav=true  → 随后 ←:hl=0 page=0 nav=false
   PROBE_EN ↑ nav=true 空格选高亮("help") / ↑后← nav=false 空格回原样上屏("helo")
   ```
   "动过高亮才选词、动过光标不算"这条语义自洽(光标动了就是新一轮查询)。**唯一真问题在空格的去向(F4-1),与 navigated 无关。**
3. **候选只按光标之前的拼音算(文档口径)**:实测成立 —— `PROBE_CC` 光标在 `ni|hao`(cur=2)时候选前 3 = `["你","你好","你们"]`(你排第一),光标回末尾就变回 `["你好","你"]` ✓。cursor=0 时仍给全量候选是引擎行为(光标之前为空),macOS 走同一引擎,不算壳的问题。
4. **光标停中间时的上屏/退格/标点路径**:实测符合文档("上屏后其余拼音保留,光标移到末尾"):
   ```
   PROBE_MC 光标@4 空格 → 上屏 Some("你好"),其后 preedit="ao"@2(余下拼音保留)
   PROBE_MC 光标@4 退格 → preedit="ni'ao"@2(删光标前一个字母)
   PROBE_MC 光标@4 回车 → 上屏 Some("nihao")(整串原样上屏)
   PROBE_MC 光标@4 数字1 → 上屏 Some("你好")(选第 2 音节候选)
   ```
5. **Alt/Super 臂与修饰键数字快捷键区的先后顺序**:无冲突——两块的 keysym 集合不相交(数字/Shift 数字符号 vs 方向键/退格),先后无关;`Ctrl+Alt+←`、`Alt+Shift+←` 因 `pressed` 是精确匹配而正确落回 `state & CTRL_ALT_SUPER != 0` 的透传;不在组句时 Alt/Super 臂整体不进(应用照常拿到 ⌥←/⌘←)。
6. **松键记账(F1 修复本体)全貌**:实测对称且能自愈 ——
   ```
   PROBE_LG 组句按 !(Shift+1) 吞=true 账本记 0x21 → 松键漂移成 1(0x31) 吞=true 且销账 ✓(R3-4 修复生效)
   PROBE_LG 空闲按 1(透传) 吞=false 账本清空 ✓(陈账被同物理键的透传按下清掉)
   PROBE_LG 空闲按 , 吞=true → 松 , 吞=true 销账 ✓
   ```
   与巡检三"不是问题"1/2/3/5 条一致(`contains` 去重、`retain` 清陈账、auto-repeat 只欠一条)。
7. **`shift_counterpart` 的误吞面**:只对 `! @ # $ % ^ & * (` ↔ `1..9` 这 9 对生效,而这几对只在"数字键配修饰键"的快捷键路径被吞;即便键序错配(同一物理键两个 keysym 同名条目),查账按 `k == keyval || Some(k) == twin` 双向匹配,最终两边都会被消耗。**没有找到"吞掉一次没被吞的松键"的可达序列**;唯一残留是巡检三 R3-5(reset 不清账本)已裁定不动,不重复报。
8. **F3 修复在 model_poll 里的时序**:实测有效 ——
   ```
   PROBE_F3 加载中打字 pending=false deadline=false 载入挂着=true
   PROBE_F3 载入耗时≈146.845611ms 期间位={"0b1","0b10"} 收到重画=true
   ```
   加载窗口里打的那轮在模型接上后确实补到了重画位(0b1),与巡检三"不是问题"7/8/9/10 条的结论一致(补查只立 deadline、不抢跑 request、refresh 开头 attach 是空转)。
9. **日志与 F2/F5/F6 修复本体**:`try_init` 失败返回 None(不接管日志)✓;`app.get(..prefix.len())` 切不到边界即不匹配,多字节应用名不再 panic ✓;`log_dir()` 空串回退 ✓。三处都读过代码,巡检三/二已实测,本轮未再复跑(F2/F5/F6 不在本轮增量之外的范围)。
10. **开发模式下的迁移是干净的**(与 F4-2 对照,证明缺陷只在分发模式):沙箱场景 1 预置完整旧布局,跑开发模式安装后用户层根只剩 `dist/` 与 `user.tsv`,23 个文件进了 `dist/`、学习数据原封不动 ✓。原因:开发模式的来源列表里有 `assets/lexicon/dict.tsv` 与 `assets/glossary/*.tsv`,cmp 能对上。
11. **安装脚本的其余正常路径**:全新装(场景 3)根下只有 `dist/`;配置播种只写一次;`environment.d/qingjian-fcitx5.conf` 内容正确;`classicui.conf` 的 5 个键写入正确;`dist.new` 在成功路径下无残留 ✓。
12. **`put_data` 的 `.tsv` 跳过守卫**:`dist_new` 里已有 `X.qj` 就不再装 `X.tsv` ✓ 成立(包形状实测:只有 `.qj`);`glossary-es.tsv`(无 qj 变体)照常装 ✓。
13. **`rm -rf "${qdata:?}/dist"` 的目标安全**:`${qdata:?}` 在 qdata 空/未设时直接让脚本失败 ✓;qdata 恒以 `/qingjian` 结尾 ✓(与巡检三"不是问题"14 条一致)。
14. **`set -e` 与 `[[ -d X ]] && while …` 组合**:`[[ ]]` 为假时整条 AND 列表返回 1,但按 bash 语义"AND 列表中非最后一条命令失败"不触发 `set -e` ✓(沙箱场景 2/3 均未误炸)。
15. **`migrate_legacy` 的返回值**:`for` + `if`(无 else)结构恒返回 0,不会因 cmp 全不中炸掉调用链 ✓(与巡检三"不是问题"13 条一致)。
16. **模型迁移**:旧布局的 `$qdata/model/model.qjm` 在分发模式沙箱里被正确删掉(包里有同源 `data/model.qjm` 可比)✓ —— 与 F4-2 的 `.tsv` 形成对照,说明缺陷是"**包不再带的那个格式**"特有的。
17. **`config_watch.rs` 的 `dist/dicts` 与 `model.rs` 的两层查找**:全仓数据目录 join 只有这三处(`dist/dicts`+`user-dicts`、`model`→`dist/model`、`paths.rs` 的 `data_layers`),没有漏掉的第三处 ✓(与巡检三"不是问题"17 条一致)。
18. **`navigated` 在 raw 模式下不影响空格**:raw 分支先判 `raw` 再判 english ✓ 顺序正确。
19. **测试基线**:57 tests 全绿,clippy 未报警(本轮未跑 clippy,以仓库既有门禁为准)。

---

## 四、与巡检三报告的交叉验证(逐条)

| 巡检三条目 | 我的复验 |
|---|---|
| F1/F2/F3/F5/F6 已修 | ✓ 同意(F3 用真模型复跑,见"不是问题"8) |
| R3-1 迁移判据漏跨版旧布局(中) | ✓ 方向一致;**但我的证据把触发条件放宽了**:在真实包形状下连"同版重装"都不干净(见 F4-2),所以 R3-1 选的"可见化"是必要不充分——警告打了,默认结果仍是"升级不生效",得用户手动删 |
| R3-2 旧 `dicts/` 死区(低) | ✓ 不重复;本轮沙箱开发模式实测迁走后 `rmdir` 成功 ✓ |
| R3-3 `rm -rf dist` 先删后验(低) | ✓ 暂存层已上,坏包保留上一版实测成立;**但注释与实现不符(先删后挪),见 F4-5** |
| R3-4 Shift+数字 keysym 漂移(低) | ✓ 修复实测生效(见"不是问题"6);根治(keycode)仍在记档待办 |
| R3-5 reset 不清账本 | 已裁定不动,本轮不重复报 |
| "不是问题"18「用户层 tsv 盖过 dist qj 是设计本意」 | **部分不同意**:单看分层语义 ✓ 是设计;但与 F4-2 的迁移缺口叠加后,"用户层 tsv" 里混着**安装器残留**,结果是随包升级永不生效。设计本意保护的是"用户自己放的文件",不是"上一版安装器放的旧格式文件"——两者现在无法区分,这才是缺口所在 |
| "不是问题"15「词库守卫 dist 或用户层根任一有即过」 | **不完整**:漏了 `$qdata/dist/`,见 F4-4(沙箱场景 4 复现 exit 1) |
| "不是问题"7-10(F3 时序、双查、克制守卫) | ✓ 同意,我用真模型复跑得到同样结论 |

---

## 五、未覆盖面(如实交代)

1. **没在真 fcitx5 里做交互测试**:F4-1(英文模式空格)、F4-3(Tab/⇧+Tab)都是**壳内行为实测 + 三壳源码对照**;真机上"用户按下去看到什么"没验(按沙箱纪律不能碰现役 fcitx5)。
2. **⇧+Tab 的实际 keysym**:我按 `rawKey()` 的"未归一化"语义推 X11 下是 ISO_Left_Tab(0xfe20),**没有逐帧抓真机按键**。若某些前端做归一化(送 `Tab+SHIFT` 态),子问题①(Tab 死键)仍成立,子问题②的形态会变。
3. **Alt/Super 臂在真实合成器下是否够得着**:niri 等合成器通常先截走 `Super+←/→`,IME 可能根本收不到;本轮只在壳层验证了分支正确,**没在真桌面验证键能否到达插件**。
4. **方向键长按 auto-repeat**:未测连续 press 的时序(只测了单次 press/release)。
5. **跨壳复制的那两块(logging、模型状态机)**:按委托属提库范围,本轮只验了 Linux 侧行为,没审 macOS/Windows 对应实现。
6. **客户端 preedit 光标落点的真机观感**:F4-6 只做了口径核对,没看真机上光标画在哪(需要真 fcitx5)。

---

## 附录 A:探针代码(可直接粘回去重跑)

追加到 `apps/linux/host/src/host/tests.rs` 末尾,跑完 `git checkout --` 还原:

```bash
cd /vol1/1000/docker/qingjian
export RUSTUP_HOME=$PWD/.toolchain/rustup CARGO_HOME=$PWD/.toolchain/cargo PATH=$PWD/.toolchain/cargo/bin:$PATH
cat >> apps/linux/host/src/host/tests.rs <<'EOF'
... 下面这段 ...
EOF
cargo test -p qingjian-linux-host --release probe_ -- --nocapture --test-threads=1 2>&1 | grep -oE "PROBE_[A-Z0-9]+ .*"
git checkout -- apps/linux/host/src/host/tests.rs
```

> 注:`libtest` 只吃一个过滤词;`--nocapture` 才有 println 输出;每个探针的**第一行**会被 cargo 的 "test xxx ... " 前缀吞掉,用 `grep -oE "PROBE_.*"` 抓而不是 `^PROBE_`。

```rust
// ===== 第四轮巡检临时探针(跑完还原)=====
fn p_dir(tag: &str, real_dict_in_dist: bool) -> PathBuf {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let dir = std::env::temp_dir().join(format!("qj-p4-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("dist/model")).unwrap();
    let sample = repo.join("assets/sample");
    for f in ["dict.tsv", "english.tsv", "glossary-en.tsv"] {
        std::fs::copy(sample.join(f), dir.join(f)).unwrap();
    }
    if real_dict_in_dist {
        std::fs::create_dir_all(dir.join("dist")).unwrap();
        std::os::unix::fs::symlink(repo.join("data/generated/dict.qj"), dir.join("dist/dict.qj")).unwrap();
    }
    if repo.join("data/model/model.qjm").is_file() {
        std::os::unix::fs::symlink(repo.join("data/model/model.qjm"), dir.join("dist/model/model.qjm")).unwrap();
    }
    dir
}
fn first3(h: &Host) -> Vec<String> {
    (0..h.layout.len().min(3)).filter_map(|i| h.layout.candidate(i).map(|c| c.text.clone())).collect()
}

// ① 方向键:光标移动后 refresh 对 navigated/page/highlight 的影响
#[test]
fn probe_arrows_state() {
    let mut h = sample_host();
    type_str(&mut h, "nihao");
    println!("PROBE_AR 初始      preedit={:?} cur={} 候选前3={:?} hl={} page={} nav={}", h.preedit, h.preedit_cursor, first3(&h), h.highlighted, h.page, h.navigated);
    h.key(0xff52, 0, false); // ↑ 移高亮
    println!("PROBE_AR ↑        hl={} page={} nav={} 候选前3={:?}", h.highlighted, h.page, h.navigated, first3(&h));
    h.key(0xff51, 0, false); // ← 移光标
    println!("PROBE_AR ↑后←     preedit={:?} cur={} hl={} page={} nav={} 候选前3={:?}", h.preedit, h.preedit_cursor, h.highlighted, h.page, h.navigated, first3(&h));
    h.key(0xff53, 0, false); // →
    println!("PROBE_AR →        preedit={:?} cur={} nav={}", h.preedit, h.preedit_cursor, h.navigated);
    h.key(0xff50, 0, false); // Home
    println!("PROBE_AR Home     preedit={:?} cur={} 候选前3={:?}", h.preedit, h.preedit_cursor, first3(&h));
    h.key(0xff57, 0, false); // End
    println!("PROBE_AR End      preedit={:?} cur={} 候选前3={:?}", h.preedit, h.preedit_cursor, first3(&h));
    // 一次 ← 到位后继续 ← 会怎样
    h.key(0xff50, 0, false);
    let sw = h.key(0xff51, 0, false);
    println!("PROBE_AR 已在头再← 吞={sw} preedit={:?} cur={}", h.preedit, h.preedit_cursor);
}

// ① 英文模式空格:不动 / ↑动高亮 / ←动光标 / ↑后←
#[test]
fn probe_english_space_after_nav_kinds() {
    for kind in ["不动", "↑", "←", "↑后←"] {
        let mut h = sample_host();
        h.key(0xffe1, 0, false);
        h.key(0xffe1, 1, true); // 进英文模式
        type_str(&mut h, "helo");
        match kind {
            "↑" => { h.key(0xff52, 0, false); }
            "←" => { h.key(0xff51, 0, false); }
            "↑后←" => { h.key(0xff52, 0, false); h.key(0xff51, 0, false); }
            _ => {}
        }
        let nav = h.navigated;
        let sw = h.key(0x20, 0, false);
        println!("PROBE_EN {kind:<6} nav={nav} 空格吞={sw} 上屏={:?} 组句={}", h.pending_commit.take(), h.composing());
    }
}

// ① raw / 表达式 / 问字模式下的方向键与 Home/End
#[test]
fn probe_arrows_in_modes() {
    // 直输段
    let mut h = sample_host();
    type_str(&mut h, "ni");
    h.key(0x2d, 0, false);
    type_str(&mut h, "hao");
    let (p0, c0) = (h.preedit.clone(), h.preedit_cursor);
    let s1 = h.key(0xff51, 0, false);
    println!("PROBE_MD 直输段     ←吞={s1} {p0:?}@{c0} → {:?}@{}", h.preedit, h.preedit_cursor);
    let s2 = h.key(0xff50, 0, false);
    println!("PROBE_MD 直输段     Home吞={s2} → {:?}@{}", h.preedit, h.preedit_cursor);
    let s3 = h.key(0xff52, 0, false);
    println!("PROBE_MD 直输段     ↑吞={s3} nav={} hl={}", h.navigated, h.highlighted);
    // 表达式
    let mut h = sample_host();
    type_str(&mut h, "v");
    type_str(&mut h, "12+3");
    let (p0, c0) = (h.preedit.clone(), h.preedit_cursor);
    let s1 = h.key(0xff51, 0, false);
    let s2 = h.key(0xff57, 0, false);
    println!("PROBE_MD 表达式     ←吞={s1} End吞={s2} {p0:?}@{c0} → {:?}@{}", h.preedit, h.preedit_cursor);
    // 问字
    let mut h = sample_host();
    type_str(&mut h, "u4e00");
    let (p0, c0) = (h.preedit.clone(), h.preedit_cursor);
    let s1 = h.key(0xff51, 0, false);
    let s2 = h.key(0xff50, 0, false);
    println!("PROBE_MD 问字       ←吞={s1} Home吞={s2} {p0:?}@{c0} → {:?}@{}", h.preedit, h.preedit_cursor);
}

// ① 光标停中间:上屏/退格/标点/数字/回车
#[test]
fn probe_mid_cursor_paths() {
    for (name, keyval) in [("空格", 0x20u32), ("退格", 0xff08), ("逗号", 0x2c), ("数字1", 0x31), ("回车", 0xff0d), ("Delete", 0xffff)] {
        let mut h = sample_host();
        type_str(&mut h, "nihao");
        h.key(0xff51, 0, false);
        h.key(0xff51, 0, false);
        let cur = h.preedit_cursor;
        let sw = h.key(keyval, 0, false);
        println!("PROBE_MC 光标@{cur} 按{name:<6} 吞={sw} 上屏={:?} 之后 preedit={:?}@{ } 组句={}",
                 h.pending_commit.take(), h.preedit, h.preedit_cursor, h.composing());
    }
}

// ② 松键记账 + 误吞面
#[test]
fn probe_ledger() {
    let mut h = sample_host();
    println!("PROBE_LG 初始账本={:?}", h.swallowed_presses);
    type_str(&mut h, "ni");
    // Shift+1(!)按下:组句中删候选
    let p = h.key(0x21, 1 << 0, false);
    println!("PROBE_LG 组句按! 吞={p} 账本={:?}", h.swallowed_presses);
    let r = h.key(0x31, 0, true); // 松键漂移成 1
    println!("PROBE_LG 松1     吞={r} 账本={:?}", h.swallowed_presses);
    // 空闲逗号
    let mut h2 = sample_host();
    h2.key(0x2c, 0, false);
    println!("PROBE_LG 空闲按, 吞={:?} 账本={:?}", true, h2.swallowed_presses);
    let r = h2.key(0x2c, 0, true);
    println!("PROBE_LG 松,     吞={r} 账本={:?}", h2.swallowed_presses);
    // 透传的按下会清同物理键陈账
    let mut h3 = sample_host();
    h3.swallowed_presses.push(0x31);
    let p = h3.key(0x31, 0, false);
    println!("PROBE_LG 空闲按1(透传)吞={p} 账本={:?}(应清掉 0x31)", h3.swallowed_presses);
    // Tab / Shift+Tab
    let mut h4 = sample_host();
    type_str(&mut h4, "ni");
    let tab = h4.key(0xff09, 0, false);
    let after_tab = (h4.composing(), h4.page, h4.pending_commit.is_some());
    let stab = h4.key(0xfe20, 0, false); // ISO_Left_Tab
    println!("PROBE_LG 组句 Tab吞={tab} 之后(组句,页,有上屏)={after_tab:?}; ⇧+Tab(0xfe20)吞={stab}");
    let rtab = h4.key(0xfe20, 0, true);
    println!("PROBE_LG ⇧+Tab 松键吞={rtab}");
}

// ④ F3:加载窗口里那一轮是否补上重排
#[test]
fn probe_f3_late_attach() {
    let dir = p_dir("f3", true);
    let mut h = Host::init(dir.clone(), Some(qingjian_platform::Config::default())).expect("应能装配");
    type_str(&mut h, "nihao");
    println!("PROBE_F3 加载中打字 pending={} deadline={:?} 载入挂着={}", h.engine.rescoring_pending(), h.rescore_deadline.is_some(), h.model_loader.is_some());
    let t0 = std::time::Instant::now();
    let mut bits = Vec::new();
    let mut repaint = false;
    while t0.elapsed() < std::time::Duration::from_secs(30) {
        let p = h.model_poll();
        bits.push(p);
        if p & 1 != 0 { repaint = true; break; }
        if h.engine.has_sentence_scorer() && !h.rescore_deadline.is_some() && t0.elapsed().as_millis() > 200 { break; }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    println!("PROBE_F3 载入耗时≈{:?} 期间位={:?} 收到重画={repaint} pending={} deadline={:?}",
             t0.elapsed(), bits.iter().map(|b| format!("{b:#b}")).collect::<std::collections::BTreeSet<_>>(),
             h.engine.rescoring_pending(), h.rescore_deadline.is_some());
}

// ④-附:分层遮蔽(用户层 tsv 盖过随包层 qj)
#[test]
fn probe_layer_shadowing() {
    let dir = p_dir("shadow", true); // 用户层 dict.tsv(样例) + dist/dict.qj(真词库)
    let h = Host::init(dir, Some(qingjian_platform::Config::default())).unwrap();
    println!("PROBE_LY 用户层 dict.tsv + dist/dict.qj → 装载词条数={}(真库 92810 / 样例约 20)", h.engine.dictionary().len());
    let dir2 = p_dir("distonly", true);
    std::fs::remove_file(dir2.join("dict.tsv")).unwrap();
    let h2 = Host::init(dir2, Some(qingjian_platform::Config::default())).unwrap();
    println!("PROBE_LY 只有 dist/dict.qj → 装载词条数={}", h2.engine.dictionary().len());
}

// 英文模式打"hello world":空格会不会被吃掉
#[test]
fn probe_english_typing_spaces() {
    let mut h = sample_host();
    h.key(0xffe1, 0, false);
    h.key(0xffe1, 1, true);
    println!("PROBE_ES 进英文模式={} 有英文词表={}", h.engine.english_mode(), h.layout.len());
    let mut log = Vec::new();
    for c in "helo".chars() { h.key(c as u32, 0, false); }
    let sw = h.key(0x20, 0, false);
    log.push(format!("空格:吞={sw} 上屏={:?}", h.pending_commit.take()));
    for c in "wrld".chars() { h.key(c as u32, 0, false); }
    let sw2 = h.key(0x20, 0, false);
    log.push(format!("空格2:吞={sw2} 上屏={:?}", h.pending_commit.take()));
    let sw3 = h.key(0x20, 0, false); // 空闲空格
    log.push(format!("空闲空格:吞={sw3} 上屏={:?}", h.pending_commit.take()));
    println!("PROBE_ES {}", log.join(" | "));
}

// 光标中间时候选是否只按光标前拼音算(文档口径)
#[test]
fn probe_candidates_at_cursor() {
    let mut h = sample_host();
    type_str(&mut h, "nihao");
    println!("PROBE_CC cur={} 候选前3={:?}", h.preedit_cursor, first3(&h));
    h.key(0xff50, 0, false); // Home → 光标 0
    println!("PROBE_CC Home cur={} preedit={:?} 候选前3={:?}", h.preedit_cursor, h.preedit, first3(&h));
    h.key(0xff53, 0, false); // →
    h.key(0xff53, 0, false); // →
    println!("PROBE_CC →→ cur={} preedit={:?} 候选前3={:?}", h.preedit_cursor, h.preedit, first3(&h));
    h.key(0xff57, 0, false); // End
    println!("PROBE_CC End cur={} preedit={:?} 候选前3={:?}", h.preedit_cursor, h.preedit, first3(&h));
}

// 模式内 ← 到底动没动(逐键打印)
#[test]
fn probe_mode_cursor_stepwise() {
    for (name, setup) in [("表达式", "v12+3"), ("问字", "u4e00")] {
        let mut h = sample_host();
        type_str(&mut h, setup);
        let mut line = format!("PROBE_MS {name} 起 cur={}", h.preedit_cursor);
        for _ in 0..3 {
            let sw = h.key(0xff51, 0, false);
            line += &format!(" | ←吞={sw} cur={}", h.preedit_cursor);
        }
        println!("{line}");
    }
    // 中文模式对照
    let mut h = sample_host();
    type_str(&mut h, "nihao");
    let mut line = format!("PROBE_MS 中文 起 cur={}", h.preedit_cursor);
    for _ in 0..3 {
        let sw = h.key(0xff51, 0, false);
        line += &format!(" | ←吞={sw} cur={}", h.preedit_cursor);
    }
    println!("{line}");
    // 英文模式
    let mut h = sample_host();
    h.key(0xffe1, 0, false);
    h.key(0xffe1, 1, true);
    type_str(&mut h, "helo");
    let mut line = format!("PROBE_MS 英文 起 cur={}", h.preedit_cursor);
    for _ in 0..2 {
        let sw = h.key(0xff51, 0, false);
        line += &format!(" | ←吞={sw} cur={}", h.preedit_cursor);
    }
    println!("{line}");
}

// ③ 沙箱装完以后:分层查找实际命中哪个文件(对应 install.sh 分发模式场景)
#[test]
fn probe_sandbox_layers() {
    let dir = PathBuf::from("/tmp/qj-sb4/home2/.local/share/qingjian");
    if !dir.is_dir() {
        println!("PROBE_SB 跳过(沙箱不在)");
        return;
    }
    for stem in ["dict", "glossary-en", "glossary-zh", "lm"] {
        println!("PROBE_SB find_data({stem}) = {:?}", super::find_data(&dir, stem).map(|p| p.to_string_lossy().into_owned()));
    }
    let t = std::time::Instant::now();
    let h = Host::init(dir.clone(), Some(qingjian_platform::Config::default())).unwrap();
    println!("PROBE_SB 分层目录装载:词条数={} 耗时={:?} 学习语言={:?}", h.engine.dictionary().len(), t.elapsed(), h.learning_language.map(|l| l.code()));
    // 对照:只有 dist 层(用户层同名文件不存在)
    let solo = std::env::temp_dir().join(format!("qj-solo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&solo);
    std::fs::create_dir_all(&solo).unwrap();
    std::os::unix::fs::symlink(dir.join("dist"), solo.join("dist")).unwrap();
    std::fs::copy(dir.join("glossary-en.tsv"), solo.join("english.tsv")).ok();
    let t2 = std::time::Instant::now();
    let h2 = Host::init(solo.clone(), Some(qingjian_platform::Config::default())).unwrap();
    println!("PROBE_SB 只 dist 层装载:词条数={} 耗时={:?}", h2.engine.dictionary().len(), t2.elapsed());
    let _ = std::fs::remove_dir_all(&solo);
}

```

## 附录 B:install.sh 沙箱场景与命令

**纪律**:全程假 HOME、假 XDG_*、PATH 前置 fcitx5 空桩,`env -i` 清环境:

```bash
SB=/tmp/qj-sb4
mkdir -p $SB/bin $SB/log
cat > $SB/bin/fcitx5 <<'EOF'
#!/bin/sh
echo "STUB-CALLED $(date +%T) args=$*" >> /tmp/qj-sb4/log/fcitx5.log
exit 0
EOF
chmod +x $SB/bin/fcitx5
PATH=$SB/bin:/usr/bin:/bin command -v fcitx5   # 必须先确认解析到桩

env -i HOME=$SB/home XDG_DATA_HOME=$SB/home/.local/share \
       XDG_CONFIG_HOME=$SB/home/.config XDG_STATE_HOME=$SB/home/.local/state \
       PATH="$SB/bin:/usr/bin:/bin" bash /vol1/1000/docker/qingjian/apps/linux/install.sh
pgrep -a fcitx5   # 运行前后进程集合一致 = 没碰现役
```

| 场景 | 建法 | 结果 |
|---|---|---|
| 1 开发模式 + 完整旧布局 | 根下预置 dict.qj/dict.tsv/glossary-en.{qj,tsv}/glossary-zh.tsv/model/、dicts/、user.tsv | 用户层根只剩 `dist/` + `user.tsv`;**迁移干净** ✓ |
| 2 分发模式 + 旧布局(镜像 pack.sh 的 data/:只有 .qj) | 同场景 1 的旧布局 + `pkg/`(install.sh + .so 桩 + conf + theme + data) | **dict.tsv / glossary-en.tsv / glossary-zh.tsv 留守并遮蔽**;警告点名三件(F4-2) |
| 3 全新装(分发模式) | 空 HOME | 根下只有 `dist/`;配置/主题/classicui/environment.d 全部正确 ✓ |
| 4 空数据包盖在新布局上 | 先装好包(新布局),再跑 `data/` 为空的包 | `exit=1` + "词库缺失";`dist/dict.qj` 保住 ✓;留一个空 `dist.new`(F4-4) |

**桩日志**(证明每次"重启"都打在桩上):

```
STUB-CALLED 20:56:39 args=-rd
STUB-CALLED 20:56:54 args=-rd
```
