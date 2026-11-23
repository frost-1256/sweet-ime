package io.github.sweetime;

import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Paint;
import android.graphics.RectF;
import android.media.AudioManager;
import android.os.Handler;
import android.os.Looper;
import android.view.HapticFeedbackConstants;
import android.view.MotionEvent;
import android.view.View;
import android.view.inputmethod.EditorInfo;

import java.util.ArrayList;
import java.util.List;

/** A QWERTY keyboard with a symbols page, drawn on a canvas. */
final class KeyboardView extends View {
    interface Listener {
        /** A character or a Native.KEY_* code. */
        void onKey(int code);

        /** Text from the symbols page or a long-press shortcut. */
        void onSymbol(String text);

        void onToggleMode();

        /** Opens the emoji keyboard (a long press on the comma key). */
        void onEmoji();
    }

    static final int SHIFT = -100;
    static final int MODE = -101;
    static final int SYMBOLS = -102;
    static final int TEXT = -103;
    static final int MORE_SYMBOLS = -104;
    static final int LETTERS = -105;
    static final int NUMPAD = -106;
    /** A hint that opens the emoji keyboard instead of typing. */
    private static final String EMOJI_HINT = "<emoji>";

    private static final class KeyDef {
        final String label;
        final int code;
        final float width;
        final RectF rect = new RectF();
        /** Shown small in the corner and typed by a long press. */
        String hint;
        /** Drawn in the function key color. */
        boolean fn;

        KeyDef(String label, int code, float width) {
            this.label = label;
            this.code = code;
            this.width = width;
        }

        KeyDef hint(String h) {
            hint = h;
            return this;
        }

        /** Shift, Backspace, the page and Enter keys. */
        boolean isFunction() {
            return fn || code == SHIFT || code == Native.KEY_BACKSPACE || code == SYMBOLS
                    || code == MORE_SYMBOLS || code == LETTERS || code == Native.KEY_ENTER;
        }

        KeyDef fn() {
            fn = true;
            return this;
        }

        boolean isLetter() {
            return code >= 'a' && code <= 'z';
        }

        boolean previews() {
            return code >= 0 || code == TEXT;
        }
    }

    private final List<List<KeyDef>> rows = new ArrayList<>();
    private final Paint keyPaint = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Paint textPaint = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Handler handler = new Handler(Looper.getMainLooper());
    private final AudioManager audio;
    private final float density;
    private Theme theme;
    private Listener listener;
    private KeyDef pressed;
    private boolean shift;
    private boolean capsLock;
    private long lastShiftTap;
    private boolean english;
    private boolean urlField;
    /** 0 letters, 1 symbols, 2 more symbols. */
    private int page;
    private boolean composing;
    private boolean converting;
    /** The editor's Enter action (EditorInfo.IME_ACTION_*), shown as an icon. */
    private int enterAction = EditorInfo.IME_ACTION_NONE;
    private final Icons icons = new Icons();
    private final RectF hintRect = new RectF();

    // Settings.
    private boolean haptic = true;
    private boolean sound;
    private boolean numberRow = false;
    /** The long press fired; the release types nothing. */
    private boolean longPressed;
    private final Runnable longPress = new Runnable() {
        @Override
        public void run() {
            if (pressed != null && pressed.hint != null && listener != null) {
                longPressed = true;
                if (haptic) performHapticFeedback(HapticFeedbackConstants.LONG_PRESS);
                String h = hintOf(pressed);
                if (h.equals(EMOJI_HINT)) {
                    used = true;
                    pressed = null;
                    invalidate();
                    listener.onEmoji();
                    return;
                }
                // Digits join the reading (1じ converts to 1時); symbols
                // are typed as they are.
                if (h.length() == 1 && h.charAt(0) >= '0' && h.charAt(0) <= '9') {
                    listener.onKey(h.charAt(0));
                } else {
                    listener.onSymbol(h);
                }
                invalidate();
            }
        }
    };
    private boolean keyPreview = true;
    private float rowHeightDp = 58;
    private float punctuationWidth = 1;

