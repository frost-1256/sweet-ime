// Sweet UI: custom animated fcitx5 UserInterface addon.
//
// Draws a Fluent-style candidate window with Cairo + Pango featuring:
// - Per-pixel ARGB rounded corners, drop shadow, and user-theme background opacity
// - 90 ms fade-in animation and 70 ms cubic ease-out selection highlight slide
// - Optional animated rainbow border (gaming_mode)
// - Wayland ext-background-effect-v1 compositor blur when translucent
// - Direct theme sync from Sweet IME's config.txt + user_theme.txt
// - Wayland (wlr-layer-shell-v1 + double-buffered wl_shm) and X11/XCB backends

#include <cairo/cairo.h>
#include <pango/pangocairo.h>

#include <fcitx-utils/event.h>
#include <fcitx-utils/log.h>
#include <fcitx-utils/macros.h>
#include <fcitx/addonfactory.h>
#include <fcitx/addoninstance.h>
#include <fcitx/addonmanager.h>
#include <fcitx/candidatelist.h>
#include <fcitx/inputcontext.h>
#include <fcitx/inputpanel.h>
#include <fcitx/instance.h>
#include <fcitx/text.h>
#include <fcitx/userinterface.h>
#include <fcitx/userinterfacemanager.h>

#ifdef SWEET_HAVE_WAYLAND
#include <linux/input-event-codes.h>
#include <sys/mman.h>
#include <unistd.h>
#include <wayland-client.h>
#include <wayland_public.h>
#include "ext-background-effect-v1-client-protocol.h"
#define namespace wl_namespace
#include "wlr-layer-shell-unstable-v1-client-protocol.h"
#undef namespace
#endif

#ifdef SWEET_HAVE_XCB
#include <xcb/xcb.h>
#include <xcb_public.h>
#endif

#ifdef SWEET_HAVE_NOTIFICATIONITEM
#include <notificationitem_public.h>
#endif

#include <algorithm>
#include <array>
#include <chrono>
#include <cmath>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <memory>
#include <string>
#include <unordered_map>
#include <vector>

#include "sweet_ime.h"

namespace {

FCITX_DEFINE_LOG_CATEGORY(sweet_ui_log, "sweetui");
#define SWEETUI_DEBUG() FCITX_LOGC(sweet_ui_log, Debug)
#define SWEETUI_WARN() FCITX_LOGC(sweet_ui_log, Warn)

using Clock = std::chrono::steady_clock;

constexpr double FADE_MS = 90.0;
constexpr double SLIDE_MS = 70.0;
constexpr double REFADE_AFTER_MS = 600.0;
constexpr double SHADOW_PAD = 6.0;
constexpr double CORNER_RADIUS = 8.0;
constexpr double HIGHLIGHT_RADIUS = 5.0;
constexpr double MIN_PANEL_W = 148.0;

struct RectF {
    double x = 0.0;
    double y = 0.0;
    double w = 0.0;
    double h = 0.0;

    bool contains(double px, double py) const {
        return px >= x && px <= x + w && py >= y && py <= y + h;
    }

    bool approxEqual(const RectF &o) const {
        return std::fabs(x - o.x) < 0.5 && std::fabs(y - o.y) < 0.5 &&
               std::fabs(w - o.w) < 0.5 && std::fabs(h - o.h) < 0.5;
    }
};

struct CandidateItem {
    std::string label;
    std::string text;
};

struct PanelSnapshot {
    std::string preedit;
    std::string aux_up;
    std::string aux_down;
    std::vector<CandidateItem> items;
    int cursor = -1;
    bool visible = false;
};

struct ClassicUIPrefs {
    std::string font = "Sans 11";
    bool vertical = true;
};

ClassicUIPrefs readClassicUIPrefs() {
    ClassicUIPrefs prefs;
    const char *xdg = std::getenv("XDG_CONFIG_HOME");
    std::string path;
    if (xdg && xdg[0] == '/') {
        path = std::string(xdg) + "/fcitx5/conf/classicui.conf";
    } else if (const char *home = std::getenv("HOME")) {
        path = std::string(home) + "/.config/fcitx5/conf/classicui.conf";
    } else {
        return prefs;
    }
    std::ifstream in(path);
    std::string line;
    while (std::getline(in, line)) {
        auto trim = [](std::string s) {
            auto a = s.find_first_not_of(" \t\r\n");
            if (a == std::string::npos) return std::string();
            auto b = s.find_last_not_of(" \t\r\n");
            return s.substr(a, b - a + 1);
        };
        line = trim(line);
        if (line.empty() || line[0] == '#') continue;
        if (line.front() == '[' && line.back() == ']') break;
        auto eq = line.find('=');
        if (eq == std::string::npos) continue;
        std::string key = trim(line.substr(0, eq));
        std::string val = trim(line.substr(eq + 1));
        if (val.size() >= 2 && val.front() == '"' && val.back() == '"') {
            val = val.substr(1, val.size() - 2);
        }
        if (key == "Font" && !val.empty()) {
            prefs.font = val;
        } else if (key == "Vertical Candidate List") {
            prefs.vertical = (val == "True" || val == "true" || val == "1");
        }
    }
    return prefs;
}

void roundedRectPath(cairo_t *cr, double x, double y, double w, double h, double r) {
    r = std::min(r, std::min(w, h) * 0.5);
    constexpr double deg = M_PI / 180.0;
    cairo_new_sub_path(cr);
    cairo_arc(cr, x + w - r, y + r, r, -90.0 * deg, 0.0);
    cairo_arc(cr, x + w - r, y + h - r, r, 0.0, 90.0 * deg);
    cairo_arc(cr, x + r, y + h - r, r, 90.0 * deg, 180.0 * deg);
    cairo_arc(cr, x + r, y + r, r, 180.0 * deg, 270.0 * deg);
    cairo_close_path(cr);
}

void hsvToRgb(double h, double s, double v, double &r, double &g, double &b) {
    h = std::fmod(h, 1.0);
    if (h < 0.0) h += 1.0;
    const double c = v * s;
    const double x = c * (1.0 - std::fabs(std::fmod(h * 6.0, 2.0) - 1.0));
    const double m = v - c;
    double rp = 0, gp = 0, bp = 0;
    const int seg = static_cast<int>(h * 6.0) % 6;
    switch (seg) {
        case 0: rp = c; gp = x; bp = 0; break;
        case 1: rp = x; gp = c; bp = 0; break;
        case 2: rp = 0; gp = c; bp = x; break;
        case 3: rp = 0; gp = x; bp = c; break;
        case 4: rp = x; gp = 0; bp = c; break;
        default: rp = c; gp = 0; bp = x; break;
    }
    r = rp + m;
    g = gp + m;
    b = bp + m;
}

double easeOutCubic(double t) {
    t = std::clamp(t, 0.0, 1.0);
    const double u = 1.0 - t;
    return 1.0 - u * u * u;
}

class SweetPanelRenderer {
public:
    SweetPanelRenderer() { reloadConfig(); }

    void reloadConfig() {
        sweet_ui_load_theme(&theme_);
        prefs_ = readClassicUIPrefs();
        lastConfigReload_ = Clock::now();
        if (snap_.visible) {
            measure();
            if (snap_.cursor >= 0 && snap_.cursor < static_cast<int>(itemRects_.size())) {
                slideFrom_ = itemRects_[snap_.cursor];
                slideTo_ = itemRects_[snap_.cursor];
            }
        }
    }

    void maybeReloadConfig() {
        const auto elapsed = std::chrono::duration_cast<std::chrono::milliseconds>(
                                 Clock::now() - lastConfigReload_)
                                 .count();
        if (elapsed >= 500) {
            sweet_ui_load_theme(&theme_);
            prefs_ = readClassicUIPrefs();
            lastConfigReload_ = Clock::now();
        }
    }

    void updateData(const PanelSnapshot &next) {
        const auto now = Clock::now();
        maybeReloadConfig();

        const bool wasVisible = snap_.visible;
        const RectF oldHighlight = currentHighlightRect(now);

        snap_ = next;

        if (!snap_.visible) {
            if (wasVisible) {
                hiddenAt_ = now;
            }
            itemRects_.clear();
            hoverIndex_ = -1;
            return;
        }

        measure();

        if (!wasVisible) {
            const auto sinceHidden =
                std::chrono::duration_cast<std::chrono::milliseconds>(now - hiddenAt_).count();
            fadeStart_ = (sinceHidden < static_cast<long long>(REFADE_AFTER_MS))
                             ? (now - std::chrono::milliseconds(static_cast<long long>(FADE_MS)))
                             : now;
            if (snap_.cursor >= 0 && snap_.cursor < static_cast<int>(itemRects_.size())) {
                slideFrom_ = itemRects_[snap_.cursor];
                slideTo_ = itemRects_[snap_.cursor];
            }
            slideStart_ = now - std::chrono::milliseconds(static_cast<long long>(SLIDE_MS));
        } else if (snap_.cursor >= 0 && snap_.cursor < static_cast<int>(itemRects_.size())) {
            const RectF target = itemRects_[snap_.cursor];
            // Keep an in-progress slide intact if the target geometry has not changed.
            if (!target.approxEqual(slideTo_)) {
                if (oldHighlight.w > 0.0 && oldHighlight.h > 0.0) {
                    slideFrom_ = oldHighlight;
                    slideTo_ = target;
                    slideStart_ = now;
                } else {
                    slideFrom_ = target;
                    slideTo_ = target;
                }
            }
        }
    }

