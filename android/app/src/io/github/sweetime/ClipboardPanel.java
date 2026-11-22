package io.github.sweetime;

import android.content.ClipData;
import android.content.ClipDescription;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.SharedPreferences;
import android.content.res.ColorStateList;
import android.graphics.Bitmap;
import android.graphics.drawable.GradientDrawable;
import android.net.Uri;
import android.text.TextUtils;
import android.util.Size;
import android.view.Gravity;
import android.view.View;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.Switch;
import android.widget.TextView;

import org.json.JSONArray;
import org.json.JSONException;

import java.util.ArrayList;
import java.util.concurrent.Executors;
import java.util.concurrent.Executor;

/** The system clipboard plus an opt-in text history local to this keyboard. */
final class ClipboardPanel extends ScrollView {
    interface Listener {
        void pasteText(String value);
        void pasteCurrent();
    }

    private static final String HISTORY = "clipboard_history";
    private static final String PINNED = "clipboard_pins";
    private static final int MAX_ITEMS = 12;
    private static final int MAX_SAVED_CHARS = 8192;
    private static final Executor THUMBNAILS = Executors.newSingleThreadExecutor();

    private final ClipboardManager clipboard;
    private final SharedPreferences prefs;
    private final Listener listener;
    private final LinearLayout items;
    private final ArrayList<String> recent;
    private final ArrayList<String> pinned = new ArrayList<>();
    private final ClipboardManager.OnPrimaryClipChangedListener clipChanged = this::onClipboardChanged;
    private boolean listening;
    private boolean privateField;
    private Theme theme;

    ClipboardPanel(Context context, ArrayList<String> recent, Listener listener) {
        super(context);
        this.recent = recent;
        this.listener = listener;
        clipboard = context.getSystemService(ClipboardManager.class);
        prefs = Prefs.keyboard(context);
        items = new LinearLayout(context);
        items.setOrientation(LinearLayout.VERTICAL);
        items.setPadding(dp(10), 0, dp(10), dp(8));
        addView(items);
        try {
            JSONArray saved = new JSONArray(prefs.getString(PINNED, "[]"));
            for (int i = 0; i < Math.min(MAX_ITEMS, saved.length()); i++) {
                String text = saved.optString(i);
                if (storable(text) && !pinned.contains(text)) pinned.add(text);
            }
        } catch (JSONException ignored) {
            // A malformed preference is treated as an empty list.
        }
    }

    void setInputActive(boolean active, boolean privateField) {
        this.privateField = privateField;
        boolean watch = active && !privateField && clipboard != null;
        if (watch == listening) return;
        listening = watch;
        if (watch) {
            clipboard.addPrimaryClipChangedListener(clipChanged);
            onClipboardChanged();
        } else {
            clipboard.removePrimaryClipChangedListener(clipChanged);
        }
    }

    void open(Theme theme) {
        this.theme = theme;
        scrollTo(0, 0);
        render(currentClip());
    }

    void close() {
        theme = null;
        items.removeAllViews();
    }

    private int dp(int value) {
        return Math.round(value * getResources().getDisplayMetrics().density);
    }

    private ClipData currentClip() {
        if (clipboard == null) return null;
        try {
            ClipData clip = clipboard.getPrimaryClip();
            if (clip == null || clip.getItemCount() == 0) return null;
            ClipData.Item item = clip.getItemAt(0);
            return TextUtils.isEmpty(item.getText()) && item.getUri() == null
                    && item.getIntent() == null && item.getHtmlText() == null ? null : clip;
        } catch (SecurityException ignored) {
            return null;
        }
    }

    private static boolean storable(String text) {
        return text != null && text.length() <= MAX_SAVED_CHARS && !text.trim().isEmpty();
    }

    private boolean historyEnabled() {
        return prefs.getBoolean(HISTORY, false);
    }

    private void onClipboardChanged() {
        // Listener callbacks can race with a field change or window hide.
        if (!listening) return;
        boolean showing = isShown() && theme != null;
        if (!showing && !historyEnabled()) return;
        ClipData clip = currentClip();
        remember(clip);
        if (showing) render(clip);
    }

    private String plainText(ClipData clip) {
        if (privateField || clip == null || clip.getItemCount() == 0) return null;
        if (sensitive(clip)) return null;
        CharSequence text = clip.getItemAt(0).getText();
        return text == null ? null : text.toString();
    }

    private boolean sensitive(ClipData clip) {
        ClipDescription description = clip.getDescription();
        return description.getExtras() != null
                && description.getExtras().getBoolean(ClipDescription.EXTRA_IS_SENSITIVE);
    }

    private void remember(ClipData clip) {
        if (!historyEnabled()) return;
        String text = plainText(clip);
        if (!storable(text) || pinned.contains(text)) return;
        recent.remove(text);
        recent.add(0, text);
        if (recent.size() > MAX_ITEMS) recent.remove(recent.size() - 1);
    }

