package io.github.sweetime;

import android.app.Activity;
import android.content.Context;
import android.content.Intent;
import android.content.res.ColorStateList;
import android.graphics.Canvas;
import android.graphics.Paint;
import android.graphics.RectF;
import android.graphics.Typeface;
import android.graphics.drawable.ColorDrawable;
import android.graphics.drawable.Drawable;
import android.graphics.drawable.GradientDrawable;
import android.graphics.drawable.RippleDrawable;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.provider.Settings;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.View;
import android.view.ViewGroup;
import android.view.inputmethod.InputMethodInfo;
import android.view.inputmethod.InputMethodManager;
import android.widget.Button;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.RadioButton;
import android.widget.RadioGroup;
import android.widget.ScrollView;
import android.widget.Switch;
import android.widget.TextView;
import android.widget.Toast;
import android.window.OnBackInvokedCallback;
import android.window.OnBackInvokedDispatcher;

import java.io.BufferedReader;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;

/**
 * The settings: a home page with the setup state, a try-it field and the
 * categories, each of which opens its own page.
 */
public final class SettingsActivity extends Activity {
    private static final String HOME = "";
    private static final String CONVERSION = "conversion";
    private static final String AI = "ai";
    private static final String LEARNING = "learning";
    private static final String KEYBOARD = "keyboard";
    private static final String THEME = "theme";
    private static final String ABOUT = "about";
    private static final int PICK_FONT = 1;

    /** The page shown: HOME or one of the categories. */
    private String page = HOME;
    private ScrollView scroll;
    private LinearLayout box;
    private int bottomInset;
    private Theme theme;
    private Prefs.Engine engine;
    private TextView status;
    private Button enable;
    private Button pick;
    /** Home page summaries, refreshed on resume. */
    private TextView aiSummary;
    private TextView learningSummary;
    /** Handles back on a category page (Android 13 and later). */
    private Object backCallback;
    private boolean fontImporting;

    @Override
    protected void onCreate(Bundle saved) {
        super.onCreate(saved);
        if (getActionBar() != null) getActionBar().hide();
        theme = Theme.of(this);
        engine = new Prefs.Engine(this);
        getWindow().getDecorView().setBackgroundColor(theme.background);
        // Dark status bar icons on the light theme.
        int light = android.view.WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS
                | android.view.WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS;
        android.view.WindowInsetsController controller = getWindow().getInsetsController();
        if (controller != null) controller.setSystemBarsAppearance(theme.dark ? 0 : light, light);
        scroll = new ScrollView(this);
        scroll.setBackgroundColor(theme.background);
        // Not the try-it field: no keyboard until it is tapped.
        scroll.setFocusableInTouchMode(true);
        // Below the status bar and above the navigation bar and the keyboard.
        scroll.setOnApplyWindowInsetsListener((v, insets) -> {
            android.graphics.Insets bars = insets.getInsets(android.view.WindowInsets.Type.systemBars());
            int ime = insets.getInsets(android.view.WindowInsets.Type.ime()).bottom;
            // The status bar area stays clear while scrolling.
            v.setPadding(0, bars.top, 0, 0);
            // The page ends above the keyboard, so the focused field scrolls into view;
            // without it the content runs on under the navigation bar.
            ViewGroup.MarginLayoutParams lp = (ViewGroup.MarginLayoutParams) v.getLayoutParams();
            if (lp != null && lp.bottomMargin != ime) {
                lp.bottomMargin = ime;
                v.setLayoutParams(lp);
            }
            bottomInset = ime > 0 ? 0 : bars.bottom;
            if (box != null) padBox();
            return android.view.WindowInsets.CONSUMED;
        });
        getWindow().setSoftInputMode(android.view.WindowManager.LayoutParams.SOFT_INPUT_STATE_HIDDEN
                | android.view.WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE);
        setContentView(scroll);
        // The update notification opens the about page.
        String start = saved != null ? saved.getString("page", HOME) : getIntent().getStringExtra("page");
        show(start != null ? start : HOME);
        Updater.schedule(this);
        if (saved == null) {
            handleInstallStatus(getIntent());
            askNotificationsOnce();
            if (Updater.autoCheck(this) && Updater.due(this)) checkUpdate(true);
        }
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        if (handleInstallStatus(intent)) return;
        String p = intent.getStringExtra("page");
        if (p != null) show(p);
    }

    @Override
    protected void onSaveInstanceState(Bundle out) {
        super.onSaveInstanceState(out);
        // A theme change recreates the activity; stay on the theme page.
        out.putString("page", page);
    }

    @Override
    protected void onResume() {
        super.onResume();
        updateStatus();
        updateLearned();
        updateSummaries();
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        // The keyboard picker closes without a callback.
        if (hasFocus) updateStatus();
    }

    /** Before Android 13: back from a category page goes home. */
    @SuppressWarnings("deprecation")
    @Override
    public void onBackPressed() {
        if (!page.equals(HOME)) show(HOME);
        else super.onBackPressed();
    }

    private int dp(int v) {
        return (int) (v * getResources().getDisplayMetrics().density);
    }

    private void padBox() {
        box.setPadding(dp(16), dp(8), dp(16), dp(32) + bottomInset);
    }

    /** Replaces the content with page `p`. */
    private void show(String p) {
        page = p;
        status = null;
        enable = null;
        pick = null;
        learnedStatus = null;
        aiStatus = null;
        aiSummary = null;
        learningSummary = null;
        updateText = null;
        updateNotes = null;
        updateButton = null;
        updateSkip = null;
        homeBanner = null;
        homeBannerText = null;
        box = new LinearLayout(this);
        box.setOrientation(LinearLayout.VERTICAL);
        padBox();
        switch (p) {
            case CONVERSION:
                conversionPage();
                break;
            case AI:
                aiPage();
                break;
            case LEARNING:
                learningPage();
                break;
            case KEYBOARD:
                keyboardPage();
                break;
            case THEME:
                themePage();
                break;
            case ABOUT:
                aboutPage();
                break;
            default:
                page = HOME;
                homePage();
                break;
        }
        scroll.removeAllViews();
        scroll.addView(box);
        scroll.scrollTo(0, 0);
        scroll.requestFocus();
        updateBack();
        updateStatus();
        updateSummaries();
        updateUpdate();
    }

    /** Lets back return home from a category page (Android 13 and later). */
    private void updateBack() {
        if (Build.VERSION.SDK_INT < 33) return;
        OnBackInvokedDispatcher d = getOnBackInvokedDispatcher();
        if (page.equals(HOME)) {
            if (backCallback != null) {
                d.unregisterOnBackInvokedCallback((OnBackInvokedCallback) backCallback);
                backCallback = null;
            }
        } else if (backCallback == null) {
            OnBackInvokedCallback cb = () -> show(HOME);
            d.registerOnBackInvokedCallback(OnBackInvokedDispatcher.PRIORITY_DEFAULT, cb);
            backCallback = cb;
        }
    }

    // ---- Pages ----