    bool setHover(double x, double y) {
        int found = -1;
        for (size_t i = 0; i < itemRects_.size(); ++i) {
            if (itemRects_[i].contains(x, y)) {
                found = static_cast<int>(i);
                break;
            }
        }
        if (found != hoverIndex_) {
            hoverIndex_ = found;
            return true;
        }
        return false;
    }

    int hitTest(double x, double y) const {
        for (size_t i = 0; i < itemRects_.size(); ++i) {
            if (itemRects_[i].contains(x, y)) {
                return static_cast<int>(i);
            }
        }
        return -1;
    }

    bool needsAnimation() const {
        if (!snap_.visible) return false;
        if (theme_.gaming_mode) return true;
        const auto now = Clock::now();
        const double fadeMs =
            std::chrono::duration<double, std::milli>(now - fadeStart_).count();
        const double slideMs =
            std::chrono::duration<double, std::milli>(now - slideStart_).count();
        return fadeMs < FADE_MS || slideMs < SLIDE_MS;
    }

    int width() const { return width_; }
    int height() const { return height_; }
    bool visible() const { return snap_.visible; }
    float bgAlpha() const { return theme_.bg_alpha; }

    void paint(cairo_t *cr) {
        if (!snap_.visible || width_ <= 0 || height_ <= 0) return;
        const auto now = Clock::now();
        const double fadeRaw =
            std::chrono::duration<double, std::milli>(now - fadeStart_).count() / FADE_MS;
        const double fade = easeOutCubic(fadeRaw);

        cairo_save(cr);
        cairo_set_operator(cr, CAIRO_OPERATOR_SOURCE);
        cairo_set_source_rgba(cr, 0.0, 0.0, 0.0, 0.0);
        cairo_paint(cr);
        cairo_set_operator(cr, CAIRO_OPERATOR_OVER);

        const double cardX = SHADOW_PAD;
        const double cardY = SHADOW_PAD;
        const double cardW = width_ - 2.0 * SHADOW_PAD;
        const double cardH = height_ - 2.0 * SHADOW_PAD;

        // Soft drop shadow.
        for (int i = 3; i >= 1; --i) {
            const double spread = static_cast<double>(i) * 1.5;
            roundedRectPath(cr, cardX - spread * 0.5, cardY + spread * 0.6,
                            cardW + spread, cardH + spread, CORNER_RADIUS + spread * 0.4);
            cairo_set_source_rgba(cr, 0.0, 0.0, 0.0, (theme_.dark ? 0.14 : 0.07) * fade);
            cairo_fill(cr);
        }

        // Translucent rounded panel background.
        roundedRectPath(cr, cardX, cardY, cardW, cardH, CORNER_RADIUS);
        cairo_set_source_rgba(cr, theme_.bg[0] / 255.0, theme_.bg[1] / 255.0,
                              theme_.bg[2] / 255.0, theme_.bg_alpha * fade);
        cairo_fill(cr);

        // Border (normal or animated rainbow in gaming mode).
        roundedRectPath(cr, cardX + 0.5, cardY + 0.5, cardW - 1.0, cardH - 1.0, CORNER_RADIUS);
        if (theme_.gaming_mode) {
            const double sec =
                std::chrono::duration<double>(now.time_since_epoch()).count();
            cairo_pattern_t *pat =
                cairo_pattern_create_linear(cardX, cardY, cardX + cardW, cardY + cardH);
            for (int s = 0; s <= 4; ++s) {
                double r, g, b;
                hsvToRgb(sec * 0.35 + s * 0.25, 0.85, 1.0, r, g, b);
                cairo_pattern_add_color_stop_rgba(pat, s * 0.25, r, g, b, fade);
            }
            cairo_set_source(cr, pat);
            cairo_set_line_width(cr, 1.8);
            cairo_stroke(cr);
            cairo_pattern_destroy(pat);
        } else {
            cairo_set_source_rgba(cr, theme_.border[0] / 255.0, theme_.border[1] / 255.0,
                                  theme_.border[2] / 255.0, fade);
            cairo_set_line_width(cr, 1.0);
            cairo_stroke(cr);
        }

        // Hover highlight (when hovering a non-selected item).
        if (hoverIndex_ >= 0 && hoverIndex_ < static_cast<int>(itemRects_.size()) &&
            hoverIndex_ != snap_.cursor) {
            const auto &hr = itemRects_[hoverIndex_];
            roundedRectPath(cr, hr.x, hr.y, hr.w, hr.h, HIGHLIGHT_RADIUS);
            cairo_set_source_rgba(cr, theme_.hover[0] / 255.0, theme_.hover[1] / 255.0,
                                  theme_.hover[2] / 255.0, theme_.hover_alpha * 0.55 * fade);
            cairo_fill(cr);
        }

        // Animated selection highlight pill + accent bar.
        if (snap_.cursor >= 0 && snap_.cursor < static_cast<int>(itemRects_.size())) {
            const RectF pill = currentHighlightRect(now);
            roundedRectPath(cr, pill.x, pill.y, pill.w, pill.h, HIGHLIGHT_RADIUS);
            cairo_set_source_rgba(cr, theme_.hover[0] / 255.0, theme_.hover[1] / 255.0,
                                  theme_.hover[2] / 255.0, theme_.hover_alpha * fade);
            cairo_fill(cr);

            if (prefs_.vertical) {
                const double barH = std::max(8.0, pill.h - 14.0);
                roundedRectPath(cr, pill.x, pill.y + (pill.h - barH) * 0.5, 3.0, barH, 1.5);
            } else {
                const double barW = std::max(10.0, pill.w - 14.0);
                roundedRectPath(cr, pill.x + (pill.w - barW) * 0.5, pill.y + pill.h - 3.0,
                                barW, 3.0, 1.5);
            }
            cairo_set_source_rgba(cr, theme_.accent[0] / 255.0, theme_.accent[1] / 255.0,
                                  theme_.accent[2] / 255.0, fade);
            cairo_fill(cr);
        }

        PangoFontDescription *fontDesc = pango_font_description_from_string(prefs_.font.c_str());
        PangoFontDescription *smallDesc = pango_font_description_copy(fontDesc);
        if (pango_font_description_get_size(smallDesc) > 0) {
            const int sz = pango_font_description_get_size(smallDesc);
            const int smallSz = std::max(8 * PANGO_SCALE, static_cast<int>(sz * 0.78));
            if (pango_font_description_get_size_is_absolute(smallDesc)) {
                pango_font_description_set_absolute_size(smallDesc, smallSz);
            } else {
                pango_font_description_set_size(smallDesc, smallSz);
            }
        }

        PangoLayout *layout = pango_cairo_create_layout(cr);
        const double padV = static_cast<double>(theme_.panel_padding);
        const double padH = padV + 4.0;
        const double contentPad = std::max(4.0, padV - 1.0);
        double headerY = cardY + contentPad + 2.0;

        // Optional auxUp line (e.g. input-method notification banner).
        if (!snap_.aux_up.empty()) {
            pango_layout_set_font_description(layout, fontDesc);
            pango_layout_set_text(layout, snap_.aux_up.c_str(), -1);
            int w = 0, h = 0;
            pango_layout_get_pixel_size(layout, &w, &h);
            cairo_set_source_rgba(cr, theme_.text[0] / 255.0, theme_.text[1] / 255.0,
                                  theme_.text[2] / 255.0, fade);
            cairo_move_to(cr, cardX + contentPad + padH, headerY);
            pango_cairo_show_layout(cr, layout);
            headerY += h + 6.0;
        }

        // Optional preedit line (when client doesn't display inline preedit).
        if (!snap_.preedit.empty()) {
            pango_layout_set_font_description(layout, fontDesc);
            pango_layout_set_text(layout, snap_.preedit.c_str(), -1);
            cairo_set_source_rgba(cr, theme_.text[0] / 255.0, theme_.text[1] / 255.0,
                                  theme_.text[2] / 255.0, fade);
            cairo_move_to(cr, cardX + contentPad + padH, headerY);
            pango_cairo_show_layout(cr, layout);
        }

        // Candidate items.
        for (size_t i = 0; i < snap_.items.size() && i < itemRects_.size(); ++i) {
            const auto &item = snap_.items[i];
            const auto &r = itemRects_[i];
            double tx = r.x + padH;
            if (!item.label.empty()) {
                pango_layout_set_font_description(layout, smallDesc);
                pango_layout_set_text(layout, item.label.c_str(), -1);
                int lw = 0, lh = 0;
                pango_layout_get_pixel_size(layout, &lw, &lh);
                cairo_set_source_rgba(cr, theme_.sub[0] / 255.0, theme_.sub[1] / 255.0,
                                      theme_.sub[2] / 255.0, fade);
                cairo_move_to(cr, tx, r.y + (r.h - lh) * 0.5);
                pango_cairo_show_layout(cr, layout);
                tx += lw + 4.0;
            }
            pango_layout_set_font_description(layout, fontDesc);
            pango_layout_set_text(layout, item.text.c_str(), -1);
            int tw = 0, th = 0;
            pango_layout_get_pixel_size(layout, &tw, &th);
            cairo_set_source_rgba(cr, theme_.text[0] / 255.0, theme_.text[1] / 255.0,
                                  theme_.text[2] / 255.0, fade);
            cairo_move_to(cr, tx, r.y + (r.h - th) * 0.5);
            pango_cairo_show_layout(cr, layout);
        }

        // Footer (aux_down).
        if (!snap_.aux_down.empty()) {
            pango_layout_set_font_description(layout, smallDesc);
            pango_layout_set_text(layout, snap_.aux_down.c_str(), -1);
            int fw = 0, fh = 0;
            pango_layout_get_pixel_size(layout, &fw, &fh);
            cairo_set_source_rgba(cr, theme_.sub[0] / 255.0, theme_.sub[1] / 255.0,
                                  theme_.sub[2] / 255.0, fade);
            cairo_move_to(cr, cardX + cardW - contentPad - padH - fw,
                          cardY + cardH - contentPad - fh - 1.0);
            pango_cairo_show_layout(cr, layout);
        }

        g_object_unref(layout);
        pango_font_description_free(smallDesc);
        pango_font_description_free(fontDesc);
        cairo_restore(cr);
    }

private:
    RectF currentHighlightRect(Clock::time_point now) const {
        if (snap_.cursor < 0 || snap_.cursor >= static_cast<int>(itemRects_.size())) {
            return {};
        }
        const double raw =
            std::chrono::duration<double, std::milli>(now - slideStart_).count() / SLIDE_MS;
        const double t = easeOutCubic(raw);
        return {
            slideFrom_.x + (slideTo_.x - slideFrom_.x) * t,
            slideFrom_.y + (slideTo_.y - slideFrom_.y) * t,
            slideFrom_.w + (slideTo_.w - slideFrom_.w) * t,
            slideFrom_.h + (slideTo_.h - slideFrom_.h) * t,
        };
    }

