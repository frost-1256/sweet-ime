package io.github.sweetime;

import android.content.Context;
import android.content.res.Configuration;
import android.graphics.Color;
import android.os.Build;

/**
 * Keyboard colors and key shapes. The Sweet themes follow or fix light and
 * dark; the Material 3 themes take the wallpaper colors (Material You) on
 * Android 12 and later and the Material 3 baseline colors before that; the
 * color themes match the candidate window themes of the desktop versions.
 */
final class Theme {
    /** Names of the THEME setting's values, in order. */
    static final String[] LABELS = {
        "Sweet(システムに合わせる)",
        "Sweet ライト",
        "Sweet ダーク",
        "Material 3",
        "Material 3 Expressive",
        "さくら",
        "抹茶",
        "ミッドナイト",
        "ラベンダー",
        "Dracula",
    };

    static final int MATERIAL = 3;
    static final int EXPRESSIVE = 4;

    final boolean dark;
    int background;
    int key;
    int specialKey;
    int pressed;
    /** Drawn below each key; 0 for flat keys. */
    int shadow;
    int text;
    int subText;
    int accent;
    int onAccent;
    int focus;
    int preview;
    /** Key corner radius in dp. */
    float radius = 8;
    /** Corner radius of a pressed key in dp (Expressive keys change shape). */
    float pressedRadius = 8;
    /** Bold labels on the function keys. */
    boolean boldFunctions;

    private Theme(boolean dark) {
        this.dark = dark;
    }

    static Theme of(Context c) {
        return of(c, Prefs.number(c, Prefs.THEME, 0));
    }

    /** The theme for THEME setting value `setting`. */
    static Theme of(Context c, int setting) {
        int night = c.getResources().getConfiguration().uiMode & Configuration.UI_MODE_NIGHT_MASK;
        boolean systemDark = night == Configuration.UI_MODE_NIGHT_YES;
        switch (setting) {
            case 1:
                return sweet(false);
            case 2:
                return sweet(true);
            case MATERIAL:
                return material(c, systemDark, false);
            case EXPRESSIVE:
                return material(c, systemDark, true);
            case 5:
                return palette(false, 0xfff4f7, 0xffffff, 0xf5c6d6, 0xf3a5c0, 0x4a2332, 0xa06a7d, 0xe85a8a);
            case 6:
                return palette(false, 0xeef3e6, 0xfbfdf7, 0xd3dfc3, 0xb4c99c, 0x26331c, 0x6b7a5c, 0x4f8032);
            case 7:
                return palette(true, 0x141827, 0x262c43, 0x343b55, 0x4a5372, 0xe6eaf5, 0x939cb8, 0x7aa2ff);
            case 8:
                return palette(false, 0xf3effa, 0xfdfbff, 0xdcd0f0, 0xc6b4e8, 0x2f2540, 0x7a6e90, 0x8a63d2);
            case 9:
                return palette(true, 0x21222c, 0x343746, 0x44475a, 0x6272a4, 0xf8f8f2, 0x8b96c4, 0xff79c6);
            default:
                return sweet(systemDark);
        }
    }

    private static Theme sweet(boolean dark) {
        Theme t = new Theme(dark);
        if (dark) {
            t.background = Color.rgb(0x1e, 0x1b, 0x22);
            t.key = Color.rgb(0x3a, 0x35, 0x40);
            t.specialKey = Color.rgb(0x5c, 0x40, 0x4e);
            t.pressed = Color.rgb(0x5a, 0x4a, 0x60);
            t.shadow = Color.rgb(0x10, 0x0e, 0x12);
            t.text = Color.rgb(0xf2, 0xee, 0xf4);
            t.subText = Color.rgb(0xb0, 0xa8, 0xb8);
            t.accent = Color.rgb(0xf0, 0x86, 0xa8);
            t.onAccent = Color.rgb(0x2a, 0x10, 0x1a);
            t.focus = Color.argb(0x66, 0xf0, 0x86, 0xa8);
            t.preview = Color.rgb(0x4a, 0x42, 0x52);
        } else {
            t.background = Color.rgb(0xf6, 0xee, 0xf1);
            t.key = Color.WHITE;
            t.specialKey = Color.rgb(0xf2, 0xc6, 0xd5);
            t.pressed = Color.rgb(0xf8, 0xc8, 0xd8);
            t.shadow = Color.rgb(0xd2, 0xc2, 0xc9);
            t.text = Color.rgb(0x2a, 0x22, 0x26);
            t.subText = Color.rgb(0x80, 0x70, 0x78);
            t.accent = Color.rgb(0xe0, 0x5a, 0x86);
            t.onAccent = Color.WHITE;
            t.focus = Color.argb(0x55, 0xe0, 0x5a, 0x86);
            t.preview = Color.WHITE;
        }
        return t;
    }

