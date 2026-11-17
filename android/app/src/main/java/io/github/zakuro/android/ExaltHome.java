package io.github.zakuro.android;

import android.app.Activity;
import android.app.AlertDialog;
import android.content.Intent;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.net.Uri;
import android.os.Bundle;
import android.view.*;
import android.widget.*;
import java.util.Locale;

/**
 * ExaltDS's launcher activity. Once the game is set up it starts the game straight away (the sidebar holds everything
 * else); before that, or after a failed start, it shows this page: set the game up from the player's own files
 * (ExaltSetup: the game, optional DLC, Naga's Tear), and move in-game saves in and out as Zakuroishi save ZIPs.
 */
public final class ExaltHome extends Activity {
    private static final int PICK_GAME = 1, EXPORT_SAVES = 2, RESTORE_SAVES = 3, PICK_DLC = 4;
    private TextView status;
    private ProgressBar progress;
    private LinearLayout actions;
    private boolean busy, setupOpen;
    private Uri game, dlc;
    /** Naga's Tear is on unless the player unticks it */
    private boolean naga = true;

    @Override protected void onCreate(Bundle state) {
        super.onCreate(state);
        String id = Exalt.gameId(this);
        boolean failed = Exalt.prefs(this).getBoolean("launch_failed", false);
        if (state == null && id != null && !failed && !getIntent().getBooleanExtra("home", false) && !askDiagnostics()) { play(); return; }
        build();
        if (id != null) askDiagnostics();
    }

    /** Asks once (in a build with a diagnostics server) whether to share anonymous diagnostics; true while it asks. */
    private boolean asking;
    private boolean askDiagnostics() {
        if (asking || !ExaltTelemetry.available(this) || ExaltTelemetry.consent(this) != null) return asking;
        asking = true;
        if (actions == null) build();
        AlertDialog d = new AlertDialog.Builder(this, android.R.style.Theme_DeviceDefault_Dialog_Alert).setTitle("Share anonymous diagnostics?")
                .setMessage(ExaltTelemetry.QUESTION).setCancelable(false)
                .setPositiveButton("Share", (x, w) -> { ExaltTelemetry.setConsent(this, true); asking = false; })
                .setNegativeButton("No thanks", (x, w) -> { ExaltTelemetry.setConsent(this, false); asking = false; })
                .create();
        d.show();
        return true;
    }

    private int dp(int v) { return Math.round(v * getResources().getDisplayMetrics().density); }

    private void build() {
        getWindow().setDecorFitsSystemWindows(false);
        LinearLayout page = new LinearLayout(this);
        page.setOrientation(LinearLayout.VERTICAL);
        page.setGravity(Gravity.CENTER_HORIZONTAL);
        GradientDrawable bg = new GradientDrawable(GradientDrawable.Orientation.TL_BR, new int[]{0xFF17234A, 0xFF0A0F22});
        page.setBackground(bg);
        page.setPadding(dp(32), dp(28), dp(32), dp(24));

        ImageView logo = new ImageView(this);
        logo.setImageResource(getApplicationInfo().icon);
        page.addView(logo, new LinearLayout.LayoutParams(dp(88), dp(88)));
        TextView title = new TextView(this);
        title.setText("ExaltDS"); title.setTextSize(34); title.setTextColor(Exalt.INK); title.setTypeface(Typeface.DEFAULT_BOLD);
        title.setGravity(Gravity.CENTER);
        page.addView(title);
        View bar = new View(this);
        GradientDrawable gold = new GradientDrawable(); gold.setColor(Exalt.GOLD); gold.setCornerRadius(dp(2));
        bar.setBackground(gold);
        LinearLayout.LayoutParams barSize = new LinearLayout.LayoutParams(dp(40), dp(3)); barSize.setMargins(0, dp(6), 0, dp(10));
        page.addView(bar, barSize);
        TextView sub = new TextView(this);
        sub.setText("Fire Emblem Awakening, recompiled"); sub.setTextSize(15); sub.setTextColor(Exalt.MUTED); sub.setGravity(Gravity.CENTER);
        page.addView(sub);

        progress = new ProgressBar(this, null, android.R.attr.progressBarStyleHorizontal);
        progress.setIndeterminate(true);
        progress.setIndeterminateTintList(android.content.res.ColorStateList.valueOf(Exalt.GOLD));
        progress.setVisibility(View.GONE);
        LinearLayout.LayoutParams ps = new LinearLayout.LayoutParams(dp(360), dp(4)); ps.setMargins(0, dp(16), 0, 0);
        page.addView(progress, ps);
        status = new TextView(this);
        status.setTextSize(14); status.setTextColor(Exalt.MUTED); status.setGravity(Gravity.CENTER);
        status.setPadding(0, dp(10), 0, dp(10));
        page.addView(status);

        actions = new LinearLayout(this);
        actions.setOrientation(LinearLayout.VERTICAL);
        page.addView(actions, new LinearLayout.LayoutParams(dp(420), -2));
        refresh();

        ScrollView scroll = new ScrollView(this);
        scroll.setFillViewport(true);
        scroll.setBackground(bg);
        scroll.addView(page);
        setContentView(scroll);
        WindowInsetsController c = getWindow().getInsetsController();
        if (c != null) { c.hide(WindowInsets.Type.systemBars()); c.setSystemBarsBehavior(WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE); }
    }

