package io.github.sweetime;

import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Paint;
import android.graphics.RectF;
import android.os.Handler;
import android.os.Looper;
import android.view.HapticFeedbackConstants;
import android.view.MotionEvent;
import android.view.View;

import java.util.ArrayList;
import java.util.List;

/** A QWERTY keyboard drawn on a canvas. */
final class KeyboardView extends View {
    interface Listener {
        /** A character or a Native.KEY_* code. */
        void onKey(int code);

        void onToggleMode();
    }

    static final int SHIFT = -100;
    static final int MODE = -101;

    private static final class KeyDef {
        final String label;
        final int code;
        final float width;
        final RectF rect = new RectF();

        KeyDef(String label, int code, float width) {
            this.label = label;
            this.code = code;
            this.width = width;
        }
    }

    private final List<List<KeyDef>> rows = new ArrayList<>();
    private final Paint keyPaint = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Paint textPaint = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Handler handler = new Handler(Looper.getMainLooper());
    private final Theme theme;
    private final float density;
    private Listener listener;
    private KeyDef pressed;
    private boolean shift;
    private boolean capsLock;
    private long lastShiftTap;
    private boolean english;

    private final Runnable repeat = new Runnable() {
        @Override
        public void run() {
            if (pressed != null && pressed.code == Native.KEY_BACKSPACE && listener != null) {
                listener.onKey(Native.KEY_BACKSPACE);
                handler.postDelayed(this, 50);
            }
        }
    };

    KeyboardView(Context context, Theme theme) {
        super(context);
        this.theme = theme;
        density = context.getResources().getDisplayMetrics().density;
        textPaint.setTextAlign(Paint.Align.CENTER);
        addRow("1234567890");
        addRow("qwertyuiop");
        List<KeyDef> r2 = addRow("asdfghjkl");
        r2.add(new KeyDef("ー", '-', 1));
        List<KeyDef> r3 = new ArrayList<>();
        r3.add(new KeyDef("⇧", SHIFT, 1.5f));
        for (char c : "zxcvbnm".toCharArray()) r3.add(new KeyDef(String.valueOf(c), c, 1));
        r3.add(new KeyDef("⌫", Native.KEY_BACKSPACE, 1.5f));
        rows.add(r3);
        List<KeyDef> r4 = new ArrayList<>();
        r4.add(new KeyDef("あ", MODE, 1.5f));
        r4.add(new KeyDef("◀", Native.KEY_LEFT, 1));
        r4.add(new KeyDef("、", ',', 1));
        r4.add(new KeyDef("", Native.KEY_SPACE, 3));
        r4.add(new KeyDef("。", '.', 1));
        r4.add(new KeyDef("▶", Native.KEY_RIGHT, 1));
        r4.add(new KeyDef("⏎", Native.KEY_ENTER, 1.5f));
        rows.add(r4);
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

    void setEnglish(boolean english) {
        this.english = english;
        invalidate();
    }

    @Override
    protected void onMeasure(int widthSpec, int heightSpec) {
        int w = MeasureSpec.getSize(widthSpec);
        int h = (int) (rows.size() * 54 * density + 8 * density);
        setMeasuredDimension(w, h);
    }

    @Override
    protected void onSizeChanged(int w, int h, int ow, int oh) {
        float pad = 3 * density;
        float rowH = (h - 8 * density) / rows.size();
        float top = 4 * density;
        for (List<KeyDef> row : rows) {
            float units = 0;
            for (KeyDef k : row) units += k.width;
            float unit = Math.min(w / 10f, w / units);
            float x = (w - unit * units) / 2;
            for (KeyDef k : row) {
                k.rect.set(x + pad, top + pad, x + k.width * unit - pad, top + rowH - pad);
                x += k.width * unit;
            }
            top += rowH;
        }
    }

    private boolean upper() {
        return shift || capsLock;
    }

    private String labelOf(KeyDef k) {
        switch (k.code) {
            case MODE:
                return english ? "A" : "あ";
            case Native.KEY_SPACE:
                return english ? "space" : "変換";
            case ',':
                return english ? "," : "、";
            case '.':
                return english ? "." : "。";
            case '-':
                return english ? "-" : "ー";
            case SHIFT:
                return capsLock ? "⇪" : "⇧";
            default:
                if (k.code >= 'a' && k.code <= 'z' && upper()) return k.label.toUpperCase();
                return k.label;
        }
    }

    @Override
    protected void onDraw(Canvas canvas) {
        canvas.drawColor(theme.background);
        float radius = 6 * density;
        for (List<KeyDef> row : rows) {
            for (KeyDef k : row) {
                boolean special = k.code < 0 && k.code != Native.KEY_SPACE;
                boolean on = k.code == SHIFT && upper();
                int color = k == pressed ? theme.pressed
                        : on ? theme.accent
                        : special ? theme.specialKey : theme.key;
                keyPaint.setColor(theme.shadow);
                canvas.drawRoundRect(k.rect.left, k.rect.top + density, k.rect.right,
                        k.rect.bottom + density, radius, radius, keyPaint);
                keyPaint.setColor(color);
                canvas.drawRoundRect(k.rect, radius, radius, keyPaint);
                String label = labelOf(k);
                boolean small = label.length() > 1;
                textPaint.setTextSize((small ? 14 : 20) * density);
                textPaint.setColor(on ? theme.onAccent : theme.text);
                float y = k.rect.centerY() - (textPaint.descent() + textPaint.ascent()) / 2;
                canvas.drawText(label, k.rect.centerX(), y, textPaint);
            }
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

    @Override
    public boolean onTouchEvent(MotionEvent e) {
        switch (e.getActionMasked()) {
            case MotionEvent.ACTION_DOWN:
                pressed = keyAt(e.getX(), e.getY());
                performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP);
                if (pressed != null && pressed.code == Native.KEY_BACKSPACE) {
                    fire(pressed);
                    handler.postDelayed(repeat, 400);
                }
                invalidate();
                return true;
            case MotionEvent.ACTION_MOVE:
                KeyDef k = keyAt(e.getX(), e.getY());
                if (k != pressed && (pressed == null || pressed.code != Native.KEY_BACKSPACE)) {
                    pressed = k;
                    invalidate();
                }
                return true;
            case MotionEvent.ACTION_UP:
                handler.removeCallbacks(repeat);
                if (pressed != null && pressed.code != Native.KEY_BACKSPACE) fire(pressed);
                pressed = null;
                invalidate();
                return true;
            case MotionEvent.ACTION_CANCEL:
                handler.removeCallbacks(repeat);
                pressed = null;
                invalidate();
                return true;
            default:
                return false;
        }
    }

    private void fire(KeyDef k) {
        if (listener == null) return;
        if (k.code == SHIFT) {
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
        if (k.code == MODE) {
            listener.onToggleMode();
            return;
        }
        if (k.code >= 'a' && k.code <= 'z' && upper()) {
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