    void measure() {
        itemRects_.clear();
        cairo_surface_t *tmp = cairo_image_surface_create(CAIRO_FORMAT_ARGB32, 1, 1);
        cairo_t *cr = cairo_create(tmp);
        PangoLayout *layout = pango_cairo_create_layout(cr);
        PangoFontDescription *fontDesc = pango_font_description_from_string(prefs_.font.c_str());
        PangoFontDescription *smallDesc = pango_font_description_copy(fontDesc);
        if (pango_font_description_get_size(smallDesc) > 0) {
            const int sz = pango_font_description_get_size(smallDesc);
            const int smallSz = std::max(8 * PANGO_SCALE, static_cast<int>(sz * 0.78));
            if (pango_font_description_get_size_is_absolute(smallDesc)) {
                pango_font_description_set_absolute_size(smallDesc, smallSz);
            } else {
                pango_font_description_set_size(smallDesc, smallSz);
            }
        }

        const double padV = static_cast<double>(theme_.panel_padding);
        const double padH = padV + 4.0;
        const double contentPad = std::max(4.0, padV - 1.0);

        double headW = 0.0, headH = 0.0;
        if (!snap_.aux_up.empty()) {
            pango_layout_set_font_description(layout, fontDesc);
            pango_layout_set_text(layout, snap_.aux_up.c_str(), -1);
            int w = 0, h = 0;
            pango_layout_get_pixel_size(layout, &w, &h);
            headW = std::max(headW, w + 2.0 * padH);
            headH += h + 6.0;
        }
        if (!snap_.preedit.empty()) {
            pango_layout_set_font_description(layout, fontDesc);
            pango_layout_set_text(layout, snap_.preedit.c_str(), -1);
            int w = 0, h = 0;
            pango_layout_get_pixel_size(layout, &w, &h);
            headW = std::max(headW, w + 2.0 * padH);
            headH += h + 6.0;
        }

        double footW = 0.0, footH = 0.0;
        if (!snap_.aux_down.empty()) {
            pango_layout_set_font_description(layout, smallDesc);
            pango_layout_set_text(layout, snap_.aux_down.c_str(), -1);
            int w = 0, h = 0;
            pango_layout_get_pixel_size(layout, &w, &h);
            footW = w + 2.0 * padH;
            footH = h + 6.0;
        }

        std::vector<std::pair<double, double>> itemSizes;
        itemSizes.reserve(snap_.items.size());
        double maxItemW = 0.0;
        double maxItemH = 0.0;
        double sumItemW = 0.0;
        for (const auto &item : snap_.items) {
            double w = 2.0 * padH;
            double h = 0.0;
            if (!item.label.empty()) {
                pango_layout_set_font_description(layout, smallDesc);
                pango_layout_set_text(layout, item.label.c_str(), -1);
                int lw = 0, lh = 0;
                pango_layout_get_pixel_size(layout, &lw, &lh);
                w += lw + 4.0;
                h = std::max(h, static_cast<double>(lh));
            }
            pango_layout_set_font_description(layout, fontDesc);
            pango_layout_set_text(layout, item.text.c_str(), -1);
            int tw = 0, th = 0;
            pango_layout_get_pixel_size(layout, &tw, &th);
            w += tw;
            h = std::max(h, static_cast<double>(th)) + 2.0 * padV;
            itemSizes.emplace_back(w, h);
            maxItemW = std::max(maxItemW, w);
            maxItemH = std::max(maxItemH, h);
            sumItemW += w + 4.0;
        }
        if (!itemSizes.empty()) {
            sumItemW -= 4.0;
        }

        double cardW = 0.0;
        double cardH = 0.0;
        if (prefs_.vertical) {
            const double innerW = std::max({MIN_PANEL_W, headW, footW, maxItemW});
            double y = SHADOW_PAD + contentPad + headH;
            for (const auto &sz : itemSizes) {
                itemRects_.push_back({SHADOW_PAD + contentPad, y, innerW, sz.second});
                y += sz.second + 2.0;
            }
            if (!itemSizes.empty()) y -= 2.0;
            cardW = innerW + 2.0 * contentPad;
            cardH = (y - SHADOW_PAD) + footH + contentPad;
        } else {
            const double innerW = std::max({MIN_PANEL_W, headW, footW, sumItemW});
            double x = SHADOW_PAD + contentPad;
            const double y = SHADOW_PAD + contentPad + headH;
            for (const auto &sz : itemSizes) {
                itemRects_.push_back({x, y, sz.first, maxItemH});
                x += sz.first + 4.0;
            }
            cardW = innerW + 2.0 * contentPad;
            cardH = headH + maxItemH + footH + 2.0 * contentPad;
        }

        width_ = static_cast<int>(std::ceil(cardW + 2.0 * SHADOW_PAD));
        height_ = static_cast<int>(std::ceil(cardH + 2.0 * SHADOW_PAD));

        g_object_unref(layout);
        pango_font_description_free(smallDesc);
        pango_font_description_free(fontDesc);
        cairo_destroy(cr);
        cairo_surface_destroy(tmp);
    }

    SweetUiTheme theme_{};
    ClassicUIPrefs prefs_;
    PanelSnapshot snap_;
    std::vector<RectF> itemRects_;
    int width_ = 0;
    int height_ = 0;
    int hoverIndex_ = -1;
    Clock::time_point lastConfigReload_{};
    Clock::time_point fadeStart_{};
    Clock::time_point slideStart_{};
    Clock::time_point hiddenAt_{};
    RectF slideFrom_{};
    RectF slideTo_{};
};

// ---------------------------------------------------------------------------
// X11 / XCB backend
// ---------------------------------------------------------------------------
#ifdef SWEET_HAVE_XCB
class XcbPopup {
public:
    XcbPopup(xcb_connection_t *conn, int screenNum) : conn_(conn) {
        const xcb_setup_t *setup = xcb_get_setup(conn_);
        xcb_screen_iterator_t it = xcb_setup_roots_iterator(setup);
        for (int i = 0; i < screenNum && it.rem; ++i) {
            xcb_screen_next(&it);
        }
        screen_ = it.data;
        if (!screen_) return;

        // Prefer 32-bit TrueColor ARGB visual for per-pixel transparency.
        depth_ = screen_->root_depth;
        visual_ = screen_->root_visual;
        xcb_depth_iterator_t dit = xcb_screen_allowed_depths_iterator(screen_);
        for (; dit.rem; xcb_depth_next(&dit)) {
            if (dit.data->depth == 32) {
                xcb_visualtype_iterator_t vit = xcb_depth_visuals_iterator(dit.data);
                for (; vit.rem; xcb_visualtype_next(&vit)) {
                    if (vit.data->_class == XCB_VISUAL_CLASS_TRUE_COLOR) {
                        depth_ = 32;
                        visual_ = vit.data->visual_id;
                        break;
                    }
                }
            }
            if (depth_ == 32) break;
        }
    }

