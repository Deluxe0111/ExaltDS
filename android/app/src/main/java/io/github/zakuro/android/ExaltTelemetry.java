package io.github.zakuro.android;

import android.app.ActivityManager;
import android.content.Context;
import android.os.Build;
import android.os.Handler;
import android.os.HandlerThread;
import android.os.PowerManager;
import android.os.SystemClock;
import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.nio.charset.StandardCharsets;
import java.util.Arrays;
import java.util.UUID;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import org.json.JSONObject;

/**
 * Opt-in, anonymous diagnostics for ExaltDS, in the report format of Somniel's telemetry server (its /v1/ingest, "v": 1),
 * with "app" = "ExaltDS <version>" so the two apps stay apart there.
 *
 * Nothing is collected or sent until the player says yes, and never in a build whose res/values/exalt.xml has no
 * telemetry_url. Sent: a random install id (reset-able), the app version, device model, chipset and GPU names, Android
 * version, RAM, frame-rate and frame-time totals with the device's heat-throttling state, and, after a native crash, the
 * crashing frames as library name + offset. Never: names, accounts, IP addresses (the server keeps none), file paths,
 * saves, ROM or game data.
 */
final class ExaltTelemetry {
    private static final long REPORT_EVERY_MS = 5 * 60_000L, SAMPLE_EVERY_MS = 2_000L;
    private static final int QUEUE_MAX = 20;

    private final Context context;
    private final PerfOverlay.Source frames;
    private final String url, key;
    private final File queue;
    private HandlerThread thread;
    private Handler handler;
    private boolean running;

    // frame totals since the last report
    private long seen = -1, counted;
    private double seconds, longest;
    private int over50, over100, over250, over1000, longGaps, thermalMax;
    private final int[] histogram = new int[1001]; // frame times in whole ms, the last bucket 1000 ms and over
    private double windowSeconds, windowFrames, lowestWindowFps = Double.MAX_VALUE;

    ExaltTelemetry(Context context, PerfOverlay.Source frames) {
        this.context = context;
        this.frames = frames;
        url = res(context, "telemetry_url");
        key = res(context, "telemetry_key");
        queue = new File(context.getFilesDir(), "telemetry_queue");
    }

    private static String res(Context c, String name) {
        int id = c.getResources().getIdentifier(name, "string", c.getPackageName());
        return id == 0 ? "" : c.getString(id).trim();
    }

    /** False in a build without a server address: no question, no switch, nothing sent. */
    static boolean available(Context c) { return !res(c, "telemetry_url").isEmpty(); }
    /** null until the player has been asked. */
    static Boolean consent(Context c) { return Exalt.prefs(c).contains("telemetry") ? Exalt.prefs(c).getBoolean("telemetry", false) : null; }
    static boolean enabled(Context c) { return available(c) && Boolean.TRUE.equals(consent(c)); }

    static void setConsent(Context c, boolean yes) {
        Exalt.prefs(c).edit().putBoolean("telemetry", yes).commit();
        if (!yes) {
            File[] queued = new File(c.getFilesDir(), "telemetry_queue").listFiles();
            if (queued != null) for (File f : queued) f.delete();
        }
    }

    /** A new random id; the server can no longer tie new reports to the old ones. */
    static void resetId(Context c) { Exalt.prefs(c).edit().remove("telemetry_id").commit(); }

    static final String QUESTION = "Help improve ExaltDS by sharing anonymous diagnostics?\n\n"
            + "Shared: device model, chipset and GPU names, Android version, RAM, frame-rate and frame-time totals with the "
            + "device's heat-throttling state, and where a crash happened (library and offset), tied to a random ID.\n\n"
            + "Never shared: your name, accounts, IP address, files, saves or game data.\n\n"
            + "You can change this at any time in the sidebar.";

    private static String installId(Context c) {
        String id = Exalt.prefs(c).getString("telemetry_id", null);
        if (id == null) { id = UUID.randomUUID().toString(); Exalt.prefs(c).edit().putString("telemetry_id", id).commit(); }
        return id;
    }

    // ---- running in the game process ----

    synchronized void start() {
        if (running || !enabled(context)) return;
        running = true;
        thread = new HandlerThread("exalt-telemetry");
        thread.start();
        handler = new Handler(thread.getLooper());
        handler.post(this::crashes);
        handler.postDelayed(this::sample, SAMPLE_EVERY_MS);
        handler.postDelayed(this::report, REPORT_EVERY_MS);
        try {
            PowerManager pm = context.getSystemService(PowerManager.class);
            thermalMax = pm.getCurrentThermalStatus();
            pm.addThermalStatusListener(handler::post, s -> { if (s > thermalMax) thermalMax = s; });
        } catch (Exception ignored) { }
    }

