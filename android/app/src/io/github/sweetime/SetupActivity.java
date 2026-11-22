package io.github.sweetime;

import android.app.Activity;
import android.content.Context;
import android.content.Intent;
import android.os.Bundle;
import android.provider.Settings;
import android.util.TypedValue;
import android.view.ViewGroup;
import android.view.inputmethod.InputMethodManager;
import android.widget.Button;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;

/** Explains how to turn the keyboard on, with a field to try it. */
public final class SetupActivity extends Activity {
    @Override
    protected void onCreate(Bundle saved) {
        super.onCreate(saved);
        int pad = (int) (20 * getResources().getDisplayMetrics().density);
        LinearLayout box = new LinearLayout(this);
        box.setOrientation(LinearLayout.VERTICAL);
        box.setPadding(pad, pad * 2, pad, pad);

        TextView title = new TextView(this);
        title.setText("Sweet IME");
        title.setTextSize(TypedValue.COMPLEX_UNIT_SP, 28);
        box.addView(title);

        TextView body = new TextView(this);
        body.setText("1. 「キーボードを有効にする」で Sweet IME をオンにします。\n"
                + "2. 「キーボードを切り替える」で Sweet IME を選びます。\n"
                + "3. 下の欄で試せます。\n\n"
                + "Shift を押してから打った英字は英単語になり、日本語が続くと日本語入力に戻ります。"
                + "「あ」キーで英字の直接入力と切り替えます。");
        body.setTextSize(TypedValue.COMPLEX_UNIT_SP, 16);
        body.setPadding(0, pad, 0, pad);
        box.addView(body);

        Button enable = new Button(this);
        enable.setText("キーボードを有効にする");
        enable.setOnClickListener(v -> startActivity(new Intent(Settings.ACTION_INPUT_METHOD_SETTINGS)));
        box.addView(enable);

        Button pick = new Button(this);
        pick.setText("キーボードを切り替える");
        pick.setOnClickListener(v -> ((InputMethodManager) getSystemService(Context.INPUT_METHOD_SERVICE))
                .showInputMethodPicker());
        box.addView(pick);

        EditText test = new EditText(this);
        test.setHint("ここで試す");
        test.setMinLines(3);
        box.addView(test, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));

        ScrollView scroll = new ScrollView(this);
        scroll.addView(box);
        setContentView(scroll);
    }
}
