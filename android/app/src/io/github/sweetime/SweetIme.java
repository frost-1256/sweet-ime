package io.github.sweetime;

import android.content.pm.PackageInfo;
import android.graphics.Typeface;
import android.inputmethodservice.InputMethodService;
import android.os.Handler;
import android.os.Looper;
import android.text.SpannableString;
import android.text.Spanned;
import android.text.style.BackgroundColorSpan;
import android.text.style.UnderlineSpan;
import android.util.Log;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.KeyEvent;
import android.view.View;
import android.view.ViewGroup;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.widget.HorizontalScrollView;
import android.widget.LinearLayout;
import android.widget.TextView;

import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;

/** The input method service: a QWERTY keyboard with a candidate bar. */
public final class SweetIme extends InputMethodService implements KeyboardView.Listener {
    private static final String TAG = "SweetIME";

    private static volatile boolean ready;
    private static volatile boolean loading;

    private final Handler main = new Handler(Looper.getMainLooper());
    private long ctx;
    private Theme theme;
    private KeyboardView keyboard;
    private LinearLayout candidateRow;
    private HorizontalScrollView candidateScroll;
    /** Direct English input (the あ/A key) or a field that wants no conversion. */
    private boolean english;
    private boolean composing;
    private String themeKey = "";
    private int composingLength;

    @Override
    public void onCreate() {
        super.onCreate();
        loadEngine();
    }

    @Override
    public void onDestroy() {
        if (ctx != 0) {
            Native.free(ctx);
            ctx = 0;
        }
        super.onDestroy();
    }

    /** Copies the dictionary out of the APK (once per version) and loads it. */
    private void loadEngine() {
        if (ready || loading) return;
        loading = true;
        new Thread(() -> {
            try {
                File dict = new File(getFilesDir(), "system.dict");
                File stamp = new File(getFilesDir(), "system.dict.version");
                PackageInfo pi = getPackageManager().getPackageInfo(getPackageName(), 0);
                String want = pi.versionName + "/" + pi.lastUpdateTime;
                String have = stamp.exists()
                        ? new String(Files.readAllBytes(stamp.toPath()), StandardCharsets.UTF_8) : "";
                if (!dict.exists() || !want.equals(have)) {
                    File tmp = new File(getFilesDir(), "system.dict.tmp");
                    try (InputStream in = getAssets().open("system.dict");
                         OutputStream out = new FileOutputStream(tmp)) {
                        byte[] buf = new byte[1 << 20];
                        int n;
                        while ((n = in.read(buf)) > 0) out.write(buf, 0, n);
                    }
                    if (!tmp.renameTo(dict)) throw new java.io.IOException("rename failed");
                    Files.write(stamp.toPath(), want.getBytes(StandardCharsets.UTF_8));
                }
                boolean ok = Native.init(dict.getAbsolutePath(), getFilesDir().getAbsolutePath());
                Log.i(TAG, "dictionary loaded: " + ok);
                ready = ok;
            } catch (Exception e) {
                Log.e(TAG, "loading the dictionary failed", e);
            } finally {
                loading = false;
            }
        }, "sweet-ime-load").start();
    }

    private boolean engine() {
        if (!ready) return false;
        if (ctx == 0) {
            ctx = Native.create();
            String before = textBeforeCursor();
            if (before != null) Native.setLeftContext(ctx, before);
        }
        return ctx != 0;
    }

    @Override
    public View onCreateInputView() {
        theme = Theme.of(this);
        themeKey = themeKey();
        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setBackgroundColor(theme.background);
        // The key preview draws over the candidate bar.
        root.setClipChildren(false);

        candidateScroll = new HorizontalScrollView(this);
        candidateScroll.setHorizontalScrollBarEnabled(false);
        candidateRow = new LinearLayout(this);
        candidateRow.setOrientation(LinearLayout.HORIZONTAL);
        candidateScroll.addView(candidateRow);
        root.addView(candidateScroll, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, dp(44)));

