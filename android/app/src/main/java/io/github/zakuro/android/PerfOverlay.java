package io.github.zakuro.android;

import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Paint;
import android.graphics.RectF;
import android.graphics.Typeface;
import android.view.View;

/**
 * ExaltDS's performance monitor: one slim bar in the top-left corner that grows to the right with the level.
 *   1  FPS
 *   2  FPS, the frame time and a graph of the last 90 frame times
 *   3  level 2 and the CPU and GPU temperatures
 * Frame times are the time between the game's own pictures (bridge.rs record_frame), so 30 fps combat reads 33 ms.
 * The graph's middle line is the frame-time target (16.7 ms, or 33.3 ms while the game runs at 30), and its height is
 * twice that, so a frame that takes 4 ms too long moves a bar an eighth of the graph: hitches stand out, jitter shows.
 * Numbers change twice a second so they can be read; the graph moves ten times a second.
 */
final class PerfOverlay extends View {
    interface Source { int frameTimes(int[] out); }
    private static final int RING = 256, BARS = 90;
    private final Source source;
    private final int[] ring = new int[RING];
    private final float[] bars = new float[BARS];
    private int level, barCount;
    private String fps = "--", ms = "--.-", temps = "";
    private float target = 16_667f;
    private long nextText, nextTemps;
    private final Paint text = new Paint(Paint.ANTI_ALIAS_FLAG), unit = new Paint(Paint.ANTI_ALIAS_FLAG),
            fill = new Paint(Paint.ANTI_ALIAS_FLAG), line = new Paint(Paint.ANTI_ALIAS_FLAG), bar = new Paint();
    private final RectF box = new RectF();
    private final float d;
    private final Runnable tick = new Runnable() {
        @Override public void run() { if (level > 0) { sample(); invalidate(); postDelayed(this, 100); } }
    };

    PerfOverlay(Context c, Source source) {
        super(c);
        this.source = source;
        d = c.getResources().getDisplayMetrics().density;
        Typeface face = Typeface.create("sans-serif-medium", Typeface.NORMAL);
        text.setTypeface(face); text.setTextSize(13 * d); text.setColor(Exalt.INK); text.setFontFeatureSettings("tnum");
        unit.setTypeface(face); unit.setTextSize(10 * d); unit.setColor(Exalt.MUTED); unit.setFontFeatureSettings("tnum");
        fill.setColor(0xB30A0F22);
        line.setColor(Exalt.GOLD_SOFT); line.setStrokeWidth(Math.max(1, d));
        setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
        setLevel(0);
    }

    void setLevel(int l) {
        level = Math.max(0, Math.min(3, l));
        setVisibility(level == 0 ? GONE : VISIBLE);
        nextText = nextTemps = 0;
        removeCallbacks(tick);
        if (level > 0) post(tick);
        requestLayout(); invalidate();
    }

    @Override protected void onDetachedFromWindow() { removeCallbacks(tick); super.onDetachedFromWindow(); }

    private void sample() {
        int n = source.frameTimes(ring);
        int have = Math.min(n < 0 ? RING : n, RING);
        // newest first: the last second for the frame rate, the last half second for the frame time
        float sum = 0, half = 0; int count = 0, halfCount = 0;
        barCount = Math.min(have, BARS);
        for (int i = 0; i < have; i++) {
            float us = ring[Math.floorMod(n - 1 - i, RING)];
            if (i < BARS) bars[BARS - 1 - i] = us;
            if (sum < 1_000_000f) { sum += us; count++; }
            if (half < 500_000f) { half += us; halfCount++; }
        }
        long now = android.os.SystemClock.uptimeMillis();
        if (now >= nextText) {
            nextText = now + 500;
            fps = count == 0 || sum <= 0 ? "--" : String.valueOf(Math.round(count * 1e6f / sum));
            float avg = halfCount == 0 ? 0 : half / halfCount;
            ms = halfCount == 0 ? "--.-" : String.format(java.util.Locale.ROOT, "%.1f", avg / 1000f);
            target = avg > 25_000f ? 33_333f : 16_667f;
        }
        if (level >= 3 && now >= nextTemps) {
            nextTemps = now + 2000;
            int[] t = Exalt.temperatures();
            temps = (t[0] >= 0 ? "CPU " + t[0] + "°" : "") + (t[1] >= 0 ? (t[0] >= 0 ? "  " : "") + "GPU " + t[1] + "°" : "");
            requestLayout();
        }
    }

    private float pad() { return 8 * d; }
    private float gap() { return 10 * d; }
    private float graphW() { return BARS * 1.5f * d; }

    @Override protected void onMeasure(int w, int h) {
        float x = pad() + text.measureText("000") + unit.measureText(" FPS");
        if (level >= 2) x += gap() + text.measureText("00.0") + unit.measureText(" ms") + gap() + graphW();
        if (level >= 3 && !temps.isEmpty()) x += gap() + text.measureText(temps);
        setMeasuredDimension((int) Math.ceil(x + pad()), (int) Math.ceil(26 * d));
    }

    @Override protected void onDraw(Canvas c) {
        float h = getHeight(), r = 7 * d;
        box.set(0, 0, getWidth(), h);
        c.drawRoundRect(box, r, r, fill);
        float base = h / 2 - (text.descent() + text.ascent()) / 2, x = pad();
        x = value(c, fps, " FPS", x, base, fpsColor());
        if (level >= 2) {
            x = value(c, ms, " ms", x + gap(), base, Exalt.INK);
            x += gap();
            graph(c, x, 4 * d, graphW(), h - 8 * d);
            x += graphW();
        }
        if (level >= 3 && !temps.isEmpty()) c.drawText(temps, x + gap(), base, text);
    }

    private float value(Canvas c, String v, String u, float x, float base, int color) {
        text.setColor(color);
        c.drawText(v, x, base, text);
        x += text.measureText(v);
        c.drawText(u, x, base, unit);
        text.setColor(Exalt.INK);
        return x + unit.measureText(u);
    }

    private int fpsColor() {
        try {
            int f = Integer.parseInt(fps), want = target > 25_000f ? 30 : 60;
            return f >= want - 2 ? 0xFF8BE0A4 : f >= want * 3 / 4 ? Exalt.GOLD : Exalt.DANGER;
        } catch (NumberFormatException e) { return Exalt.INK; }
    }

    private void graph(Canvas c, float x, float y, float w, float h) {
        float span = target * 2, step = w / BARS;
        for (int i = BARS - barCount; i < BARS; i++) {
            float us = bars[i], top = y + h - Math.min(1f, us / span) * h;
            bar.setColor(us > target * 1.5f ? Exalt.DANGER : us > target + 4_000f ? Exalt.GOLD : 0xFF8BE0A4);
            c.drawRect(x + i * step, Math.min(top, y + h - d), x + (i + 1) * step - 0.25f * d, y + h, bar);
        }
        c.drawLine(x, y + h / 2, x + w, y + h / 2, line); // the target
    }
}
