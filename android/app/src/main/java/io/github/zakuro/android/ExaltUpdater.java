package io.github.zakuro.android;

import android.app.PendingIntent;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.content.pm.PackageInfo;
import android.content.pm.PackageInstaller;
import android.net.Uri;
import android.provider.Settings;
import java.io.File;
import java.io.FileInputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.util.Locale;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import org.json.JSONArray;
import org.json.JSONObject;

/**
 * ExaltDS updates from its newest GitHub release (the repository named by res/values/exalt.xml's exalt_repo). The check
 * asks GitHub's public API for the latest release and nothing else; the APK is only downloaded after the player says
 * yes. A download must match the SHA-256 the release notes give (when they give one), be ExaltDS's package and carry a
 * higher version code; Android then installs it only if it is signed with the same key as the installed app.
 */
final class ExaltUpdater {
    private static final long MAX_APK = 400L << 20; // the clang kit makes the APK large
    private static final String RESULT = "io.github.exaltds.UPDATE_RESULT";

    interface Progress { void say(String message); }

    static final class Release {
        String tag, apkUrl, sha256, notes;
        long size;
    }

    private ExaltUpdater() { }

    /** owner/name, or "" in a build without a repository (no update checks at all). */
    static String repo(Context c) {
        int id = c.getResources().getIdentifier("exalt_repo", "string", c.getPackageName());
        return id == 0 ? "" : c.getString(id).trim();
    }

    static boolean available(Context c) { return repo(c).matches("[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+"); }

    static String currentVersion(Context c) {
        try { return c.getPackageManager().getPackageInfo(c.getPackageName(), 0).versionName; } catch (Exception e) { return "0"; }
    }

    /** The latest published release. Blocking: call it from a worker thread. */
    static Release latest(Context c) throws IOException {
        String repo = repo(c), downloads = "https://github.com/" + repo + "/releases/download/";
        Release r = new Release();
        try {
            JSONObject rel = new JSONObject(get("https://api.github.com/repos/" + repo + "/releases/latest", "application/vnd.github+json"));
            r.tag = rel.getString("tag_name");
            r.notes = rel.optString("body", "");
            JSONArray assets = rel.getJSONArray("assets");
            for (int i = 0; i < assets.length(); i++) {
                JSONObject a = assets.getJSONObject(i);
                String url = a.getString("browser_download_url");
                if (a.getString("name").toLowerCase(Locale.ROOT).endsWith(".apk") && url.startsWith(downloads) && r.apkUrl == null) {
                    r.apkUrl = url;
                    r.size = a.optLong("size", 0);
                }
            }
        } catch (org.json.JSONException e) {
            throw new IOException("GitHub's answer could not be read");
        }
        if (r.apkUrl == null) throw new IOException("the latest release has no APK");
        Matcher m = Pattern.compile("(?i)sha-?256\\W{0,6}([0-9a-f]{64})").matcher(r.notes);
        if (m.find()) r.sha256 = m.group(1).toLowerCase(Locale.ROOT);
        return r;
    }

    /** True when tag ("v0.1.1") is a higher version than current ("0.1.0"); a tag without a version never is. */
    static boolean newer(String tag, String current) {
        int[] a = numbers(tag), b = numbers(current);
        if (a == null || b == null) return false;
        for (int i = 0; i < Math.max(a.length, b.length); i++) {
            int x = i < a.length ? a[i] : 0, y = i < b.length ? b[i] : 0;
            if (x != y) return x > y;
        }
        return false;
    }

    private static int[] numbers(String v) {
        Matcher m = Pattern.compile("(\\d+(?:\\.\\d+)*)").matcher(v == null ? "" : v);
        if (!m.find()) return null;
        String[] parts = m.group(1).split("\\.");
        int[] out = new int[parts.length];
        try { for (int i = 0; i < parts.length; i++) out[i] = Integer.parseInt(parts[i]); } catch (NumberFormatException e) { return null; }
        return out;
    }

    /** Downloads and checks the release's APK. Blocking. */
    static File download(Context c, Release r, Progress p) throws IOException {
        File dir = new File(c.getCacheDir(), "update");
        GameImport.mkdir(dir);
        File apk = new File(dir, "ExaltDS-" + r.tag.replaceAll("[^A-Za-z0-9._-]", "_") + ".apk"), part = new File(dir, apk.getName() + ".part");
        File[] old = dir.listFiles();
        if (old != null) for (File f : old) if (!f.equals(apk)) f.delete();
        if (apk.isFile() && verifies(c, apk, r)) return apk;
        HttpURLConnection conn = open(r.apkUrl, "application/octet-stream");
        long total = conn.getContentLengthLong(), got = 0, shown = 0;
        if (total > MAX_APK) { conn.disconnect(); throw new IOException("the download is too large"); }
        try (InputStream in = conn.getInputStream(); OutputStream out = new java.io.FileOutputStream(part)) {
            byte[] buffer = new byte[1 << 16];
            for (int n; (n = in.read(buffer)) > 0; ) {
                out.write(buffer, 0, n);
                if ((got += n) > MAX_APK) throw new IOException("the download is too large");
                long now = System.currentTimeMillis();
                if (now - shown > 300) { shown = now; p.say("Downloading " + r.tag + "… " + (total > 0 ? got * 100 / total + "%" : (got >> 20) + " MB")); }
            }
        } catch (IOException e) {
            part.delete();
            throw e;
        } finally {
            conn.disconnect();
        }
        if (!part.renameTo(apk)) throw new IOException("the download could not be stored");
        if (!verifies(c, apk, r)) { apk.delete(); throw new IOException("the download did not pass its checks"); }
        return apk;
    }