    // Swiping on the space bar moves the caret.
    private float swipeX;
    private boolean swiped;

    /** Backspace and the arrows act on press and repeat while held. */
    private static boolean repeats(KeyDef k) {
        return k.code == Native.KEY_BACKSPACE || k.code == Native.KEY_LEFT
                || k.code == Native.KEY_RIGHT;
    }

    /** The pressed key already acted (repeat, long press, swipe): releasing types nothing. */
    private boolean used;

    private final Runnable repeat = new Runnable() {
        @Override
        public void run() {
            if (pressed != null && repeats(pressed) && listener != null) {
                listener.onKey(pressed.code);
                handler.postDelayed(this, pressed.code == Native.KEY_BACKSPACE ? 50 : 70);
            }
        }
    };

    KeyboardView(Context context, Theme theme) {
        super(context);
        this.theme = theme;
        density = context.getResources().getDisplayMetrics().density;
        audio = (AudioManager) context.getSystemService(Context.AUDIO_SERVICE);
        textPaint.setTextAlign(Paint.Align.CENTER);
        build();
    }

    /** Applies the keyboard settings. */
    void configure(Theme theme, boolean haptic, boolean sound, boolean numberRow,
                   boolean keyPreview, int keyHeight, int symbolWidth) {
        this.theme = theme;
        this.haptic = haptic;
        this.sound = sound;
        this.keyPreview = keyPreview;
        // Gboard-sized by default; low and high step about 8% each way.
        rowHeightDp = keyHeight == 0 ? 53 : keyHeight == 2 ? 64 : 58;
        float width = symbolWidth == 0 ? 0.85f : symbolWidth == 2 ? 1.35f : 1f;
        if (numberRow != this.numberRow || punctuationWidth != width) {
            this.numberRow = numberRow;
            punctuationWidth = width;
            build();
        }
        requestLayout();
        invalidate();
    }