    private void render(ClipData clip) {
        items.removeAllViews();
        if (!privateField) {
            Switch toggle = new Switch(getContext());
            toggle.setText("テキスト履歴");
            toggle.setTextColor(theme.text);
            toggle.setMinHeight(dp(48));
            int[][] states = {{android.R.attr.state_checked}, {}};
            toggle.setThumbTintList(new ColorStateList(states,
                    new int[] {theme.accent, theme.text}));
            toggle.setTrackTintList(new ColorStateList(states,
                    new int[] {theme.focus, theme.specialKey}));
            toggle.setChecked(historyEnabled());
            toggle.setOnCheckedChangeListener((button, checked) -> {
                prefs.edit().putBoolean(HISTORY, checked).apply();
                if (checked) remember(currentClip());
                else recent.clear();
                render(currentClip());
            });
            items.addView(toggle);
        }

        String latest = plainText(clip);
        boolean image = clip != null && !privateField && !sensitive(clip)
                && clip.getDescription().hasMimeType("image/*") && clip.getItemAt(0).getUri() != null;
        if (image) {
            addImage(clip.getItemAt(0).getUri());
        } else if (clip != null && (latest == null || !historyEnabled() || !storable(latest))) {
            addRow(!TextUtils.isEmpty(latest) ? latest : "現在の内容を貼り付け",
                    listener::pasteCurrent, null, null);
        } else if (clip != null && !privateField) {
            remember(clip);
        }

        if (!privateField && historyEnabled()) {
            if (!pinned.isEmpty()) items.addView(label("固定済み", 13, theme.subText));
            for (String text : pinned) {
                addRow(text, () -> listener.pasteText(text), "解除", () -> unpin(text));
            }
            if (!recent.isEmpty()) items.addView(label("最近", 13, theme.subText));
            for (String text : recent) {
                addRow(text, () -> listener.pasteText(text),
                        pinned.size() < MAX_ITEMS ? "固定" : null, () -> pin(text));
            }
        }
        if (clip == null && (privateField || !historyEnabled() || (recent.isEmpty() && pinned.isEmpty()))) {
            TextView empty = label("クリップボードは空です", 15, theme.subText);
            empty.setPadding(0, dp(12), 0, 0);
            items.addView(empty);
        }
    }

    private void addImage(Uri uri) {
        ImageView preview = new ImageView(getContext());
        preview.setScaleType(ImageView.ScaleType.CENTER_CROP);
        preview.setVisibility(GONE);
        items.addView(preview, new LinearLayout.LayoutParams(dp(112), dp(64)));
        preview.setContentDescription("コピーした画像");
        preview.setOnClickListener(v -> listener.pasteCurrent());
        addRow("画像を貼り付け", listener::pasteCurrent, null, null);
        THUMBNAILS.execute(() -> {
            try {
                Bitmap bitmap = getContext().getContentResolver().loadThumbnail(uri,
                        new Size(dp(112), dp(64)), null);
                preview.post(() -> {
                    if (preview.getParent() == items && isShown()) {
                        preview.setImageBitmap(bitmap);
                        preview.setVisibility(VISIBLE);
                    }
                });
            } catch (java.io.IOException | RuntimeException ignored) {
                // Providers can disallow thumbnails. The paste action remains available.
            }
        });
    }

    private TextView label(String text, int size, int color) {
        TextView view = new TextView(getContext());
        view.setText(text);
        view.setTextSize(size);
        view.setTextColor(color);
        return view;
    }

    private void addRow(String text, Runnable paste, String action, Runnable handleAction) {
        LinearLayout row = new LinearLayout(getContext());
        row.setGravity(Gravity.CENTER_VERTICAL);
        GradientDrawable background = new GradientDrawable();
        background.setCornerRadius(dp(8));
        background.setColor(theme.key);
        row.setBackground(background);
        TextView content = label(text, 16, theme.text);
        content.setMinHeight(dp(46));
        content.setGravity(Gravity.CENTER_VERTICAL);
        content.setPadding(dp(12), dp(5), dp(8), dp(5));
        content.setMaxLines(2);
        content.setEllipsize(TextUtils.TruncateAt.END);
        content.setOnClickListener(v -> paste.run());
        row.addView(content, new LinearLayout.LayoutParams(0, LayoutParams.WRAP_CONTENT, 1));
        if (action != null) {
            TextView button = label(action, 13, theme.text);
            button.setGravity(Gravity.CENTER);
            button.setMinHeight(dp(46));
            button.setMinWidth(dp(56));
            button.setOnClickListener(v -> handleAction.run());
            row.addView(button);
        }
        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(
                LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT);
        params.bottomMargin = dp(5);
        items.addView(row, params);
    }

    private void pin(String text) {
        if (pinned.size() >= MAX_ITEMS) return;
        recent.remove(text);
        pinned.add(0, text);
        savePins();
    }

    private void unpin(String text) {
        pinned.remove(text);
        recent.add(0, text);
        if (recent.size() > MAX_ITEMS) recent.remove(recent.size() - 1);
        savePins();
    }

    private void savePins() {
        prefs.edit().putString(PINNED, new JSONArray(pinned).toString()).apply();
        render(currentClip());
    }
}