    ~XcbPopup() { destroy(); }

    xcb_window_t window() const { return win_; }

    void hide() {
        if (win_ != XCB_NONE && mapped_) {
            xcb_unmap_window(conn_, win_);
            xcb_flush(conn_);
            mapped_ = false;
        }
    }

    bool present(SweetPanelRenderer &renderer, const fcitx::Rect &cursor) {
        if (!screen_) return false;
        const int w = renderer.width();
        const int h = renderer.height();
        if (w <= 0 || h <= 0) {
            hide();
            return false;
        }
        ensureWindow(w, h);

        int x = cursor.left() - static_cast<int>(SHADOW_PAD);
        int y = cursor.bottom() + 4 - static_cast<int>(SHADOW_PAD);
        if (y + h > screen_->height_in_pixels) {
            y = cursor.top() - h - 4 + static_cast<int>(SHADOW_PAD);
        }
        x = std::clamp(x, 0, std::max(0, static_cast<int>(screen_->width_in_pixels) - w));
        y = std::clamp(y, 0, std::max(0, static_cast<int>(screen_->height_in_pixels) - h));

        const uint32_t cfg[] = {
            static_cast<uint32_t>(x),
            static_cast<uint32_t>(y),
            static_cast<uint32_t>(w),
            static_cast<uint32_t>(h),
            XCB_STACK_MODE_ABOVE,
        };
        xcb_configure_window(
            conn_, win_,
            XCB_CONFIG_WINDOW_X | XCB_CONFIG_WINDOW_Y | XCB_CONFIG_WINDOW_WIDTH |
                XCB_CONFIG_WINDOW_HEIGHT | XCB_CONFIG_WINDOW_STACK_MODE,
            cfg);

        if (!mapped_) {
            xcb_map_window(conn_, win_);
            mapped_ = true;
        }

        cairo_surface_t *surf = cairo_image_surface_create(CAIRO_FORMAT_ARGB32, w, h);
        cairo_t *cr = cairo_create(surf);
        renderer.paint(cr);
        cairo_surface_flush(surf);
        const unsigned char *data = cairo_image_surface_get_data(surf);
        const int stride = cairo_image_surface_get_stride(surf);

        // Chunk uploads into <= 64 KiB strips so we never exceed libxcb's
        // 262 KiB non-BIG-REQUESTS request limit on the shared connection.
        const int maxRows = std::max(1, 65536 / std::max(1, stride));
        for (int row = 0; row < h; row += maxRows) {
            const int stripH = std::min(maxRows, h - row);
            xcb_put_image(conn_, XCB_IMAGE_FORMAT_Z_PIXMAP, win_, gc_,
                          static_cast<uint16_t>(w), static_cast<uint16_t>(stripH), 0,
                          static_cast<int16_t>(row), 0, depth_,
                          static_cast<uint32_t>(stride * stripH), data + row * stride);
        }
        xcb_flush(conn_);
        cairo_destroy(cr);
        cairo_surface_destroy(surf);
        return true;
    }

private:
    void ensureWindow(int w, int h) {
        if (win_ != XCB_NONE) return;
        win_ = xcb_generate_id(conn_);
        gc_ = xcb_generate_id(conn_);
        uint32_t mask = XCB_CW_BACK_PIXEL | XCB_CW_BORDER_PIXEL | XCB_CW_OVERRIDE_REDIRECT |
                        XCB_CW_EVENT_MASK;
        std::vector<uint32_t> vals;
        if (depth_ == 32) {
            cmap_ = xcb_generate_id(conn_);
            xcb_create_colormap(conn_, XCB_COLORMAP_ALLOC_NONE, cmap_, screen_->root, visual_);
            mask |= XCB_CW_COLORMAP;
            vals = {0, 0, 1,
                    XCB_EVENT_MASK_POINTER_MOTION | XCB_EVENT_MASK_BUTTON_PRESS |
                        XCB_EVENT_MASK_LEAVE_WINDOW,
                    cmap_};
        } else {
            vals = {screen_->black_pixel, 0, 1,
                    XCB_EVENT_MASK_POINTER_MOTION | XCB_EVENT_MASK_BUTTON_PRESS |
                        XCB_EVENT_MASK_LEAVE_WINDOW};
        }
        xcb_create_window(conn_, depth_, win_, screen_->root, 0, 0, static_cast<uint16_t>(w),
                          static_cast<uint16_t>(h), 0, XCB_WINDOW_CLASS_INPUT_OUTPUT, visual_,
                          mask, vals.data());
        xcb_create_gc(conn_, gc_, win_, 0, nullptr);
    }

    void destroy() {
        if (!conn_) return;
        if (gc_ != XCB_NONE) xcb_free_gc(conn_, gc_);
        if (win_ != XCB_NONE) xcb_destroy_window(conn_, win_);
        if (cmap_ != XCB_NONE) xcb_free_colormap(conn_, cmap_);
        xcb_flush(conn_);
        gc_ = XCB_NONE;
        win_ = XCB_NONE;
        cmap_ = XCB_NONE;
        mapped_ = false;
    }

    xcb_connection_t *conn_ = nullptr;
    xcb_screen_t *screen_ = nullptr;
    uint8_t depth_ = 24;
    xcb_visualid_t visual_ = 0;
    xcb_colormap_t cmap_ = XCB_NONE;
    xcb_window_t win_ = XCB_NONE;
    xcb_gcontext_t gc_ = XCB_NONE;
    bool mapped_ = false;
};
#endif

// ---------------------------------------------------------------------------
// Wayland (wlr-layer-shell-v1 + double-buffered wl_shm + ext-background-effect-v1)
// ---------------------------------------------------------------------------
#ifdef SWEET_HAVE_WAYLAND
class WaylandPopup {
public:
    using HoverCb = std::function<void(double, double)>;
    using ClickCb = std::function<void(double, double)>;
    using NotifyCb = std::function<void()>;

    WaylandPopup(wl_display *display, HoverCb onHover, ClickCb onClick, NotifyCb onReady,
                 NotifyCb onConfigured, NotifyCb onInitialClosed)
        : display_(display),
          onHover_(std::move(onHover)),
          onClick_(std::move(onClick)),
          onReady_(std::move(onReady)),
          onConfigured_(std::move(onConfigured)),
          onInitialClosed_(std::move(onInitialClosed)) {
        registry_ = wl_display_get_registry(display_);
        if (registry_) {
            wl_registry_add_listener(registry_, &kRegistryListener, this);
            // Never call wl_display_roundtrip() on fcitx5's shared wl_display*:
            // fcitx5 drives the Wayland event loop itself, and synchronous roundtrips
            // inside callbacks or key dispatch can deadlock or corrupt waylandim.
            wl_display_flush(display_);
        }
    }

    ~WaylandPopup() {
        hide();
        if (pointer_) wl_pointer_destroy(pointer_);
        if (seat_) wl_seat_destroy(seat_);
        for (auto &out : outputs_) {
            out->destroy();
        }
        outputs_.clear();
        if (bgEffectManager_) ext_background_effect_manager_v1_destroy(bgEffectManager_);
        if (layerShell_) {
            if (layerShellVersion_ >= 3) {
                zwlr_layer_shell_v1_destroy(layerShell_);
            } else {
                wl_proxy_destroy(reinterpret_cast<wl_proxy *>(layerShell_));
            }
        }
        if (shm_) wl_shm_destroy(shm_);
        if (compositor_) wl_compositor_destroy(compositor_);
        if (registry_) wl_registry_destroy(registry_);
    }

    bool ready() const {
        return compositor_ != nullptr && shm_ != nullptr && layerShell_ != nullptr;
    }

    bool configured() const { return configured_; }

    void hide() {
        if (bgEffectSurface_) {
            ext_background_effect_surface_v1_destroy(bgEffectSurface_);
            bgEffectSurface_ = nullptr;
        }
        if (layerSurface_) {
            zwlr_layer_surface_v1_destroy(layerSurface_);
            layerSurface_ = nullptr;
        }
        if (surface_) {
            wl_surface_destroy(surface_);
            surface_ = nullptr;
        }
        for (auto &slot : slots_) {
            slot.destroy();
        }
        hostingOutput_ = nullptr;
        configured_ = false;
        pointerInside_ = false;
        if (display_) wl_display_flush(display_);
    }