    /**
     * The pages: 0 letters, 1 symbols, 2 more symbols, 3 number pad. The
     * layout follows common Android keyboards.
     */
    private void build() {
        rows.clear();
        List<KeyDef> bottom = new ArrayList<>();
        if (page == 0) {
            if (numberRow) addRow("1234567890");
            List<KeyDef> top = addRow("qwertyuiop");
            if (!numberRow) {
                for (int i = 0; i < top.size(); i++) top.get(i).hint(String.valueOf((i + 1) % 10));
            }
            List<KeyDef> r2 = addRow("asdfghjkl");
            if (!english) r2.add(new KeyDef("ー", '-', 1));
            List<KeyDef> r3 = new ArrayList<>();
            r3.add(new KeyDef("⇧", SHIFT, 1.5f));
            for (char c : "zxcvbnm".toCharArray()) r3.add(new KeyDef(String.valueOf(c), c, 1));
            r3.add(new KeyDef("⌫", Native.KEY_BACKSPACE, 1.5f));
            rows.add(r3);
            bottom.add(new KeyDef("?123", SYMBOLS, 1.5f));
            // As on Gboard: the comma key carries the emoji key and its
            // color, and the row adds up to ten keys like the ones above.
            if (english && urlField) {
                bottom.add(new KeyDef("/", '/', punctuationWidth).hint(":").fn());
            } else {
                bottom.add(new KeyDef("、", ',', punctuationWidth).hint(EMOJI_HINT).fn());
            }
            bottom.add(new KeyDef("", MODE, 1));
            if (english) {
                bottom.add(new KeyDef("", Native.KEY_SPACE, 6 - 2 * punctuationWidth));
                KeyDef period = new KeyDef("。", '.', punctuationWidth).fn();
                bottom.add(urlField ? period.hint(".com") : period);
            } else {
                bottom.add(new KeyDef("", Native.KEY_SPACE, 4 - 2 * punctuationWidth));
                bottom.add(new KeyDef("。", '.', punctuationWidth));
                bottom.add(new KeyDef("◀", Native.KEY_LEFT, 1).fn());
                bottom.add(new KeyDef("▶", Native.KEY_RIGHT, 1).fn());
            }
            bottom.add(new KeyDef("⏎", Native.KEY_ENTER, 1.5f));
        } else if (page == 3) {
            String[][] pad = {{"+", "1", "2", "3", "%"}, {"-", "4", "5", "6", ","}, {"*", "7", "8", "9", ""}};
            for (int r = 0; r < pad.length; r++) {
                List<KeyDef> row = new ArrayList<>();
                for (int c = 0; c < pad[r].length; c++) {
                    if (r == 2 && c == 4) {
                        row.add(new KeyDef("⌫", Native.KEY_BACKSPACE, 2));
                        continue;
                    }
                    KeyDef k = new KeyDef(pad[r][c], TEXT, 2);
                    if (c == 0 || c == 4) k.fn();
                    row.add(k);
                }
                rows.add(row);
            }
            bottom.add(new KeyDef(english ? "ABC" : "あいう", LETTERS, 2));
            bottom.add(new KeyDef("?123", SYMBOLS, 1.5f));
            bottom.add(new KeyDef("/", TEXT, 1.5f).fn());
            bottom.add(new KeyDef("0", TEXT, 2));
            bottom.add(new KeyDef(".", TEXT, 1).fn());
            bottom.add(new KeyDef("⏎", Native.KEY_ENTER, 2));
        } else {
            boolean more = page == 2;
            rows.add(textRow(more
                    ? new String[] {"~", "`", "|", "•", "√", "π", "÷", "×", "§", "∆"}
                    : new String[] {"1", "2", "3", "4", "5", "6", "7", "8", "9", "0"}));
            rows.add(textRow(more
                    ? new String[] {"£", "€", "$", "¢", "^", "°", "=", "{", "}", "\\"}
                    : new String[] {"@", "#", english ? "$" : "¥", "%", "&", "-", "+", "(", ")", "/"}));
            List<KeyDef> r3 = new ArrayList<>();
            r3.add(new KeyDef(more ? "?123" : "=\\<", MORE_SYMBOLS, 1.5f));
            r3.addAll(textRow(more
                    ? new String[] {"_", "©", "®", "™", "✓", "[", "]"}
                    : new String[] {"*", "\"", "'", ":", ";", "!", "?"}));
            r3.add(new KeyDef("⌫", Native.KEY_BACKSPACE, 1.5f));
            rows.add(r3);
            bottom.add(new KeyDef(english ? "ABC" : "あいう", LETTERS, 1.5f));
            bottom.add(new KeyDef(more ? "<" : ",", TEXT, 1).fn());
            bottom.add(new KeyDef("1234", NUMPAD, 1));
            bottom.add(new KeyDef("", Native.KEY_SPACE, 4));
            bottom.add(new KeyDef(more ? ">" : ".", TEXT, 1).fn());
            bottom.add(new KeyDef("⏎", Native.KEY_ENTER, 1.5f));
        }
        rows.add(bottom);
        if (getWidth() > 0) layoutKeys(getWidth(), getHeight());
        // The pages can have a different number of rows.
        requestLayout();
        invalidate();
    }

    private List<KeyDef> textRow(String[] keys) {
        List<KeyDef> row = new ArrayList<>();
        for (String k : keys) row.add(new KeyDef(k, TEXT, 1));
        return row;
    }

    private List<KeyDef> addRow(String keys) {
        List<KeyDef> row = new ArrayList<>();
        for (char c : keys.toCharArray()) row.add(new KeyDef(String.valueOf(c), c, 1));
        rows.add(row);
        return row;
    }

    void setListener(Listener l) {
        listener = l;
    }

    void setUrlField(boolean url) {
        if (urlField == url) return;
        urlField = url;
        build();
    }

    void setEnglish(boolean english) {
        if (this.english != english) {
            this.english = english;
            capsLock = false;
            shift = false;
            build();
        }
        invalidate();
    }

    /** Shift for the next letter (English at the start of a sentence). */
    void setAutoShift(boolean on) {
        if (capsLock) return;
        if (shift != on) {
            shift = on;
            invalidate();
        }
    }

