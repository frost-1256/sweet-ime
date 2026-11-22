package io.github.sweetime;

import android.content.Context;
import android.content.SharedPreferences;
import android.graphics.Canvas;
import android.graphics.Paint;
import android.graphics.RectF;
import android.os.Handler;
import android.os.Looper;
import android.view.HapticFeedbackConstants;
import android.view.MotionEvent;
import android.view.View;
import android.widget.LinearLayout;
import android.widget.ScrollView;

import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStreamReader;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;

/**
 * The emoji keyboard: category tabs, a scrolling grid of every emoji the
 * system font can draw (from Unicode's emoji list in res/raw/emoji.txt),
 * recently used ones first, and keys back to the letters, space and delete.
 */
final class EmojiPanel extends LinearLayout {
    interface Listener {
        void onEmoji(String emoji);

        void onSpace();

        void onBackspace();

        void onClose();
    }

    private static final String RECENT = "emoji_recent";
    private static final int MAX_RECENT = 32;

    /** A titled group of emoji. */
    private static final class Section {
        final String title;
        final List<String> items;
        /** Top of the section's title in the grid, in pixels. */
        float top;

        Section(String title, List<String> items) {
            this.title = title;
            this.items = items;
        }
    }

    private static List<Section> all;

    private final Listener listener;
    private final SharedPreferences prefs;
    private final List<String> recent = new ArrayList<>();
    private final List<Section> sections = new ArrayList<>();
    private final Icons icons = new Icons();
    private final Handler handler = new Handler(Looper.getMainLooper());
    private final ScrollView scroll;
    private final Grid grid;
    private final Tabs tabs;
    private final Bar bar;
    private Theme theme;
    private int current;
    private String lettersLabel = "あいう";

    EmojiPanel(Context context, Listener listener) {
        super(context);
        this.listener = listener;
        prefs = Prefs.keyboard(context);
        setOrientation(VERTICAL);
        tabs = new Tabs(context);
        addView(tabs, new LayoutParams(LayoutParams.MATCH_PARENT, dp(40)));
        scroll = new ScrollView(context);
        scroll.setVerticalScrollBarEnabled(false);
        grid = new Grid(context);
        scroll.addView(grid);
        scroll.setOnScrollChangeListener((v, x, y, ox, oy) -> follow(y));
        addView(scroll, new LayoutParams(LayoutParams.MATCH_PARENT, 0, 1));
        bar = new Bar(context);
        addView(bar, new LayoutParams(LayoutParams.MATCH_PARENT, dp(48)));
    }

    private int dp(float v) {
        return Math.round(v * getResources().getDisplayMetrics().density);
    }

    void open(Theme theme, boolean english) {
        this.theme = theme;
        lettersLabel = english ? "ABC" : "あいう";
        setBackgroundColor(theme.background);
        load();
        current = 0;
        scroll.scrollTo(0, 0);
        grid.requestLayout();
        grid.invalidate();
        tabs.invalidate();
        bar.invalidate();
    }

    void close() {
        handler.removeCallbacksAndMessages(null);
    }

    /** Reads the emoji list once, keeping what the font can draw. */
    private void load() {
        if (all == null) {
            all = new ArrayList<>();
            Paint probe = new Paint();
            try (BufferedReader r = new BufferedReader(new InputStreamReader(
                    getResources().openRawResource(R.raw.emoji), StandardCharsets.UTF_8))) {
                String line;
                while ((line = r.readLine()) != null) {
                    int tab = line.indexOf('\t');
                    if (tab < 0) continue;
                    List<String> items = new ArrayList<>();
                    for (String e : line.substring(tab + 1).split(" ")) {
                        if (!e.isEmpty() && probe.hasGlyph(e)) items.add(e);
                    }
                    all.add(new Section(line.substring(0, tab), items));
                }
            } catch (IOException ignored) {
                // No emoji list: only the recent ones.
            }
        }
        recent.clear();
        for (String e : prefs.getString(RECENT, "").split("\n")) {
            if (!e.isEmpty()) recent.add(e);
        }
        sections.clear();
        sections.add(new Section("最近使った絵文字", recent));
        sections.addAll(all);
    }

    private void use(String emoji) {
        listener.onEmoji(emoji);
        recent.remove(emoji);
        recent.add(0, emoji);
        while (recent.size() > MAX_RECENT) recent.remove(recent.size() - 1);
        prefs.edit().putString(RECENT, String.join("\n", recent)).apply();
        // The recent row changes the next time the panel opens, so what
        // is under the finger does not move now.
    }

