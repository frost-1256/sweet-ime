package io.github.sweetime;

import android.Manifest;
import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.job.JobInfo;
import android.app.job.JobScheduler;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.content.SharedPreferences;
import android.content.pm.PackageInstaller;
import android.content.pm.PackageManager;
import android.os.Build;

import org.json.JSONArray;
import org.json.JSONObject;

import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;

/**
 * Updates from GitHub Releases, as the desktop settings app does: a daily
 * check (with a notification), then the APK is downloaded, checked against
 * the size and SHA-256 GitHub publishes, and handed to the system installer.
 */
final class Updater {
    static final String REPO = "frost-1256/sweet-ime";
    static final String APK = "SweetIME-Android.apk";
    /** Keyboard preference: check automatically (on by default, as on the desktop). */
    static final String AUTO_CHECK = "update_check";
    /** Sent back to SettingsActivity by the installer session. */
    static final String ACTION_INSTALL_STATUS = "io.github.sweetime.INSTALL_STATUS";

    private static final int JOB_ID = 1;
    private static final String CHANNEL = "update";
    private static final long CHECK_EVERY_MS = 20L * 3600 * 1000;

    private Updater() {}

    /** A published release. */
    static final class Release {
        /** Without the leading v. */
        final String version;
        final String notes;
        final String page;
        /** The APK asset, or null if the release has none. */
        final String apkUrl;
        final long apkSize;
        /** Lowercase hex, or null when GitHub gives no digest. */
        final String apkSha256;

        Release(String version, String notes, String page, String apkUrl, long apkSize, String apkSha256) {
            this.version = version;
            this.notes = notes;
            this.page = page;
            this.apkUrl = apkUrl;
            this.apkSize = apkSize;
            this.apkSha256 = apkSha256;
        }
    }

    // ---- Versions ----

    static String current(Context c) {
        try {
            String v = c.getPackageManager().getPackageInfo(c.getPackageName(), 0).versionName;
            return v != null ? v : "";
        } catch (PackageManager.NameNotFoundException e) {
            return "";
        }
    }

    /** `1.2.3` as numbers; a pre-release suffix is ignored. */
    private static long[] parse(String v) {
        v = v.trim();
        if (v.startsWith("v")) v = v.substring(1);
        String core = v.split("[-+]", 2)[0];
        String[] parts = core.split("\\.");
        long[] out = new long[Math.max(parts.length, 3)];
        for (int i = 0; i < parts.length; i++) {
            try {
                out[i] = Long.parseLong(parts[i]);
            } catch (NumberFormatException e) {
                out[i] = 0;
            }
        }
        return out;
    }

    static boolean isNewer(String latest, String current) {
        long[] a = parse(latest);
        long[] b = parse(current);
        for (int i = 0; i < Math.max(a.length, b.length); i++) {
            long x = i < a.length ? a[i] : 0;
            long y = i < b.length ? b[i] : 0;
            if (x != y) return x > y;
        }
        return false;
    }

    // ---- Check ----

    /** Fetches the latest (non-pre-release) release. Blocks; not on the UI thread. */
    static Release fetchLatest() throws IOException {
        HttpURLConnection c = (HttpURLConnection) new URL(
                "https://api.github.com/repos/" + REPO + "/releases/latest").openConnection();
        c.setConnectTimeout(20000);
        c.setReadTimeout(20000);
        c.setRequestProperty("Accept", "application/vnd.github+json");
        c.setRequestProperty("User-Agent", "sweet-ime-updater");
        try {
            int code = c.getResponseCode();
            if (code != 200) throw new IOException("HTTP " + code);
            ByteArrayOutputStream body = new ByteArrayOutputStream();
            try (InputStream in = c.getInputStream()) {
                byte[] buf = new byte[1 << 14];
                int n;
                while ((n = in.read(buf)) > 0) body.write(buf, 0, n);
            }
            return parse(new JSONObject(body.toString("UTF-8")));
        } catch (org.json.JSONException e) {
            throw new IOException("応答を読めません: " + e.getMessage());
        } finally {
            c.disconnect();
        }
    }