    bool present(SweetPanelRenderer &renderer, const fcitx::Rect &cursor) {
        if (!ready()) return false;
        const int w = renderer.width();
        const int h = renderer.height();
        if (w <= 0 || h <= 0) {
            hide();
            return false;
        }

        if (!surface_) {
            surface_ = wl_compositor_create_surface(compositor_);
            if (!surface_) return false;
            wl_surface_add_listener(surface_, &kSurfaceListener, this);
            layerSurface_ = zwlr_layer_shell_v1_get_layer_surface(
                layerShell_, surface_, nullptr, ZWLR_LAYER_SHELL_V1_LAYER_OVERLAY, "sweet-ime");
            if (!layerSurface_) {
                hide();
                return false;
            }
            zwlr_layer_surface_v1_add_listener(layerSurface_, &kLayerSurfaceListener, this);
            zwlr_layer_surface_v1_set_keyboard_interactivity(
                layerSurface_, ZWLR_LAYER_SURFACE_V1_KEYBOARD_INTERACTIVITY_NONE);
            zwlr_layer_surface_v1_set_exclusive_zone(layerSurface_, -1);
            configured_ = false;
        }

        lastCursor_ = cursor;
        lastW_ = w;
        lastH_ = h;
        applyPlacement();
        zwlr_layer_surface_v1_set_size(layerSurface_, static_cast<uint32_t>(w),
                                       static_cast<uint32_t>(h));

        if (!configured_) {
            // Commit the initial role state and wait asynchronously for
            // zwlr_layer_surface_v1.configure via fcitx5's Wayland event loop.
            wl_surface_commit(surface_);
            wl_display_flush(display_);
            return true;
        }

        // Apply Wayland ext-background-effect-v1 compositor blur when translucent.
        if (bgEffectManager_ && blurSupported_) {
            const bool wantBlur = renderer.bgAlpha() < 0.99f;
            if (wantBlur) {
                if (!bgEffectSurface_) {
                    bgEffectSurface_ = ext_background_effect_manager_v1_get_background_effect(
                        bgEffectManager_, surface_);
                }
                if (bgEffectSurface_) {
                    wl_region *region = wl_compositor_create_region(compositor_);
                    const int rx = static_cast<int>(SHADOW_PAD);
                    const int ry = static_cast<int>(SHADOW_PAD);
                    const int rw = std::max(0, w - 2 * rx);
                    const int rh = std::max(0, h - 2 * ry);
                    wl_region_add(region, rx, ry, rw, rh);
                    ext_background_effect_surface_v1_set_blur_region(bgEffectSurface_, region);
                    wl_region_destroy(region);
                }
            } else if (bgEffectSurface_) {
                ext_background_effect_surface_v1_set_blur_region(bgEffectSurface_, nullptr);
            }
        }

        ShmSlot *slot = acquireSlot(w, h);
        if (!slot) {
            return false;
        }

        cairo_surface_t *surf = cairo_image_surface_create_for_data(
            static_cast<unsigned char *>(slot->data), CAIRO_FORMAT_ARGB32, w, h, w * 4);
        cairo_t *cr = cairo_create(surf);
        renderer.paint(cr);
        cairo_surface_flush(surf);
        cairo_destroy(cr);
        cairo_surface_destroy(surf);

        slot->busy = true;
        wl_surface_attach(surface_, slot->buffer, 0, 0);
        wl_surface_damage_buffer(surface_, 0, 0, w, h);
        wl_surface_commit(surface_);
        wl_display_flush(display_);
        return true;
    }

private:
    struct ShmSlot {
        wl_buffer *buffer = nullptr;
        void *data = nullptr;
        size_t size = 0;
        int w = 0;
        int h = 0;
        bool busy = false;

        void destroy() {
            if (buffer) {
                wl_buffer_destroy(buffer);
                buffer = nullptr;
            }
            if (data) {
                munmap(data, size);
                data = nullptr;
            }
            size = 0;
            w = 0;
            h = 0;
            busy = false;
        }

        bool ensure(wl_shm *shm, int nw, int nh) {
            if (buffer && w == nw && h == nh) return true;
            destroy();
            const size_t nsize = static_cast<size_t>(nw) * static_cast<size_t>(nh) * 4;
            const int fd = memfd_create("sweetui-shm", MFD_CLOEXEC);
            if (fd < 0) return false;
            if (ftruncate(fd, static_cast<off_t>(nsize)) < 0) {
                close(fd);
                return false;
            }
            void *mem = mmap(nullptr, nsize, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
            if (mem == MAP_FAILED) {
                close(fd);
                return false;
            }
            wl_shm_pool *pool = wl_shm_create_pool(shm, fd, static_cast<int32_t>(nsize));
            buffer = wl_shm_pool_create_buffer(pool, 0, nw, nh, nw * 4, WL_SHM_FORMAT_ARGB8888);
            wl_shm_pool_destroy(pool);
            close(fd);
            if (!buffer) {
                munmap(mem, nsize);
                return false;
            }
            wl_buffer_add_listener(buffer, &kBufferListener, this);
            data = mem;
            size = nsize;
            w = nw;
            h = nh;
            busy = false;
            return true;
        }

        static void onRelease(void *user, wl_buffer *) {
            static_cast<ShmSlot *>(user)->busy = false;
        }

        static constexpr wl_buffer_listener kBufferListener = {
            .release = onRelease,
        };
    };

    ShmSlot *acquireSlot(int w, int h) {
        for (auto &slot : slots_) {
            if (!slot.busy) {
                return slot.ensure(shm_, w, h) ? &slot : nullptr;
            }
        }
        return nullptr;
    }

    struct OutputInfo {
        WaylandPopup *owner = nullptr;
        uint32_t name = 0;
        uint32_t version = 1;
        wl_output *output = nullptr;
        int32_t modeW = 0;
        int32_t modeH = 0;
        int32_t scale = 1;
        int32_t transform = WL_OUTPUT_TRANSFORM_NORMAL;

        void destroy() {
            if (!output) return;
            if (version >= 3) {
                wl_output_release(output);
            } else {
                wl_output_destroy(output);
            }
            output = nullptr;
        }

        int logicalW() const {
            if (modeW <= 0 || modeH <= 0) return 0;
            const bool rotated = (transform == WL_OUTPUT_TRANSFORM_90 ||
                                  transform == WL_OUTPUT_TRANSFORM_270 ||
                                  transform == WL_OUTPUT_TRANSFORM_FLIPPED_90 ||
                                  transform == WL_OUTPUT_TRANSFORM_FLIPPED_270);
            const int rawW = rotated ? modeH : modeW;
            return std::max(1, rawW / std::max(1, scale));
        }

        int logicalH() const {
            if (modeW <= 0 || modeH <= 0) return 0;
            const bool rotated = (transform == WL_OUTPUT_TRANSFORM_90 ||
                                  transform == WL_OUTPUT_TRANSFORM_270 ||
                                  transform == WL_OUTPUT_TRANSFORM_FLIPPED_90 ||
                                  transform == WL_OUTPUT_TRANSFORM_FLIPPED_270);
            const int rawH = rotated ? modeW : modeH;
            return std::max(1, rawH / std::max(1, scale));
        }
    };

    std::pair<int, int> currentOutputBounds() const {
        if (hostingOutput_) {
            for (const auto &out : outputs_) {
                if (out->output == hostingOutput_) {
                    return {out->logicalW(), out->logicalH()};
                }
            }
        }
        // Before wl_surface.enter reports the hosting output, use the sole output on single-output
        // setups, or the conservative minimum across all outputs on multi-output setups so the
        // initial surface is guaranteed visible and receives wl_surface.enter.
        int minW = 0;
        int minH = 0;
        for (const auto &out : outputs_) {
            const int lw = out->logicalW();
            const int lh = out->logicalH();
            if (lw > 0 && (minW == 0 || lw < minW)) minW = lw;
            if (lh > 0 && (minH == 0 || lh < minH)) minH = lh;
        }
        return {minW, minH};
    }

    void applyPlacement() {
        if (!layerSurface_ || lastW_ <= 0 || lastH_ <= 0) return;
        const bool hasCursor = (lastCursor_.left() != 0 || lastCursor_.top() != 0 ||
                                lastCursor_.width() > 0 || lastCursor_.height() > 0);
        if (hasCursor) {
            const auto [outW, outH] = currentOutputBounds();
            zwlr_layer_surface_v1_set_anchor(
                layerSurface_,
                ZWLR_LAYER_SURFACE_V1_ANCHOR_TOP | ZWLR_LAYER_SURFACE_V1_ANCHOR_LEFT);
            int mx = lastCursor_.left() - static_cast<int>(SHADOW_PAD);
            int my = lastCursor_.bottom() + 4 - static_cast<int>(SHADOW_PAD);
            if (outH > 0 && my + lastH_ > outH) {
                my = lastCursor_.top() - lastH_ - 4 + static_cast<int>(SHADOW_PAD);
            }
            if (outW > 0) {
                mx = std::clamp(mx, 0, std::max(0, outW - lastW_));
            } else {
                mx = std::max(0, mx);
            }
            if (outH > 0) {
                my = std::clamp(my, 0, std::max(0, outH - lastH_));
            } else {
                my = std::max(0, my);
            }
            zwlr_layer_surface_v1_set_margin(layerSurface_, my, 0, 0, mx);
        } else {
            zwlr_layer_surface_v1_set_anchor(layerSurface_, ZWLR_LAYER_SURFACE_V1_ANCHOR_BOTTOM);
            zwlr_layer_surface_v1_set_margin(layerSurface_, 0, 0, 36, 0);
        }
    }

