package io.github.sweetime;

import android.content.ClipData;
import android.content.ClipDescription;
import android.content.ClipboardManager;
import android.content.pm.PackageInfo;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.inputmethodservice.InputMethodService;
import android.net.Uri;
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
import android.view.inputmethod.ExtractedText;
import android.view.inputmethod.ExtractedTextRequest;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputContentInfo;
import android.widget.FrameLayout;
import android.widget.HorizontalScrollView;
import android.widget.LinearLayout;
import android.widget.ScrollView;
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
    private LinearLayout inputRoot;
    private KeyboardView keyboard;
    private LinearLayout candidateRow;
    private HorizontalScrollView candidateScroll;
    private ScrollView expandedScroll;
    private CandidateFlow expandedCandidates;
    private ClipboardPanel clipboardPanel;
    private final java.util.ArrayList<String> recentClips = new java.util.ArrayList<>();
    private View expandButton;
    private View clipboardButton;
    private TextView panelTitle;
    private String[] candidateItems = new String[0];
    private boolean candidatesExpanded;
    private boolean clipboardOpen;
    private boolean privateField;
    /** Direct English input (the あ/A key) or a field that wants no conversion. */
    private boolean english;
    /** The globe key's choice for ordinary text fields, kept across fields. */
    private boolean chosenEnglish;
    /** This field wants no conversion (passwords, URLs, numbers). */
    private boolean fieldEnglish;
    private boolean composing;
    private int selectedCandidate = -1;
    private int composingLength;
    /** When Space last typed a space (for the double-space period). */
    private long lastSpace;
    /** The editor reported the current composing text (see onUpdateSelection). */
    private boolean compositionSeen;
    /**
     * Recent composing states we showed, as length << 32 | caret: a caret
     * inside the composing text that matches none was moved by the user.
     */
    private final java.util.ArrayDeque<Long> shown = new java.util.ArrayDeque<>();

    @Override
    public void onCreate() {
        super.onCreate();
        refineThread.start();
        refiner = new Handler(refineThread.getLooper());
        loadEngine();
    }

    /** Runs the zenz model for live conversion off the main thread. */
    private final android.os.HandlerThread refineThread = new android.os.HandlerThread("sweet-ime-zenz");
    private Handler refiner;
    /** The newest live conversion to refine: {reading, left context}. */
    private volatile String[] liveJob;
    /** The last request sent, so a refined result does not ask again. */
    private String lastLive = "";

    /**
     * Refines the live conversion with the model in the background; the
     * result replaces it if the reading is still the same.
     */
    private void requestLive() {
        if (ctx == 0 || !composing || refiner == null) return;
        String req = Native.liveRequest(ctx);
        if (req == null || req.equals(lastLive)) return;
        lastLive = req;
        liveJob = req.split("\u001f", 2);
        refiner.removeCallbacksAndMessages(null);
        refiner.post(() -> {
            String[] job = liveJob;
            liveJob = null;
            if (job == null || job.length < 2) return;
            String surface = Native.refine(job[0], job[1]);
            if (surface == null) return;
            main.post(() -> {
                if (ctx == 0 || !composing) return;
                String snap = Native.applyLive(ctx, job[0], surface);
                if (snap != null) apply(snap);
            });
        });
    }

    @Override
    public void onDestroy() {
        refineThread.quitSafely();
        if (clipboardPanel != null) clipboardPanel.setInputActive(false, privateField);
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
        if (clipboardPanel != null) clipboardPanel.setInputActive(false, privateField);
        candidatesExpanded = false;
        clipboardOpen = false;
        LinearLayout root = new LinearLayout(this);
        inputRoot = root;
        root.setOrientation(LinearLayout.VERTICAL);
        root.setBackgroundColor(theme.background);
        // The key preview draws over the candidate bar.
        root.setClipChildren(false);

        candidateScroll = new HorizontalScrollView(this);
        candidateScroll.setHorizontalScrollBarEnabled(false);
        candidateRow = new LinearLayout(this);
        candidateRow.setOrientation(LinearLayout.HORIZONTAL);
        candidateScroll.addView(candidateRow);
        LinearLayout toolbar = new LinearLayout(this);
        toolbar.addView(candidateScroll, new LinearLayout.LayoutParams(0, dp(48), 1));
        panelTitle = new TextView(this);
        panelTitle.setTextSize(16);
        panelTitle.setGravity(Gravity.CENTER_VERTICAL);
        panelTitle.setPadding(dp(14), 0, 0, 0);
        toolbar.addView(panelTitle, new LinearLayout.LayoutParams(0, dp(48), 1));
        expandButton = iconButton(() -> candidatesExpanded ? Icons.COLLAPSE : Icons.EXPAND,
                "候補を展開", () -> {
                    candidatesExpanded = !candidatesExpanded;
                    if (candidatesExpanded) {
                        renderExpandedCandidates();
                        expandedScroll.scrollTo(0, 0);
                    }
                    updatePanelVisibility();
                });
        toolbar.addView(expandButton, new LinearLayout.LayoutParams(dp(48), dp(48)));
        clipboardButton = iconButton(() -> clipboardOpen ? Icons.BACK : Icons.CLIPBOARD,
                "クリップボード", () -> {
                    clipboardOpen = !clipboardOpen;
                    candidatesExpanded = false;
                    updatePanelVisibility();
                    if (clipboardOpen) clipboardPanel.open(theme);
                });
        toolbar.addView(clipboardButton, new LinearLayout.LayoutParams(dp(48), dp(48)));
        root.addView(toolbar, new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(48)));

        keyboard = new KeyboardView(this, theme);
        keyboard.setListener(this);
        keyboard.setEnglish(english);
        configureKeyboard();
        KeyboardView keys = keyboard;
        FrameLayout board = new FrameLayout(this) {
            @Override protected void onMeasure(int widthSpec, int heightSpec) {
                keys.measure(widthSpec, heightSpec);
                super.onMeasure(widthSpec,
                        MeasureSpec.makeMeasureSpec(keys.getMeasuredHeight(), MeasureSpec.EXACTLY));
            }
        };
        board.setClipChildren(false);
        board.addView(keyboard, new FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));
        expandedScroll = new ScrollView(this);
        expandedCandidates = new CandidateFlow(this, dp(5));
        expandedScroll.addView(expandedCandidates);
        board.addView(expandedScroll, new FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));
        clipboardPanel = new ClipboardPanel(this, recentClips, new ClipboardPanel.Listener() {
            @Override public void pasteText(String value) {
                onText(value);
                closeClipboard();
            }
            @Override public void pasteCurrent() { pasteSystemClipboard(); }
        });
        board.addView(clipboardPanel, new FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));
        root.addView(board, new LinearLayout.LayoutParams(
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

    private void refreshTheme() {
        theme = Theme.of(this);
        inputRoot.setBackgroundColor(theme.background);
        panelTitle.setTextColor(theme.text);
        clipboardButton.invalidate();
        expandButton.invalidate();
        for (int i = 0; i < candidateRow.getChildCount(); i++) {
            View child = candidateRow.getChildAt(i);
            if (child instanceof TextView) {
                ((TextView) child).setTextColor(theme.text);
                if (selectedCandidate >= 0 && i == selectedCandidate * 2) {
                    child.setBackgroundColor(theme.focus);
                }
            } else if (i % 2 == 1) {
                child.setBackgroundColor(theme.specialKey);
            } else {
                child.invalidate(); // Settings icon
            }
        }
    }

    private void configureKeyboard() {
        keyboard.configure(theme,
                Prefs.flag(this, Prefs.HAPTIC, true),
                Prefs.flag(this, Prefs.SOUND, false),
                Prefs.flag(this, Prefs.NUMBER_ROW, false),
                Prefs.flag(this, Prefs.KEY_PREVIEW, true),
                Prefs.number(this, Prefs.KEY_HEIGHT, 1),
                Prefs.number(this, Prefs.PUNCTUATION_WIDTH, 1));
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
        boolean password = (cls == EditorInfo.TYPE_CLASS_TEXT
                && (variation == EditorInfo.TYPE_TEXT_VARIATION_PASSWORD
                    || variation == EditorInfo.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD
                    || variation == EditorInfo.TYPE_TEXT_VARIATION_WEB_PASSWORD))
                || (cls == EditorInfo.TYPE_CLASS_NUMBER
                    && variation == EditorInfo.TYPE_NUMBER_VARIATION_PASSWORD);
        privateField = password || (attr.imeOptions & EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING) != 0;
        if (clipboardPanel != null) clipboardPanel.setInputActive(false, privateField);
        clipboardOpen = false;
        boolean url = variation == EditorInfo.TYPE_TEXT_VARIATION_URI
                || variation == EditorInfo.TYPE_TEXT_VARIATION_EMAIL_ADDRESS
                || variation == EditorInfo.TYPE_TEXT_VARIATION_WEB_EMAIL_ADDRESS;
        fieldEnglish = cls != EditorInfo.TYPE_CLASS_TEXT || password || url;
        english = fieldEnglish || chosenEnglish;
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
        // Keep the attached view and its navigation-bar insets when changing themes.
        if (keyboard != null) {
            refreshTheme();
            configureKeyboard();
            clipboardPanel.setInputActive(true, privateField);
            if (candidatesExpanded) renderExpandedCandidates();
            if (clipboardOpen) clipboardPanel.open(theme);
        }
        if (ctx != 0) Native.reloadOptions(ctx);
        if (keyboard != null) {
            keyboard.setEnglish(english);
            keyboard.showLetters();
        }
        updateKeys();
    }

    @Override
    public void onFinishInputView(boolean finishingInput) {
        if (clipboardPanel != null) clipboardPanel.setInputActive(false, privateField);
        clipboardOpen = false;
        candidatesExpanded = false;
        updatePanelVisibility();
        // Hiding the keyboard keeps the text being typed, as it is shown.
        if (composing && ctx != 0) apply(Native.commitAll(ctx));
        super.onFinishInputView(finishingInput);
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
        // The editor shows our composing text: from now on, losing it
        // means the app committed it (e.g. on a tap). Updates can be late,
        // so only one matching the current text counts.
        if (composing && candidatesStart >= 0 && candidatesEnd - candidatesStart == composingLength) {
            compositionSeen = true;
        }
        boolean lost = composing && compositionSeen && candidatesStart < 0;
        // A tap inside the composing text: the reading's caret cannot
        // follow it, so the text is kept as it is.
        boolean moved = composing && candidatesStart >= 0 && newSelStart == newSelEnd
                && newSelStart >= candidatesStart && newSelStart <= candidatesEnd
                && !shown.contains(((long) (candidatesEnd - candidatesStart) << 32)
                        | (newSelStart - candidatesStart));
        // The caret left the composing text (a tap elsewhere), or the app
        // finished it: keep the text and start over, or the next key
        // would type the old reading again.
        if (lost || moved || composing && candidatesStart >= 0
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
        if (!fieldEnglish) chosenEnglish = english;
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

    private void closeClipboard() {
        clipboardOpen = false;
        updatePanelVisibility();
    }

    private void pasteSystemClipboard() {
        if (composing && ctx != 0) apply(Native.commitAll(ctx));
        InputConnection ic = getCurrentInputConnection();
        if (ic == null) return;
        if (pasteClipboardImage(ic) || ic.performContextMenuAction(android.R.id.paste)) {
            closeClipboard();
            updateKeys();
        } else {
            android.widget.Toast.makeText(this, "貼り付けられませんでした", android.widget.Toast.LENGTH_SHORT).show();
        }
    }

    private boolean pasteClipboardImage(InputConnection ic) {
        EditorInfo editor = getCurrentInputEditorInfo();
        if (privateField || editor == null || editor.contentMimeTypes == null) return false;
        ClipboardManager clipboard = getSystemService(ClipboardManager.class);
        if (clipboard == null) return false;
        try {
            ClipData clip = clipboard.getPrimaryClip();
            if (clip == null || clip.getItemCount() == 0) return false;
            ClipDescription description = clip.getDescription();
            Uri uri = clip.getItemAt(0).getUri();
            if (uri == null || !"content".equals(uri.getScheme())
                    || !description.hasMimeType("image/*")) return false;
            for (String mime : editor.contentMimeTypes) {
                if (description.hasMimeType(mime)) {
                    return ic.commitContent(new InputContentInfo(uri, description, null),
                            InputConnection.INPUT_CONTENT_GRANT_READ_URI_PERMISSION, null);
                }
            }
        } catch (IllegalArgumentException | SecurityException ignored) {
            // The editor can still try the system paste operation.
        }
        return false;
    }

    /** The action Enter performs in this field, or IME_ACTION_NONE for a new line. */
    private int enterAction() {
        EditorInfo ei = getCurrentInputEditorInfo();
        if (ei == null) return EditorInfo.IME_ACTION_NONE;
        int action = ei.imeOptions & EditorInfo.IME_MASK_ACTION;
        // Multi-line text views set IME_FLAG_NO_ENTER_ACTION themselves; a
        // multi-line web field can still ask for an action (enterkeyhint).
        boolean noEnterAction = (ei.imeOptions & EditorInfo.IME_FLAG_NO_ENTER_ACTION) != 0;
        if (noEnterAction || action == EditorInfo.IME_ACTION_UNSPECIFIED) {
            return EditorInfo.IME_ACTION_NONE;
        }
        return action;
    }

    /** Space and Enter labels, and Shift at the start of an English sentence. */
    private void updateKeys() {
        if (keyboard == null) return;
        boolean converting = composing && ctx != 0 && Native.converting(ctx);
        keyboard.setState(composing, converting, enterAction());
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
            case Native.KEY_SPACE: {
                // English: a second space right after a word ends the
                // sentence (". "), as on common keyboards.
                long now = android.os.SystemClock.uptimeMillis();
                CharSequence before = english && now - lastSpace < 700
                        ? ic.getTextBeforeCursor(2, 0) : null;
                if (before != null && before.length() == 2 && before.charAt(1) == ' '
                        && Character.isLetterOrDigit(before.charAt(0))) {
                    ic.beginBatchEdit();
                    ic.deleteSurroundingText(1, 0);
                    ic.commitText(". ", 1);
                    ic.endBatchEdit();
                    lastSpace = 0;
                } else {
                    ic.commitText(" ", 1);
                    lastSpace = now;
                }
                break;
            }
            case Native.KEY_LEFT:
            case Native.KEY_RIGHT:
                moveCaret(ic, code == Native.KEY_RIGHT);
                break;
            case Native.KEY_ESCAPE:
                break;
            default:
                if (code >= 0) ic.commitText(new String(Character.toChars(code & ~Native.SHIFTED)), 1);
        }
    }

    /**
     * Moves the caret one character. Arrow key events would move the focus
     * to another view at either end of the text, so the caret is set
     * directly when the editor reports its text.
     */
    private void moveCaret(InputConnection ic, boolean right) {
        ExtractedText et = ic.getExtractedText(new ExtractedTextRequest(), 0);
        if (et == null || et.text == null || et.selectionStart < 0) {
            sendDownUpKeyEvents(right ? KeyEvent.KEYCODE_DPAD_RIGHT : KeyEvent.KEYCODE_DPAD_LEFT);
            return;
        }
        CharSequence t = et.text;
        int start = Math.min(et.selectionStart, et.selectionEnd);
        int end = Math.max(et.selectionStart, et.selectionEnd);
        int pos;
        if (start != end) {
            // A selection collapses to the side of the arrow.
            pos = right ? end : start;
        } else if (right) {
            if (end >= t.length()) return;
            pos = end + Character.charCount(Character.codePointAt(t, end));
        } else {
            if (start <= 0) return;
            pos = start - Character.charCount(Character.codePointBefore(t, start));
        }
        pos += et.startOffset;
        ic.setSelection(pos, pos);
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
            if (!composing) compositionSeen = false;
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
            // The caret inside the reading (moved with the arrow keys).
            int back = st.preedit.length() - Math.max(0, Math.min(st.caret, st.preedit.length()));
            if (back > 0) {
                ExtractedText et = ic.getExtractedText(new ExtractedTextRequest(), 0);
                if (et != null && et.selectionEnd >= 0) {
                    int pos = et.startOffset + et.selectionEnd - back;
                    ic.setSelection(pos, pos);
                }
            }
        }
        composingLength = st.preedit.length();
        if (composing) {
            long state = ((long) composingLength << 32)
                    | Math.max(0, Math.min(st.caret, composingLength));
            shown.addLast(state);
            // setComposingText leaves the caret at the end first.
            shown.addLast(((long) composingLength << 32) | composingLength);
            while (shown.size() > 32) shown.removeFirst();
        }
        ic.endBatchEdit();
        showCandidates(st.candidates, st.selected, st.suggestions);
        updateKeys();
        if (composing) {
            requestLive();
        } else {
            lastLive = "";
        }
    }

    private View iconButton(java.util.function.IntSupplier icon, String label, Runnable action) {
        Icons icons = new Icons();
        android.graphics.RectF bounds = new android.graphics.RectF();
        View button = new View(this) {
            @Override protected void onDraw(android.graphics.Canvas canvas) {
                bounds.set(0, 0, getWidth(), getHeight());
                icons.draw(canvas, icon.getAsInt(), bounds, dp(22), theme.subText);
            }

            @Override public CharSequence getAccessibilityClassName() {
                return android.widget.Button.class.getName();
            }
        };
        button.setContentDescription(label);
        button.setTooltipText(label);
        button.setFocusable(true);
        button.setOnClickListener(v -> action.run());
        return button;
    }

    private void updatePanelVisibility() {
        if (keyboard == null) return;
        boolean panel = clipboardOpen || candidatesExpanded;
        keyboard.setVisibility(panel ? View.INVISIBLE : View.VISIBLE);
        candidateScroll.setVisibility(panel ? View.GONE : View.VISIBLE);
        expandedScroll.setVisibility(candidatesExpanded ? View.VISIBLE : View.GONE);
        if (!clipboardOpen && clipboardPanel.getVisibility() == View.VISIBLE) clipboardPanel.close();
        clipboardPanel.setVisibility(clipboardOpen ? View.VISIBLE : View.GONE);
        panelTitle.setVisibility(panel ? View.VISIBLE : View.GONE);
        panelTitle.setText(clipboardOpen ? "クリップボード" : "候補一覧");
        panelTitle.setTextColor(theme.text);
        expandButton.setVisibility(!clipboardOpen && candidateItems.length > 1 ? View.VISIBLE : View.GONE);
        String expandLabel = candidatesExpanded ? "候補を閉じる" : "候補を展開";
        expandButton.setContentDescription(expandLabel);
        expandButton.setTooltipText(expandLabel);
        expandButton.invalidate();
        String clipboardLabel = clipboardOpen ? "キーボードに戻る" : "クリップボード";
        clipboardButton.setContentDescription(clipboardLabel);
        clipboardButton.setTooltipText(clipboardLabel);
        clipboardButton.invalidate();
    }

    private void renderExpandedCandidates() {
        expandedCandidates.removeAllViews();
        for (int i = 0; i < candidateItems.length; i++) {
            expandedCandidates.addView(candidateView(i, true));
        }
    }

    private TextView candidateView(int index, boolean expanded) {
        TextView view = new TextView(this);
        view.setText(candidateItems[index]);
        view.setTextSize(TypedValue.COMPLEX_UNIT_SP, 18);
        view.setTextColor(theme.text);
        view.setGravity(Gravity.CENTER);
        view.setMinWidth(dp(44));
        view.setPadding(dp(12), 0, dp(12), 0);
        if (expanded) {
            view.setMinHeight(dp(44));
            view.setMaxLines(2);
            view.setEllipsize(android.text.TextUtils.TruncateAt.END);
            GradientDrawable background = new GradientDrawable();
            background.setCornerRadius(dp(8));
            background.setColor(index == selectedCandidate ? theme.focus : theme.key);
            view.setBackground(background);
        } else if (index == selectedCandidate) {
            view.setBackgroundColor(theme.focus);
        }
        if (index == selectedCandidate) view.setTypeface(Typeface.DEFAULT_BOLD);
        view.setOnClickListener(v -> {
            if (ctx != 0) apply(Native.select(ctx, index));
        });
        return view;
    }

    private void showCandidates(String[] items, int selected, boolean suggestions) {
        if (candidateRow == null) return;
        boolean newItems = !java.util.Arrays.equals(candidateItems, items);
        candidateItems = items;
        selectedCandidate = selected;
        if (items.length <= 1) candidatesExpanded = false;
        updatePanelVisibility();
        if (candidatesExpanded) {
            renderExpandedCandidates();
            if (newItems) expandedScroll.scrollTo(0, 0);
        }
        candidateRow.removeAllViews();
        if (items.length == 0) {
            View gear = iconButton(() -> Icons.SETTINGS, "設定", () -> {
                android.content.Intent i = new android.content.Intent(this, SettingsActivity.class);
                i.addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK);
                startActivity(i);
            });
            candidateRow.addView(gear, new LinearLayout.LayoutParams(
                    dp(52), ViewGroup.LayoutParams.MATCH_PARENT));
            candidateScroll.scrollTo(0, 0);
            return;
        }
        for (int i = 0; i < items.length; i++) {
            candidateRow.addView(candidateView(i, false), new LinearLayout.LayoutParams(
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
