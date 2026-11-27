// Sweet IME input method engine for fcitx5.
//
// A thin frontend: key events go to the Rust engine (sweet_ime.h), and the
// resulting commit text, preedit and candidate page are shown through
// fcitx5's input panel. Candidate windows are drawn by fcitx5 (classicui on
// Wayland compositors with input-method-v2, such as niri).

#include <fcitx-config/configuration.h>
#include <fcitx-config/iniparser.h>
#include <fcitx-config/rawconfig.h>
#include <fcitx-utils/eventdispatcher.h>
#include <fcitx-utils/key.h>
#include <fcitx-utils/log.h>
#include <fcitx-utils/misc.h>
#include <fcitx-utils/utf8.h>
#include <fcitx/addonfactory.h>
#include <fcitx/addoninstance.h>
#include <fcitx/addonmanager.h>
#include <fcitx/candidatelist.h>
#include <fcitx/inputcontext.h>
#include <fcitx/inputcontextmanager.h>
#include <fcitx/inputcontextproperty.h>
#include <fcitx/inputmethodengine.h>
#include <fcitx/inputpanel.h>
#include <fcitx/instance.h>
#include <fcitx/text.h>
#include <fcitx/userinterface.h>

#include <memory>
#include <string>
#include <utility>
#include <vector>

#include "sweet_ime.h"

#ifndef SWEET_SETTINGS_PATH
#define SWEET_SETTINGS_PATH "sweet-settings"
#endif

namespace {

FCITX_DEFINE_LOG_CATEGORY(sweet_log, "sweetime");
#define SWEET_DEBUG() FCITX_LOGC(sweet_log, Debug)
#define SWEET_WARN() FCITX_LOGC(sweet_log, Warn)

FCITX_CONFIGURATION(
    SweetConfig,
    fcitx::ExternalOption settings{this, "Settings", "Sweet IME の設定", "fcitx://config/addon/sweetime/settings"};);

class SweetEngine;

/// Identifies an input context for the live-conversion callback, which
/// runs on a worker thread and may outlive the context.
struct NotifyTarget {
    SweetEngine *engine;
    fcitx::ICUUID uuid;
};

class SweetState : public fcitx::InputContextProperty {
public:
    SweetState(SweetEngine *engine, fcitx::InputContext &ic);
    ~SweetState() override { sweet_context_free(ctx_); }
    SweetState(const SweetState &) = delete;
    SweetState &operator=(const SweetState &) = delete;

    SweetContext *ctx() const { return ctx_; }
    void resetKeys() {
        sweet_reset_modifier_tap(ctx_);
        firstLetter_ = true;
    }
    bool takeFirstLetter() { return std::exchange(firstLetter_, false); }

private:
    SweetContext *ctx_;
    NotifyTarget target_;
    bool firstLetter_ = true;
};

class SweetEngine : public fcitx::InputMethodEngineV2 {
public:
    explicit SweetEngine(fcitx::Instance *instance)
        : instance_(instance),
          factory_([this](fcitx::InputContext &ic) { return new SweetState(this, ic); }) {
        if (!sweet_init()) {
            SWEET_WARN() << "system.dict not found; Sweet IME will pass keys through";
        }
        instance_->inputContextManager().registerProperty("sweetimeState", &factory_);
        dispatcher_.attach(&instance_->eventLoop());
    }

    ~SweetEngine() override { dispatcher_.detach(); }

