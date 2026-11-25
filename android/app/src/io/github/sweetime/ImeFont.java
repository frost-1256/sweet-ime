package io.github.sweetime;

import android.content.Context;
import android.database.Cursor;
import android.graphics.Typeface;
import android.graphics.fonts.Font;
import android.graphics.fonts.FontFamily;
import android.net.Uri;
import android.provider.OpenableColumns;

import java.io.File;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.file.Files;
import java.nio.file.StandardCopyOption;

/** Display font shared by the keyboard, candidates and settings preview. */
final class ImeFont {
    static final int SYSTEM = 0;
    static final int SERIF = 1;
    static final int MONOSPACE = 2;
    static final int CUSTOM = 3;
    private static final long MAX_BYTES = 64L * 1024 * 1024;
    private static Typeface custom;

    private ImeFont() {}

    static Typeface selected(Context context) {
        switch (Prefs.number(context, Prefs.FONT, SYSTEM)) {
            case SERIF: return Typeface.SERIF;
            case MONOSPACE: return Typeface.MONOSPACE;
            case CUSTOM: return custom(context);
            default: return Typeface.DEFAULT;
        }
    }

    private static synchronized Typeface custom(Context context) {
        if (custom != null) return custom;
        try {
            custom = read(file(context));
            return custom;
        } catch (IOException | RuntimeException e) {
            return Typeface.DEFAULT;
        }
    }

    private static File file(Context context) {
        return new File(context.getFilesDir(), "ime-font");
    }

    private static Typeface read(File file) throws IOException {
        Font font = new Font.Builder(file).build();
        FontFamily family = new FontFamily.Builder(font).build();
        return new Typeface.CustomFallbackBuilder(family).setSystemFallback("sans-serif").build();
    }

    static boolean hasCustom(Context context) {
        return file(context).isFile();
    }

    static String customName(Context context) {
        return Prefs.keyboard(context).getString(Prefs.FONT_FILE_NAME, "カスタムフォント");
    }

    static void importFile(Context context, Uri uri) throws IOException {
        File temp = File.createTempFile("ime-font-", ".tmp", context.getFilesDir());
        try {
            try (InputStream input = context.getContentResolver().openInputStream(uri);
                 OutputStream output = Files.newOutputStream(temp.toPath())) {
                if (input == null) throw new IOException("No font data");
                byte[] buffer = new byte[8192];
                long total = 0;
                for (int n; (n = input.read(buffer)) != -1;) {
                    total += n;
                    if (total > MAX_BYTES) throw new IOException("Font exceeds 64 MiB");
                    output.write(buffer, 0, n);
                }
            }
            Typeface face = read(temp);
            String name = displayName(context, uri);
            synchronized (ImeFont.class) {
                Files.move(temp.toPath(), file(context).toPath(), StandardCopyOption.ATOMIC_MOVE,
                        StandardCopyOption.REPLACE_EXISTING);
                custom = face;
                Prefs.keyboard(context).edit().putString(Prefs.FONT_FILE_NAME, name)
                        .putInt(Prefs.FONT, CUSTOM).apply();
            }
        } finally {
            temp.delete();
        }
    }

    private static String displayName(Context context, Uri uri) {
        try (Cursor cursor = context.getContentResolver().query(uri,
                new String[] {OpenableColumns.DISPLAY_NAME}, null, null, null)) {
            if (cursor != null && cursor.moveToFirst()) {
                String name = cursor.getString(0);
                if (name != null && !name.trim().isEmpty()) return name;
            }
        } catch (RuntimeException ignored) {
            // Some document providers do not expose a display name.
        }
        return "カスタムフォント";
    }
}
