// fcitx5 shim:唯一的 C++ 文件,只做转发,不放任何业务判断(architecture.md 约束一)。
// 引擎逻辑全在 Rust(qingjian.h 声明的 C ABI,实现在 apps/linux/host)。
#include "qingjian.h"

#include <memory>
#include <string>

#include <fcitx/addonfactory.h>
#include <fcitx/addoninstance.h>
#include <fcitx/addonmanager.h>
#include <fcitx/candidatelist.h>
#include <fcitx/inputcontext.h>
#include <fcitx/inputmethodengine.h>
#include <fcitx/inputpanel.h>
#include <fcitx/instance.h>
#include <fcitx-utils/log.h>

namespace {

// 页内某格的候选:文本 + 译文 comment;点选转回 Rust。
class QjCandidate final : public fcitx::CandidateWord {
public:
    QjCandidate(int offset, const std::string &text, const std::string &comment)
        : offset_(offset) {
        setText(fcitx::Text(text));
        if (!comment.empty()) {
            setComment(fcitx::Text(comment));
        }
    }

    void select(fcitx::InputContext *ic) const override;

private:
    int offset_;
};

class QingjianEngine final : public fcitx::InputMethodEngineV2 {
public:
    explicit QingjianEngine(fcitx::Instance *instance) : instance_(instance) {
        if (qj_init()) {
            ready_ = true;
            FCITX_INFO() << "青简就绪: " << qj_version();
        } else {
            FCITX_ERROR() << "青简引擎装配失败: " << qj_init_error();
        }
    }

    void keyEvent(const fcitx::InputMethodEntry & /*entry*/,
                  fcitx::KeyEvent &event) override {
        if (!ready_) {
            return;
        }
        const bool consumed =
            qj_key_event(event.rawKey().sym(), event.rawKey().states(),
                         event.isRelease());
        if (!consumed) {
            return;
        }
        event.filterAndAccept();
        sync(event.inputContext());
    }

    void activate(const fcitx::InputMethodEntry & /*entry*/,
                  fcitx::InputContextEvent & /*event*/) override {
        qj_focus_in();
    }

    void deactivate(const fcitx::InputMethodEntry & /*entry*/,
                    fcitx::InputContextEvent &event) override {
        qj_focus_out();
        clearPanel(event.inputContext());
    }

    void reset(const fcitx::InputMethodEntry & /*entry*/,
               fcitx::InputContextEvent &event) override {
        qj_reset();
        clearPanel(event.inputContext());
    }

    // 每个被吞掉的键之后:上屏文本、重建 preedit 与候选面板。
    void sync(fcitx::InputContext *ic) {
        if (const char *commit = qj_take_commit()) {
            ic->commitString(commit);
        }
        std::string preedit = qj_preedit();
        auto &panel = ic->inputPanel();
        panel.reset();
        if (!preedit.empty()) {
            const int cursor = qj_preedit_cursor();
            fcitx::Text preeditText(preedit, fcitx::TextFormatFlag::Underline);
            preeditText.setCursor(cursor);
            if (ic->capabilityFlags().test(fcitx::CapabilityFlag::Preedit)) {
                panel.setClientPreedit(preeditText);
            } else {
                panel.setPreedit(preeditText);
            }
            auto list = std::make_unique<fcitx::CommonCandidateList>();
            const int count = qj_candidate_count();
            list->setPageSize(count > 0 ? count : 1); // 分页在 Rust 侧,这里永远单页
            for (int i = 0; i < count; ++i) {
                std::string text = qj_candidate_text(i);
                std::string comment = qj_candidate_comment(i);
                list->append(std::make_unique<QjCandidate>(i, text, comment));
            }
            const int highlight = qj_highlight();
            if (highlight >= 0 && highlight < count) {
                list->setGlobalCursorIndex(highlight);
            }
            if (count > 0) {
                panel.setCandidateList(std::move(list));
            }
        }
        ic->updatePreedit();
        ic->updateUserInterface(fcitx::UserInterfaceComponent::InputPanel);
    }

    void clearPanel(fcitx::InputContext *ic) {
        ic->inputPanel().reset();
        ic->updatePreedit();
        ic->updateUserInterface(fcitx::UserInterfaceComponent::InputPanel);
    }

private:
    fcitx::Instance *instance_;
    bool ready_ = false;
};

// 点选:全局拿引擎不方便,直接调 Rust 再让事件循环里的 sync 兜底——
// 点选后必须立刻刷新面板,这里通过 ic 自己重建(与 keyEvent 后的 sync 同逻辑)。
QingjianEngine *g_engine = nullptr;

void QjCandidate::select(fcitx::InputContext *ic) const {
    qj_select(offset_);
    if (g_engine != nullptr) {
        g_engine->sync(ic);
    }
}

class QingjianFactory final : public fcitx::AddonFactory {
public:
    fcitx::AddonInstance *create(fcitx::AddonManager *manager) override {
        auto *engine = new QingjianEngine(manager->instance());
        g_engine = engine;
        return engine;
    }
};

} // namespace

FCITX_ADDON_FACTORY_V2(qingjian, QingjianFactory);