    /** What Space and Enter do now, for their labels. */
    void setState(boolean composing, boolean converting, int enterAction) {
        this.composing = composing;
        this.converting = converting;
        this.enterAction = enterAction;
        invalidate();
    }

    /** Back to the letters (after typing a symbol). */
    void showLetters() {
        if (page != 0) {
            page = 0;
            build();
            invalidate();
        }
    }

    @Override
    protected void onMeasure(int widthSpec, int heightSpec) {
        int w = MeasureSpec.getSize(widthSpec);
        int h = (int) (rows.size() * rowHeightDp * density + 8 * density);
        setMeasuredDimension(w, h);
    }

    @Override
    protected void onSizeChanged(int w, int h, int ow, int oh) {
        layoutKeys(w, h);
    }

    private void layoutKeys(int w, int h) {
        // Narrower gaps between keys than between rows, so keys look wider.
        float padX = 2.5f * density;
        float padY = 3 * density;
        float rowH = (h - 8 * density) / rows.size();
        float top = 4 * density;
        for (List<KeyDef> row : rows) {
            float units = 0;
            for (KeyDef k : row) units += k.width;
            float unit = Math.min(w / 10f, w / units);
            float x = (w - unit * units) / 2;
            for (KeyDef k : row) {
                k.rect.set(x + padX, top + padY, x + k.width * unit - padX, top + rowH - padY);
                x += k.width * unit;
            }
            top += rowH;
        }
    }

    private boolean upper() {
        return shift || capsLock;
    }

    /**
     * While a Japanese reading is being typed, the language key types ？
     * into it instead (as on Gboard); switching languages mid-word is rare.
     */
    private boolean questionKey() {
        return composing && !english && page == 0;
    }

    private String hintOf(KeyDef key) {
        return key.hint;
    }

    private String labelOf(KeyDef k) {
        switch (k.code) {
            case MODE:
                return questionKey() ? "？" : "";
            case Native.KEY_SPACE:
                if (english || (page != 0 && !composing)) return english ? "English" : "日本語";
                return converting ? "次候補" : composing ? "変換" : "日本語";
            case Native.KEY_ENTER:
                return composing ? "確定" : "";
            case ',':
                return english ? "," : "、";
            case '.':
                return english ? "." : "。";
            case '-':
                return english ? "-" : "ー";
            case SHIFT:
                return capsLock ? "⇪" : "⇧";
            default:
                if (k.isLetter() && upper()) return k.label.toUpperCase();
                return k.label;
        }
    }