    static void onSurfaceEnter(void *data, wl_surface *, wl_output *output) {
        auto *self = static_cast<WaylandPopup *>(data);
        if (self->hostingOutput_ != output) {
            self->hostingOutput_ = output;
            if (self->surface_ && self->layerSurface_ && self->configured_) {
                self->applyPlacement();
                wl_surface_commit(self->surface_);
                wl_display_flush(self->display_);
            }
        }
    }

    static void onSurfaceLeave(void *data, wl_surface *, wl_output *output) {
        auto *self = static_cast<WaylandPopup *>(data);
        if (self->hostingOutput_ == output) {
            self->hostingOutput_ = nullptr;
        }
    }

    static constexpr wl_surface_listener kSurfaceListener = {
        .enter = onSurfaceEnter,
        .leave = onSurfaceLeave,
    };

    static void onGlobal(void *data, wl_registry *reg, uint32_t name, const char *iface,
                         uint32_t version) {
        auto *self = static_cast<WaylandPopup *>(data);
        const bool wasReady = self->ready();
        if (std::strcmp(iface, wl_compositor_interface.name) == 0) {
            self->compositor_ = static_cast<wl_compositor *>(
                wl_registry_bind(reg, name, &wl_compositor_interface, std::min(version, 4u)));
        } else if (std::strcmp(iface, wl_shm_interface.name) == 0) {
            self->shm_ = static_cast<wl_shm *>(
                wl_registry_bind(reg, name, &wl_shm_interface, 1));
        } else if (std::strcmp(iface, zwlr_layer_shell_v1_interface.name) == 0) {
            self->layerShellVersion_ = std::min(version, 3u);
            self->layerShell_ = static_cast<zwlr_layer_shell_v1 *>(wl_registry_bind(
                reg, name, &zwlr_layer_shell_v1_interface, self->layerShellVersion_));
        } else if (std::strcmp(iface, ext_background_effect_manager_v1_interface.name) == 0) {
            self->bgEffectManager_ = static_cast<ext_background_effect_manager_v1 *>(
                wl_registry_bind(reg, name, &ext_background_effect_manager_v1_interface, 1));
            ext_background_effect_manager_v1_add_listener(self->bgEffectManager_,
                                                          &kBgEffectListener, self);
        } else if (std::strcmp(iface, wl_output_interface.name) == 0) {
            auto info = std::make_unique<OutputInfo>();
            info->owner = self;
            info->name = name;
            info->version = std::min(version, 3u);
            info->output = static_cast<wl_output *>(
                wl_registry_bind(reg, name, &wl_output_interface, info->version));
            if (info->output) {
                wl_output_add_listener(info->output, &kOutputListener, info.get());
                self->outputs_.push_back(std::move(info));
            }
        } else if (std::strcmp(iface, wl_seat_interface.name) == 0 && !self->seat_) {
            self->seat_ = static_cast<wl_seat *>(
                wl_registry_bind(reg, name, &wl_seat_interface, std::min(version, 5u)));
            wl_seat_add_listener(self->seat_, &kSeatListener, self);
        }
        if (!wasReady && self->ready() && self->onReady_) {
            self->onReady_();
        }
    }

    static void onGlobalRemove(void *data, wl_registry *, uint32_t name) {
        auto *self = static_cast<WaylandPopup *>(data);
        for (auto it = self->outputs_.begin(); it != self->outputs_.end(); ++it) {
            if ((*it)->name == name) {
                if (self->hostingOutput_ == (*it)->output) {
                    self->hostingOutput_ = nullptr;
                }
                (*it)->destroy();
                self->outputs_.erase(it);
                break;
            }
        }
    }

    static constexpr wl_registry_listener kRegistryListener = {
        .global = onGlobal,
        .global_remove = onGlobalRemove,
    };

    static void onOutputGeometry(void *data, wl_output *, int32_t, int32_t, int32_t, int32_t,
                                 int32_t, const char *, const char *, int32_t transform) {
        static_cast<OutputInfo *>(data)->transform = transform;
    }
    static void onOutputMode(void *data, wl_output *, uint32_t flags, int32_t w, int32_t h,
                             int32_t) {
        if ((flags & WL_OUTPUT_MODE_CURRENT) != 0 && w > 0 && h > 0) {
            auto *info = static_cast<OutputInfo *>(data);
            info->modeW = w;
            info->modeH = h;
        }
    }
    static void onOutputDone(void *data, wl_output *) {
        auto *info = static_cast<OutputInfo *>(data);
        auto *self = info->owner;
        if (!self || !self->surface_ || !self->layerSurface_ || !self->configured_) {
            return;
        }
        if (self->hostingOutput_ == info->output || self->hostingOutput_ == nullptr) {
            self->applyPlacement();
            wl_surface_commit(self->surface_);
            wl_display_flush(self->display_);
        }
    }
    static void onOutputScale(void *data, wl_output *, int32_t factor) {
        if (factor > 0) {
            static_cast<OutputInfo *>(data)->scale = factor;
        }
    }

    static constexpr wl_output_listener kOutputListener = {
        .geometry = onOutputGeometry,
        .mode = onOutputMode,
        .done = onOutputDone,
        .scale = onOutputScale,
    };

    static void onSeatCapabilities(void *data, wl_seat *seat, uint32_t caps) {
        auto *self = static_cast<WaylandPopup *>(data);
        const bool hasPtr = (caps & WL_SEAT_CAPABILITY_POINTER) != 0;
        if (hasPtr && !self->pointer_) {
            self->pointer_ = wl_seat_get_pointer(seat);
            wl_pointer_add_listener(self->pointer_, &kPointerListener, self);
        } else if (!hasPtr && self->pointer_) {
            wl_pointer_destroy(self->pointer_);
            self->pointer_ = nullptr;
        }
    }

    static void onSeatName(void *, wl_seat *, const char *) {}

    static constexpr wl_seat_listener kSeatListener = {
        .capabilities = onSeatCapabilities,
        .name = onSeatName,
    };

    static void onPointerEnter(void *data, wl_pointer *, uint32_t, wl_surface *surf,
                               wl_fixed_t sx, wl_fixed_t sy) {
        auto *self = static_cast<WaylandPopup *>(data);
        if (surf == self->surface_) {
            self->pointerInside_ = true;
            self->ptrX_ = wl_fixed_to_double(sx);
            self->ptrY_ = wl_fixed_to_double(sy);
            if (self->onHover_) self->onHover_(self->ptrX_, self->ptrY_);
        }
    }

    static void onPointerLeave(void *data, wl_pointer *, uint32_t, wl_surface *surf) {
        auto *self = static_cast<WaylandPopup *>(data);
        if (surf == self->surface_) {
            self->pointerInside_ = false;
            if (self->onHover_) self->onHover_(-1.0, -1.0);
        }
    }

    static void onPointerMotion(void *data, wl_pointer *, uint32_t, wl_fixed_t sx, wl_fixed_t sy) {
        auto *self = static_cast<WaylandPopup *>(data);
        if (!self->pointerInside_) return;
        self->ptrX_ = wl_fixed_to_double(sx);
        self->ptrY_ = wl_fixed_to_double(sy);
        if (self->onHover_) self->onHover_(self->ptrX_, self->ptrY_);
    }

    static void onPointerButton(void *data, wl_pointer *, uint32_t, uint32_t, uint32_t button,
                                uint32_t state) {
        auto *self = static_cast<WaylandPopup *>(data);
        if (self->pointerInside_ && button == BTN_LEFT &&
            state == WL_POINTER_BUTTON_STATE_PRESSED && self->onClick_) {
            self->onClick_(self->ptrX_, self->ptrY_);
        }
    }

    static void onPointerAxis(void *, wl_pointer *, uint32_t, uint32_t, wl_fixed_t) {}
    static void onPointerFrame(void *, wl_pointer *) {}
    static void onPointerAxisSource(void *, wl_pointer *, uint32_t) {}
    static void onPointerAxisStop(void *, wl_pointer *, uint32_t, uint32_t) {}
    static void onPointerAxisDiscrete(void *, wl_pointer *, uint32_t, int32_t) {}

    static constexpr wl_pointer_listener kPointerListener = {
        .enter = onPointerEnter,
        .leave = onPointerLeave,
        .motion = onPointerMotion,
        .button = onPointerButton,
        .axis = onPointerAxis,
        .frame = onPointerFrame,
        .axis_source = onPointerAxisSource,
        .axis_stop = onPointerAxisStop,
        .axis_discrete = onPointerAxisDiscrete,
    };

