package io.github.zakuro.android;

import android.app.ActivityManager;
import android.app.AlertDialog;
import android.app.ApplicationExitInfo;
import android.content.ContentResolver;
import android.content.ContentValues;
import android.content.Context;
import android.net.Uri;
import android.os.Build;
import android.os.Environment;
import android.os.Handler;
import android.os.Looper;
import android.provider.MediaStore;
import android.widget.Toast;
import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.text.SimpleDateFormat;
import java.util.Date;
import java.util.List;
import java.util.Locale;

/**
 * ExaltDS: saves this app's log to Download/exaltds_logs so a bug report can come with it. From the sidebar (Save
 * diagnostic log), and after the game crashed or stopped responding: the next launch asks. Nothing is sent anywhere.
 *
 * The log is logcat (the app writes no log file), which outlives the process, so the previous run's lines are still
 * there on the next launch. Faults come from the exit records Android keeps (API 30): a crash, native crash or ANR
 * asks, a swipe-away, Quit or restart does not. Each launch remembers the newest exit it saw in files/fault_asked, so
 * every fault asks once.
 */
final class LogExporter {
    static final String FOLDER = "exaltds_logs";
    private static final String STASH = "fault_log.txt", SEEN = "fault_asked";
    private static final int TRACE_BYTES = 256 << 10;
    private LogExporter() {}

    /** The sidebar's Save diagnostic log. Blocking (reads logcat); returns where the file went. */
    static String saveCurrent(Context c) throws Exception {
        return write(c, "manual", "Saved from the sidebar (Save diagnostic log).", Diagnostics.log());
    }

    /**
     * Game.onCreate: when the run before this one crashed or stopped responding, copies its log right away (this
     * run's lines push it out of logcat's ring) and then asks whether to save it. On a thread of its own.
     */
    static void checkPreviousRun(Game game) {
        Context c = game.getApplicationContext();
        new Thread(() -> {
            String[] fault = previousFault(c);
            if (fault == null) return;
            game.runOnUiThread(() -> game.getWindow().getDecorView().post(() -> ask(game, fault[0], fault[1])));
        }, "ExaltDS fault log").start();
    }

    /** {file prefix, reason} for the run before this one if it ended in a fault not asked about yet; else null. */
    private static String[] previousFault(Context c) {
        File seen = new File(c.getFilesDir(), SEEN);
        long last = -1;
        try { last = Long.parseLong(new String(Files.readAllBytes(seen.toPath()), StandardCharsets.UTF_8).trim()); }
        catch (Exception ignored) {}
        List<ApplicationExitInfo> exits;
        try { exits = c.getSystemService(ActivityManager.class).getHistoricalProcessExitReasons(c.getPackageName(), 0, 16); }
        catch (RuntimeException e) { android.util.Log.w("Zakuro", "Exit reasons", e); return null; }
        long newest = last;
        ApplicationExitInfo fault = null;
        for (ApplicationExitInfo e : exits) {
            newest = Math.max(newest, e.getTimestamp());
            if (fault == null && last >= 0 && e.getTimestamp() > last && hang(e) != null) fault = e;
        }
        // first launch with this feature: older faults have no log left to save, so they only set the baseline
        if (last < 0) newest = Math.max(newest, System.currentTimeMillis());
        try { Files.write(seen.toPath(), Long.toString(newest).getBytes(StandardCharsets.UTF_8)); }
        catch (IOException e) { android.util.Log.w("Zakuro", "Remembering the last exit", e); return null; }
        if (fault == null) return null;

        boolean hang = hang(fault);
        String reason = (hang ? "The game stopped responding" : "The game crashed") + " at " + stamp(fault.getTimestamp(), "yyyy-MM-dd HH:mm:ss z")
                + " (" + kind(fault) + ", process " + fault.getProcessName() + ", status " + fault.getStatus()
                + (fault.getDescription() != null ? ", " + fault.getDescription() : "") + ").";
        StringBuilder log = new StringBuilder(Diagnostics.log());
        if (fault.getReason() == ApplicationExitInfo.REASON_ANR) log.append("\n---- ANR traces ----\n").append(trace(fault));
        try { Files.write(new File(c.getFilesDir(), STASH).toPath(), log.toString().getBytes(StandardCharsets.UTF_8)); }
        catch (IOException e) { android.util.Log.w("Zakuro", "Copying the previous log", e); return null; }
        android.util.Log.e("Zakuro", "Previous run: " + reason);
        return new String[] {hang ? "hang" : "crash", reason};
    }

    /** True for an ANR, false for a crash, null for an exit that is not a fault (swipe-away, Quit, low memory, update…). */
    private static Boolean hang(ApplicationExitInfo e) {
        switch (e.getReason()) {
            case ApplicationExitInfo.REASON_ANR: return true;
            case ApplicationExitInfo.REASON_CRASH: case ApplicationExitInfo.REASON_CRASH_NATIVE: return false;
            default: return null;
        }
    }