    void keyEvent(const fcitx::InputMethodEntry &, fcitx::KeyEvent &event) override {
        auto *ic = event.inputContext();
        auto *input = state(ic);
        auto *ctx = input->ctx();
        // Not normalized: normalize() drops Shift from Shift+letter, which
        // starts a temporary English word.
        const fcitx::Key key = event.rawKey();
        const auto states = key.states();
        uint32_t mods = event.isRelease() ? SWEET_MOD_RELEASE : 0;
        if (states.test(fcitx::KeyState::Super)) {
            mods |= SWEET_MOD_SUPER;
        }
        if (states.test(fcitx::KeyState::Shift)) {
            mods |= SWEET_MOD_SHIFT;
        }
        if (states.test(fcitx::KeyState::Ctrl)) {
            mods |= SWEET_MOD_CTRL;
        }
        if (states.test(fcitx::KeyState::Alt)) {
            mods |= SWEET_MOD_ALT;
        }
        const uint32_t sym = static_cast<uint32_t>(key.sym());
        const uint32_t unicode = fcitx::Key::keySymToUnicode(key.sym());
        if (!event.isRelease() && !sweet_is_active(ctx)) {
            setLeftContext(ic, ctx);
        }
        const uint32_t r = sweet_process_key(ctx, sym, unicode, mods, allowCloud(ic));
        if (!event.isRelease() &&
            ((unicode >= 'a' && unicode <= 'z') || (unicode >= 'A' && unicode <= 'Z')) &&
            input->takeFirstLetter()) {
            SWEET_DEBUG() << "First letter after input-context activation: open=" << sweet_is_open(ctx)
                          << " modifiers=" << mods << " consumed=" << !!(r & SWEET_RESULT_CONSUMED);
        }
        if (!(r & SWEET_RESULT_CONSUMED)) {
            return;
        }
        // Modifier presses go to the client, so send the matching release too.
        if (!event.isRelease()) {
            event.filterAndAccept();
        }
        updateUI(ic, ctx);
        if (r & SWEET_RESULT_MODE_CHANGED) {
            ic->updateUserInterface(fcitx::UserInterfaceComponent::StatusArea);
            instance_->showInputMethodInformation(ic);
        }
    }

    void activate(const fcitx::InputMethodEntry &, fcitx::InputContextEvent &event) override {
        auto *ic = event.inputContext();
        state(ic)->resetKeys();
        ic->updateUserInterface(fcitx::UserInterfaceComponent::StatusArea);
    }

    void deactivate(const fcitx::InputMethodEntry &, fcitx::InputContextEvent &event) override {
        // Focus moved away or another input method was chosen: keep the
        // text, as Windows does.
        auto *ic = event.inputContext();
        auto *input = state(ic);
        input->resetKeys();
        auto *ctx = input->ctx();
        if (sweet_is_active(ctx)) {
            sweet_commit_all(ctx);
            updateUI(ic, ctx);
        }
    }

    void reset(const fcitx::InputMethodEntry &, fcitx::InputContextEvent &event) override {
        auto *ic = event.inputContext();
        auto *input = state(ic);
        input->resetKeys();
        auto *ctx = input->ctx();
        if (sweet_is_active(ctx)) {
            sweet_commit_all(ctx);
            updateUI(ic, ctx);
        }
    }

    std::string subModeLabelImpl(const fcitx::InputMethodEntry &, fcitx::InputContext &ic) override {
        return sweet_is_open(state(&ic)->ctx()) ? "あ" : "A";
    }

    const fcitx::Configuration *getConfig() const override { return &config_; }

    void setSubConfig(const std::string &path, const fcitx::RawConfig &) override {
        if (path == "settings") {
            launchSettings();
        }
    }

    /// A candidate was clicked (absolute index).
    void selectCandidate(fcitx::InputContext *ic, uint32_t index) {
        auto *ctx = state(ic)->ctx();
        if (sweet_select_candidate(ctx, index)) {
            updateUI(ic, ctx);
        }
    }

    /// Worker thread: a refined live conversion is ready.
    static void onLiveReady(void *user) {
        auto target = *static_cast<NotifyTarget *>(user);
        target.engine->dispatcher_.schedule([target]() { target.engine->applyLive(target.uuid); });
    }

private:
    SweetState *state(fcitx::InputContext *ic) { return ic->propertyFor(&factory_); }

    static bool allowCloud(fcitx::InputContext *ic) {
        const auto flags = ic->capabilityFlags();
        return !flags.test(fcitx::CapabilityFlag::Password) &&
               !flags.test(fcitx::CapabilityFlag::Sensitive);
    }

    void applyLive(const fcitx::ICUUID &uuid) {
        auto *ic = instance_->inputContextManager().findByUUID(uuid);
        if (!ic || instance_->inputMethodEngine(ic) != this) {
            return;
        }
        auto *ctx = state(ic)->ctx();
        if (sweet_apply_live(ctx, allowCloud(ic))) {
            updateUI(ic, ctx);
        }
    }