    static void onLayerConfigure(void *data, zwlr_layer_surface_v1 *ls, uint32_t serial,
                                 uint32_t, uint32_t) {
        auto *self = static_cast<WaylandPopup *>(data);
        zwlr_layer_surface_v1_ack_configure(ls, serial);
        const bool first = !self->configured_;
        self->configured_ = true;
        if (first && self->onConfigured_) {
            self->onConfigured_();
        }
    }

    static void onLayerClosed(void *data, zwlr_layer_surface_v1 *) {
        auto *self = static_cast<WaylandPopup *>(data);
        const bool wasConfigured = self->configured_;
        self->hide();
        if (!wasConfigured && self->onInitialClosed_) {
            self->onInitialClosed_();
        }
    }

    static void onBgEffectCapabilities(void *data, ext_background_effect_manager_v1 *,
                                       uint32_t flags) {
        auto *self = static_cast<WaylandPopup *>(data);
        self->blurSupported_ = (flags & EXT_BACKGROUND_EFFECT_MANAGER_V1_CAPABILITY_BLUR) != 0;
    }

    static constexpr ext_background_effect_manager_v1_listener kBgEffectListener = {
        .capabilities = onBgEffectCapabilities,
    };

    static constexpr zwlr_layer_surface_v1_listener kLayerSurfaceListener = {
        .configure = onLayerConfigure,
        .closed = onLayerClosed,
    };

    wl_display *display_ = nullptr;
    wl_registry *registry_ = nullptr;
    wl_compositor *compositor_ = nullptr;
    wl_shm *shm_ = nullptr;
    std::vector<std::unique_ptr<OutputInfo>> outputs_;
    wl_output *hostingOutput_ = nullptr;
    wl_seat *seat_ = nullptr;
    wl_pointer *pointer_ = nullptr;
    zwlr_layer_shell_v1 *layerShell_ = nullptr;
    uint32_t layerShellVersion_ = 1;
    ext_background_effect_manager_v1 *bgEffectManager_ = nullptr;
    ext_background_effect_surface_v1 *bgEffectSurface_ = nullptr;
    bool blurSupported_ = false;
    wl_surface *surface_ = nullptr;
    zwlr_layer_surface_v1 *layerSurface_ = nullptr;
    std::array<ShmSlot, 2> slots_{};
    fcitx::Rect lastCursor_{};
    int lastW_ = 0;
    int lastH_ = 0;
    bool configured_ = false;
    bool pointerInside_ = false;
    double ptrX_ = 0.0;
    double ptrY_ = 0.0;
    HoverCb onHover_;
    ClickCb onClick_;
    NotifyCb onReady_;
    NotifyCb onConfigured_;
    NotifyCb onInitialClosed_;
};
#endif

// ---------------------------------------------------------------------------
// SweetUI fcitx5 UserInterface addon
// ---------------------------------------------------------------------------
class SweetUI : public fcitx::UserInterface {
public:
    explicit SweetUI(fcitx::Instance *instance) : instance_(instance) {
#ifdef SWEET_HAVE_WAYLAND
        if (auto *wayland = instance_->addonManager().addon("wayland", true)) {
            wlCreated_ = wayland->call<fcitx::IWaylandModule::addConnectionCreatedCallback>(
                [this](const std::string &name, wl_display *display, fcitx::FocusGroup *) {
                    auto popup = std::make_unique<WaylandPopup>(
                        display,
                        [this, name](double x, double y) { onHover(BackendKind::Wayland, name, x, y); },
                        [this, name](double x, double y) { onClick(BackendKind::Wayland, name, x, y); },
                        [this]() {
                            notifyAvailability();
                            if (!suspended_ && renderer_.visible()) {
                                renderNow();
                            }
                        },
                        [this, name]() {
                            if (!suspended_ && renderer_.visible() &&
                                presentedKind_ == BackendKind::Wayland && presentedName_ == name) {
                                retryCount_ = 0;
                                renderNow();
                            }
                        },
                        [this, name]() {
                            if (presentedKind_ == BackendKind::Wayland && presentedName_ == name) {
                                presentedKind_ = BackendKind::None;
                                presentedName_.clear();
                                if (!suspended_ && renderer_.visible() && retryCount_ < 3) {
                                    ++retryCount_;
                                    ensureTimer();
                                } else {
                                    stopTimer();
                                }
                            }
                        });
                    const bool isReady = popup->ready();
                    wlPopups_[name] = std::move(popup);
                    if (isReady) {
                        notifyAvailability();
                    }
                });
            wlClosed_ = wayland->call<fcitx::IWaylandModule::addConnectionClosedCallback>(
                [this](const std::string &name, wl_display *) {
                    if (presentedKind_ == BackendKind::Wayland && presentedName_ == name) {
                        presentedKind_ = BackendKind::None;
                        presentedName_.clear();
                    }
                    wlPopups_.erase(name);
                    notifyAvailability();
                });
        }
#endif
#ifdef SWEET_HAVE_XCB
        if (auto *xcb = instance_->addonManager().addon("xcb", true)) {
            xcb_ = xcb;
            xcbCreated_ = xcb->call<fcitx::IXCBModule::addConnectionCreatedCallback>(
                [this, xcb](const std::string &name, xcb_connection_t *conn, int screen,
                            fcitx::FocusGroup *) {
                    xcbPopups_[name] = std::make_unique<XcbPopup>(conn, screen);
                    xcbFilters_[name] = xcb->call<fcitx::IXCBModule::addEventFilter>(
                        name, [this, name](xcb_connection_t *, xcb_generic_event_t *ev) {
                            return handleXcbEvent(name, ev);
                        });
                    notifyAvailability();
                });
            xcbClosed_ = xcb->call<fcitx::IXCBModule::addConnectionClosedCallback>(
                [this](const std::string &name, xcb_connection_t *) {
                    if (presentedKind_ == BackendKind::Xcb && presentedName_ == name) {
                        presentedKind_ = BackendKind::None;
                        presentedName_.clear();
                    }
                    xcbFilters_.erase(name);
                    xcbPopups_.erase(name);
                    notifyAvailability();
                });
        }
#endif
        constructing_ = false;
    }

    ~SweetUI() override {
        setNotificationItemEnabled(false);
        stopTimer();
    }

    bool available() override {
        bool ok = false;
#ifdef SWEET_HAVE_WAYLAND
        for (const auto &[_, p] : wlPopups_) {
            if (p->ready()) {
                ok = true;
                break;
            }
        }
#endif
#ifdef SWEET_HAVE_XCB
        ok = ok || !xcbPopups_.empty();
#endif
        return ok;
    }

    void suspend() override {
        suspended_ = true;
        hideAll();
        stopTimer();
        setNotificationItemEnabled(false);
    }

    void resume() override {
        suspended_ = false;
        renderer_.reloadConfig();
        setNotificationItemEnabled(true);
    }

    void reloadConfig() override {
        renderer_.reloadConfig();
        if (!suspended_ && renderer_.visible()) {
            renderNow();
        }
    }

    void update(fcitx::UserInterfaceComponent component, fcitx::InputContext *ic) override {
        if (suspended_ || component != fcitx::UserInterfaceComponent::InputPanel || !ic) {
            return;
        }
        PanelSnapshot snap;
        const auto &panel = ic->inputPanel();
        if (!panel.preedit().empty()) {
            snap.preedit = panel.preedit().toString();
        }
        if (!panel.auxUp().empty()) {
            snap.aux_up = panel.auxUp().toString();
        }
        if (!panel.auxDown().empty()) {
            snap.aux_down = panel.auxDown().toString();
        }
        if (auto cands = panel.candidateList()) {
            const int n = cands->size();
            for (int i = 0; i < n; ++i) {
                const auto &cand = cands->candidate(i);
                const std::string label = cands->label(i).toString();
                snap.items.push_back({label, cand.text().toString()});
            }
            snap.cursor = cands->cursorIndex();
        }
        snap.visible = !snap.items.empty() || !snap.preedit.empty() || !snap.aux_up.empty() ||
                       !snap.aux_down.empty();

        activeIC_ = ic->watch();
        activeDisplay_ = ic->display();
        activeCursor_ = ic->cursorRect();
        retryCount_ = 0;
        renderer_.updateData(snap);
        renderNow();
    }

private:
    enum class BackendKind { None, Wayland, Xcb };

    void notifyAvailability() {
        if (!constructing_) {
            instance_->userInterfaceManager().updateAvailability();
        }
    }

    void setNotificationItemEnabled(bool enable) {
#ifdef SWEET_HAVE_NOTIFICATIONITEM
        if (sniEnabled_ == enable) {
            return;
        }
        if (auto *sni = instance_->addonManager().addon("notificationitem", true)) {
            if (enable) {
                sni->call<fcitx::INotificationItem::enable>();
            } else {
                sni->call<fcitx::INotificationItem::disable>();
            }
            sniEnabled_ = enable;
        }
#else
        (void)enable;
#endif
    }