    private void refresh() {
        actions.removeAllViews();
        String id = Exalt.gameId(this);
        if (busy) {
            actions.addView(button("Stop", false, v -> { ExaltSetup.nativeCancel(); status.setText("Stopping…"); }));
        } else if (id == null || setupOpen) {
            if (status.getText().length() == 0 || !setupOpen) status.setText(SETUP_TEXT);
            setupOpen = true;
            actions.addView(button(game == null ? "Choose your game file…" : "Game: " + ExaltSetup.displayName(this, game), game == null, v -> pick(PICK_GAME)));
            actions.addView(button(dlc == null ? "Add the DLC (optional)…" : "DLC: " + ExaltSetup.displayName(this, dlc), false, v -> pick(PICK_DLC)));
            actions.addView(check("Naga's Tear (experimental)", naga, on -> {
                naga = on;
                if (on) new AlertDialog.Builder(this, android.R.style.Theme_DeviceDefault_Dialog_Alert).setTitle("Naga's Tear")
                        .setMessage("You'll receive Naga's Tear the next time you enter the world map: Robin and Chrom find it, and "
                                + "Recall Fate appears in the map menu during battles.\n\nNaga's Tear is experimental. You can turn it "
                                + "off or on later in the sidebar (it then needs Apply and restart).")
                        .setPositiveButton("OK", null).show();
            }));
            Button go = button("Set up", game != null, v -> setup());
            go.setEnabled(game != null);
            go.setAlpha(game != null ? 1f : 0.5f);
            actions.addView(go);
            if (id != null) actions.addView(button("Cancel", false, v -> { setupOpen = false; status.setText(""); refresh(); }));
        } else {
            if (status.getText().length() == 0) status.setText(Exalt.prefs(this).getBoolean("launch_failed", false)
                    ? "The game stopped last time. Set it up again if its files are damaged, or try again." : "Ready.");
            actions.addView(button("Play", true, v -> { Exalt.prefs(this).edit().putBoolean("launch_failed", false).commit(); play(); }));
            actions.addView(button("Set up the game again…", false, v -> { setupOpen = true; status.setText(SETUP_TEXT); refresh(); }));
            actions.addView(button("Export in-game saves…", false, v -> pickSaves(true)));
            actions.addView(button("Restore in-game saves…", false, v -> pickSaves(false)));
        }
        View first = actions.getChildAt(0);
        if (first != null) first.post(first::requestFocusFromTouch);
    }

    private static final String SETUP_TEXT = "Choose your own decrypted Fire Emblem Awakening (US) as a .3ds, .cci or .cia file, and its DLC "
            + ".cia if you have it. ExaltDS builds the game's code for this device from it: this takes about 5 to 10 minutes. "
            + "Keep ExaltDS open and the device charging. Your saves stay as they are.";

    private Button button(String label, boolean primary, View.OnClickListener l) {
        Button b = new Button(this);
        b.setText(label); b.setAllCaps(false); b.setTextSize(16);
        b.setTextColor(primary ? 0xFF1A1405 : Exalt.INK);
        b.setTypeface(Typeface.create("sans-serif-medium", Typeface.NORMAL));
        b.setStateListAnimator(null);
        b.setSingleLine(true); b.setEllipsize(android.text.TextUtils.TruncateAt.MIDDLE);
        android.graphics.drawable.StateListDrawable s = new android.graphics.drawable.StateListDrawable();
        s.addState(new int[]{android.R.attr.state_focused}, shape(primary ? 0xFFF0C766 : Exalt.PRESS, 0xFFFFFFFF));
        s.addState(new int[]{android.R.attr.state_pressed}, shape(primary ? 0xFFF0C766 : Exalt.PRESS, 0));
        s.addState(new int[]{}, shape(primary ? Exalt.GOLD : Exalt.FOCUS, 0));
        b.setBackground(s);
        b.setOnClickListener(l);
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(-1, dp(52)); lp.setMargins(0, dp(6), 0, dp(6));
        b.setLayoutParams(lp);
        return b;
    }

    private interface Toggle { void on(boolean v); }

    private CheckBox check(String label, boolean on, Toggle t) {
        CheckBox c = new CheckBox(this);
        c.setText(label); c.setTextSize(16); c.setTextColor(Exalt.INK);
        c.setButtonTintList(android.content.res.ColorStateList.valueOf(Exalt.GOLD));
        c.setChecked(on);
        c.setPadding(dp(8), 0, dp(14), 0);
        android.graphics.drawable.StateListDrawable s = new android.graphics.drawable.StateListDrawable();
        s.addState(new int[]{android.R.attr.state_focused}, shape(Exalt.FOCUS, 0xFFFFFFFF));
        s.addState(new int[]{}, new android.graphics.drawable.ColorDrawable(0));
        c.setBackground(s);
        c.setOnCheckedChangeListener((b, v) -> t.on(v));
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(-1, dp(52)); lp.setMargins(0, dp(6), 0, dp(6));
        c.setLayoutParams(lp);
        return c;
    }

