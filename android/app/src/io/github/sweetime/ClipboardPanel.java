package io.github.sweetime;

import android.content.ClipData;
import android.content.ClipDescription;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.SharedPreferences;
import android.content.res.ColorStateList;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
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

import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.Set;
import java.util.concurrent.Executors;
import java.util.concurrent.Executor;

/**
 * The system clipboard plus an opt-in history of copied text and images,
 * local to this keyboard. Images are copied into the app's private storage
 * and served to editors by {@link ClipProvider}.
 */
final class ClipboardPanel extends ScrollView {
    interface Listener {
        void pasteText(String value);
        void pasteCurrent();
        /** Pastes a saved image (a {@link ClipProvider} URI). */
        void pasteImage(Uri uri, String mime);
    }

    /** History entries for images: this prefix, then the file name. */
    private static final String IMAGE = "\u0000image:";
    /** Larger images are pasted only while they are on the clipboard. */
    private static final long MAX_IMAGE_BYTES = 20L << 20;

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
    /** The image last saved and its entry, so a copy is saved once. */
    private String savedSource;
    private String savedEntry;

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
        removeUnusedImages();
    }

    static File imageDir(Context c) {
        return new File(c.getFilesDir(), "clips");
    }

    static String mimeOf(String name) {
        return name.endsWith(".jpg") ? "image/jpeg" : name.endsWith(".gif") ? "image/gif"
                : name.endsWith(".webp") ? "image/webp" : "image/png";
    }

    private static boolean isImage(String entry) {
        return entry.startsWith(IMAGE);
    }

    private File imageFile(String entry) {
        return new File(imageDir(getContext()), entry.substring(IMAGE.length()));
    }

    /** Deletes saved images that are neither recent nor pinned. */
    private void removeUnusedImages() {
        Set<String> used = new HashSet<>();
        for (String e : recent) if (isImage(e)) used.add(imageFile(e).getName());
        for (String e : pinned) if (isImage(e)) used.add(imageFile(e).getName());
        File[] files = imageDir(getContext()).listFiles();
        if (files == null) return;
        for (File f : files) {
            if (!used.contains(f.getName())) f.delete();
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
        Uri image = imageUri(clip);
        if (image != null) {
            // Pasting a saved image puts it on the clipboard: already saved.
            if (!ClipProvider.AUTHORITY.equals(image.getAuthority())) saveImage(clip);
            return;
        }
        String text = plainText(clip);
        if (!storable(text) || pinned.contains(text)) return;
        addRecent(text);
    }

    private void addRecent(String entry) {
        recent.remove(entry);
        recent.add(0, entry);
        if (recent.size() > MAX_ITEMS) recent.remove(recent.size() - 1);
        removeUnusedImages();
    }

    /** The copied image, unless it is private, or null. */
    private Uri imageUri(ClipData clip) {
        if (clip == null || privateField || sensitive(clip) || clip.getItemCount() == 0) return null;
        Uri uri = clip.getItemAt(0).getUri();
        return uri != null && "content".equals(uri.getScheme())
                && clip.getDescription().hasMimeType("image/*") ? uri : null;
    }

    /** Copies the clipboard's image into the history, in the background. */
    private void saveImage(ClipData clip) {
        Uri uri = imageUri(clip);
        String source = uri + "@" + clip.getDescription().getTimestamp();
        if (source.equals(savedSource)) {
            if (savedEntry != null && !pinned.contains(savedEntry)) addRecent(savedEntry);
            return;
        }
        savedSource = source;
        savedEntry = null;
        String mime = clip.getDescription().getMimeType(0);
        String ext = mime.equals("image/jpeg") ? "jpg" : mime.equals("image/gif") ? "gif"
                : mime.equals("image/webp") ? "webp" : "png";
        Context c = getContext();
        THUMBNAILS.execute(() -> {
            File dir = imageDir(c);
            File out = new File(dir, Long.toHexString(System.currentTimeMillis())
                    + "-" + Integer.toHexString(source.hashCode() & 0xffff) + "." + ext);
            boolean ok = false;
            try {
                if (!dir.isDirectory() && !dir.mkdirs()) throw new IOException("mkdir " + dir);
                long size = 0;
                try (InputStream in = c.getContentResolver().openInputStream(uri);
                     OutputStream o = new FileOutputStream(out)) {
                    if (in == null) throw new IOException("no stream");
                    byte[] buf = new byte[1 << 16];
                    int n;
                    while ((n = in.read(buf)) > 0) {
                        size += n;
                        if (size > MAX_IMAGE_BYTES) throw new IOException("too large");
                        o.write(buf, 0, n);
                    }
                }
                ok = true;
            } catch (IOException | RuntimeException ignored) {
                // Not saved: the image stays pasteable while it is on the clipboard.
                out.delete();
            }
            boolean saved = ok;
            post(() -> {
                if (!saved || !source.equals(savedSource) || !historyEnabled()) {
                    if (saved) out.delete();
                    return;
                }
                savedEntry = IMAGE + out.getName();
                addRecent(savedEntry);
                if (isShown() && theme != null) render(currentClip());
            });
        });
    }

    private void render(ClipData clip) {
        items.removeAllViews();
        if (!privateField) {
            Switch toggle = new Switch(getContext());
            toggle.setText("履歴を保存(テキストと画像)");
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
                if (checked) {
                    remember(currentClip());
                } else {
                    recent.clear();
                    savedSource = null;
                    removeUnusedImages();
                }
                render(currentClip());
            });
            items.addView(toggle);
        }

        String latest = plainText(clip);
        boolean image = clip != null && !privateField && !sensitive(clip)
                && clip.getDescription().hasMimeType("image/*") && clip.getItemAt(0).getUri() != null;
        boolean inHistory = image && historyEnabled()
                && (ClipProvider.AUTHORITY.equals(clip.getItemAt(0).getUri().getAuthority())
                    || savedEntry != null && (recent.contains(savedEntry) || pinned.contains(savedEntry)));
        if (image && !inHistory) {
            addImage(clip.getItemAt(0).getUri());
            if (historyEnabled()) remember(clip);
        } else if (clip != null && (latest == null || !historyEnabled() || !storable(latest))) {
            addRow(!TextUtils.isEmpty(latest) ? latest : "現在の内容を貼り付け",
                    listener::pasteCurrent, null, null);
        } else if (clip != null && !privateField) {
            remember(clip);
        }

        if (!privateField && historyEnabled()) {
            if (!pinned.isEmpty()) items.addView(label("固定済み", 13, theme.subText));
            for (String text : pinned) {
                if (isImage(text)) addSavedImage(text, "解除", () -> unpin(text));
                else addRow(text, () -> listener.pasteText(text), "解除", () -> unpin(text));
            }
            if (!recent.isEmpty()) items.addView(label("最近", 13, theme.subText));
            for (String text : recent) {
                String action = pinned.size() < MAX_ITEMS ? "固定" : null;
                if (isImage(text)) addSavedImage(text, action, () -> pin(text));
                else addRow(text, () -> listener.pasteText(text), action, () -> pin(text));
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

    /** A row with a saved image's thumbnail; a tap pastes it. */
    private void addSavedImage(String entry, String action, Runnable handleAction) {
        File file = imageFile(entry);
        LinearLayout row = new LinearLayout(getContext());
        row.setGravity(Gravity.CENTER_VERTICAL);
        GradientDrawable background = new GradientDrawable();
        background.setCornerRadius(dp(8));
        background.setColor(theme.key);
        row.setBackground(background);
        row.setPadding(dp(6), dp(6), 0, dp(6));
        ImageView preview = new ImageView(getContext());
        preview.setScaleType(ImageView.ScaleType.CENTER_CROP);
        preview.setClipToOutline(true);
        GradientDrawable shape = new GradientDrawable();
        shape.setCornerRadius(dp(6));
        preview.setBackground(shape);
        preview.setContentDescription("保存した画像");
        row.addView(preview, new LinearLayout.LayoutParams(dp(112), dp(72)));
        TextView text = label("画像", 15, theme.subText);
        text.setPadding(dp(12), 0, 0, 0);
        row.addView(text, new LinearLayout.LayoutParams(0, LayoutParams.WRAP_CONTENT, 1));
        Runnable paste = () -> listener.pasteImage(ClipProvider.uriFor(file.getName()), mimeOf(file.getName()));
        preview.setOnClickListener(v -> paste.run());
        text.setOnClickListener(v -> paste.run());
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
        int w = dp(112);
        THUMBNAILS.execute(() -> {
            BitmapFactory.Options o = new BitmapFactory.Options();
            o.inJustDecodeBounds = true;
            BitmapFactory.decodeFile(file.getPath(), o);
            o.inSampleSize = 1;
            while (o.outWidth / (o.inSampleSize * 2) >= w) o.inSampleSize *= 2;
            o.inJustDecodeBounds = false;
            Bitmap bitmap = BitmapFactory.decodeFile(file.getPath(), o);
            if (bitmap != null) preview.post(() -> preview.setImageBitmap(bitmap));
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
        addRecent(text);
        savePins();
    }

    private void savePins() {
        prefs.edit().putString(PINNED, new JSONArray(pinned).toString()).apply();
        render(currentClip());
    }
}