    private void homePage() {
        TextView title = text("Sweet IME", 30, theme.text);
        title.setTypeface(Typeface.DEFAULT_BOLD);
        title.setPadding(dp(4), dp(24), 0, 0);
        box.addView(title);
        TextView sub = text("Rust で書いた日本語入力", 14, theme.subText);
        sub.setPadding(dp(4), 0, 0, 0);
        box.addView(sub);

        // Filled in once a check finds a new release.
        homeBanner = card(null);
        homeBannerText = text("", 16, theme.text);
        homeBannerText.setPadding(0, dp(4), 0, dp(10));
        homeBanner.addView(homeBannerText);
        Button update = button("アップデート");
        update.setOnClickListener(v -> {
            show(ABOUT);
            startUpdate();
        });
        homeBanner.addView(update);
        Button skip = tonalButton("このバージョンをスキップ");
        skip.setOnClickListener(v -> skipRelease());
        homeBanner.addView(skip);

        LinearLayout setup = card(null);
        status = text("", 16, theme.text);
        status.setPadding(0, dp(4), 0, dp(10));
        setup.addView(status);
        enable = button("キーボードを有効にする");
        enable.setOnClickListener(v -> startActivity(new Intent(Settings.ACTION_INPUT_METHOD_SETTINGS)));
        setup.addView(enable);
        pick = button("キーボードを切り替える");
        pick.setOnClickListener(v -> ((InputMethodManager) getSystemService(Context.INPUT_METHOD_SERVICE))
                .showInputMethodPicker());
        setup.addView(pick);

        LinearLayout tryIt = card("試し打ち");
        EditText test = new EditText(this);
        test.setHint("ここをタップして入力してみてください");
        test.setTextColor(theme.text);
        test.setHintTextColor(theme.subText);
        test.setMinLines(2);
        test.setBackground(null);
        tryIt.addView(test, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));

        LinearLayout list = card("設定");
        list.setPadding(0, dp(6), 0, dp(6));
        entry(list, Icons.CONVERT, "変換", "ライブ変換・予測・英単語・絵文字など", CONVERSION);
        aiSummary = entry(list, Icons.SPARKLE, "AI 変換", "", AI);
        learningSummary = entry(list, Icons.HISTORY, "学習とプライバシー", "", LEARNING);
        entry(list, Icons.KEYBOARD, "キーボード", "かな・英数フリック、キーの高さ、振動と音", KEYBOARD);
        int t = Math.min(Prefs.number(this, Prefs.THEME, 0), Theme.LABELS.length - 1);
        entry(list, Icons.PALETTE, "テーマとフォント", Theme.LABELS[t], THEME);
        entry(list, Icons.INFO, "このアプリについて", "バージョン " + version() + "・ライセンス", ABOUT);