    synchronized void stop() {
        if (!running) return;
        running = false;
        handler.removeCallbacksAndMessages(null);
        thread.quitSafely();
    }

    /** The game goes to the background: queue what was counted, send it. */
    void pause() { if (running) handler.post(() -> { queuePerf(); send(); }); }

    private void sample() {
        if (!running) return;
        try {
            int[] ring = new int[256];
            long n = frames.frameTimes(ring) & 0xFFFFFFFFL;
            if (seen >= 0 && n > seen) {
                long from = Math.max(seen, n - ring.length);
                double window = 0;
                for (long i = from; i < n; i++) {
                    double ms = ring[(int) (i % ring.length)] / 1000.0;
                    if (ms <= 0) continue;
                    if (ms > 5000) { longGaps++; continue; } // a freeze or the app in the background, not a frame
                    counted++; seconds += ms / 1000; window += ms / 1000;
                    longest = Math.max(longest, ms);
                    if (ms > 50) over50++;
                    if (ms > 100) over100++;
                    if (ms > 250) over250++;
                    if (ms > 1000) over1000++;
                    histogram[(int) Math.min(1000, Math.round(ms))]++;
                }
                windowSeconds += window; windowFrames += n - from;
                if (windowSeconds >= 5) { lowestWindowFps = Math.min(lowestWindowFps, windowFrames / windowSeconds); windowSeconds = windowFrames = 0; }
            }
            seen = n;
        } catch (Exception ignored) { }
        handler.postDelayed(this::sample, SAMPLE_EVERY_MS);
    }

    private void report() {
        if (!running) return;
        queuePerf();
        send();
        handler.postDelayed(this::report, REPORT_EVERY_MS);
    }

    private int percentile(double p) {
        long target = (long) Math.ceil(counted * p), sum = 0;
        for (int ms = 0; ms < histogram.length; ms++) if ((sum += histogram[ms]) >= target) return ms;
        return histogram.length - 1;
    }

    private void queuePerf() {
        try {
            if (seconds < 10) return; // under 10 s of frames is not a sample
            JSONObject perf = new JSONObject();
            perf.put("seconds", Math.round(seconds));
            perf.put("fps_avg", Math.round(counted / seconds * 10) / 10.0);
            perf.put("fps_low_window", lowestWindowFps == Double.MAX_VALUE ? 0 : Math.round(lowestWindowFps * 10) / 10.0);
            perf.put("longest_frame_ms", Math.round(longest));
            perf.put("over_50ms", over50); perf.put("over_100ms", over100); perf.put("over_250ms", over250); perf.put("over_1000ms", over1000);
            perf.put("long_gaps", longGaps);
            perf.put("p50_ms", percentile(0.5)); perf.put("p95_ms", percentile(0.95)); perf.put("p99_ms", percentile(0.99)); perf.put("p999_ms", percentile(0.999));
            perf.put("target_fps", 60);
            perf.put("thermal_max", thermalMax);
            perf.put("up_s", Math.max(0, (SystemClock.elapsedRealtime() - android.os.Process.getStartElapsedRealtime()) / 1000));
            JSONObject o = base("perf");
            o.put("perf", perf);
            enqueue(o);
        } catch (Exception ignored) { }
        counted = 0; seconds = longest = 0; over50 = over100 = over250 = over1000 = longGaps = 0;
        Arrays.fill(histogram, 0);
        lowestWindowFps = Double.MAX_VALUE;
        try { thermalMax = context.getSystemService(PowerManager.class).getCurrentThermalStatus(); } catch (Exception e) { thermalMax = 0; }
    }

    private static final Pattern FRAME = Pattern.compile("#(\\d\\d) pc ([0-9a-f]+)\\s+(\\S+)");
    private static final Pattern SIGNAL = Pattern.compile("signal (\\d+) \\((\\w+)\\)");
    private static final Pattern WHEN = Pattern.compile("^(\\d\\d-\\d\\d \\d\\d:\\d\\d:\\d\\d\\.\\d+)");

