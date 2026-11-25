package io.github.sweetime;

import android.os.SystemClock;

import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.net.URLEncoder;
import java.nio.charset.StandardCharsets;
import java.util.LinkedHashSet;

/** Fetches face-mark candidates for a reading explicitly submitted from the keyboard. */
final class SimejiCloud {
    private static final String ENDPOINT =
            "https://cloud.simeji.me/py?ol=1&switch=2&section=0&ver=10.7&api_version=2&web=1&py=";
    private static final int TIMEOUT_MS = 3000;
    private static final int MAX_RESPONSE_BYTES = 1024 * 1024;
    private static final int MAX_CANDIDATES = 12;

    private SimejiCloud() {}

    static String lookup(String reading) throws IOException, JSONException {
        if (Thread.currentThread().isInterrupted()) throw new IOException("Lookup cancelled");
        URL url = new URL(ENDPOINT + URLEncoder.encode(reading, StandardCharsets.UTF_8.name()));
        HttpURLConnection connection = (HttpURLConnection) url.openConnection();
        connection.setConnectTimeout(TIMEOUT_MS);
        connection.setReadTimeout(TIMEOUT_MS);
        connection.setInstanceFollowRedirects(false);
        connection.setUseCaches(false);
        connection.setRequestProperty("Accept", "application/json");
        long deadline = SystemClock.elapsedRealtime() + TIMEOUT_MS * 2L;
        try {
            if (connection.getResponseCode() != HttpURLConnection.HTTP_OK) {
                throw new IOException("Simeji request failed");
            }
            try (InputStream in = connection.getInputStream()) {
                ByteArrayOutputStream body = new ByteArrayOutputStream();
                byte[] buffer = new byte[4096];
                for (int n; (n = in.read(buffer)) != -1; ) {
                    if (body.size() + n > MAX_RESPONSE_BYTES
                            || SystemClock.elapsedRealtime() >= deadline
                            || Thread.currentThread().isInterrupted()) {
                        throw new IOException("Simeji response limit exceeded");
                    }
                    body.write(buffer, 0, n);
                }
                return parse(reading, body.toString(StandardCharsets.UTF_8.name()));
            }
        } finally {
            connection.disconnect();
        }
    }

    private static String parse(String reading, String response) throws JSONException {
        JSONArray data = new JSONObject(response).optJSONArray("data");
        LinkedHashSet<String> words = new LinkedHashSet<>();
        for (int i = 0; data != null && i < data.length() && words.size() < MAX_CANDIDATES; i++) {
            JSONObject group = data.optJSONObject(i);
            if (group == null || !reading.equals(group.optString("hira"))) continue;
            JSONArray candidates = group.optJSONArray("candidates");
            for (int j = 0; candidates != null && j < candidates.length()
                    && words.size() < MAX_CANDIDATES; j++) {
                JSONObject candidate = candidates.optJSONObject(j);
                if (candidate == null) continue;
                int type = candidate.optInt("type");
                Object value = candidate.opt("word");
                if (!(value instanceof String)) continue;
                String word = (String) value;
                if ((type == 9 || type == 10) && !word.trim().isEmpty() && word.length() <= 160
                        && word.codePoints().noneMatch(Character::isISOControl)) {
                    words.add(word);
                }
            }
        }
        return String.join("\u001e", words);
    }
}