    /** Highlights the tab of the section at scroll position `y`. */
    private void follow(int y) {
        int at = 0;
        for (int i = 0; i < sections.size(); i++) {
            if (sections.get(i).top <= y + dp(8)) at = i;
        }
        if (at != current) {
            current = at;
            tabs.invalidate();
        }
    }

    /** The emoji shown on a section's tab. */
    private String tabEmoji(int i) {
        String[] fixed = {"🕘", "😀", "👋", "🐶", "🍙", "🚃", "⚽", "💡", "❤️", "🏁"};
        return i < fixed.length ? fixed[i] : sections.get(i).items.isEmpty() ? "" : sections.get(i).items.get(0);
    }

    /** One tab per section; a tap scrolls to it. */
    private final class Tabs extends View {
        private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG);

        Tabs(Context c) {
            super(c);
            paint.setTextAlign(Paint.Align.CENTER);
        }

        @Override
        protected void onDraw(Canvas c) {
            if (theme == null || sections.isEmpty()) return;
            float w = getWidth() / (float) sections.size();
            for (int i = 0; i < sections.size(); i++) {
                float cx = w * i + w / 2;
                paint.setAlpha(255);
                if (i == current) {
                    paint.setColor(theme.specialKey);
                    c.drawRoundRect(cx - dp(16), dp(4), cx + dp(16), getHeight() - dp(4), dp(10), dp(10), paint);
                }
                paint.setColor(theme.text);
                paint.setTextSize(dp(18));
                paint.setAlpha(i == current ? 255 : 150);
                c.drawText(tabEmoji(i), cx, getHeight() / 2f + dp(6.5f), paint);
            }
        }

        @Override
        public boolean onTouchEvent(MotionEvent e) {
            if (e.getAction() == MotionEvent.ACTION_UP && !sections.isEmpty()) {
                int i = (int) (e.getX() / (getWidth() / (float) sections.size()));
                i = Math.max(0, Math.min(sections.size() - 1, i));
                current = i;
                scroll.scrollTo(0, (int) sections.get(i).top);
                invalidate();
                performClick();
            }
            return true;
        }

