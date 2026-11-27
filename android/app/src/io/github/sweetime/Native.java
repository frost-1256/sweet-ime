package io.github.sweetime;

/** JNI bindings over the Rust engine (crates/android). */
final class Native {
    static {
        System.loadLibrary("sweet_android");
    }

    static final int KEY_BACKSPACE = -1;
    static final int KEY_ENTER = -2;
    static final int KEY_SPACE = -3;
    static final int KEY_LEFT = -4;
    static final int KEY_RIGHT = -5;
    static final int KEY_ESCAPE = -6;
    static final int KEY_EXPAND = -7;
    static final int KEY_KANA_VARIANT = -8;
    static final int KEY_HALF_SPACE = -9;
    /** Added to a letter typed with Shift. */
    static final int SHIFTED = 0x40000000;

    private Native() {}

    /** Loads the dictionary; per-user files go under filesDir. */
    static native boolean init(String dictPath, String filesDir);

    static native long create();

    static native void free(long ctx);

    /** Handles a key; null when the app should get it. Returns a snapshot. */
    static native String key(long ctx, int code);

    static native String symbol(long ctx, int code);

    static native String select(long ctx, int index);

    static native String undoCommit(long ctx, String committedText);

    static native String commitAll(long ctx);

    static native void reset(long ctx);

    static native void setLeftContext(long ctx, String text);

    /** Reads the settings file again. */
    static native void reloadOptions(long ctx);

    /** Whether conversion candidates are shown. */
    static native boolean converting(long ctx);

    /** The live conversion to refine with zenz ("reading\u001fleft"), or null. */
    static native String liveRequest(long ctx);

    static native String cloudReading(long ctx);

    /** Adds cloud candidates for the same reading, separated by U+001E. */
    static native String applyCloud(long ctx, String reading, String items);

    /** Reading and context for a live conversion being confirmed with Enter. */
    static native String liveCommitRequest(long ctx);

    static native String commitRefinedLive(long ctx, String reading, String leftContext, String surface);

    /** The best conversion with the model; slow, for a background thread. */
    static native String refine(String reading, String leftContext);

    /** Shows a refined live conversion; null when nothing changed. */
    static native String applyLive(long ctx, String reading, String surface);

    /** Converts a downloaded Hugging Face zenz model; null on success, else the error. */
    static native String packZenz(String hfDir, String out);

    /** Whether conversion uses the zenz model now (loads or drops it as needed). */
    static native boolean zenzActive();
    /** Learn nothing in this field (a password or a private tab). */
    static native void setPrivate(long ctx, boolean on);
    /** Registered readings and words, as tab-separated rows. */
    static native String userWords(String path);
    /** Adds, replaces or deletes a word. Returns null on success. */
    static native String updateUserWord(String path, String oldReading, String oldSurface,
                                        String reading, String surface, boolean remove);
    /** How many words the conversion history file holds. */
    static native int learnedCount(String path);
    /** Forgets the last {@code secs} seconds of learning, or all of it if 0; -1 on failure. */
    static native int forgetLearned(String path, long secs);

    /** A parsed snapshot (see Context::snapshot in crates/android). */
    static final class State {
        String commit = "";
        String preedit = "";
        int caret;
        int[][] spans = new int[0][];
        boolean suggestions;
        int selected = -1;
        String[] candidates = new String[0];
        boolean undoable;

        static State parse(String s) {
            State st = new State();
            String[] f = s.split("\u001f", -1);
            if (f.length < 7) return st;
            st.commit = f[0];
            st.preedit = f[1];
            st.caret = Integer.parseInt(f[2]);
            if (!f[3].isEmpty()) {
                String[] parts = f[3].split(";");
                st.spans = new int[parts.length][];
                for (int i = 0; i < parts.length; i++) {
                    String[] n = parts[i].split(",");
                    st.spans[i] = new int[] {
                        Integer.parseInt(n[0]), Integer.parseInt(n[1]), Integer.parseInt(n[2])
                    };
                }
            }
            st.suggestions = f[4].equals("1");
            st.selected = Integer.parseInt(f[5]);
            st.candidates = f[6].isEmpty() ? new String[0] : f[6].split("\u001e", -1);
            st.undoable = f.length >= 8 && f[7].equals("1");
            return st;
        }
    }
}