    /** A native crash of this app since the last report, from the crash log Android keeps (this app may read its own). */
    private void crashes() {
        try {
            Process p = new ProcessBuilder("logcat", "-b", "crash", "-d", "-v", "time").redirectErrorStream(true).start();
            String log;
            try (InputStream in = p.getInputStream()) { log = new String(in.readAllBytes(), StandardCharsets.UTF_8); }
            String last = Exalt.prefs(context).getString("telemetry_crash", "");
            String[] lines = log.split("\n");
            JSONObject crash = null;
            String when = null;
            int frame = 0;
            for (String line : lines) {
                if (line.contains(">>> " + context.getPackageName())) {
                    Matcher w = WHEN.matcher(line);
                    String t = w.find() ? w.group(1) : "";
                    if (t.compareTo(last) <= 0) { crash = null; continue; }
                    if (crash != null) enqueueCrash(crash);
                    crash = new JSONObject(); crash.put("kind", "native"); when = t; frame = 0;
                    continue;
                }
                if (crash == null) continue;
                Matcher s = SIGNAL.matcher(line);
                if (s.find() && !crash.has("signal")) crash.put("signal", s.group(2));
                Matcher f = FRAME.matcher(line);
                if (f.find() && frame < 10) {
                    String lib = f.group(3);
                    lib = lib.substring(lib.lastIndexOf('/') + 1); // no paths
                    crash.put("frame" + frame++, lib + "+0x" + f.group(2).replaceFirst("^0+(?=.)", ""));
                }
            }
            if (crash != null) enqueueCrash(crash);
            if (when != null) Exalt.prefs(context).edit().putString("telemetry_crash", when).commit();
        } catch (Exception ignored) { }
        send();
    }

    private void enqueueCrash(JSONObject crash) throws Exception {
        if (!crash.has("frame0")) return;
        JSONObject o = base("crash");
        o.put("crash", crash);
        enqueue(o);
    }

    private JSONObject base(String type) throws Exception {
        JSONObject o = new JSONObject();
        o.put("v", 1);
        o.put("type", type);
        o.put("install", installId(context));
        o.put("rid", UUID.randomUUID().toString().substring(0, 18));
        o.put("ts", System.currentTimeMillis() / 1000);
        o.put("app", "ExaltDS " + ExaltUpdater.currentVersion(context));
        JSONObject d = new JSONObject();
        d.put("model", Build.MANUFACTURER + " " + Build.MODEL);
        d.put("soc", (Build.SOC_MANUFACTURER + " " + Build.SOC_MODEL).trim());
        d.put("hardware", Build.HARDWARE);
        d.put("android", Build.VERSION.RELEASE);
        d.put("sdk", Build.VERSION.SDK_INT);
        ActivityManager.MemoryInfo memory = new ActivityManager.MemoryInfo();
        context.getSystemService(ActivityManager.class).getMemoryInfo(memory);
        d.put("ram_gb", Math.round(memory.totalMem / 1073741824.0));
        o.put("device", d);
        return o;
    }

    private synchronized void enqueue(JSONObject o) throws Exception {
        if (!queue.isDirectory() && !queue.mkdirs()) return;
        File[] old = queue.listFiles();
        if (old != null && old.length >= QUEUE_MAX) {
            Arrays.sort(old);
            for (int i = 0; i <= old.length - QUEUE_MAX; i++) old[i].delete();
        }
        try (FileOutputStream out = new FileOutputStream(new File(queue, System.currentTimeMillis() + "_" + (int) (Math.random() * 1000) + ".json"))) {
            out.write(o.toString().getBytes(StandardCharsets.UTF_8));
        }
    }

    private void send() {
        if (!enabled(context)) return;
        File[] files = queue.listFiles();
        if (files == null) return;
        Arrays.sort(files);
        for (File f : files) {
            int code = post(f);
            if (code < 0) return; // offline or the server is down: keep them for later
            f.delete();           // delivered, or refused for good
        }
    }

    private int post(File f) {
        HttpURLConnection c = null;
        try {
            byte[] body = java.nio.file.Files.readAllBytes(f.toPath());
            c = (HttpURLConnection) new URL(url).openConnection();
            c.setConnectTimeout(8000); c.setReadTimeout(8000);
            c.setRequestMethod("POST"); c.setDoOutput(true);
            c.setRequestProperty("Content-Type", "application/json");
            c.setRequestProperty("X-Somniel-Key", key);
            try (OutputStream out = c.getOutputStream()) { out.write(body); }
            int code = c.getResponseCode();
            try (InputStream in = code < 400 ? c.getInputStream() : c.getErrorStream()) { if (in != null) in.readAllBytes(); }
            return code >= 500 ? -1 : code;
        } catch (Exception e) {
            return -1;
        } finally {
            if (c != null) c.disconnect();
        }
    }
}