        @Override
        public boolean performClick() {
            return super.performClick();
        }
    }

    /** Every section's title and emoji, drawn in one view. */
    private final class Grid extends View {
        private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG);
        private final RectF r = new RectF();
        private int cols = 8;
        private float cell;
        private float titleH;
        private float downX;
        private float downY;
        private int pressed = -1;

        Grid(Context c) {
            super(c);
        }

        @Override
        protected void onMeasure(int ws, int hs) {
            int w = MeasureSpec.getSize(ws);
            cols = Math.max(6, (int) (w / (float) dp(46)));
            cell = w / (float) cols;
            titleH = dp(30);
            float y = 0;
            for (Section s : sections) {
                s.top = y;
                y += titleH;
                int n = Math.max(1, (s.items.size() + cols - 1) / cols);
                y += n * cell;
            }
            setMeasuredDimension(w, (int) y + dp(8));
        }

        @Override
        protected void onDraw(Canvas c) {
            if (theme == null) return;
            paint.setTextAlign(Paint.Align.LEFT);
            for (int si = 0; si < sections.size(); si++) {
                Section s = sections.get(si);
                paint.setColor(theme.subText);
                paint.setTextSize(dp(13));
                c.drawText(s.title, dp(12), s.top + titleH - dp(9), paint);
                if (s.items.isEmpty()) {
                    paint.setTextSize(dp(13));
                    c.drawText("使った絵文字がここに出ます", dp(12), s.top + titleH + cell / 2 + dp(4), paint);
                    continue;
                }
                paint.setTextAlign(Paint.Align.CENTER);
                paint.setTextSize(cell * 0.56f);
                paint.setColor(theme.text);
                for (int i = 0; i < s.items.size(); i++) {
                    float x = (i % cols) * cell;
                    float y = s.top + titleH + (i / cols) * cell;
                    if (pressed == index(si, i)) {
                        paint.setColor(theme.pressed);
                        r.set(x + dp(2), y + dp(2), x + cell - dp(2), y + cell - dp(2));
                        c.drawRoundRect(r, dp(10), dp(10), paint);
                        paint.setColor(theme.text);
                    }
                    c.drawText(s.items.get(i), x + cell / 2, y + cell / 2 + cell * 0.2f, paint);
                }
                paint.setTextAlign(Paint.Align.LEFT);
            }
        }

        /** A flat index over all sections' emoji. */
        private int index(int section, int item) {
            return section * 10000 + item;
        }

        /** The emoji at (x, y), as {section, item}, or null. */
        private int[] at(float x, float y) {
            for (int si = 0; si < sections.size(); si++) {
                Section s = sections.get(si);
                float top = s.top + titleH;
                int rows = (s.items.size() + cols - 1) / cols;
                if (y < top || y >= top + rows * cell) continue;
                int i = (int) ((y - top) / cell) * cols + (int) (x / cell);
                return i < s.items.size() ? new int[] {si, i} : null;
            }
            return null;
        }

        @Override
        public boolean onTouchEvent(MotionEvent e) {
            switch (e.getActionMasked()) {
                case MotionEvent.ACTION_DOWN: {
                    downX = e.getX();
                    downY = e.getY();
                    int[] hit = at(downX, downY);
                    pressed = hit == null ? -1 : index(hit[0], hit[1]);
                    invalidate();
                    return true;
                }
                case MotionEvent.ACTION_MOVE:
                    if (Math.abs(e.getY() - downY) > dp(8) || Math.abs(e.getX() - downX) > dp(8)) {
                        pressed = -1;
                        invalidate();
                    }
                    return true;
                case MotionEvent.ACTION_UP: {
                    int[] hit = at(e.getX(), e.getY());
                    if (pressed >= 0 && hit != null && index(hit[0], hit[1]) == pressed) {
                        performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP);
                        use(sections.get(hit[0]).items.get(hit[1]));
                        performClick();
                    }
                    pressed = -1;
                    invalidate();
                    return true;
                }
                case MotionEvent.ACTION_CANCEL:
                    pressed = -1;
                    invalidate();
                    return true;
                default:
                    return true;
            }
        }

        @Override
        public boolean performClick() {
            return super.performClick();
        }
    }

    /** Back to the letters, space, and delete (repeating while held). */
    private final class Bar extends View {
        private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG);
        private final RectF r = new RectF();
        private int pressedKey = -1;
        private final Runnable repeat = new Runnable() {
            @Override
            public void run() {
                listener.onBackspace();
                handler.postDelayed(this, 50);
            }
        };

        Bar(Context c) {
            super(c);
            paint.setTextAlign(Paint.Align.CENTER);
        }

        /** Key `i` (0 letters, 1 space, 2 delete) into `r`. */
        private void keyRect(int i) {
            float w = getWidth();
            float pad = dp(3);
            float side = w * 0.18f;
            float left = i == 0 ? 0 : i == 1 ? side : w - side;
            float right = i == 0 ? side : i == 1 ? w - side : w;
            r.set(left + pad + (i == 0 ? dp(3) : 0), pad, right - pad - (i == 2 ? dp(3) : 0), getHeight() - pad);
        }

        @Override
        protected void onDraw(Canvas c) {
            if (theme == null) return;
            for (int i = 0; i < 3; i++) {
                keyRect(i);
                paint.setColor(i == pressedKey ? theme.pressed : i == 1 ? theme.key : theme.specialKey);
                float rad = Math.min(theme.radius * getResources().getDisplayMetrics().density, r.height() / 2);
                c.drawRoundRect(r, rad, rad, paint);
            }
            keyRect(0);
            paint.setColor(theme.text);
            paint.setTextSize(dp(15));
            c.drawText(lettersLabel, r.centerX(), r.centerY() + dp(5), paint);
            keyRect(2);
            icons.draw(c, Icons.BACKSPACE, r, dp(22), theme.text);
        }

        @Override
        public boolean onTouchEvent(MotionEvent e) {
            int action = e.getActionMasked();
            if (action == MotionEvent.ACTION_DOWN) {
                pressedKey = -1;
                for (int i = 0; i < 3; i++) {
                    keyRect(i);
                    if (e.getX() >= r.left - dp(3) && e.getX() <= r.right + dp(3)) pressedKey = i;
                }
                performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP);
                if (pressedKey == 2) {
                    listener.onBackspace();
                    handler.postDelayed(repeat, 400);
                }
                invalidate();
            } else if (action == MotionEvent.ACTION_UP || action == MotionEvent.ACTION_CANCEL) {
                handler.removeCallbacks(repeat);
                if (action == MotionEvent.ACTION_UP) {
                    if (pressedKey == 0) listener.onClose();
                    else if (pressedKey == 1) listener.onSpace();
                    if (pressedKey >= 0) performClick();
                }
                pressedKey = -1;
                invalidate();
            }
            return true;
        }

        @Override
        public boolean performClick() {
            return super.performClick();
        }
    }
}