    private static boolean verifies(Context c, File apk, Release r) {
        try {
            if (r.sha256 != null && !r.sha256.equals(sha256(apk))) return false;
            PackageInfo info = c.getPackageManager().getPackageArchiveInfo(apk.getPath(), 0);
            PackageInfo installed = c.getPackageManager().getPackageInfo(c.getPackageName(), 0);
            return info != null && c.getPackageName().equals(info.packageName) && info.getLongVersionCode() > installed.getLongVersionCode();
        } catch (Exception e) {
            return false;
        }
    }

    private static String sha256(File f) throws Exception {
        MessageDigest md = MessageDigest.getInstance("SHA-256");
        try (InputStream in = new FileInputStream(f)) {
            byte[] buffer = new byte[1 << 16];
            for (int n; (n = in.read(buffer)) > 0; ) md.update(buffer, 0, n);
        }
        StringBuilder s = new StringBuilder();
        for (byte b : md.digest()) s.append(String.format(Locale.ROOT, "%02x", b));
        return s.toString();
    }

    static boolean canInstall(Context c) { return c.getPackageManager().canRequestPackageInstalls(); }

    static void openInstallSettings(Context c) {
        c.startActivity(new Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:" + c.getPackageName())).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
    }

    /** Hands the APK to the system installer; done gets null once it is installed, or the reason it was not. Blocking. */
    static void install(Context c, File apk, java.util.function.Consumer<String> done) throws IOException {
        PackageInstaller installer = c.getPackageManager().getPackageInstaller();
        PackageInstaller.SessionParams params = new PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL);
        params.setAppPackageName(c.getPackageName());
        if (android.os.Build.VERSION.SDK_INT >= 31) params.setRequireUserAction(PackageInstaller.SessionParams.USER_ACTION_NOT_REQUIRED);
        int id = installer.createSession(params);
        try (PackageInstaller.Session session = installer.openSession(id)) {
            try (OutputStream out = session.openWrite("exaltds.apk", 0, apk.length()); InputStream in = new FileInputStream(apk)) {
                byte[] buffer = new byte[1 << 16];
                for (int n; (n = in.read(buffer)) > 0; ) out.write(buffer, 0, n);
                session.fsync(out);
            }
            Context app = c.getApplicationContext();
            BroadcastReceiver receiver = new BroadcastReceiver() {
                @Override public void onReceive(Context ctx, Intent result) {
                    int status = result.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE);
                    if (status == PackageInstaller.STATUS_PENDING_USER_ACTION) {
                        @SuppressWarnings("deprecation") Intent confirm = result.getParcelableExtra(Intent.EXTRA_INTENT);
                        if (confirm != null) try { ctx.startActivity(confirm.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)); } catch (Exception e) { finish(ctx, "the install prompt could not open"); }
                        return;
                    }
                    finish(ctx, status == PackageInstaller.STATUS_SUCCESS ? null : String.valueOf(result.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE)));
                }
                private void finish(Context ctx, String error) {
                    try { ctx.unregisterReceiver(this); } catch (Exception ignored) { }
                    done.accept(error);
                }
            };
            app.registerReceiver(receiver, new IntentFilter(RESULT), 4 /* Context.RECEIVER_NOT_EXPORTED */);
            PendingIntent pending = PendingIntent.getBroadcast(app, id, new Intent(RESULT).setPackage(app.getPackageName()),
                    PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_MUTABLE);
            session.commit(pending.getIntentSender());
        } catch (IOException | RuntimeException e) {
            try { installer.abandonSession(id); } catch (Exception ignored) { }
            throw e instanceof IOException ? (IOException) e : new IOException(e.getMessage());
        }
    }

    private static HttpURLConnection open(String url, String accept) throws IOException {
        HttpURLConnection conn = (HttpURLConnection) new URL(url).openConnection();
        conn.setConnectTimeout(15000);
        conn.setReadTimeout(30000);
        conn.setRequestProperty("User-Agent", "ExaltDS");
        conn.setRequestProperty("Accept", accept);
        int code = conn.getResponseCode();
        if (code != 200) {
            conn.disconnect();
            throw new IOException(code == 403 || code == 429 ? "GitHub is limiting requests from this network; try again later" : "GitHub answered " + code);
        }
        return conn;
    }

    private static String get(String url, String accept) throws IOException {
        HttpURLConnection conn = open(url, accept);
        try (InputStream in = conn.getInputStream()) { return new String(in.readAllBytes(), StandardCharsets.UTF_8); } finally { conn.disconnect(); }
    }
}