    @Override
    protected void onDraw(Canvas canvas) {
        // Only our own area: the key preview draws outside it, unclipped.
        keyPaint.setColor(theme.background);
        canvas.drawRect(0, 0, getWidth(), getHeight(), keyPaint);
        float radius = theme.radius * density;
        for (List<KeyDef> row : rows) {
            for (KeyDef k : row) {
                boolean on = (k.code == SHIFT && upper()) || k.code == Native.KEY_ENTER;
                int color = k == pressed ? theme.pressed
                        : on ? theme.accent
                        : k.isFunction() ? theme.specialKey : theme.key;
                // Enter and the page key are pills, as on common keyboards.
                boolean pill = k.code == Native.KEY_ENTER || k.code == LETTERS
                        || (k.code == SYMBOLS && page == 0);
                float r = k == pressed ? theme.pressedRadius * density
                        : pill ? k.rect.height() / 2 : radius;
                r = Math.min(r, Math.min(k.rect.width(), k.rect.height()) / 2);
                if (theme.shadow != 0) {
                    keyPaint.setColor(theme.shadow);
                    canvas.drawRoundRect(k.rect.left, k.rect.top + density, k.rect.right,
                            k.rect.bottom + density, r, r, keyPaint);
                }
                keyPaint.setColor(color);
                canvas.drawRoundRect(k.rect, r, r, keyPaint);
                if (k.hint != null) {
                    textPaint.setTextSize(10 * density);
                    textPaint.setColor(theme.subText);
                    // Further in on rounder keys (Expressive), clear of the
                    // corner; it stays put while the pressed key changes shape.
                    float corner = Math.min(radius, Math.min(k.rect.width(), k.rect.height()) / 2);
                    float in = Math.max(0, 0.6f * (corner - 8 * density));
                    float hx = k.rect.right - 7 * density - in;
                    float hy = k.rect.top + 12 * density + in;
                    if (k.hint.equals(EMOJI_HINT)) {
                        // Above the label, as on Gboard.
                        float ex = k.rect.centerX();
                        float ey = k.rect.top + 14 * density;
                        hintRect.set(ex - 7 * density, ey - 7 * density, ex + 7 * density, ey + 7 * density);
                        icons.draw(canvas, Icons.EMOJI, hintRect, 12 * density, theme.subText);
                    } else {
                        canvas.drawText(hintOf(k), hx, hy, textPaint);
                    }
                }
                int icon = iconOf(k);
                if (icon != 0) {
                    icons.draw(canvas, icon, k.rect, 22 * density, on ? theme.onAccent : theme.text);
                    continue;
                }
                if (k.code == NUMPAD) {
                    textPaint.setTextSize(13 * density);
                    textPaint.setColor(theme.text);
                    canvas.drawText("12", k.rect.centerX(), k.rect.centerY() - 2 * density, textPaint);
                    canvas.drawText("34", k.rect.centerX(), k.rect.centerY() + 12 * density, textPaint);
                    continue;
                }
                String label = labelOf(k);
                float size = label.length() > 2 ? 14 : label.length() == 2 && !label.equals("1/2")
                        && !label.equals("2/2") ? 15 : 22;
                if (label.length() > 1 && k.code == TEXT) size = 18;
                textPaint.setTextSize(size * density);
                textPaint.setColor(on ? theme.onAccent : theme.text);
                // Punctuation stays regular: bold 、 and 。 read as blots.
                textPaint.setFakeBoldText(theme.boldFunctions && k.isFunction() && k.code != ',' && k.code != '.');
                float dy = EMOJI_HINT.equals(k.hint) ? 8 * density : 0;
                drawCentered(canvas, label, k.rect.centerX(), k.rect.centerY() + dy);
                textPaint.setFakeBoldText(false);
            }
        }
        // The pressed key, enlarged above it.
        if (keyPreview && pressed != null && pressed.previews()) {
            RectF r = pressed.rect;
            float w = r.width() * 1.3f;
            float h = r.height() * 1.15f;
            float cx = Math.max(w / 2, Math.min(getWidth() - w / 2, r.centerX()));
            RectF p = new RectF(cx - w / 2, r.top - h - 4 * density, cx + w / 2, r.top - 4 * density);
            if (theme.shadow != 0) {
                keyPaint.setColor(theme.shadow);
                canvas.drawRoundRect(p.left, p.top + 2 * density, p.right, p.bottom + 2 * density,
                        radius * 1.5f, radius * 1.5f, keyPaint);
            }
            keyPaint.setColor(theme.preview);
            canvas.drawRoundRect(p, radius * 1.5f, radius * 1.5f, keyPaint);
            textPaint.setTextSize(28 * density);
            textPaint.setColor(theme.text);
            drawCentered(canvas, longPressed && pressed.hint != null && !pressed.hint.equals(EMOJI_HINT)
                    ? hintOf(pressed) : labelOf(pressed), p.centerX(), p.centerY());
        }
    }

    private final android.graphics.Rect bounds = new android.graphics.Rect();

