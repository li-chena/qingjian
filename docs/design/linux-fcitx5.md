# Linux 端设计:Fcitx5 壳

本文地位:qingjian G1(青简 Linux 端)的定稿设计,上位于 `architecture.md`(冲突时以 architecture.md 的三条架构约束为准)。甲方 2026-09-15 拍定技术路线(复刻 macOS 壳),本文把路线落成可实现的结构。

一句话:在 fcitx5 里做一个进程内插件——薄 C++ shim 负责与 fcitx5 对话,全部业务逻辑在 Rust(照搬 macOS 壳的 host 层),译文标注走 fcitx5 候选原生的 comment 通道。

## 拍板记录

| 决策 | 结论 | 依据 |
|---|---|---|
| 目标框架 | Fcitx5,不做 IBus | 甲方系统(CachyOS)在跑 fcitx5;IBus 阵营候选窗常由桌面代绘,每候选译文无落点(甲方 2026-09-15 拍) |
| 进程模型 | Core 与壳同进程(fcitx5 插件即进程内动态库) | 甲方拍「复刻 macOS」;architecture.md 明写 macOS/Linux 同进程 |
| 壳形态 | 薄 C++ shim + Rust staticlib,C ABI 边界 | crates.io 无维护中的 fcitx5 引擎绑定(2026-09-15 查,仅 fcitx5-dbus 控制客户端);fcitx5 插件必须实现 C++ 虚类;上游 architecture.md 预判「Fcitx5 需要 C++ shim」 |
| 候选窗 UI | **自绘,复刻青简候选窗观感**;开发期先用 fcitx5 面板当脚手架打通输入,UI 单列一个阶段替换 | 甲方 2026-09-15 拍「UI 我想要 qingjian 的 UI」,推翻本稿初版「交框架」条 |
| 自绘的实现通道 | **修正(2026-09-15 侦察后)**:第一版走「classicui + 青简主题」——本机 classicui 支持 `CommentTextSizeFactor`/`CandidateCommentColor` 等键(strings libclassicui.so 实证),译文小字号+浅色的视觉层级主题就能复刻;光标定位继续由 classicui 的 `zwp_input_popup_surface_v2` 托管(niri 26.04 支持,rime 定位正常是活证)。整窗自绘降级为后备:定位弹窗 API(waylandim 私有)树外插件够不着,真要自绘需 vendor fcitx5 私有头,代价大——主题版观感经甲方过目后不达标的部分,才是自绘要买的东西 | 甲方会话 = Wayland + niri:Wayland 下外部窗口拿不到光标全局坐标,合成器托管的弹窗是唯一正道;公共 API 侦察 = /usr/include/Fcitx5/Module 无 waylandim 头 |
| 状态区/设置 UI | 交框架;设置首版直接编辑 TOML 配置文件(host 层配置热加载照搬) | 甲方拍的是候选窗 UI;状态区无青简观感诉求 |
| 上游同步 | merge upstream/main、跟 tag 不追 commit;除接缝文件外不改上游文件,通用改动回馈上游 PR | 与甲方 2026-09-15 对话研判,详见下「上游同步」 |

## 模块结构(复刻对照表)

```text
apps/linux/
├── fcitx5-shim/            # C++,越薄越好(目标 <500 行)
│   ├── CMakeLists.txt      # 找 Fcitx5Core,链 Rust staticlib
│   ├── addon.cpp           # AddonFactory / InputMethodEngine 虚类实现,纯转发
│   └── qingjian.h          # C ABI 边界声明(与 Rust 侧 #[no_mangle] 一一对应)
└── host/                   # Rust,package qingjian-linux-host,crate-type = staticlib
    ├── src/bridge.rs       # C ABI 出口:按键进(keyval/state)、动作出(commit/preedit/候选列表)
    ├── src/host/…          # 照搬 apps/macos/src/host(引擎装配/配置+热加载/词库/云/模型/学习落盘)
    └── src/paths.rs        # XDG 目录约定(数据 ~/.local/share/qingjian,配置 ~/.config/qingjian)
```

