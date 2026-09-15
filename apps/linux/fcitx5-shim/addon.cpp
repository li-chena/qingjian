// fcitx5 shim:唯一的 C++ 文件,只做转发,不放任何业务判断(architecture.md 约束一)。
// 引擎逻辑全在 Rust(qingjian.h 声明的 C ABI,实现在 apps/linux/host)。
#include "qingjian.h"

#include <fcitx/addonfactory.h>
#include <fcitx/addoninstance.h>
#include <fcitx/addonmanager.h>
#include <fcitx/inputcontext.h>
#include <fcitx/inputmethodengine.h>
#include <fcitx/instance.h>
#include <fcitx-utils/log.h>

namespace {

class QingjianEngine final : public fcitx::InputMethodEngineV2 {
public:
    explicit QingjianEngine(fcitx::Instance *instance) : instance_(instance) {
        FCITX_INFO() << "青简 Rust 侧已接线: " << qj_version();
    }

    void keyEvent(const fcitx::InputMethodEntry & /*entry*/,
                  fcitx::KeyEvent &event) override {
        const bool consumed =
            qj_key_event(event.rawKey().sym(), event.rawKey().states(),
                         event.isRelease());
        if (consumed) {
            event.filterAndAccept();
        }
    }

    void activate(const fcitx::InputMethodEntry & /*entry*/,
                  fcitx::InputContextEvent & /*event*/) override {
        qj_focus_in();
    }

    void deactivate(const fcitx::InputMethodEntry & /*entry*/,
                    fcitx::InputContextEvent & /*event*/) override {
        qj_focus_out();
    }

    void reset(const fcitx::InputMethodEntry & /*entry*/,
               fcitx::InputContextEvent & /*event*/) override {
        qj_reset();
    }

private:
    fcitx::Instance *instance_;
};

class QingjianFactory final : public fcitx::AddonFactory {
public:
    fcitx::AddonInstance *create(fcitx::AddonManager *manager) override {
        return new QingjianEngine(manager->instance());
    }
};

} // namespace

FCITX_ADDON_FACTORY_V2(qingjian, QingjianFactory);