    /** A fixed color theme; colors are 0xRRGGBB. */
    private static Theme palette(boolean dark, int background, int key, int specialKey, int pressed,
            int text, int subText, int accent) {
        Theme t = new Theme(dark);
        t.background = 0xff000000 | background;
        t.key = 0xff000000 | key;
        t.specialKey = 0xff000000 | specialKey;
        t.pressed = 0xff000000 | pressed;
        t.shadow = dark ? Color.argb(0x80, 0, 0, 0) : Color.argb(0x26, 0, 0, 0);
        t.text = 0xff000000 | text;
        t.subText = 0xff000000 | subText;
        t.accent = 0xff000000 | accent;
        t.onAccent = dark ? 0xff000000 | background : Color.WHITE;
        t.focus = (0x55 << 24) | accent;
        t.preview = t.key;
        return t;
    }

    /**
     * Material 3 color roles: tonal palettes from the wallpaper (Android 12+)
     * or the baseline scheme. Keys are flat. Expressive keys are rounder,
     * square up while pressed, and use the tertiary color for Enter.
     */
    private static Theme material(Context c, boolean dark, boolean expressive) {
        Tones p = Tones.of(c);
        Theme t = new Theme(dark);
        if (dark) {
            t.background = p.neutral[11];      // surface container (tone 6..10)
            t.key = p.neutralVariant[9];       // surface container highest
            t.specialKey = expressive ? p.primary[8] : p.secondary[8];
            t.pressed = expressive ? p.primary[7] : p.secondary[7];
            t.text = p.neutral[2];
            t.subText = p.neutralVariant[3];
            t.accent = expressive ? p.tertiary[3] : p.primary[3];
            t.onAccent = expressive ? p.tertiary[9] : p.primary[9];
            t.preview = p.neutralVariant[8];
        } else {
            t.background = p.neutral[1];
            t.key = p.neutral[0];
            t.specialKey = expressive ? p.primary[2] : p.secondary[2];
            t.pressed = expressive ? p.primary[3] : p.secondary[3];
            t.text = p.neutral[10];
            t.subText = p.neutralVariant[7];
            t.accent = expressive ? p.tertiary[7] : p.primary[7];
            t.onAccent = expressive ? p.tertiary[0] : p.primary[0];
            t.preview = p.neutral[0];
        }
        t.shadow = 0;
        t.focus = (0x66 << 24) | (t.accent & 0xffffff);
        if (expressive) {
            t.radius = 14;
            t.pressedRadius = 6;
            t.boldFunctions = true;
        } else {
            t.radius = 10;
            t.pressedRadius = 10;
        }
        return t;
    }

    /**
     * The five tonal palettes, each at tones 100, 95, 90, 80, 70, 60, 50, 40,
     * 30, 20, 10 and 6 (the indexes 0..11).
     */
    private static final class Tones {
        int[] primary;
        int[] secondary;
        int[] tertiary;
        int[] neutral;
        int[] neutralVariant;

        static Tones of(Context c) {
            Tones t = new Tones();
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                t.primary = system(c, ACCENT1);
                t.secondary = system(c, ACCENT2);
                t.tertiary = system(c, ACCENT3);
                t.neutral = system(c, NEUTRAL1);
                t.neutralVariant = system(c, NEUTRAL2);
            } else {
                // Material 3 baseline (seed #6750A4).
                t.primary = rgb(0xffffff, 0xf6edff, 0xeaddff, 0xd0bcff, 0xb69df8, 0x9a82db,
                        0x7f67be, 0x6750a4, 0x4f378b, 0x381e72, 0x21005d, 0x16003f);
                t.secondary = rgb(0xffffff, 0xf6edff, 0xe8def8, 0xccc2dc, 0xb0a7c0, 0x958da5,
                        0x7a7289, 0x625b71, 0x4a4458, 0x332d41, 0x1d192b, 0x120e1f);
                t.tertiary = rgb(0xffffff, 0xffecf1, 0xffd8e4, 0xefb8c8, 0xd29dac, 0xb58392,
                        0x986977, 0x7d5260, 0x633b48, 0x492532, 0x31111d, 0x220612);
                t.neutral = rgb(0xffffff, 0xf4eff4, 0xe6e1e5, 0xc9c5ca, 0xaeaaae, 0x939094,
                        0x787579, 0x605d62, 0x484649, 0x313033, 0x1c1b1f, 0x141218);
                t.neutralVariant = rgb(0xffffff, 0xf5eefa, 0xe7e0ec, 0xcac4d0, 0xaea9b4, 0x938f99,
                        0x79747e, 0x605d66, 0x49454f, 0x322f37, 0x1d1a22, 0x121017);
            }
            return t;
        }