    private static Release parse(JSONObject v) throws org.json.JSONException {
        String tag = v.getString("tag_name");
        String prefix = "https://github.com/" + REPO + "/releases/download/";
        String url = null;
        long size = 0;
        String sha = null;
        JSONArray assets = v.optJSONArray("assets");
        for (int i = 0; assets != null && i < assets.length(); i++) {
            JSONObject a = assets.getJSONObject(i);
            String u = a.optString("browser_download_url", "");
            if (!APK.equals(a.optString("name")) || !u.startsWith(prefix)) continue;
            url = u;
            size = a.optLong("size", 0);
            String digest = a.optString("digest", "");
            if (digest.startsWith("sha256:")) sha = digest.substring(7).toLowerCase(java.util.Locale.ROOT);
        }
        return new Release(tag.startsWith("v") ? tag.substring(1) : tag, plain(v.optString("body", "")),
                v.optString("html_url", ""), url, size, sha);
    }

    /**
     * Release notes (Markdown) as plain text: headings and list items stay,
     * emphasis marks go, and tables and the changelog link are dropped.
     */
    static String plain(String markdown) {
        StringBuilder out = new StringBuilder();
        boolean blank = true;
        for (String line : markdown.replace("\r", "").split("\n")) {
            String t = line.trim();
            if (t.startsWith("|") || t.startsWith("**Full Changelog**")) continue;
            if (t.isEmpty()) {
                if (!blank) out.append('\n');
                blank = true;
                continue;
            }
            t = t.replaceAll("^#+\\s*", "").replaceAll("^[-*]\\s+", "・").replace("**", "").replace("`", "");
            if (t.isEmpty()) continue;
            out.append(t).append('\n');
            blank = false;
        }
        return out.toString().trim();
    }

    // ---- Remembered state ----

    private static SharedPreferences state(Context c) {
        return c.getSharedPreferences("update", Context.MODE_PRIVATE);
    }

    static boolean autoCheck(Context c) {
        return Prefs.flag(c, AUTO_CHECK, true);
    }

    static boolean due(Context c) {
        return System.currentTimeMillis() - state(c).getLong("last_check", 0) >= CHECK_EVERY_MS;
    }

    static void checked(Context c) {
        state(c).edit().putLong("last_check", System.currentTimeMillis()).apply();
    }

    /** The version the user chose to skip, or "". */
    static String skipped(Context c) {
        return state(c).getString("skip", "");
    }

    static void skip(Context c, String version) {
        state(c).edit().putString("skip", version).apply();
    }

    /** Whether `r` is worth offering: newer, and not skipped. */
    static boolean offer(Context c, Release r) {
        return r.apkUrl != null && isNewer(r.version, current(c)) && !r.version.equals(skipped(c));
    }

    // ---- Daily check ----

    /** Schedules (or cancels) the daily check to follow the setting. */
    static void schedule(Context c) {
        JobScheduler js = c.getSystemService(JobScheduler.class);
        if (js == null) return;
        if (!autoCheck(c)) {
            js.cancel(JOB_ID);
            return;
        }
        if (js.getPendingJob(JOB_ID) != null) return;
        try {
            js.schedule(new JobInfo.Builder(JOB_ID, new ComponentName(c, UpdateJob.class))
                    // The check is one small request; the download waits for the user.
                    .setRequiredNetworkType(JobInfo.NETWORK_TYPE_ANY)
                    .setPeriodic(CHECK_EVERY_MS)
                    .setPersisted(true)
                    .build());
        } catch (RuntimeException e) {
            // Only the background check is lost; the settings still check.
            android.util.Log.w("SweetIME", "update check not scheduled", e);
        }
    }