        keyboard = new KeyboardView(this, theme);
        keyboard.setListener(this);
        keyboard.setEnglish(english);
        configureKeyboard();
        root.addView(keyboard, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));
        // Keep the keys above the navigation bar (edge to edge since Android 15).
        root.setOnApplyWindowInsetsListener((v, insets) -> {
            // The IME's own navigation bar (back, switch keyboard) is taller
            // than navigationBars() reports; systemBars() covers it.
            int bottom = android.os.Build.VERSION.SDK_INT >= 30
                    ? insets.getInsets(android.view.WindowInsets.Type.systemBars()
                            | android.view.WindowInsets.Type.tappableElement()).bottom
                    : insets.getSystemWindowInsetBottom();
            v.setPadding(0, 0, 0, bottom);
            return insets;
        });
        showCandidates(new String[0], -1, false);
        return root;
    }

    /**
     * What the keyboard looks like depends on: the theme, the system's night
     * mode and, for Material You, the wallpaper colors.
     */
    private String themeKey() {
        Theme t = Theme.of(this);
        return Prefs.number(this, Prefs.THEME, 0) + "/" + t.dark + "/" + t.key + "/" + t.accent;
    }

    private void configureKeyboard() {
        keyboard.configure(theme,
                Prefs.flag(this, Prefs.HAPTIC, true),
                Prefs.flag(this, Prefs.SOUND, false),
                Prefs.flag(this, Prefs.NUMBER_ROW, false),
                Prefs.flag(this, Prefs.KEY_PREVIEW, true),
                Prefs.number(this, Prefs.KEY_HEIGHT, 1));
    }

    private int dp(int v) {
        return (int) (v * getResources().getDisplayMetrics().density);
    }

    @Override
    public void onStartInput(EditorInfo attr, boolean restarting) {
        super.onStartInput(attr, restarting);
        if (ctx != 0) Native.reset(ctx);
        composing = false;
        composingLength = 0;
        int cls = attr.inputType & EditorInfo.TYPE_MASK_CLASS;
        int variation = attr.inputType & EditorInfo.TYPE_MASK_VARIATION;
        boolean password = variation == EditorInfo.TYPE_TEXT_VARIATION_PASSWORD
                || variation == EditorInfo.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD
                || variation == EditorInfo.TYPE_TEXT_VARIATION_WEB_PASSWORD;
        boolean url = variation == EditorInfo.TYPE_TEXT_VARIATION_URI
                || variation == EditorInfo.TYPE_TEXT_VARIATION_EMAIL_ADDRESS
                || variation == EditorInfo.TYPE_TEXT_VARIATION_WEB_EMAIL_ADDRESS;
        english = cls != EditorInfo.TYPE_CLASS_TEXT || password || url;
        if (keyboard != null) keyboard.setEnglish(english);
        if (ctx != 0) {
            String before = textBeforeCursor();
            if (before != null) Native.setLeftContext(ctx, before);
        }
        showCandidates(new String[0], -1, false);
    }

    @Override
    public void onStartInputView(EditorInfo attr, boolean restarting) {
        super.onStartInputView(attr, restarting);
        // Settings may have changed since the keyboard was last shown.
        if (keyboard != null && !themeKey().equals(themeKey)) {
            setInputView(onCreateInputView());
        } else if (keyboard != null) {
            configureKeyboard();
        }
        if (ctx != 0) Native.reloadOptions(ctx);
        if (keyboard != null) {
            keyboard.setEnglish(english);
            keyboard.showLetters();
        }
        updateKeys();
    }

    @Override
    public void onFinishInput() {
        if (composing && ctx != 0) apply(Native.commitAll(ctx));
        super.onFinishInput();
    }

    @Override
    public void onUpdateSelection(int oldSelStart, int oldSelEnd, int newSelStart, int newSelEnd,
                                  int candidatesStart, int candidatesEnd) {
        super.onUpdateSelection(oldSelStart, oldSelEnd, newSelStart, newSelEnd,
                candidatesStart, candidatesEnd);
        // The caret left the composing text (a tap elsewhere): commit it.
        if (composing && candidatesStart >= 0
                && (newSelStart != newSelEnd || newSelStart < candidatesStart
                || newSelStart > candidatesEnd)) {
            Native.reset(ctx);
            composing = false;
            InputConnection ic = getCurrentInputConnection();
            if (ic != null) ic.finishComposingText();
            showCandidates(new String[0], -1, false);
        }
        updateKeys();
    }

    private String textBeforeCursor() {
        InputConnection ic = getCurrentInputConnection();
        if (ic == null) return null;
        CharSequence t = ic.getTextBeforeCursor(32, 0);
        return t == null ? null : t.toString();
    }

    @Override
    public void onToggleMode() {
        if (composing && ctx != 0) apply(Native.commitAll(ctx));
        english = !english;
        keyboard.setEnglish(english);
        updateKeys();
    }

    @Override
    public void onText(String text) {
        if (composing && ctx != 0) apply(Native.commitAll(ctx));
        InputConnection ic = getCurrentInputConnection();
        if (ic != null) ic.commitText(text, 1);
        updateKeys();
    }

    /** The action Enter performs in this field, or IME_ACTION_NONE for a new line. */
    private int enterAction() {
        EditorInfo ei = getCurrentInputEditorInfo();
        if (ei == null) return EditorInfo.IME_ACTION_NONE;
        int action = ei.imeOptions & EditorInfo.IME_MASK_ACTION;
        boolean multiLine = (ei.inputType & EditorInfo.TYPE_TEXT_FLAG_MULTI_LINE) != 0;
        boolean noEnterAction = (ei.imeOptions & EditorInfo.IME_FLAG_NO_ENTER_ACTION) != 0;
        if (multiLine || noEnterAction || action == EditorInfo.IME_ACTION_UNSPECIFIED) {
            return EditorInfo.IME_ACTION_NONE;
        }
        return action;
    }

    private String enterLabel() {
        switch (enterAction()) {
            case EditorInfo.IME_ACTION_SEARCH:
                return english ? "Search" : "検索";
            case EditorInfo.IME_ACTION_SEND:
                return english ? "Send" : "送信";
            case EditorInfo.IME_ACTION_GO:
                return english ? "Go" : "移動";
            case EditorInfo.IME_ACTION_NEXT:
                return english ? "Next" : "次へ";
            case EditorInfo.IME_ACTION_DONE:
                return english ? "Done" : "完了";
            case EditorInfo.IME_ACTION_PREVIOUS:
                return english ? "Prev" : "前へ";
            default:
                return "⏎";
        }
    }

    /** Space and Enter labels, and Shift at the start of an English sentence. */
    private void updateKeys() {
        if (keyboard == null) return;
        boolean converting = composing && ctx != 0 && Native.converting(ctx);
        keyboard.setState(composing, converting, enterLabel());
        boolean caps = false;
        EditorInfo ei = getCurrentInputEditorInfo();
        InputConnection ic = getCurrentInputConnection();
        if (english && !composing && ei != null && ic != null
                && (ei.inputType & EditorInfo.TYPE_MASK_CLASS) == EditorInfo.TYPE_CLASS_TEXT
                && (ei.inputType & (EditorInfo.TYPE_TEXT_FLAG_CAP_SENTENCES
                        | EditorInfo.TYPE_TEXT_FLAG_CAP_WORDS
                        | EditorInfo.TYPE_TEXT_FLAG_CAP_CHARACTERS)) != 0) {
            caps = ic.getCursorCapsMode(ei.inputType) != 0;
        }
        keyboard.setAutoShift(caps);
    }

    @Override
    public void onKey(int code) {
        if (!english && engine()) {
            String s = Native.key(ctx, code);
            if (s != null) {
                apply(s);
                return;
            }
        }
        direct(code);
        updateKeys();
    }

    /** What the key does without the IME. */
    private void direct(int code) {
        InputConnection ic = getCurrentInputConnection();
        if (ic == null) return;
        switch (code) {
            case Native.KEY_BACKSPACE:
                CharSequence sel = ic.getSelectedText(0);
                if (sel != null && sel.length() > 0) {
                    ic.commitText("", 1);
                } else {
                    sendDownUpKeyEvents(KeyEvent.KEYCODE_DEL);
                }
                break;
            case Native.KEY_ENTER:
                int action = enterAction();
                if (action != EditorInfo.IME_ACTION_NONE) {
                    ic.performEditorAction(action);
                } else {
                    sendDownUpKeyEvents(KeyEvent.KEYCODE_ENTER);
                }
                break;
            case Native.KEY_SPACE:
                ic.commitText(" ", 1);
                break;
            case Native.KEY_LEFT:
                sendDownUpKeyEvents(KeyEvent.KEYCODE_DPAD_LEFT);
                break;
            case Native.KEY_RIGHT:
                sendDownUpKeyEvents(KeyEvent.KEYCODE_DPAD_RIGHT);
                break;
            case Native.KEY_ESCAPE:
                break;
            default:
                if (code >= 0) ic.commitText(new String(Character.toChars(code & ~Native.SHIFTED)), 1);
        }
    }

    /** Shows a snapshot from the engine. */
    private void apply(String snapshot) {
        Native.State st = Native.State.parse(snapshot);
        InputConnection ic = getCurrentInputConnection();
        if (ic == null) return;
        ic.beginBatchEdit();
        if (!st.commit.isEmpty()) ic.commitText(st.commit, 1);
        if (st.preedit.isEmpty()) {
            if (composing && st.commit.isEmpty()) ic.commitText("", 1);
            composing = false;
        } else {
            SpannableString text = new SpannableString(st.preedit);
            for (int[] span : st.spans) {
                int a = Math.max(0, Math.min(span[0], st.preedit.length()));
                int b = Math.max(a, Math.min(span[1], st.preedit.length()));
                if (a == b) continue;
                text.setSpan(new UnderlineSpan(), a, b, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE);
                if (span[2] == 2) {
                    text.setSpan(new BackgroundColorSpan(theme.focus), a, b,
                            Spanned.SPAN_EXCLUSIVE_EXCLUSIVE);
                }
            }
            if (st.spans.length == 0) {
                text.setSpan(new UnderlineSpan(), 0, st.preedit.length(),
                        Spanned.SPAN_EXCLUSIVE_EXCLUSIVE);
            }
            ic.setComposingText(text, 1);
            composing = true;
        }
        composingLength = st.preedit.length();
        ic.endBatchEdit();
        showCandidates(st.candidates, st.selected, st.suggestions);
        updateKeys();
    }

    private void showCandidates(String[] items, int selected, boolean suggestions) {
        if (candidateRow == null) return;
        candidateRow.removeAllViews();
        if (items.length == 0) {
            // Nothing to pick: the settings button.
            TextView gear = new TextView(this);
            gear.setText("⚙");
            gear.setTextSize(TypedValue.COMPLEX_UNIT_SP, 20);
            gear.setTextColor(theme.subText);
            gear.setGravity(Gravity.CENTER);
            gear.setPadding(dp(14), 0, dp(14), 0);
            gear.setOnClickListener(v -> {
                android.content.Intent i = new android.content.Intent(this, SettingsActivity.class);
                i.addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK);
                startActivity(i);
            });
            candidateRow.addView(gear, new LinearLayout.LayoutParams(
                    ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.MATCH_PARENT));
            candidateScroll.scrollTo(0, 0);
            return;
        }
        for (int i = 0; i < items.length; i++) {
            final int index = i;
            TextView tv = new TextView(this);
            tv.setText(items[i]);
            tv.setTextSize(TypedValue.COMPLEX_UNIT_SP, 18);
            tv.setTextColor(theme.text);
            tv.setGravity(Gravity.CENTER);
            tv.setPadding(dp(14), 0, dp(14), 0);
            tv.setMinWidth(dp(44));
            if (i == selected) {
                tv.setBackgroundColor(theme.focus);
                tv.setTypeface(Typeface.DEFAULT_BOLD);
            }
            tv.setOnClickListener(v -> {
                if (ctx != 0) apply(Native.select(ctx, index));
            });
            candidateRow.addView(tv, new LinearLayout.LayoutParams(
                    ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.MATCH_PARENT));
            if (i + 1 < items.length) {
                View sep = new View(this);
                sep.setBackgroundColor(theme.specialKey);
                LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(dp(1), dp(20));
                lp.gravity = Gravity.CENTER_VERTICAL;
                candidateRow.addView(sep, lp);
            }
        }
        if (selected >= 0) {
            final int target = selected * 2;
            main.post(() -> {
                if (target < candidateRow.getChildCount()) {
                    View v = candidateRow.getChildAt(target);
                    candidateScroll.smoothScrollTo(Math.max(0, v.getLeft() - dp(40)), 0);
                }
            });
        } else {
            candidateScroll.scrollTo(0, 0);
        }
    }
}