macOS 壳九千行的去向:host 层(~2400 行)照搬改路径;imk 层(~1100 行)按 fcitx5 对应物重写为 bridge + shim(KeyEvent↔keyval、InputContext↔client、Secure Input↔Password 能力标志);candidates/menubar/preferences(~4700 行)不复刻。首版预计 3–4k 行 Rust + <500 行 C++。

## C ABI 边界(shim 与 Rust 谁管什么)

shim 只做四件事,每件都是一两行转发:

1. fcitx5 命中按键 → 调 `qj_key_event(ctx, keyval, state, is_release)`,拿回「吞不吞」。
2. Rust 返回的状态帧(preedit、候选文本+comment、高亮页码)→ 翻成 fcitx5 的 `InputPanel`/`CandidateList` 调用。
3. 焦点进出/重置 → `qj_focus_in` / `qj_focus_out` / `qj_reset`。
4. 密码框(`CapabilityFlag::Password`)→ `qj_set_private(true)`,语义同 macOS Secure Input(不组句、不学习)。

排序、词库、翻译、学习全部在 Core——shim 里出现任何一条业务判断都算违反 architecture.md 约束一。

## 译文标注(产品核心的落点)

每个候选构造 fcitx5 `CandidateWord` 时把译文放进 `comment`(Text 第二段),主题负责浅色渲染;翻译未就绪时 comment 留空、候选照常先出(architecture.md 约束三:输入优先于学习)。一个候选只带一条译文(约束二)。

## 上游同步

- 我方仓库 = 上游完整克隆 + `apps/linux/` 增量;remote `upstream` 指向 GitHub 上游。
- 接缝文件只有两处:根 `Cargo.toml` 的 members 加两行;`qingjian-platform` 如需加配置字段,改动做成可回馈上游的形态并提 PR。
- 同步节奏:跟上游 release/tag,merge 后跑 `cargo check --workspace`(排除 macOS/Windows 壳)+ 本机输入冒烟;Engine API 变更照上游同一提交里 macOS 壳的改法适配。

## 构建与安装

- 构建:`cargo build -p qingjian-linux-host --release` 产 staticlib → CMake 构建 shim 链接之,产 `qingjian.so`。
- 安装(用户级,不动系统目录):`qingjian.so` 与 addon/inputmethod 两份 conf 装入用户目录,`FCITX_ADDON_DIRS` 指过去;数据文件(dict.qj/lm.qj/释义表)装 `~/.local/share/qingjian`。安装脚本 `apps/linux/install.sh` 一键完成,装完 `fcitx5 -r` 重启生效。
- 系统级安装(/usr/lib/fcitx5)作为备选写进脚本注释,首版不走。

## 分步验证(纵切,每步可独立演示)

1. 骨架:插件被 fcitx5 加载,输入法列表出现「青简」,按键透传不吞。(qlf-c 的前半)
2. 引擎接线:拼音→候选→选词上屏,preedit 显示。(qlf-d 的骨干)
3. 完整键位:翻页/数字选词/标点/中英切换/整句转换,行为对照 macOS 壳。
4. 译文标注:候选旁显示学习语言译文,断网不阻塞。(qlf-e;脚手架期走 comment 通道)
5. 青简候选窗:自绘 UI 插件替换 classicui 呈现,观感对照 macOS 候选窗(卡片/高亮行/译文浅色列/云标);脚手架面板保留为配置开关退路。
6. 学习与配置:词频落盘生效、TOML 热加载;密码框静默。
7. 冒烟收口:三类应用(终端/浏览器/编辑器)实测,交甲方签字。(qlf-f)

## 不在本设计(记档防漂移)

IBus 支持;多发行版打包(AUR/deb);图形设置界面;神经重排与云联想接入(host 层留同款接线点,能顺带则顺带,不设判据);Wayland/X11 兼容矩阵(本机环境为唯一承诺面)。
