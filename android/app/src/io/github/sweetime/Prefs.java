package io.github.sweetime;

import android.content.Context;
import android.content.SharedPreferences;

import java.io.File;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * Settings: the keyboard's own (SharedPreferences) and the engine's
 * (config.txt under the app's files, the same format as on the desktop).
 */
final class Prefs {
    static final String HAPTIC = "haptic";
    static final String SOUND = "sound";
    static final String NUMBER_ROW = "number_row";
    static final String KEY_PREVIEW = "key_preview";
    /** 0 small, 1 normal, 2 large. */
    static final String KEY_HEIGHT = "key_height";
    static final String PUNCTUATION_WIDTH = "punctuation_width";
    /** 0 system, 1 light, 2 dark. */
    static final String THEME = "theme";

    private Prefs() {}

    static SharedPreferences keyboard(Context c) {
        return c.getSharedPreferences("keyboard", Context.MODE_PRIVATE);
    }

    static boolean flag(Context c, String key, boolean def) {
        return keyboard(c).getBoolean(key, def);
    }

    static int number(Context c, String key, int def) {
        return keyboard(c).getInt(key, def);
    }

    /** Engine options: key -> on/off, with the desktop defaults. */
    static final class Engine {
        static final String[][] SWITCHES = {
            {"live_conversion", "ライブ変換", "打ちながら変換結果を表示します。確定は Enter。"},
            {"suggestions", "予測変換", "入力中に続きの候補を表示します。タップで確定。"},
            {"learning", "学習", "選んだ候補を覚えて、次から優先します。"},
            {"private_fields", "シークレット入力では学習しない",
                "パスワード欄やブラウザのシークレットタブでは、何も覚えません。"},
            {"english_words", "英単語の認識", "github や test のようにつづりで打った英単語を変換します。"},
            {"english_katakana", "英単語をカタカナで出す", "best → ベスト のように、外来語を先に出します。"},
            {"shift_one_word", "Shift の英字入力を1単語で戻す",
                "Shift で始めた英単語のあとに日本語が続くと、日本語入力に戻ります。"},
            {"dates", "日付と時刻", "きょう、いま などで日付や時刻を候補に出します。"},
            {"emoji", "絵文字と顔文字", "えがお、かお などで絵文字や顔文字を候補に出します。"},
            {"calculator", "計算", "1+2= のように打つと計算結果を候補に出します。"},
        };

        private final File file;
        private final Map<String, String> values = new LinkedHashMap<>();
        private final List<String> order = new ArrayList<>();

        Engine(Context c) {
            file = new File(new File(c.getFilesDir(), "sweet-ime"), "config.txt");
            if (!file.exists()) return;
            try {
                for (String line : new String(Files.readAllBytes(file.toPath()), StandardCharsets.UTF_8).split("\n")) {
                    String l = line.split("#", 2)[0].trim();
                    int eq = l.indexOf('=');
                    if (eq < 0) continue;
                    String k = l.substring(0, eq).trim();
                    if (!values.containsKey(k)) order.add(k);
                    values.put(k, l.substring(eq + 1).trim());
                }
            } catch (IOException ignored) {
                // Defaults.
            }
        }

        boolean get(String key) {
            String v = values.get(key);
            if (v == null) return true; // every switch above defaults to on
            v = v.toLowerCase();
            return v.equals("on") || v.equals("1") || v.equals("true") || v.equals("yes");
        }

        void set(String key, boolean on) {
            if (!values.containsKey(key)) order.add(key);
            values.put(key, on ? "on" : "off");
        }

        void save() throws IOException {
            StringBuilder sb = new StringBuilder("# Sweet IME settings\n");
            for (String k : order) sb.append(k).append(" = ").append(values.get(k)).append('\n');
            File dir = file.getParentFile();
            if (dir != null && !dir.exists() && !dir.mkdirs()) throw new IOException("mkdir " + dir);
            Files.write(file.toPath(), sb.toString().getBytes(StandardCharsets.UTF_8));
        }
    }
}
