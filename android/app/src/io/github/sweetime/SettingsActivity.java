package io.github.sweetime;

import android.app.Activity;
import android.content.Context;
import android.content.Intent;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.os.Bundle;
import android.provider.Settings;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.View;
import android.view.ViewGroup;
import android.view.inputmethod.InputMethodInfo;
import android.view.inputmethod.InputMethodManager;
import android.widget.Button;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.RadioButton;
import android.widget.RadioGroup;
import android.widget.ScrollView;
import android.widget.Switch;
import android.widget.TextView;
import android.widget.Toast;

import java.io.IOException;

/** Setup steps, conversion settings and keyboard settings. */
public final class SettingsActivity extends Activity {
    private LinearLayout box;
    private TextView status;
    private Theme theme;

    @Override
    protected void onCreate(Bundle saved) {
        super.onCreate(saved);
        build();
    }

    @Override
    protected void onResume() {
        super.onResume();
        updateStatus();
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        // The keyboard picker closes without a callback.
        if (hasFocus) updateStatus();
    }

    private int dp(int v) {
        return (int) (v * getResources().getDisplayMetrics().density);
    }

    private void build() {
        if (getActionBar() != null) getActionBar().hide();
        theme = Theme.of(this);
        getWindow().getDecorView().setBackgroundColor(theme.background);
        // Dark status bar icons on the light theme.
        int light = android.view.WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS
                | android.view.WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS;
        android.view.WindowInsetsController controller = getWindow().getInsetsController();
        if (controller != null) controller.setSystemBarsAppearance(theme.dark ? 0 : light, light);
        box = new LinearLayout(this);
        box.setOrientation(LinearLayout.VERTICAL);
        box.setPadding(dp(16), dp(24), dp(16), dp(32));
        // Not the try-it field: no keyboard until it is tapped.
        box.setFocusableInTouchMode(true);
        box.requestFocus();

        TextView title = text("Sweet IME", 28, theme.text);
        title.setTypeface(Typeface.DEFAULT_BOLD);
        box.addView(title);
        box.addView(text("Rust で書いた日本語入力", 14, theme.subText));

        // Setup.
        LinearLayout setup = section("はじめに");
        status = text("", 15, theme.text);
        status.setPadding(0, 0, 0, dp(8));
        setup.addView(status);
        Button enable = button("キーボードを有効にする");
        enable.setOnClickListener(v -> startActivity(new Intent(Settings.ACTION_INPUT_METHOD_SETTINGS)));
        setup.addView(enable);
        Button pick = button("キーボードを切り替える");
        pick.setOnClickListener(v -> ((InputMethodManager) getSystemService(Context.INPUT_METHOD_SERVICE))
                .showInputMethodPicker());
        setup.addView(pick);
        EditText test = new EditText(this);
        test.setHint("ここで試せます");
        test.setTextColor(theme.text);
        test.setHintTextColor(theme.subText);
        test.setMinLines(2);
        setup.addView(test, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));
        setup.addView(text("Shift で始めた英字は英単語になり、日本語が続くと日本語入力に戻ります。"
                + "地球儀のキーで英字の直接入力と切り替え、スペースキーを左右になぞるとカーソルが動きます。",
                13, theme.subText));

        // Conversion (the engine's settings file).
        LinearLayout conv = section("変換");
        Prefs.Engine engine = new Prefs.Engine(this);
        for (String[] sw : Prefs.Engine.SWITCHES) {
            final String key = sw[0];
            conv.addView(toggle(sw[1], sw[2], engine.get(key), on -> {
                engine.set(key, on);
                try {
                    engine.save();
                } catch (IOException e) {
                    Toast.makeText(this, "保存できませんでした: " + e.getMessage(), Toast.LENGTH_LONG).show();
                }
            }));
        }

        // Keyboard.
        LinearLayout kb = section("キーボード");
        kb.addView(prefToggle("数字の段", "文字キーの上に数字の段を表示します。オフのときは q〜p の長押しで数字を入力します。", Prefs.NUMBER_ROW, false));
        kb.addView(prefToggle("キーのプレビュー", "押したキーを大きく表示します。", Prefs.KEY_PREVIEW, true));
        kb.addView(prefToggle("振動", "キーを押したときに振動します。", Prefs.HAPTIC, true));
        kb.addView(prefToggle("キー音", "キーを押したときに音を鳴らします。", Prefs.SOUND, false));
        kb.addView(choice("キーの高さ", new String[] {"低め", "標準", "高め"}, Prefs.KEY_HEIGHT, 1));
        kb.addView(choice("テーマ", Theme.LABELS, Prefs.THEME, 0));

        box.addView(text("設定はキーボードを次に表示したときに反映されます。", 13, theme.subText));