    /**
     * Draws a label so that it looks centered at (cx, cy): Latin letters and
     * digits by their x-height or cap height (descenders hang below, as on
     * other keyboards), anything else by its own ink, so the Japanese comma
     * and period, drawn low and left in their em box, and symbols such as
     * '*' or ':' sit in the middle of the key. Quotes keep their raised
     * position so they don't read as commas.
     */
    private void drawCentered(Canvas canvas, String label, float cx, float cy) {
        if (label.isEmpty()) return;
        boolean ascii = true;
        for (int i = 0; i < label.length(); i++) {
            if (label.charAt(i) > 0x7e) ascii = false;
        }
        char c = label.charAt(0);
        boolean ink = !ascii
                || label.length() == 1 && !Character.isLetterOrDigit(c) && "'\"`".indexOf(c) < 0;
        float x = cx;
        if (ink) {
            textPaint.getTextBounds(label, 0, label.length(), bounds);
            // Text is drawn centered on x: move the ink's center there.
            x = cx - bounds.exactCenterX() + textPaint.measureText(label) / 2;
        } else {
            boolean lower = label.length() == 1 && Character.isLowerCase(c);
            textPaint.getTextBounds(lower ? "x" : "H", 0, 1, bounds);
        }
        canvas.drawText(label, x, cy - bounds.exactCenterY(), textPaint);
    }

    /** The icon drawn instead of a label, or 0. */
    private int iconOf(KeyDef k) {
        switch (k.code) {
            case MODE:
                return questionKey() ? 0 : Icons.GLOBE;
            case SHIFT:
                return capsLock ? Icons.CAPS_LOCK : shift ? Icons.SHIFT_ON : Icons.SHIFT;
            case Native.KEY_BACKSPACE:
                return Icons.BACKSPACE;
            case Native.KEY_LEFT:
                return Icons.LEFT;
            case Native.KEY_RIGHT:
                return Icons.RIGHT;
            case Native.KEY_ENTER:
                if (composing) return 0;
                switch (enterAction) {
                    case EditorInfo.IME_ACTION_SEARCH:
                        return Icons.SEARCH;
                    case EditorInfo.IME_ACTION_SEND:
                        return Icons.SEND;
                    case EditorInfo.IME_ACTION_GO:
                    case EditorInfo.IME_ACTION_NEXT:
                        return Icons.FORWARD;
                    case EditorInfo.IME_ACTION_PREVIOUS:
                        return Icons.BACK;
                    case EditorInfo.IME_ACTION_DONE:
                        return Icons.DONE;
                    default:
                        return Icons.RETURN;
                }
            default:
                return 0;
        }
    }

    private KeyDef keyAt(float x, float y) {
        KeyDef best = null;
        float bestDist = Float.MAX_VALUE;
        for (List<KeyDef> row : rows) {
            for (KeyDef k : row) {
                float dx = Math.max(0, Math.max(k.rect.left - x, x - k.rect.right));
                float dy = Math.max(0, Math.max(k.rect.top - y, y - k.rect.bottom));
                float d = dx * dx + dy * dy;
                if (d < bestDist) {
                    bestDist = d;
                    best = k;
                }
            }
        }
        return best;
    }