    void setLeftContext(fcitx::InputContext *ic, SweetContext *ctx) {
        std::string left;
        if (ic->capabilityFlags().test(fcitx::CapabilityFlag::SurroundingText) && ic->surroundingText().isValid()) {
            const auto &st = ic->surroundingText();
            const std::string &text = st.text();
            const auto len = fcitx::utf8::length(text);
            if (len != fcitx::utf8::INVALID_LENGTH && st.cursor() <= len) {
                auto end = fcitx::utf8::nextNChar(text.begin(), st.cursor());
                left.assign(text.begin(), end);
            }
        }
        sweet_set_left_context(ctx, left.c_str());
    }

    void updateUI(fcitx::InputContext *ic, SweetContext *ctx);

    static void launchSettings() {
        fcitx::startProcess({SWEET_SETTINGS_PATH});
    }

    fcitx::Instance *instance_;
    fcitx::FactoryFor<SweetState> factory_;
    fcitx::EventDispatcher dispatcher_;
    SweetConfig config_;
};

SweetState::SweetState(SweetEngine *engine, fcitx::InputContext &ic)
    : ctx_(sweet_context_new()), target_{engine, ic.uuid()} {
    sweet_context_set_notify(ctx_, &SweetEngine::onLiveReady, &target_);
}

class SweetCandidateWord : public fcitx::CandidateWord {
public:
    SweetCandidateWord(SweetEngine *engine, const char *text, uint32_t index)
        : fcitx::CandidateWord(fcitx::Text(text)), engine_(engine), index_(index) {}

    void select(fcitx::InputContext *ic) const override { engine_->selectCandidate(ic, index_); }

private:
    SweetEngine *engine_;
    uint32_t index_;
};

void SweetEngine::updateUI(fcitx::InputContext *ic, SweetContext *ctx) {
    const std::string commit = sweet_commit_text(ctx);
    if (!commit.empty()) {
        ic->commitString(commit);
    }
    auto &panel = ic->inputPanel();
    panel.reset();

    const std::string pre = sweet_preedit_text(ctx);
    fcitx::Text text;
    const uint32_t spans = sweet_preedit_span_count(ctx);
    for (uint32_t i = 0; i < spans; ++i) {
        uint32_t start = 0, end = 0, attr = 0;
        sweet_preedit_span(ctx, i, &start, &end, &attr);
        if (start > end || end > pre.size()) {
            continue;
        }
        fcitx::TextFormatFlags fmt = fcitx::TextFormatFlag::Underline;
        if (attr == SWEET_SPAN_FOCUSED) {
            fmt |= fcitx::TextFormatFlag::HighLight;
        }
        text.append(pre.substr(start, end - start), fmt);
    }
    text.setCursor(static_cast<int>(sweet_preedit_caret(ctx)));
    if (ic->capabilityFlags().test(fcitx::CapabilityFlag::Preedit)) {
        panel.setClientPreedit(text);
    } else {
        panel.setPreedit(text);
    }

    const uint32_t n = sweet_candidate_count(ctx);
    if (n > 0) {
        const bool suggestion = sweet_candidate_kind(ctx) == 1;
        const uint32_t first = sweet_candidate_page_start(ctx);
        auto list = std::make_unique<fcitx::CommonCandidateList>();
        list->setLayoutHint(fcitx::CandidateLayoutHint::Vertical);
        list->setPageSize(static_cast<int>(n));
        std::vector<std::string> labels;
        for (uint32_t i = 0; i < n; ++i) {
            list->append<SweetCandidateWord>(this, sweet_candidate(ctx, i), first + i);
            // Digits type text while predictions are shown, so no labels.
            labels.push_back(suggestion ? "" : std::to_string(i + 1) + ". ");
        }
        list->setLabels(labels);
        const int cursor = sweet_candidate_cursor(ctx);
        if (cursor >= 0 && cursor < static_cast<int>(n)) {
            list->setGlobalCursorIndex(cursor);
        }
        panel.setCandidateList(std::move(list));
        const std::string footer =
            suggestion ? "Tab で選択"
                       : std::to_string(first + static_cast<uint32_t>(cursor) + 1) + " / " +
                             std::to_string(sweet_candidate_total(ctx));
        panel.setAuxDown(fcitx::Text(footer));
    }
    ic->updatePreedit();
    ic->updateUserInterface(fcitx::UserInterfaceComponent::InputPanel);
}

class SweetEngineFactory : public fcitx::AddonFactory {
public:
    fcitx::AddonInstance *create(fcitx::AddonManager *manager) override {
        return new SweetEngine(manager->instance());
    }
};

} // namespace

FCITX_ADDON_FACTORY(SweetEngineFactory);
