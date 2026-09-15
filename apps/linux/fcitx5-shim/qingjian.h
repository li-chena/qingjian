// C ABI 边界:与 apps/linux/host/src/lib.rs 的 #[unsafe(no_mangle)] 出口一一对应。
// 两边不同步会在链接期炸,不会静默错——改一边必改另一边。
#pragma once

#include <cstdint>

extern "C" {

const char *qj_version();
bool qj_key_event(uint32_t keyval, uint32_t state, bool release);
void qj_focus_in();
void qj_focus_out();
void qj_reset();

} // extern "C"