        ScrollView scroll = new ScrollView(this);
        scroll.setBackgroundColor(theme.background);
        scroll.addView(box);
        // Below the status bar and above the navigation bar and the keyboard.
        scroll.setOnApplyWindowInsetsListener((v, insets) -> {
            android.graphics.Insets bars = insets.getInsets(
                    android.view.WindowInsets.Type.systemBars() | android.view.WindowInsets.Type.ime());
            // The status bar area stays clear while scrolling.
            v.setPadding(0, bars.top, 0, 0);
            box.setPadding(dp(16), dp(24), dp(16), dp(32) + bars.bottom);
            return android.view.WindowInsets.CONSUMED;
        });
        getWindow().setSoftInputMode(android.view.WindowManager.LayoutParams.SOFT_INPUT_STATE_HIDDEN
                | android.view.WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE);
        setContentView(scroll);
    }

    private void updateStatus() {
        if (status == null) return;
        InputMethodManager imm = (InputMethodManager) getSystemService(Context.INPUT_METHOD_SERVICE);
        boolean enabled = false;
        for (InputMethodInfo info : imm.getEnabledInputMethodList()) {
            if (info.getPackageName().equals(getPackageName())) enabled = true;
        }
        String current = Settings.Secure.getString(getContentResolver(), Settings.Secure.DEFAULT_INPUT_METHOD);
        boolean selected = current != null && current.startsWith(getPackageName() + "/");
        status.setText(selected ? "✓ Sweet IME を使っています"
                : enabled ? "Sweet IME は有効です。「キーボードを切り替える」で選んでください。"
                : "まず「キーボードを有効にする」で Sweet IME をオンにしてください。");
    }

    private TextView text(String s, int sp, int color) {
        TextView t = new TextView(this);
        t.setText(s);
        t.setTextSize(TypedValue.COMPLEX_UNIT_SP, sp);
        t.setTextColor(color);
        return t;
    }

    private Button button(String label) {
        Button b = new Button(this);
        b.setText(label);
        b.setAllCaps(false);
        b.setTextColor(theme.onAccent);
        GradientDrawable bg = new GradientDrawable();
        bg.setColor(theme.accent);
        bg.setCornerRadius(dp(22));
        b.setBackground(bg);
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, dp(44));
        lp.bottomMargin = dp(8);
        b.setLayoutParams(lp);
        return b;
    }

    /** A titled card added to the page; returns its body. */
    private LinearLayout section(String title) {
        TextView t = text(title, 14, theme.accent);
        t.setTypeface(Typeface.DEFAULT_BOLD);
        t.setPadding(dp(4), dp(24), 0, dp(8));
        box.addView(t);
        LinearLayout card = new LinearLayout(this);
        card.setOrientation(LinearLayout.VERTICAL);
        card.setPadding(dp(16), dp(12), dp(16), dp(12));
        GradientDrawable bg = new GradientDrawable();
        bg.setColor(theme.key);
        bg.setCornerRadius(dp(16));
        card.setBackground(bg);
        box.addView(card);
        return card;
    }

    private interface OnToggle {
        void changed(boolean on);
    }

    private View toggle(String title, String desc, boolean on, OnToggle onToggle) {
        LinearLayout row = new LinearLayout(this);
        row.setOrientation(LinearLayout.HORIZONTAL);
        row.setGravity(Gravity.CENTER_VERTICAL);
        row.setPadding(0, dp(8), 0, dp(8));
        LinearLayout texts = new LinearLayout(this);
        texts.setOrientation(LinearLayout.VERTICAL);
        texts.addView(text(title, 16, theme.text));
        texts.addView(text(desc, 13, theme.subText));
        row.addView(texts, new LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1));
        Switch sw = new Switch(this);
        sw.setChecked(on);
        sw.setOnCheckedChangeListener((b, checked) -> onToggle.changed(checked));
        row.addView(sw);
        row.setOnClickListener(v -> sw.toggle());
        return row;
    }

    private View prefToggle(String title, String desc, String key, boolean def) {
        return toggle(title, desc, Prefs.flag(this, key, def),
                on -> Prefs.keyboard(this).edit().putBoolean(key, on).apply());
    }

    private View choice(String title, String[] labels, String key, int def) {
        LinearLayout col = new LinearLayout(this);
        col.setOrientation(LinearLayout.VERTICAL);
        col.setPadding(0, dp(8), 0, dp(8));
        col.addView(text(title, 16, theme.text));
        RadioGroup group = new RadioGroup(this);
        // Long lists (the themes) stack; short ones fit on one line.
        group.setOrientation(labels.length > 3 ? RadioGroup.VERTICAL : RadioGroup.HORIZONTAL);
        int current = Prefs.number(this, key, def);
        for (int i = 0; i < labels.length; i++) {
            RadioButton rb = new RadioButton(this);
            rb.setText(labels[i]);
            rb.setTextColor(theme.text);
            rb.setId(View.generateViewId());
            final int value = i;
            group.addView(rb);
            if (i == current) rb.setChecked(true);
            rb.setOnCheckedChangeListener((b, checked) -> {
                if (!checked) return;
                Prefs.keyboard(this).edit().putInt(key, value).apply();
                if (key.equals(Prefs.THEME)) recreate();
            });
        }
        col.addView(group);
        return col;
    }
}