    private void feedback(KeyDef k) {
        if (haptic) performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP);
        if (sound && audio != null) {
            int fx = k.code == Native.KEY_BACKSPACE ? AudioManager.FX_KEYPRESS_DELETE
                    : k.code == Native.KEY_SPACE ? AudioManager.FX_KEYPRESS_SPACEBAR
                    : k.code == Native.KEY_ENTER ? AudioManager.FX_KEYPRESS_RETURN
                    : AudioManager.FX_KEYPRESS_STANDARD;
            audio.playSoundEffect(fx, -1);
        }
    }

    /** The pointer holding `pressed`; other fingers start new presses. */
    private int pointerId = -1;

    /** Starts pressing the key under pointer `index`. */
    private void press(MotionEvent e, int index) {
        pointerId = e.getPointerId(index);
        pressed = keyAt(e.getX(index), e.getY(index));
        if (pressed == null) return;
        feedback(pressed);
        swipeX = e.getX(index);
        swiped = false;
        longPressed = false;
        used = false;
        if (pressed.hint != null) handler.postDelayed(longPress, 350);
        if (repeats(pressed)) {
            fire(pressed);
            used = true;
            handler.postDelayed(repeat, 400);
        }
    }

    /** Ends the current press, typing its key unless it was used already. */
    private void release() {
        handler.removeCallbacks(repeat);
        handler.removeCallbacks(longPress);
        KeyDef k = pressed;
        pressed = null;
        pointerId = -1;
        if (k != null && !used && !swiped && !longPressed) fire(k);
    }

    @Override
    public boolean onTouchEvent(MotionEvent e) {
        switch (e.getActionMasked()) {
            case MotionEvent.ACTION_DOWN:
                press(e, 0);
                invalidate();
                return true;
            case MotionEvent.ACTION_POINTER_DOWN:
                // Another finger while a key is held: type the held key now
                // and follow the new one (rollover, as on common keyboards).
                release();
                press(e, e.getActionIndex());
                invalidate();
                return true;
            case MotionEvent.ACTION_MOVE: {
                int index = e.findPointerIndex(pointerId);
                if (index < 0 || pressed == null) return true;
                float x = e.getX(index);
                if (pressed.code == Native.KEY_SPACE) {
                    float step = 14 * density;
                    while (Math.abs(x - swipeX) >= step) {
                        boolean right = x > swipeX;
                        swipeX += right ? step : -step;
                        swiped = true;
                        if (listener != null) listener.onKey(right ? Native.KEY_RIGHT : Native.KEY_LEFT);
                    }
                    return true;
                }
                // A finger sliding off a key types the key it ends on;
                // keys that already acted stay pressed.
                KeyDef k = keyAt(x, e.getY(index));
                if (k != pressed && k != null && !longPressed && !used) {
                    handler.removeCallbacks(longPress);
                    pressed = k;
                    if (k.code == Native.KEY_SPACE) swipeX = x;
                    invalidate();
                }
                return true;
            }
            case MotionEvent.ACTION_POINTER_UP:
                // Only the finger holding the key types it; an earlier
                // finger lifting later does nothing.
                if (e.getPointerId(e.getActionIndex()) == pointerId) {
                    release();
                    invalidate();
                }
                return true;
            case MotionEvent.ACTION_UP:
                if (e.getPointerId(e.getActionIndex()) == pointerId) release();
                pressed = null;
                pointerId = -1;
                invalidate();
                return true;
            case MotionEvent.ACTION_CANCEL:
                handler.removeCallbacks(repeat);
                handler.removeCallbacks(longPress);
                pressed = null;
                pointerId = -1;
                invalidate();
                return true;
            default:
                return false;
        }
    }

    private void fire(KeyDef k) {
        if (listener == null) return;
        switch (k.code) {
            case SHIFT: {
                long now = System.currentTimeMillis();
                if (capsLock) {
                    capsLock = false;
                    shift = false;
                } else if (shift && now - lastShiftTap < 350) {
                    capsLock = true;
                    shift = false;
                } else {
                    shift = !shift;
                }
                lastShiftTap = now;
                invalidate();
                return;
            }
            case MODE:
                if (questionKey()) {
                    listener.onKey('?');
                    return;
                }
                page = 0;
                build();
                listener.onToggleMode();
                return;
            case SYMBOLS:
                page = 1;
                build();
                return;
            case MORE_SYMBOLS:
                page = page == 1 ? 2 : 1;
                build();
                return;
            case LETTERS:
                page = 0;
                build();
                return;
            case NUMPAD:
                page = 3;
                build();
                return;
            case TEXT:
                // Digits join the reading, as from the letters page.
                if (k.label.length() == 1 && k.label.charAt(0) >= '0' && k.label.charAt(0) <= '9') {
                    listener.onKey(k.label.charAt(0));
                } else {
                    listener.onSymbol(k.label);
                }
                return;
            default:
                break;
        }
        if (k.isLetter() && upper()) {
            int upperCode = Character.toUpperCase(k.code);
            // Japanese: Shift starts an English word (the engine's Shift mode).
            listener.onKey(english ? upperCode : (upperCode | Native.SHIFTED));
            if (!capsLock) shift = false;
            invalidate();
            return;
        }
        listener.onKey(k.code);
    }
}