        TextView note = text("設定はキーボードを次に表示したときに反映されます。", 13, theme.subText);
        note.setPadding(dp(4), dp(16), dp(4), 0);
        box.addView(note);
    }

    private void conversionPage() {
        header("変換");
        LinearLayout input = card("入力中");
        engineToggle(input, "live_conversion");
        engineToggle(input, "suggestions");
        LinearLayout english = card("英単語");
        engineToggle(english, "english_words");
        engineToggle(english, "english_katakana");
        engineToggle(english, "shift_one_word");
        LinearLayout extra = card("候補に出すもの");
        engineToggle(extra, "dates");
        engineToggle(extra, "emoji");
        engineToggle(extra, "calculator");
        extra.addView(prefToggle("Simejiクラウド変換",
                "候補バーの「クラウド」を押すと、入力中のかなの読みをSimejiに送信し、顔文字候補を取得します。",
                Prefs.SIMEJI_CLOUD, false));
    }

    private void aiPage() {
        header("AI 変換");
        aiSection();
    }

    private void learningPage() {
        header("学習とプライバシー");
        LinearLayout learn = card("学習");
        engineToggle(learn, "learning");
        engineToggle(learn, "private_fields");
        learningSection();
        reportsSection();
    }

    private void keyboardPage() {
        header("キーボード");
        LinearLayout layout = card("レイアウト");
        layout.addView(prefToggle("かなフリック入力", "日本語入力に12キーのかな配列を使います。",
                Prefs.KANA_FLICK, false));
        layout.addView(prefToggle("英数フリック入力",
                "英字の直接入力に12キー配列を使います。パスワード欄ではQWERTY配列になります。",
                Prefs.ENGLISH_FLICK, false));
        layout.addView(prefToggle("数字の段",
                "QWERTY 配列の文字キーの上に数字の段を表示します。オフのときは q〜p の長押しで数字を入力します。",
                Prefs.NUMBER_ROW, false));
        layout.addView(choice("キーの高さ", new String[] {"低め", "標準", "高め"}, Prefs.KEY_HEIGHT, 1));
        layout.addView(choice("QWERTY の句読点キーの幅", new String[] {"細め", "標準", "広め"}, Prefs.PUNCTUATION_WIDTH, 1));
        LinearLayout feedback = card("押したときの反応");
        feedback.addView(prefToggle("キーのプレビュー", "押したキーを大きく表示します。", Prefs.KEY_PREVIEW, true));
        feedback.addView(prefToggle("振動", "キーを押したときに振動します。", Prefs.HAPTIC, true));
        feedback.addView(prefToggle("キー音", "キーを押したときに音を鳴らします。", Prefs.SOUND, false));
        LinearLayout help = card("使い方");
        for (String tip : new String[] {
            "Shift で始めた英字は英単語になり、日本語が続くと日本語入力に戻ります。",
            "QWERTYの地球儀キーや12キーの英数／かなキーで、日本語と英字入力を切り替えます。",
            "かなフリックでは、あのキーをタップすると「あ」、左・上・右・下にはじくと「い・う・え・お」が入力できます。",
            "英数フリックでは、abc のキーをタップすると「a」、左・上・下にはじくと「b・c・2」が入力できます。",
            "小゛゜キーで、カーソルの直前のかなを小文字・濁音・半濁音に切り替えます。",
            "スペースキーを左右になぞると、カーソルが動きます。",
            "候補バーの右の矢印で、すべての候補を一覧できます。",
            "キーを押したまま次のキーを押しても、両方とも入力されます。",
        }) {
            TextView line = text("・" + tip, 14, theme.text);
            line.setPadding(0, dp(4), 0, dp(4));
            help.addView(line);
        }
    }

    private void themePage() {
        header("テーマとフォント");
        fontCard();
        int current = Prefs.number(this, Prefs.THEME, 0);
        LinearLayout row = null;
        for (int i = 0; i < Theme.LABELS.length; i++) {
            if (i % 2 == 0) {
                row = new LinearLayout(this);
                row.setOrientation(LinearLayout.HORIZONTAL);
                box.addView(row);
            }
            row.addView(themeTile(i, i == current),
                    new LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1));
        }
        if (Theme.LABELS.length % 2 == 1 && row != null) {
            row.addView(new View(this), new LinearLayout.LayoutParams(0, 0, 1));
        }
        TextView note = text("Material 3 のテーマは、Android 12 以降では壁紙の色に合わせます。", 13, theme.subText);
        note.setPadding(dp(4), dp(12), dp(4), 0);
        box.addView(note);
    }

    private void fontCard() {
        LinearLayout card = card("キーボードと候補のフォント");
        TextView sample = text("あいうえお　日本語　Aa 123", 22, theme.text);
        sample.setTypeface(ImeFont.selected(this));
        card.addView(sample);
        RadioGroup options = new RadioGroup(this);
        String[] labels = {"標準", "明朝・セリフ", "等幅", ImeFont.customName(this)};
        int count = ImeFont.hasCustom(this) ? labels.length : labels.length - 1;
        int current = Prefs.number(this, Prefs.FONT, ImeFont.SYSTEM);
        if (current < 0 || current >= count) current = ImeFont.SYSTEM;
        for (int value = 0; value < count; value++) {
            RadioButton option = new RadioButton(this);
            option.setId(View.generateViewId());
            option.setText(labels[value]);
            option.setTextColor(theme.text);
            option.setButtonTintList(ColorStateList.valueOf(theme.accent));
            option.setTag(value);
            option.setEnabled(!fontImporting);
            options.addView(option);
            if (value == current) options.check(option.getId());
        }
        options.setOnCheckedChangeListener((group, id) -> {
            View option = group.findViewById(id);
            if (option == null) return;
            Prefs.keyboard(this).edit().putInt(Prefs.FONT, (int) option.getTag()).apply();
            sample.setTypeface(ImeFont.selected(this));
        });
        card.addView(options);
        Button choose = button(fontImporting ? "フォントを読み込み中…" : "フォントファイルを選ぶ（TTF・OTF）");
        choose.setEnabled(!fontImporting);
        choose.setOnClickListener(v -> pickFont());
        card.addView(choose);
    }

    @SuppressWarnings("deprecation")
    private void pickFont() {
        Intent picker = new Intent(Intent.ACTION_OPEN_DOCUMENT);
        picker.setType("*/*");
        picker.addCategory(Intent.CATEGORY_OPENABLE);
        startActivityForResult(picker, PICK_FONT);
    }

    @SuppressWarnings("deprecation")
    @Override
    protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        if (request != PICK_FONT || result != RESULT_OK || data == null || data.getData() == null) return;
        Uri uri = data.getData();
        Context context = getApplicationContext();
        fontImporting = true;
        show(THEME);
        new Thread(() -> {
            String message;
            try {
                ImeFont.importFile(context, uri);
                message = "フォントを読み込みました。キーボードを開くと反映されます。";
            } catch (java.io.IOException | RuntimeException e) {
                message = "フォントを読み込めませんでした。TTF・OTFファイルを確認してください。";
            }
            String resultMessage = message;
            runOnUiThread(() -> {
                fontImporting = false;
                if (isFinishing() || isDestroyed()) return;
                if (THEME.equals(page)) show(THEME);
                Toast.makeText(this, resultMessage, Toast.LENGTH_SHORT).show();
            });
        }, "sweet-ime-font-import").start();
    }

    /** Theme `index` previewed as a tiny keyboard, with its name. */
    private View themeTile(int index, boolean selected) {
        Theme t = Theme.of(this, index);
        float d = getResources().getDisplayMetrics().density;
        LinearLayout tile = new LinearLayout(this);
        tile.setOrientation(LinearLayout.VERTICAL);
        tile.setPadding(dp(6), dp(6), dp(6), dp(10));
        View preview = new View(this) {
            private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG);
            private final RectF r = new RectF();

            @Override
            protected void onDraw(Canvas c) {
                float w = getWidth();
                float h = getHeight();
                paint.setStyle(Paint.Style.FILL);
                paint.setColor(t.background);
                r.set(0, 0, w, h);
                c.drawRoundRect(r, 14 * d, 14 * d, paint);
                float pad = 9 * d;
                float gap = 4 * d;
                float rowH = (h - 2 * pad - 2 * gap) / 3;
                float rad = Math.min(t.radius, rowH / d / 2) * d * 0.7f;
                for (int y = 0; y < 3; y++) {
                    int n = y == 2 ? 3 : 5;
                    float keyW = (w - 2 * pad - (n - 1) * gap) / n;
                    for (int x = 0; x < n; x++) {
                        float left = pad + x * (keyW + gap);
                        float top = pad + y * (rowH + gap);
                        int color = t.key;
                        if (y == 2 && x == 0) color = t.specialKey;
                        if (y == 2 && x == 2) color = t.accent;
                        paint.setColor(color);
                        r.set(left, top, left + keyW, top + rowH);
                        c.drawRoundRect(r, rad, rad, paint);
                        if (y < 2) {
                            paint.setColor(t.text);
                            c.drawCircle(r.centerX(), r.centerY(), 1.6f * d, paint);
                        }
                    }
                }
                if (selected) {
                    paint.setStyle(Paint.Style.STROKE);
                    paint.setStrokeWidth(3 * d);
                    paint.setColor(theme.accent);
                    r.set(1.5f * d, 1.5f * d, w - 1.5f * d, h - 1.5f * d);
                    c.drawRoundRect(r, 13 * d, 13 * d, paint);
                }
            }
        };
        tile.addView(preview, new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(92)));
        TextView name = text((selected ? "✓ " : "") + Theme.LABELS[index], 13,
                selected ? theme.accent : theme.text);
        name.setGravity(Gravity.CENTER);
        name.setPadding(0, dp(6), 0, 0);
        if (selected) name.setTypeface(Typeface.DEFAULT_BOLD);
        tile.addView(name);
        tile.setContentDescription(Theme.LABELS[index]);
        tile.setOnClickListener(v -> {
            if (selected) return;
            Prefs.keyboard(this).edit().putInt(Prefs.THEME, index).apply();
            recreate();
        });
        return tile;
    }

    private void aboutPage() {
        header("このアプリについて");
        LinearLayout app = card(null);
        TextView name = text("Sweet IME " + version(), 18, theme.text);
        name.setTypeface(Typeface.DEFAULT_BOLD);
        app.addView(name);
        TextView desc = text("辞書と AI で変換する、オープンソースの日本語入力です。", 14, theme.subText);
        desc.setPadding(0, dp(4), 0, dp(12));
        app.addView(desc);
        Button source = button("ソースコード(GitHub)");
        source.setOnClickListener(v -> open("https://github.com/frost-1256/sweet-ime"));
        app.addView(source);
        Button releases = tonalButton("最新のリリース");
        releases.setOnClickListener(v -> open("https://github.com/frost-1256/sweet-ime/releases/latest"));
        app.addView(releases);
        updateSection();
        LinearLayout lic = card("ライセンス");
        for (String[] l : new String[][] {
            {"プログラム", "MIT License"},
            {"変換辞書", "Mozc OSS 辞書(IPAdic・沖縄辞書を含む)と SudachiDict (Works Applications Co., Ltd.、Apache License 2.0) から作成。"},
            {"英単語リスト", "SCOWL / ESDB(Kevin Atkinson)から作成。"},
            {"AI モデル(別途ダウンロード)", "zenz(作者: Miwa Keita)、CC BY-SA 4.0。アプリには含まれていません。"},
        }) {
            TextView h = text(l[0], 15, theme.text);
            h.setPadding(0, dp(8), 0, 0);
            lic.addView(h);
            lic.addView(text(l[1], 13, theme.subText));
        }
        Button notice = tonalButton("SudachiDict のライセンス文書");
        notice.setOnClickListener(v -> showSudachiLicense());
        lic.addView(notice);
    }

    private void showSudachiLicense() {
        try (BufferedReader reader = new BufferedReader(new InputStreamReader(
                getAssets().open("licenses/sudachidict.txt"), StandardCharsets.UTF_8))) {
            StringBuilder notice = new StringBuilder();
            String line;
            while ((line = reader.readLine()) != null) {
                notice.append(line).append('\n');
            }
            new android.app.AlertDialog.Builder(this)
                    .setTitle("SudachiDict")
                    .setMessage(notice.toString())
                    .setPositiveButton(android.R.string.ok, null)
                    .show();
        } catch (IOException e) {
            Toast.makeText(this, "ライセンス文書を開けませんでした", Toast.LENGTH_SHORT).show();
        }
    }

    private void open(String url) {
        try {
            startActivity(new Intent(Intent.ACTION_VIEW, Uri.parse(url)));
        } catch (android.content.ActivityNotFoundException e) {
            Toast.makeText(this, url, Toast.LENGTH_LONG).show();
        }
    }

    private String version() {
        try {
            String v = getPackageManager().getPackageInfo(getPackageName(), 0).versionName;
            return v != null ? v : "";
        } catch (Exception e) {
            return "";
        }
    }

    private void updateSummaries() {
        if (aiSummary != null) {
            aiSummary.setText(!zenzFile().exists() ? "モデル未インストール"
                    : (engine.get("neural") ? "オン・" : "オフ・") + modelName());
        }
        if (learningSummary != null) {
            learningSummary.setText((engine.get("learning") ? "学習オン" : "学習オフ")
                    + "・覚えた単語 " + Native.learnedCount(historyFile()) + " 件");
        }
    }

    private String modelName() {
        try {
            String n = new String(Files.readAllBytes(zenzNameFile().toPath()), StandardCharsets.UTF_8).trim();
            return n.isEmpty() ? "モデルあり" : n.substring(n.indexOf('/') + 1);
        } catch (IOException e) {
            return "モデルあり";
        }
    }

    // ---- Updates ----

    /** The latest release found by a check, or null. */
    private Updater.Release release;
    private boolean checking;
    /** Downloading or installing; cleared by the installer's status. */
    private boolean updating;
    private String updateProgress;
    private String updateError;
    private TextView updateText;
    private TextView updateNotes;
    private Button updateButton;
    private Button updateSkip;
    private LinearLayout homeBanner;
    private TextView homeBannerText;

    /** The アップデート card of the about page. */
    private void updateSection() {
        LinearLayout c = card("アップデート");
        updateText = text("", 16, theme.text);
        updateText.setPadding(0, dp(4), 0, dp(6));
        c.addView(updateText);
        updateNotes = text("", 13, theme.subText);
        updateNotes.setPadding(0, 0, 0, dp(10));
        c.addView(updateNotes);
        updateButton = button("");
        c.addView(updateButton);
        updateSkip = tonalButton("このバージョンをスキップ");
        updateSkip.setOnClickListener(v -> skipRelease());
        c.addView(updateSkip);
        c.addView(toggle("アップデートを自動で確認",
                "1 日に 1 回 GitHub で新しいバージョンを確認し、見つかったら通知します。",
                Updater.autoCheck(this), on -> {
                    Prefs.keyboard(this).edit().putBoolean(Updater.AUTO_CHECK, on).apply();
                    Updater.schedule(this);
                    if (on) askNotifications();
                }));
    }

    private boolean newer() {
        return release != null && release.apkUrl != null
                && Updater.isNewer(release.version, Updater.current(this));
    }

    /** Shows the update state on the about card and the home banner. */
    private void updateUpdate() {
        String current = Updater.current(this);
        boolean newer = newer();
        if (updateText != null) {
            updateText.setText(updating ? updateProgress
                    : checking ? "確認しています…"
                    : updateError != null ? updateError
                    : newer ? "新しいバージョン " + release.version + " があります(今は " + current + ")"
                    : release != null ? "最新版です(" + current + ")"
                    : "今のバージョン: " + current);
            String notes = newer && !updating ? release.notes : "";
            if (notes.length() > 800) notes = notes.substring(0, 800) + "…";
            updateNotes.setText(notes);
            updateNotes.setVisibility(notes.isEmpty() ? View.GONE : View.VISIBLE);
            updateButton.setText(newer ? "ダウンロードして更新" : "アップデートを確認");
            updateButton.setEnabled(!checking && !updating);
            updateButton.setAlpha(checking || updating ? 0.5f : 1f);
            updateButton.setOnClickListener(v -> {
                if (newer()) startUpdate();
                else checkUpdate(false);
            });
            updateSkip.setVisibility(newer && !updating && !release.version.equals(Updater.skipped(this))
                    ? View.VISIBLE : View.GONE);
        }
        if (homeBanner != null) {
            boolean offer = release != null && Updater.offer(this, release) && !updating;
            homeBanner.setVisibility(offer ? View.VISIBLE : View.GONE);
            if (offer) homeBannerText.setText("Sweet IME " + release.version + " が出ています");
        }
    }

    /** Asks GitHub for the latest release; `quiet` keeps failures to itself. */
    private void checkUpdate(boolean quiet) {
        if (checking || updating) return;
        checking = true;
        if (!quiet) updateError = null;
        updateUpdate();
        new Thread(() -> {
            Updater.Release found = null;
            String error = null;
            try {
                found = Updater.fetchLatest();
                Updater.checked(this);
            } catch (Exception e) {
                error = Updater.describe(e);
            }
            final Updater.Release r = found;
            final String failed = error;
            runOnUiThread(() -> {
                checking = false;
                if (r != null) release = r;
                if (!quiet && failed != null) updateError = "確認できませんでした: " + failed;
                updateUpdate();
            });
        }, "sweet-ime-update-check").start();
    }

    private void skipRelease() {
        if (release == null) return;
        Updater.skip(this, release.version);
        updateUpdate();
    }

    /** Downloads the new APK and hands it to the system installer. */
    private void startUpdate() {
        if (release == null || release.apkUrl == null || updating) return;
        if (!getPackageManager().canRequestPackageInstalls()) {
            new android.app.AlertDialog.Builder(this)
                    .setTitle("インストールの許可が必要です")
                    .setMessage("更新をインストールするには、Sweet IME に「不明なアプリのインストール」を"
                            + "許可してください。許可したら、もう一度「ダウンロードして更新」を押してください。")
                    .setPositiveButton("設定を開く", (d, w) -> startActivity(new Intent(
                            Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:" + getPackageName()))))
                    .setNegativeButton("キャンセル", null)
                    .show();
            return;
        }
        Updater.Release r = release;
        updating = true;
        updateError = null;
        updateProgress = "ダウンロードを準備しています…";
        updateUpdate();
        new Thread(() -> {
            String error = null;
            try {
                long[] shownMb = {-1};
                File apk = Updater.download(this, r, (done, total) -> {
                    long mb = done >> 20;
                    if (mb == shownMb[0]) return;
                    shownMb[0] = mb;
                    String t = "ダウンロード中… " + mb + (total > 0 ? " / " + (total >> 20) : "") + " MB";
                    runOnUiThread(() -> {
                        updateProgress = t;
                        updateUpdate();
                    });
                });
                runOnUiThread(() -> {
                    updateProgress = "インストールしています…";
                    updateUpdate();
                });
                Updater.install(this, apk);
            } catch (Exception e) {
                error = Updater.describe(e);
            }
            final String failed = error;
            runOnUiThread(() -> {
                if (failed != null) {
                    updating = false;
                    updateError = "更新できませんでした: " + failed;
                }
                updateUpdate();
            });
        }, "sweet-ime-update").start();
    }

    /** Handles the installer's answer; returns whether `intent` was one. */
    @SuppressWarnings("deprecation")
    private boolean handleInstallStatus(Intent intent) {
        if (intent == null || !Updater.ACTION_INSTALL_STATUS.equals(intent.getAction())) return false;
        // Not again when the activity is recreated.
        setIntent(new Intent(this, SettingsActivity.class));
        int s = intent.getIntExtra(android.content.pm.PackageInstaller.EXTRA_STATUS,
                android.content.pm.PackageInstaller.STATUS_FAILURE);
        if (s == android.content.pm.PackageInstaller.STATUS_PENDING_USER_ACTION) {
            Intent confirm = intent.getParcelableExtra(Intent.EXTRA_INTENT);
            if (confirm != null) {
                updating = true;
                updateProgress = "インストールの確認を待っています…";
                startActivity(confirm);
            }
            updateUpdate();
            return true;
        }
        updating = false;
        String msg = intent.getStringExtra(android.content.pm.PackageInstaller.EXTRA_STATUS_MESSAGE);
        switch (s) {
            case android.content.pm.PackageInstaller.STATUS_SUCCESS:
                updateError = null;
                Toast.makeText(this, "更新しました", Toast.LENGTH_SHORT).show();
                break;
            case android.content.pm.PackageInstaller.STATUS_FAILURE_ABORTED:
                updateError = "更新をキャンセルしました";
                break;
            case android.content.pm.PackageInstaller.STATUS_FAILURE_CONFLICT:
            case android.content.pm.PackageInstaller.STATUS_FAILURE_INCOMPATIBLE:
                updateError = "インストール済みのアプリと署名が違うため、更新できませんでした。"
                        + "GitHub のリリースから入れ直してください。";
                break;
            default:
                updateError = "更新できませんでした" + (msg != null ? ": " + msg : "");
                break;
        }
        if (!page.equals(ABOUT)) show(ABOUT);
        else updateUpdate();
        return true;
    }

    /** Asks once for notifications, which the daily check uses. */
    private void askNotificationsOnce() {
        if (!Updater.autoCheck(this) || Prefs.flag(this, "asked_notifications", false)) return;
        askNotifications();
    }

    private void askNotifications() {
        if (Build.VERSION.SDK_INT < 33) return;
        if (checkSelfPermission(android.Manifest.permission.POST_NOTIFICATIONS)
                == android.content.pm.PackageManager.PERMISSION_GRANTED) {
            return;
        }
        Prefs.keyboard(this).edit().putBoolean("asked_notifications", true).apply();
        requestPermissions(new String[] {android.Manifest.permission.POST_NOTIFICATIONS}, 1);
    }

    // ---- Conversion reports ----

    private static final String REPORTS_HEADER =
            "# Conversions reported in Sweet IME. Check them with `sweet-cli <dict> eval <this file>`.\n";
    private static final String ISSUE_URL = "https://github.com/frost-1256/sweet-ime/issues/new";
    private LinearLayout reportList;
    private Button reportSend;
    private Button reportClear;

    /** Same format as crates/engine/src/reports.rs: one evaluation row per report. */
    private File reportsFile() {
        return new File(new File(getFilesDir(), "sweet-ime"), "reports.tsv");
    }

    private static String reportLine(String[] r) {
        return "OK:\t" + r[0] + "\t\tConversion Expected\t" + r[1];
    }

    /** Reported conversions as {reading, wanted text}, oldest first. */
    private java.util.List<String[]> loadReports() {
        java.util.List<String[]> out = new java.util.ArrayList<>();
        File f = reportsFile();
        if (!f.exists()) return out;
        try {
            for (String line : Files.readAllLines(f.toPath(), StandardCharsets.UTF_8)) {
                if (line.startsWith("#")) continue;
                String[] p = line.split("\t", -1);
                if (p.length == 5 && p[3].equals("Conversion Expected")) out.add(new String[] {p[1], p[4]});
            }
        } catch (IOException e) {
            // Unreadable: shown as empty.
        }
        return out;
    }

    private boolean saveReports(java.util.List<String[]> reports) {
        File f = reportsFile();
        if (reports.isEmpty()) return !f.exists() || f.delete();
        File dir = f.getParentFile();
        if (dir != null && !dir.exists() && !dir.mkdirs()) return false;
        StringBuilder text = new StringBuilder(REPORTS_HEADER);
        for (String[] r : reports) text.append(reportLine(r)).append('\n');
        try {
            Files.write(f.toPath(), text.toString().getBytes(StandardCharsets.UTF_8));
            return true;
        } catch (IOException e) {
            return false;
        }
    }

    /** Conversions the user wrote down, and sending them as a GitHub issue. */
    private void reportsSection() {
        LinearLayout c = card("変換の報告");
        TextView about = text("うまく変換できなかった言葉の読みと、期待する変換を書いて追加してください。"
                + "報告は端末の中だけに保存され、「GitHub に送る」で開く画面で内容を確かめて送信するまで、"
                + "どこにも送られません。項目をタップすると消せます。",
                13, theme.subText);
        about.setPadding(0, 0, 0, dp(8));
        c.addView(about);
        EditText reading = reportField("読み(例: ほかにあんある)");
        EditText wanted = reportField("期待する変換(例: 他に案ある)");
        c.addView(reading);
        c.addView(wanted);
        Button add = tonalButton("追加");
        add.setOnClickListener(v -> {
            String r = reading.getText().toString().trim();
            String w = wanted.getText().toString().trim();
            if (!validReport(r) || !validReport(w)) {
                Toast.makeText(this, "読みと期待する変換を入力してください", Toast.LENGTH_SHORT).show();
                return;
            }
            java.util.List<String[]> now = loadReports();
            // The same reading again: the latest wish counts.
            now.removeIf(x -> x[0].equals(r));
            now.add(new String[] {r, w});
            if (!saveReports(now)) {
                Toast.makeText(this, "報告を保存できませんでした", Toast.LENGTH_SHORT).show();
                return;
            }
            reading.setText("");
            wanted.setText("");
            refreshReports();
        });
        c.addView(add);
        reportList = new LinearLayout(this);
        reportList.setOrientation(LinearLayout.VERTICAL);
        c.addView(reportList);
        reportSend = button("GitHub に送る");
        reportSend.setOnClickListener(v -> sendReports());
        c.addView(reportSend);
        reportClear = tonalButton("すべて消す");
        reportClear.setOnClickListener(v -> new android.app.AlertDialog.Builder(this)
                .setTitle("報告をすべて消しますか?")
                .setPositiveButton("消去", (dialog, w) -> {
                    saveReports(new java.util.ArrayList<>());
                    refreshReports();
                })
                .setNegativeButton("キャンセル", null)
                .show());
        c.addView(reportClear);
        refreshReports();
    }

    private EditText reportField(String hint) {
        EditText field = new EditText(this);
        field.setHint(hint);
        field.setSingleLine(true);
        field.setTextColor(theme.text);
        field.setHintTextColor(theme.subText);
        field.setBackgroundTintList(android.content.res.ColorStateList.valueOf(theme.subText));
        field.setFilters(new android.text.InputFilter[] {new android.text.InputFilter.LengthFilter(100)});
        return field;
    }

    /** One line of up to 100 characters, as crates/engine/src/reports.rs accepts. */
    private static boolean validReport(String s) {
        if (s.isEmpty() || s.codePointCount(0, s.length()) > 100) return false;
        return s.codePoints().noneMatch(Character::isISOControl);
    }

    private void refreshReports() {
        if (reportList == null) return;
        reportList.removeAllViews();
        java.util.List<String[]> reports = loadReports();
        if (reports.isEmpty()) {
            TextView none = text("まだ報告はありません", 15, theme.subText);
            none.setPadding(0, dp(4), 0, dp(8));
            reportList.addView(none);
        }
        for (int i = 0; i < reports.size(); i++) {
            String[] r = reports.get(i);
            int index = i;
            TextView row = text(r[0] + " → " + r[1], 15, theme.text);
            row.setPadding(0, dp(8), 0, dp(8));
            row.setOnClickListener(v -> new android.app.AlertDialog.Builder(this)
                    .setTitle("この報告を消しますか?")
                    .setMessage(r[0] + " → " + r[1])
                    .setPositiveButton("消去", (dialog, w) -> {
                        java.util.List<String[]> now = loadReports();
                        if (index < now.size()) now.remove(index);
                        saveReports(now);
                        refreshReports();
                    })
                    .setNegativeButton("キャンセル", null)
                    .show());
            reportList.addView(row);
        }
        reportSend.setEnabled(!reports.isEmpty());
        reportClear.setEnabled(!reports.isEmpty());
    }

    /** Opens a new GitHub issue with the reports; the user reviews it before submitting. */
    private void sendReports() {
        java.util.List<String[]> reports = loadReports();
        if (reports.isEmpty()) return;
        StringBuilder body = new StringBuilder("Sweet IME で報告した変換です(読み → 期待する変換)。\n\n```tsv\n");
        for (String[] r : reports) body.append(reportLine(r)).append('\n');
        body.append("```\n");
        open(ISSUE_URL + "?template=in_app_conversion_report.md&title="
                + urlEncode("変換の報告") + "&body=" + urlEncode(body.toString()));
    }

    private static String urlEncode(String s) {
        try {
            return java.net.URLEncoder.encode(s, "UTF-8");
        } catch (java.io.UnsupportedEncodingException e) {
            throw new AssertionError(e);
        }
    }

    // ---- Learning ----

    private TextView learnedStatus;

    private String historyFile() {
        return new File(new File(getFilesDir(), "sweet-ime"), "history.tsv").getAbsolutePath();
    }

    /** The conversion history: how much it holds, and ways to forget it. */
    private void learningSection() {
        LinearLayout data = card("学習データ");
        learnedStatus = text("", 16, theme.text);
        learnedStatus.setPadding(0, dp(4), 0, dp(10));
        data.addView(learnedStatus);
        Button hour = tonalButton("直近 1 時間の学習を消す");
        hour.setOnClickListener(v -> forget(3600, "直近 1 時間"));
        data.addView(hour);
        Button day = tonalButton("今日(24 時間)の学習を消す");
        day.setOnClickListener(v -> forget(24 * 3600, "直近 24 時間"));
        data.addView(day);
        Button all = button("すべて消去");
        all.setOnClickListener(v -> new android.app.AlertDialog.Builder(this)
                .setTitle("学習データをすべて消去しますか?")
                .setMessage("覚えた変換をすべて忘れます。元に戻せません。")
                .setPositiveButton("消去", (dialog, w) -> forget(0, null))
                .setNegativeButton("キャンセル", null)
                .show());
        data.addView(all);
        data.addView(text("うっかり覚えさせた変換を取り消せます。学習データはこの端末の中だけに保存されます。",
                13, theme.subText));
        updateLearned();
    }

    private void updateLearned() {
        if (learnedStatus != null) learnedStatus.setText("覚えた単語: " + Native.learnedCount(historyFile()) + " 件");
    }

    /** Forgets the last `secs` seconds of learning, or all of it if 0. */
    private void forget(long secs, String label) {
        int n = Native.forgetLearned(historyFile(), secs);
        String msg = n < 0 ? "消去できませんでした"
                : label == null ? "学習データを消去しました"
                : label + "に覚えた " + n + " 件を消しました";
        Toast.makeText(this, msg, Toast.LENGTH_SHORT).show();
        updateLearned();
    }

    // ---- AI conversion ----

    /** zenz models offered for download: Hugging Face id and label. */
    private static final String[][] MODELS = {
        {"Miwa-Keita/zenz-v3.1-xsmall", "xsmall(約 100 MB・速い)"},
        {"Miwa-Keita/zenz-v3.1-small", "small(約 380 MB・高精度・遅い)"},
    };
    private TextView aiStatus;
    private Button aiDownload;
    private Button aiRemove;
    private int aiModel;
    private volatile boolean downloading;

    private File zenzFile() {
        return new File(new File(getFilesDir(), "sweet-ime"), "zenz.swz");
    }

    private File zenzNameFile() {
        return new File(new File(getFilesDir(), "sweet-ime"), "zenz-model.txt");
    }

    /** AI conversion: the switch and the model download. */
    private void aiSection() {
        LinearLayout ai = card(null);
        ai.addView(toggle("AI 変換を使う",
                "文脈に合わせて、ニューラル言語モデル(zenz)で変換候補を選び直します。モデルのダウンロードが必要です。",
                engine.get("neural"), on -> saveEngine("neural", on)));
        LinearLayout model = card("モデル");
        aiStatus = text("", 16, theme.text);
        aiStatus.setPadding(0, dp(4), 0, dp(8));
        model.addView(aiStatus);
        RadioGroup group = new RadioGroup(this);
        for (int i = 0; i < MODELS.length; i++) {
            RadioButton rb = new RadioButton(this);
            rb.setText(MODELS[i][1]);
            rb.setTextColor(theme.text);
            rb.setButtonTintList(ColorStateList.valueOf(theme.accent));
            rb.setId(View.generateViewId());
            group.addView(rb);
            final int value = i;
            if (i == aiModel) rb.setChecked(true);
            rb.setOnCheckedChangeListener((b, checked) -> {
                if (checked) aiModel = value;
            });
        }
        model.addView(group);
        aiDownload = button("モデルをダウンロード");
        aiDownload.setOnClickListener(v -> startDownload(MODELS[aiModel][0]));
        LinearLayout.LayoutParams lp = (LinearLayout.LayoutParams) aiDownload.getLayoutParams();
        lp.topMargin = dp(8);
        model.addView(aiDownload);
        aiRemove = tonalButton("モデルを削除");
        aiRemove.setOnClickListener(v -> {
            if (!zenzFile().delete() && zenzFile().exists()) {
                Toast.makeText(this, "削除できませんでした", Toast.LENGTH_LONG).show();
            }
            zenzNameFile().delete();
            updateAi(null);
        });
        model.addView(aiRemove);
        model.addView(text("zenz モデル(作者: Miwa Keita、ライセンス: CC BY-SA 4.0)を Hugging Face から"
                + "ダウンロードして、この端末の中で変換します。モデルはアプリに含まれていません。"
                + "変換が正確になる代わりに、電池と処理時間を使います。", 13, theme.subText));
        updateAi(null);
    }

    /** Shows the installed model, or `progress` while downloading. */
    private void updateAi(String progress) {
        if (aiStatus == null) return;
        File f = zenzFile();
        if (progress != null) {
            aiStatus.setText(progress);
        } else if (f.exists()) {
            aiStatus.setText("✓ " + modelName() + " をインストール済み · " + (f.length() >> 20) + " MB");
        } else {
            aiStatus.setText("モデルはまだありません。下から選んでダウンロードしてください。");
        }
        aiDownload.setEnabled(!downloading);
        aiDownload.setAlpha(downloading ? 0.5f : 1f);
        aiDownload.setText(f.exists() ? "選んだモデルに入れ替える" : "モデルをダウンロード");
        aiRemove.setVisibility(f.exists() && !downloading ? View.VISIBLE : View.GONE);
    }

    /** Downloads `model` from Hugging Face and converts it on a background thread. */
    private void startDownload(String model) {
        if (downloading) return;
        downloading = true;
        updateAi("ダウンロードを準備しています…");
        new Thread(() -> {
            File dir = new File(getCacheDir(), "zenz-download");
            String error = null;
            try {
                if (!dir.exists() && !dir.mkdirs()) throw new IOException("mkdir " + dir);
                for (String name : new String[] {"config.json", "vocab.json", "merges.txt", "model.safetensors"}) {
                    download("https://huggingface.co/" + model + "/resolve/main/" + name, new File(dir, name), name);
                }
                runOnUiThread(() -> updateAi("モデルを変換しています…"));
                error = Native.packZenz(dir.getAbsolutePath(), zenzFile().getAbsolutePath());
                if (error == null) Files.write(zenzNameFile().toPath(), model.getBytes(StandardCharsets.UTF_8));
            } catch (java.net.UnknownHostException e) {
                error = "サーバーに接続できません。インターネット接続と、このアプリのネットワークアクセスが"
                        + "許可されているかを確認してください。";
            } catch (Exception e) {
                error = e.getMessage() != null ? e.getMessage() : e.toString();
            } finally {
                File[] left = dir.listFiles();
                if (left != null) {
                    for (File f : left) f.delete();
                }
                dir.delete();
            }
            final String failed = error;
            runOnUiThread(() -> {
                downloading = false;
                updateAi(null);
                updateSummaries();
                Toast.makeText(this, failed == null
                        ? "AI 変換の準備ができました。キーボードを次に表示したときから使われます。"
                        : "ダウンロードできませんでした: " + failed, Toast.LENGTH_LONG).show();
            });
        }, "sweet-ime-zenz-download").start();
    }

    private void download(String url, File out, String label) throws IOException {
        HttpURLConnection c = (HttpURLConnection) new URL(url).openConnection();
        c.setInstanceFollowRedirects(true);
        c.setConnectTimeout(20000);
        c.setReadTimeout(30000);
        try {
            int code = c.getResponseCode();
            if (code != 200) throw new IOException("HTTP " + code + " (" + label + ")");
            long total = c.getContentLengthLong();
            long done = 0;
            long shownMb = -1;
            try (InputStream in = c.getInputStream(); OutputStream o = new FileOutputStream(out)) {
                byte[] buf = new byte[1 << 16];
                int n;
                while ((n = in.read(buf)) > 0) {
                    o.write(buf, 0, n);
                    done += n;
                    long mb = done >> 20;
                    if (mb != shownMb) {
                        shownMb = mb;
                        String text = label + " をダウンロード中… " + mb
                                + (total > 0 ? " / " + (total >> 20) : "") + " MB";
                        runOnUiThread(() -> updateAi(text));
                    }
                }
            }
        } finally {
            c.disconnect();
        }
    }

    // ---- Setup state ----

    private void updateStatus() {
        if (status == null) return;
        InputMethodManager imm = (InputMethodManager) getSystemService(Context.INPUT_METHOD_SERVICE);
        boolean enabled = false;
        for (InputMethodInfo info : imm.getEnabledInputMethodList()) {
            if (info.getPackageName().equals(getPackageName())) enabled = true;
        }
        String current = Settings.Secure.getString(getContentResolver(), Settings.Secure.DEFAULT_INPUT_METHOD);
        boolean selected = current != null && current.startsWith(getPackageName() + "/");
        status.setText(selected ? "✓ Sweet IME を使っています"
                : enabled ? "Sweet IME は有効です。「キーボードを切り替える」で選んでください。"
                : "まず「キーボードを有効にする」で Sweet IME をオンにしてください。");
        // Only the next step stands out; switching stays available.
        enable.setVisibility(enabled ? View.GONE : View.VISIBLE);
        pick.setVisibility(enabled ? View.VISIBLE : View.GONE);
        pick.setBackground(pill(selected ? theme.specialKey : theme.accent));
        pick.setTextColor(selected ? theme.text : theme.onAccent);
    }

    // ---- Building blocks ----

    private TextView text(String s, int sp, int color) {
        TextView t = new TextView(this);
        t.setText(s);
        t.setTextSize(TypedValue.COMPLEX_UNIT_SP, sp);
        t.setTextColor(color);
        return t;
    }

    private GradientDrawable pill(int color) {
        GradientDrawable bg = new GradientDrawable();
        bg.setColor(color);
        bg.setCornerRadius(dp(22));
        return bg;
    }

    private Button button(String label) {
        Button b = new Button(this);
        b.setText(label);
        b.setAllCaps(false);
        b.setTextColor(theme.onAccent);
        b.setBackground(pill(theme.accent));
        b.setStateListAnimator(null);
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, dp(46));
        lp.bottomMargin = dp(8);
        b.setLayoutParams(lp);
        return b;
    }

    /** A quieter button for secondary actions. */
    private Button tonalButton(String label) {
        Button b = button(label);
        b.setTextColor(theme.text);
        b.setBackground(pill(theme.specialKey));
        return b;
    }

    /** A card added to the page, under `title` if given; returns its body. */
    private LinearLayout card(String title) {
        if (title != null) {
            TextView t = text(title, 14, theme.accent);
            t.setTypeface(Typeface.DEFAULT_BOLD);
            t.setPadding(dp(4), dp(20), 0, dp(8));
            box.addView(t);
        }
        LinearLayout card = new LinearLayout(this);
        card.setOrientation(LinearLayout.VERTICAL);
        card.setPadding(dp(16), dp(12), dp(16), dp(12));
        GradientDrawable bg = new GradientDrawable();
        bg.setColor(theme.key);
        bg.setCornerRadius(dp(20));
        card.setBackground(bg);
        card.setClipToOutline(true);
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT);
        if (title == null) lp.topMargin = dp(16);
        box.addView(card, lp);
        return card;
    }

    /** A category page's top bar: back and the title. */
    private void header(String title) {
        LinearLayout bar = new LinearLayout(this);
        bar.setOrientation(LinearLayout.HORIZONTAL);
        bar.setGravity(Gravity.CENTER_VERTICAL);
        bar.setPadding(0, dp(8), 0, 0);
        View back = icon(Icons.BACK, theme.text, 24);
        back.setContentDescription("戻る");
        back.setOnClickListener(v -> show(HOME));
        back.setBackground(new RippleDrawable(ColorStateList.valueOf(theme.pressed), null, null));
        bar.addView(back, new LinearLayout.LayoutParams(dp(48), dp(48)));
        TextView t = text(title, 24, theme.text);
        t.setTypeface(Typeface.DEFAULT_BOLD);
        t.setPadding(dp(8), 0, 0, 0);
        bar.addView(t);
        box.addView(bar);
    }

    /** A row opening category `target`; returns its summary line. */
    private TextView entry(LinearLayout list, int iconId, String title, String summary, String target) {
        LinearLayout row = new LinearLayout(this);
        row.setOrientation(LinearLayout.HORIZONTAL);
        row.setGravity(Gravity.CENTER_VERTICAL);
        row.setPadding(dp(16), dp(10), dp(12), dp(10));
        row.setBackground(ripple());
        View ic = icon(iconId, theme.accent, 22);
        ic.setBackground(roundRect(theme.specialKey, 14));
        row.addView(ic, new LinearLayout.LayoutParams(dp(44), dp(44)));
        LinearLayout texts = new LinearLayout(this);
        texts.setOrientation(LinearLayout.VERTICAL);
        texts.setPadding(dp(16), 0, dp(8), 0);
        texts.addView(text(title, 16, theme.text));
        TextView sub = text(summary, 13, theme.subText);
        texts.addView(sub);
        row.addView(texts, new LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1));
        row.addView(icon(Icons.RIGHT, theme.subText, 20), new LinearLayout.LayoutParams(dp(24), dp(24)));
        row.setOnClickListener(v -> show(target));
        list.addView(row);
        return sub;
    }

    private GradientDrawable roundRect(int color, int radiusDp) {
        GradientDrawable bg = new GradientDrawable();
        bg.setColor(color);
        bg.setCornerRadius(dp(radiusDp));
        return bg;
    }

    /** A view drawing one of the line icons, centered. */
    private View icon(int id, int color, int sizeDp) {
        Icons icons = new Icons();
        float size = dp(sizeDp);
        return new View(this) {
            private final RectF r = new RectF();

            @Override
            protected void onDraw(Canvas c) {
                r.set(0, 0, getWidth(), getHeight());
                icons.draw(c, id, r, size, color);
            }
        };
    }

    private Drawable ripple() {
        return new RippleDrawable(ColorStateList.valueOf(theme.pressed), null, new ColorDrawable(0xffffffff));
    }

    private interface OnToggle {
        void changed(boolean on);
    }

    /** A switch for engine option `key`, described in Prefs.Engine.SWITCHES. */
    private void engineToggle(LinearLayout parent, String key) {
        for (String[] sw : Prefs.Engine.SWITCHES) {
            if (!sw[0].equals(key)) continue;
            parent.addView(toggle(sw[1], sw[2], engine.get(key), on -> saveEngine(key, on)));
            return;
        }
        throw new IllegalArgumentException("no engine switch " + key);
    }

    private void saveEngine(String key, boolean on) {
        engine.set(key, on);
        try {
            engine.save();
        } catch (IOException e) {
            Toast.makeText(this, "保存できませんでした: " + e.getMessage(), Toast.LENGTH_LONG).show();
        }
    }

    private View toggle(String title, String desc, boolean on, OnToggle onToggle) {
        LinearLayout row = new LinearLayout(this);
        row.setOrientation(LinearLayout.HORIZONTAL);
        row.setGravity(Gravity.CENTER_VERTICAL);
        row.setPadding(0, dp(10), 0, dp(10));
        LinearLayout texts = new LinearLayout(this);
        texts.setOrientation(LinearLayout.VERTICAL);
        texts.setPadding(0, 0, dp(12), 0);
        texts.addView(text(title, 16, theme.text));
        texts.addView(text(desc, 13, theme.subText));
        row.addView(texts, new LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1));
        Switch sw = new Switch(this);
        sw.setChecked(on);
        int[][] states = {{android.R.attr.state_checked}, {}};
        // Material 3 style: a solid pill track and a round thumb.
        GradientDrawable track = new GradientDrawable();
        track.setCornerRadius(dp(16));
        track.setSize(dp(52), dp(32));
        track.setColor(new ColorStateList(states, new int[] {theme.accent, theme.specialKey}));
        GradientDrawable knob = new GradientDrawable();
        knob.setShape(GradientDrawable.OVAL);
        knob.setSize(dp(22), dp(22));
        knob.setColor(new ColorStateList(states, new int[] {theme.onAccent, theme.subText}));
        sw.setTrackDrawable(track);
        sw.setThumbDrawable(new android.graphics.drawable.InsetDrawable(knob, dp(5)));
        sw.setShowText(false);
        sw.setOnCheckedChangeListener((b, checked) -> {
            onToggle.changed(checked);
            updateSummaries();
        });
        row.addView(sw);
        row.setOnClickListener(v -> sw.toggle());
        return row;
    }

    private View prefToggle(String title, String desc, String key, boolean def) {
        return toggle(title, desc, Prefs.flag(this, key, def),
                on -> Prefs.keyboard(this).edit().putBoolean(key, on).apply());
    }

    private View choice(String title, String[] labels, String key, int def) {
        LinearLayout col = new LinearLayout(this);
        col.setOrientation(LinearLayout.VERTICAL);
        col.setPadding(0, dp(10), 0, dp(6));
        col.addView(text(title, 16, theme.text));
        RadioGroup group = new RadioGroup(this);
        group.setOrientation(RadioGroup.HORIZONTAL);
        int current = Prefs.number(this, key, def);
        for (int i = 0; i < labels.length; i++) {
            RadioButton rb = new RadioButton(this);
            rb.setText(labels[i]);
            rb.setTextColor(theme.text);
            rb.setButtonTintList(ColorStateList.valueOf(theme.accent));
            rb.setId(View.generateViewId());
            final int value = i;
            group.addView(rb, new RadioGroup.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1));
            if (i == current) rb.setChecked(true);
            rb.setOnCheckedChangeListener((b, checked) -> {
                if (checked) Prefs.keyboard(this).edit().putInt(key, value).apply();
            });
        }
        col.addView(group);
        return col;
    }
}
