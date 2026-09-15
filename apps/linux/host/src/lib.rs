//! Linux 壳的 Rust 侧(staticlib)。C ABI 出口给 fcitx5 的 C++ shim 调,
//! 业务逻辑(引擎装配/配置/学习落盘)后续照搬 macOS 壳的 host 层,shim 里不放业务。
//!
//! 骨架阶段:只提供版本号与按键透传(一律不吞),证明「插件加载 → 按键进出 Rust」链路通。

use std::ffi::{c_char, c_uint};

/// 版本串,静态存储,C 侧只读不释放。
static VERSION: &std::ffi::CStr = c"qingjian-linux 0.1.0";

/// 返回 Rust 侧版本串(NUL 结尾,静态生命周期)。shim 加载成功后打进 fcitx5 日志,
/// 是「C++ ↔ Rust 链路真通了」的最小证据。
#[unsafe(no_mangle)]
pub extern "C" fn qj_version() -> *const c_char {
    VERSION.as_ptr()
}

/// 按键事件。keyval/state 用 fcitx5(即 X keysym)原值,release = 松键。
/// 返回 true = 吞掉(shim 调 filterAndAccept),false = 透传给应用。
/// 骨架阶段一律透传。
#[unsafe(no_mangle)]
pub extern "C" fn qj_key_event(keyval: c_uint, state: c_uint, release: bool) -> bool {
    let _ = (keyval, state, release);
    false
}

/// 焦点进入/离开/重置。骨架阶段空转,占住 ABI 形状。
#[unsafe(no_mangle)]
pub extern "C" fn qj_focus_in() {}

#[unsafe(no_mangle)]
pub extern "C" fn qj_focus_out() {}

#[unsafe(no_mangle)]
pub extern "C" fn qj_reset() {}