    std::pair<BackendKind, std::string> resolveTarget() const {
        if (activeDisplay_.rfind("wayland:", 0) == 0) {
            const std::string name = activeDisplay_.substr(8);
#ifdef SWEET_HAVE_WAYLAND
            if (auto it = wlPopups_.find(name); it != wlPopups_.end() && it->second->ready()) {
                return {BackendKind::Wayland, name};
            }
#endif
#ifdef SWEET_HAVE_XCB
            if (name.empty() && xcb_) {
                const std::string mainDisp = xcb_->call<fcitx::IXCBModule::mainDisplay>();
                if (!mainDisp.empty() && xcbPopups_.find(mainDisp) != xcbPopups_.end()) {
                    return {BackendKind::Xcb, mainDisp};
                }
            }
#endif
            return {BackendKind::None, {}};
        }
#ifdef SWEET_HAVE_XCB
        if (activeDisplay_.rfind("x11:", 0) == 0) {
            const std::string name = activeDisplay_.substr(4);
            if (xcbPopups_.find(name) != xcbPopups_.end()) {
                return {BackendKind::Xcb, name};
            }
            return {BackendKind::None, {}};
        }
#endif
        // Only fall back to a default connection when no explicit display was named.
        if (activeDisplay_.empty()) {
#ifdef SWEET_HAVE_WAYLAND
            for (const auto &[name, popup] : wlPopups_) {
                if (popup->ready()) {
                    return {BackendKind::Wayland, name};
                }
            }
#endif
#ifdef SWEET_HAVE_XCB
            if (xcb_) {
                const std::string mainDisp = xcb_->call<fcitx::IXCBModule::mainDisplay>();
                if (!mainDisp.empty() && xcbPopups_.find(mainDisp) != xcbPopups_.end()) {
                    return {BackendKind::Xcb, mainDisp};
                }
            }
            if (!xcbPopups_.empty()) {
                return {BackendKind::Xcb, xcbPopups_.begin()->first};
            }
#endif
        }
        return {BackendKind::None, {}};
    }

    void hidePresented() {
#ifdef SWEET_HAVE_WAYLAND
        if (presentedKind_ == BackendKind::Wayland) {
            if (auto it = wlPopups_.find(presentedName_); it != wlPopups_.end()) {
                it->second->hide();
            }
        }
#endif
#ifdef SWEET_HAVE_XCB
        if (presentedKind_ == BackendKind::Xcb) {
            if (auto it = xcbPopups_.find(presentedName_); it != xcbPopups_.end()) {
                it->second->hide();
            }
        }
#endif
        presentedKind_ = BackendKind::None;
        presentedName_.clear();
    }

    // Renders the current panel state without modifying animTimer_.
    // Returns true if another animation or retry tick should be scheduled.
    bool renderFrame() {
        if (!renderer_.visible()) {
            hideAll();
            return false;
        }
        auto [kind, name] = resolveTarget();
        if (kind != presentedKind_ || name != presentedName_) {
            hidePresented();
        }
        bool ok = false;
        bool completed = false;
#ifdef SWEET_HAVE_WAYLAND
        if (kind == BackendKind::Wayland) {
            if (auto it = wlPopups_.find(name); it != wlPopups_.end()) {
                ok = it->second->present(renderer_, activeCursor_);
                completed = ok && it->second->configured();
            }
        }
#endif
#ifdef SWEET_HAVE_XCB
        if (kind == BackendKind::Xcb) {
            if (auto it = xcbPopups_.find(name); it != xcbPopups_.end()) {
                ok = it->second->present(renderer_, activeCursor_);
                completed = ok;
            }
        }
#endif
        if (ok) {
            presentedKind_ = kind;
            presentedName_ = name;
            if (completed) {
                retryCount_ = 0;
            }
            return completed && renderer_.needsAnimation();
        }
        hidePresented();
        if (kind != BackendKind::None && retryCount_ < 3) {
            ++retryCount_;
            return true;
        }
        return false;
    }

    void renderNow() {
        if (renderFrame()) {
            ensureTimer();
        } else {
            stopTimer();
        }
    }

    void hideAll() {
#ifdef SWEET_HAVE_WAYLAND
        for (auto &[_, p] : wlPopups_) p->hide();
#endif
#ifdef SWEET_HAVE_XCB
        for (auto &[_, p] : xcbPopups_) p->hide();
#endif
        presentedKind_ = BackendKind::None;
        presentedName_.clear();
        retryCount_ = 0;
    }

    void ensureTimer() {
        if (animTimer_) return;
        animTimer_ = instance_->eventLoop().addTimeEvent(
            CLOCK_MONOTONIC, fcitx::now(CLOCK_MONOTONIC) + 16000, 1000,
            [this](fcitx::EventSourceTime *src, uint64_t) {
                if (renderFrame()) {
                    src->setNextInterval(16000);
                    src->setOneShot();
                } else {
                    animTimer_.reset();
                }
                return true;
            });
    }

    void stopTimer() { animTimer_.reset(); }

    void onHover(BackendKind kind, const std::string &name, double x, double y) {
        if (kind != presentedKind_ || name != presentedName_) return;
        if (renderer_.setHover(x, y)) {
            renderNow();
        }
    }

    void onClick(BackendKind kind, const std::string &name, double x, double y) {
        if (kind != presentedKind_ || name != presentedName_) return;
        const int idx = renderer_.hitTest(x, y);
        if (idx < 0) return;
        auto *ic = activeIC_.get();
        if (!ic) return;
        if (auto cands = ic->inputPanel().candidateList()) {
            if (idx < cands->size()) {
                cands->candidate(idx).select(ic);
            }
        }
    }

#ifdef SWEET_HAVE_XCB
    bool handleXcbEvent(const std::string &name, xcb_generic_event_t *ev) {
        auto it = xcbPopups_.find(name);
        if (it == xcbPopups_.end()) return false;
        const xcb_window_t win = it->second->window();
        if (win == XCB_NONE) return false;
        const uint8_t type = ev->response_type & ~0x80;
        if (type == XCB_MOTION_NOTIFY) {
            auto *m = reinterpret_cast<xcb_motion_notify_event_t *>(ev);
            if (m->event == win) {
                onHover(BackendKind::Xcb, name, m->event_x, m->event_y);
                return true;
            }
        } else if (type == XCB_LEAVE_NOTIFY) {
            auto *l = reinterpret_cast<xcb_leave_notify_event_t *>(ev);
            if (l->event == win) {
                onHover(BackendKind::Xcb, name, -1.0, -1.0);
                return true;
            }
        } else if (type == XCB_BUTTON_PRESS) {
            auto *b = reinterpret_cast<xcb_button_press_event_t *>(ev);
            if (b->event == win && b->detail == 1) {
                onClick(BackendKind::Xcb, name, b->event_x, b->event_y);
                return true;
            }
        }
        return false;
    }
#endif

    fcitx::Instance *instance_;
    SweetPanelRenderer renderer_;
    fcitx::TrackableObjectReference<fcitx::InputContext> activeIC_;
    std::string activeDisplay_;
    fcitx::Rect activeCursor_;
    BackendKind presentedKind_ = BackendKind::None;
    std::string presentedName_;
    int retryCount_ = 0;
    std::unique_ptr<fcitx::EventSourceTime> animTimer_;
    bool constructing_ = true;
    bool suspended_ = false;
    bool sniEnabled_ = false;

#ifdef SWEET_HAVE_WAYLAND
    std::unique_ptr<fcitx::HandlerTableEntry<fcitx::WaylandConnectionCreated>> wlCreated_;
    std::unique_ptr<fcitx::HandlerTableEntry<fcitx::WaylandConnectionClosed>> wlClosed_;
    std::unordered_map<std::string, std::unique_ptr<WaylandPopup>> wlPopups_;
#endif
#ifdef SWEET_HAVE_XCB
    fcitx::AddonInstance *xcb_ = nullptr;
    std::unique_ptr<fcitx::HandlerTableEntry<fcitx::XCBConnectionCreated>> xcbCreated_;
    std::unique_ptr<fcitx::HandlerTableEntry<fcitx::XCBConnectionClosed>> xcbClosed_;
    std::unordered_map<std::string, std::unique_ptr<XcbPopup>> xcbPopups_;
    std::unordered_map<std::string, std::unique_ptr<fcitx::HandlerTableEntry<fcitx::XCBEventFilter>>>
        xcbFilters_;
#endif
};

class SweetUIFactory : public fcitx::AddonFactory {
public:
    fcitx::AddonInstance *create(fcitx::AddonManager *manager) override {
        return new SweetUI(manager->instance());
    }
};

} // namespace

FCITX_ADDON_FACTORY(SweetUIFactory);