    /** Tells the user about `r` in a notification that opens the settings. */
    static void notifyAvailable(Context c, Release r) {
        if (Build.VERSION.SDK_INT >= 33 && c.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS)
                != PackageManager.PERMISSION_GRANTED) {
            return;
        }
        NotificationManager nm = c.getSystemService(NotificationManager.class);
        nm.createNotificationChannel(new NotificationChannel(CHANNEL, "アップデート",
                NotificationManager.IMPORTANCE_DEFAULT));
        Intent open = new Intent(c, SettingsActivity.class).putExtra("page", "about");
        PendingIntent pi = PendingIntent.getActivity(c, 0, open,
                PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE);
        nm.notify(JOB_ID, new Notification.Builder(c, CHANNEL)
                .setSmallIcon(android.R.drawable.stat_sys_download_done)
                .setContentTitle("Sweet IME " + r.version + " があります")
                .setContentText("タップして更新します")
                .setContentIntent(pi)
                .setAutoCancel(true)
                .build());
    }

    // ---- Download and install ----

    interface Progress {
        void onProgress(long done, long total);
    }

    /** Downloads the APK of `r` and checks its size and hash. Blocks. */
    static File download(Context c, Release r, Progress progress) throws IOException {
        File dir = new File(c.getCacheDir(), "update");
        File[] old = dir.listFiles();
        if (old != null) {
            for (File f : old) f.delete();
        }
        if (!dir.exists() && !dir.mkdirs()) throw new IOException("mkdir " + dir);
        File out = new File(dir, "SweetIME-Android-" + r.version + ".apk");
        HttpURLConnection conn = (HttpURLConnection) new URL(r.apkUrl).openConnection();
        conn.setInstanceFollowRedirects(true);
        conn.setConnectTimeout(20000);
        conn.setReadTimeout(30000);
        MessageDigest sha;
        try {
            sha = MessageDigest.getInstance("SHA-256");
        } catch (NoSuchAlgorithmException e) {
            throw new IOException(e);
        }
        long done = 0;
        try {
            int code = conn.getResponseCode();
            if (code != 200) throw new IOException("HTTP " + code);
            long total = r.apkSize > 0 ? r.apkSize : conn.getContentLengthLong();
            try (InputStream in = conn.getInputStream(); OutputStream o = new FileOutputStream(out)) {
                byte[] buf = new byte[1 << 16];
                int n;
                while ((n = in.read(buf)) > 0) {
                    o.write(buf, 0, n);
                    sha.update(buf, 0, n);
                    done += n;
                    progress.onProgress(done, total);
                }
            }
        } finally {
            conn.disconnect();
        }
        if (r.apkSize > 0 && done != r.apkSize) {
            out.delete();
            throw new IOException("ダウンロードしたファイルのサイズが違います(" + done + " / " + r.apkSize + " バイト)");
        }
        if (r.apkSha256 != null) {
            StringBuilder hex = new StringBuilder();
            for (byte b : sha.digest()) hex.append(String.format("%02x", b));
            if (!hex.toString().equals(r.apkSha256)) {
                out.delete();
                throw new IOException("ダウンロードしたファイルのハッシュが一致しません");
            }
        }
        return out;
    }

    /**
     * Hands `apk` to the system installer. Its status (including the
     * confirmation to show) comes back to SettingsActivity as
     * ACTION_INSTALL_STATUS. Blocks while the APK is copied.
     */
    static void install(Context c, File apk) throws IOException {
        PackageInstaller pi = c.getPackageManager().getPackageInstaller();
        PackageInstaller.SessionParams params =
                new PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL);
        params.setAppPackageName(c.getPackageName());
        if (Build.VERSION.SDK_INT >= 31) {
            // Updates without asking when this app installed itself last time.
            params.setRequireUserAction(PackageInstaller.SessionParams.USER_ACTION_NOT_REQUIRED);
        }
        int id = pi.createSession(params);
        try (PackageInstaller.Session s = pi.openSession(id)) {
            try (InputStream in = new FileInputStream(apk);
                    OutputStream out = s.openWrite("base.apk", 0, apk.length())) {
                byte[] buf = new byte[1 << 16];
                int n;
                while ((n = in.read(buf)) > 0) out.write(buf, 0, n);
                s.fsync(out);
            }
            Intent status = new Intent(c, SettingsActivity.class).setAction(ACTION_INSTALL_STATUS);
            PendingIntent p = PendingIntent.getActivity(c, id, status,
                    PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_MUTABLE);
            s.commit(p.getIntentSender());
        } catch (IOException | RuntimeException e) {
            pi.abandonSession(id);
            throw e;
        }
    }

    static String describe(Exception e) {
        if (e instanceof java.net.UnknownHostException) {
            return "サーバーに接続できません。インターネット接続と、このアプリのネットワークアクセスが"
                    + "許可されているかを確認してください。";
        }
        return e.getMessage() != null ? e.getMessage() : e.toString();
    }
}
