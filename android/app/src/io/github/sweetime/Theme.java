package io.github.sweetime;

import android.content.Context;
import android.content.res.Configuration;
import android.graphics.Color;

/** Keyboard colors: light or dark, by the setting or with the system. */
final class Theme {
    final boolean dark;
    final int background;
    final int key;
    final int specialKey;
    final int pressed;
    final int shadow;
    final int text;
    final int subText;
    final int accent;
    final int onAccent;
    final int focus;
    final int preview;

    private Theme(boolean dark) {
        this.dark = dark;
        if (dark) {
            background = Color.rgb(0x1e, 0x1b, 0x22);
            key = Color.rgb(0x3a, 0x35, 0x40);
            specialKey = Color.rgb(0x5c, 0x40, 0x4e);
            pressed = Color.rgb(0x5a, 0x4a, 0x60);
            shadow = Color.rgb(0x10, 0x0e, 0x12);
            text = Color.rgb(0xf2, 0xee, 0xf4);
            subText = Color.rgb(0xb0, 0xa8, 0xb8);
            accent = Color.rgb(0xf0, 0x86, 0xa8);
            onAccent = Color.rgb(0x2a, 0x10, 0x1a);
            focus = Color.argb(0x66, 0xf0, 0x86, 0xa8);
            preview = Color.rgb(0x4a, 0x42, 0x52);
        } else {
            background = Color.rgb(0xf6, 0xee, 0xf1);
            key = Color.WHITE;
            specialKey = Color.rgb(0xf2, 0xc6, 0xd5);
            pressed = Color.rgb(0xf8, 0xc8, 0xd8);
            shadow = Color.rgb(0xd2, 0xc2, 0xc9);
            text = Color.rgb(0x2a, 0x22, 0x26);
            subText = Color.rgb(0x80, 0x70, 0x78);
            accent = Color.rgb(0xe0, 0x5a, 0x86);
            onAccent = Color.WHITE;
            focus = Color.argb(0x55, 0xe0, 0x5a, 0x86);
            preview = Color.WHITE;
        }
    }

    static Theme of(Context c) {
        int setting = Prefs.number(c, Prefs.THEME, 0);
        if (setting == 1) return new Theme(false);
        if (setting == 2) return new Theme(true);
        int night = c.getResources().getConfiguration().uiMode & Configuration.UI_MODE_NIGHT_MASK;
        return new Theme(night == Configuration.UI_MODE_NIGHT_YES);
    }
}