        private static final int[] ACCENT1 = {
            android.R.color.system_accent1_0, android.R.color.system_accent1_50,
            android.R.color.system_accent1_100, android.R.color.system_accent1_200,
            android.R.color.system_accent1_300, android.R.color.system_accent1_400,
            android.R.color.system_accent1_500, android.R.color.system_accent1_600,
            android.R.color.system_accent1_700, android.R.color.system_accent1_800,
            android.R.color.system_accent1_900, android.R.color.system_accent1_1000,
        };
        private static final int[] ACCENT2 = {
            android.R.color.system_accent2_0, android.R.color.system_accent2_50,
            android.R.color.system_accent2_100, android.R.color.system_accent2_200,
            android.R.color.system_accent2_300, android.R.color.system_accent2_400,
            android.R.color.system_accent2_500, android.R.color.system_accent2_600,
            android.R.color.system_accent2_700, android.R.color.system_accent2_800,
            android.R.color.system_accent2_900, android.R.color.system_accent2_1000,
        };
        private static final int[] ACCENT3 = {
            android.R.color.system_accent3_0, android.R.color.system_accent3_50,
            android.R.color.system_accent3_100, android.R.color.system_accent3_200,
            android.R.color.system_accent3_300, android.R.color.system_accent3_400,
            android.R.color.system_accent3_500, android.R.color.system_accent3_600,
            android.R.color.system_accent3_700, android.R.color.system_accent3_800,
            android.R.color.system_accent3_900, android.R.color.system_accent3_1000,
        };
        private static final int[] NEUTRAL1 = {
            android.R.color.system_neutral1_0, android.R.color.system_neutral1_50,
            android.R.color.system_neutral1_100, android.R.color.system_neutral1_200,
            android.R.color.system_neutral1_300, android.R.color.system_neutral1_400,
            android.R.color.system_neutral1_500, android.R.color.system_neutral1_600,
            android.R.color.system_neutral1_700, android.R.color.system_neutral1_800,
            android.R.color.system_neutral1_900, android.R.color.system_neutral1_1000,
        };
        private static final int[] NEUTRAL2 = {
            android.R.color.system_neutral2_0, android.R.color.system_neutral2_50,
            android.R.color.system_neutral2_100, android.R.color.system_neutral2_200,
            android.R.color.system_neutral2_300, android.R.color.system_neutral2_400,
            android.R.color.system_neutral2_500, android.R.color.system_neutral2_600,
            android.R.color.system_neutral2_700, android.R.color.system_neutral2_800,
            android.R.color.system_neutral2_900, android.R.color.system_neutral2_1000,
        };

        private static int[] system(Context c, int[] ids) {
            int[] out = new int[ids.length];
            for (int i = 0; i < ids.length; i++) out[i] = c.getColor(ids[i]);
            // The last system tone is 0 (black); tone 6 sits a little above it.
            out[11] = blend(out[10], out[11], 0.4f);
            return out;
        }

        private static int[] rgb(int... colors) {
            int[] out = new int[colors.length];
            for (int i = 0; i < colors.length; i++) out[i] = 0xff000000 | colors[i];
            return out;
        }

        private static int blend(int a, int b, float f) {
            return Color.rgb(
                    Math.round(Color.red(a) + (Color.red(b) - Color.red(a)) * f),
                    Math.round(Color.green(a) + (Color.green(b) - Color.green(a)) * f),
                    Math.round(Color.blue(a) + (Color.blue(b) - Color.blue(a)) * f));
        }
    }
}