    private static String kind(ApplicationExitInfo e) {
        switch (e.getReason()) {
            case ApplicationExitInfo.REASON_ANR: return "not responding";
            case ApplicationExitInfo.REASON_CRASH_NATIVE: return "native crash";
            default: return "Java crash";
        }
    }

    private static String trace(ApplicationExitInfo e) {
        try (InputStream in = e.getTraceInputStream()) {
            if (in == null) return "(none kept)\n";
            ByteArrayOutputStream out = new ByteArrayOutputStream();
            byte[] buffer = new byte[8192]; int n;
            while (out.size() < TRACE_BYTES && (n = in.read(buffer)) > 0) out.write(buffer, 0, n);
            return out.toString("UTF-8");
        } catch (Exception x) { return "(unavailable: " + x.getClass().getSimpleName() + ")\n"; }
    }

    private static void ask(Game game, String prefix, String reason) {
        if (game.isFinishing() || game.isDestroyed()) return;
        File stash = new File(game.getFilesDir(), STASH);
        AlertDialog d = new AlertDialog.Builder(game, game.palette().dialogTheme())
                .setTitle("ExaltDS closed unexpectedly")
                .setMessage("Last time the game " + ("hang".equals(prefix) ? "stopped responding" : "crashed")
                        + ". Save its log to Download/" + FOLDER + " so you can send it with a bug report?")
                .setPositiveButton("Save log", (dialog, which) -> {
                    Context c = game.getApplicationContext();
                    new Thread(() -> {
                        try {
                            String log = new String(Files.readAllBytes(stash.toPath()), StandardCharsets.UTF_8);
                            toast(c, "Log saved to " + write(c, prefix, reason, log));
                            stash.delete();
                        } catch (Exception e) {
                            android.util.Log.e("Zakuro", "Saving the previous log", e);
                            toast(c, "Could not save the log: " + e.getMessage());
                        }
                    }, "ExaltDS fault log").start();
                })
                .setNegativeButton("No thanks", (dialog, which) -> stash.delete())
                .create();
        d.show(); game.palette().dialog(d, false);
        d.getButton(AlertDialog.BUTTON_POSITIVE).requestFocus();
    }

    static void toast(Context c, String text) {
        new Handler(Looper.getMainLooper()).post(() -> Toast.makeText(c, text, Toast.LENGTH_LONG).show());
    }

    /** Download/exaltds_logs/exaltds_<prefix>_<time>.txt through MediaStore: no storage permission. Returns its path. */
    private static String write(Context c, String prefix, String reason, String log) throws Exception {
        String name = "exaltds_" + prefix + "_" + stamp(System.currentTimeMillis(), "yyyy-MM-dd_HHmmss") + ".txt";
        byte[] text = (header(c, reason) + Diagnostics.redact(log)).getBytes(StandardCharsets.UTF_8);
        ContentResolver cr = c.getContentResolver();
        ContentValues v = new ContentValues();
        v.put(MediaStore.MediaColumns.DISPLAY_NAME, name);
        v.put(MediaStore.MediaColumns.MIME_TYPE, "text/plain");
        v.put(MediaStore.MediaColumns.RELATIVE_PATH, Environment.DIRECTORY_DOWNLOADS + "/" + FOLDER);
        v.put(MediaStore.MediaColumns.IS_PENDING, 1);
        Uri uri = cr.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, v);
        if (uri == null) throw new IOException("Downloads refused the file");
        try {
            try (OutputStream out = cr.openOutputStream(uri, "w")) {
                if (out == null) throw new IOException("Downloads gave no file to write");
                out.write(text);
            }
            v.clear();
            v.put(MediaStore.MediaColumns.IS_PENDING, 0);
            cr.update(uri, v, null, null);
        } catch (Exception e) {
            try { cr.delete(uri, null, null); } catch (Exception ignored) {}
            throw e;
        }
        return "Download/" + FOLDER + "/" + name;
    }

    private static String header(Context c, String reason) throws Exception {
        android.content.pm.PackageInfo info = c.getPackageManager().getPackageInfo(c.getPackageName(), 0);
        return "ExaltDS diagnostic log\n"
                + "Reason: " + reason + "\n"
                + "Saved: " + stamp(System.currentTimeMillis(), "yyyy-MM-dd HH:mm:ss z") + "\n"
                + "App version: " + info.versionName + " (" + info.getLongVersionCode() + ")\n"
                + "Device: " + Build.MANUFACTURER + " " + Build.MODEL + "\n"
                + "SoC: " + (Build.VERSION.SDK_INT >= 31 ? Build.SOC_MANUFACTURER + " " + Build.SOC_MODEL : Build.HARDWARE) + "\n"
                + "Android: " + Build.VERSION.RELEASE + " (API " + Build.VERSION.SDK_INT + ")\n"
                + "\nThe log below is this app's own (logcat). Document picker addresses are removed; app file paths and file "
                + "names remain. Nothing was sent anywhere: attach this file to your bug report.\n\n";
    }

    private static String stamp(long millis, String pattern) { return new SimpleDateFormat(pattern, Locale.ROOT).format(new Date(millis)); }
}