    private GradientDrawable shape(int fill, int stroke) {
        GradientDrawable g = new GradientDrawable(); g.setColor(fill); g.setCornerRadius(dp(12));
        if (stroke != 0) g.setStroke(dp(2), stroke);
        return g;
    }

    @Override public boolean dispatchKeyEvent(KeyEvent e) {
        if (e.getKeyCode() == KeyEvent.KEYCODE_BUTTON_A) {
            if (e.getAction() == KeyEvent.ACTION_UP && getCurrentFocus() != null) getCurrentFocus().performClick();
            return true;
        }
        if (e.getKeyCode() == KeyEvent.KEYCODE_BUTTON_B) { if (e.getAction() == KeyEvent.ACTION_UP && !busy) finish(); return true; }
        return super.dispatchKeyEvent(e);
    }

    @Override public void onBackPressed() { if (!busy) super.onBackPressed(); }

    private void play() {
        String id = Exalt.gameId(this);
        try {
            GameLaunch.start(this, GameLaunch.intent(this, id));
            finish();
        } catch (Exception e) {
            Exalt.prefs(this).edit().putBoolean("launch_failed", true).commit();
            if (actions == null) build();
            status.setText("The game could not start: " + e.getMessage());
        }
    }

    private void pick(int request) {
        if (busy) return;
        startActivityForResult(new Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType("*/*")
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION), request);
    }

    private void pickSaves(boolean export) {
        if (busy) return;
        Intent i = new Intent(export ? Intent.ACTION_CREATE_DOCUMENT : Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType(export ? "application/zip" : "*/*");
        if (export) i.putExtra(Intent.EXTRA_TITLE, "ExaltDS-saves-" + new java.text.SimpleDateFormat("yyyyMMdd-HHmmss", Locale.ROOT).format(new java.util.Date()) + ".zip");
        startActivityForResult(i, export ? EXPORT_SAVES : RESTORE_SAVES);
    }

    private interface Job { String run() throws Exception; }

    private void run(String working, Job job) {
        busy = true; progress.setVisibility(View.VISIBLE); status.setText(working); refresh();
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        new Thread(() -> {
            String done;
            boolean ok = false;
            try { done = job.run(); ok = true; } catch (Exception e) { done = "Failed: " + (e.getMessage() == null ? e.getClass().getSimpleName() : e.getMessage()); }
            String text = done;
            boolean succeeded = ok;
            runOnUiThread(() -> {
                busy = false; progress.setVisibility(View.GONE);
                getWindow().clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
                if (succeeded && setupOpen) { setupOpen = false; game = null; dlc = null; }
                status.setText(text); refresh();
            });
        }, "exalt-home").start();
    }

    private void setup() {
        if (busy || game == null) return;
        Uri g = game, d = dlc;
        boolean n = naga;
        run("Starting setup…", () -> {
            ExaltSetup.run(getApplicationContext(), g, d, n, text -> runOnUiThread(() -> status.setText(text)));
            return "The game is set up. Press Play; the sidebar (Back in game) has the settings, mods and textures."
                    + (n ? "\n\nNaga's Tear is on (experimental): you'll receive it the next time you enter the world map." : "");
        });
    }

    @Override protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        if (result != RESULT_OK || data == null || data.getData() == null || status == null) return;
        Uri uri = data.getData();
        if (request == PICK_GAME) { game = uri; refresh(); return; }
        if (request == PICK_DLC) { dlc = uri; refresh(); return; }
        String id = Exalt.gameId(this);
        if (id == null) return;
        if (request == EXPORT_SAVES || request == RESTORE_SAVES) run(request == EXPORT_SAVES ? "Exporting saves…" : "Restoring saves…", () -> {
            try (SaveArchive.Guard ignored = SaveArchive.lock(getFilesDir(), id)) {
                SaveArchive.recover(getFilesDir(), id);
                if (request == EXPORT_SAVES) try (java.io.OutputStream out = getContentResolver().openOutputStream(uri, "w")) {
                    if (out == null) throw new java.io.IOException("Cannot write the chosen file.");
                    SaveArchive.exportZip(getFilesDir(), id, out);
                } else try (java.io.InputStream in = getContentResolver().openInputStream(uri)) {
                    if (in == null) throw new java.io.IOException("Cannot read the chosen file.");
                    SaveArchive.restoreZip(getFilesDir(), id, in);
                }
            }
            return request == EXPORT_SAVES ? "Saves exported." : "Saves restored. Your previous saves are kept as a recovery copy.";
        });
    }
}
