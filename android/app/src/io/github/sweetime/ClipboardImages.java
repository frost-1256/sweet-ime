package io.github.sweetime;

import android.content.ClipData;
import android.content.ClipDescription;
import android.content.ClipboardManager;
import android.content.Context;
import android.net.Uri;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputContentInfo;

/** Transfers clipboard images to the active editor. */
final class ClipboardImages {
    private ClipboardImages() {}

    static boolean pasteSaved(Context context, InputConnection ic, EditorInfo editor,
                              Uri uri, String mime) {
        ClipDescription description = new ClipDescription("image", new String[] {mime});
        boolean accepts = false;
        if (editor != null && editor.contentMimeTypes != null) {
            for (String m : editor.contentMimeTypes) {
                if (ClipDescription.compareMimeTypes(mime, m)) accepts = true;
            }
        }
        if (accepts && ic.commitContent(new InputContentInfo(uri, description, null),
                InputConnection.INPUT_CONTENT_GRANT_READ_URI_PERMISSION, null)) {
            return true;
        }
        ClipboardManager clipboard = context.getSystemService(ClipboardManager.class);
        if (clipboard != null) {
            clipboard.setPrimaryClip(new ClipData(description, new ClipData.Item(uri)));
            return ic.performContextMenuAction(android.R.id.paste);
        }
        return false;
    }

    static boolean pasteCurrent(Context context, InputConnection ic, EditorInfo editor) {
        if (editor == null || editor.contentMimeTypes == null) return false;
        ClipboardManager clipboard = context.getSystemService(ClipboardManager.class);
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
}
