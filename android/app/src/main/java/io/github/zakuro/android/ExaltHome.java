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
 * ExaltDS's launcher activity. Once the game files are set up it starts the game straight away (the sidebar holds
 * everything else); before that, or after a failed start, it shows this page: set up or replace the game files, and
 * move in-game saves in and out as Zakuroishi save ZIPs.
 */
public final class ExaltHome extends Activity {
    private static final int IMPORT = 1, EXPORT_SAVES = 2, RESTORE_SAVES = 3;
    private TextView status;
    private ProgressBar progress;
    private LinearLayout actions;
    private boolean busy;

    @Override protected void onCreate(Bundle state) {
        super.onCreate(state);
        String id = Exalt.gameId(this);
        boolean failed = Exalt.prefs(this).getBoolean("launch_failed", false);
        if (state == null && id != null && !failed && !getIntent().getBooleanExtra("home", false)) { play(); return; }
        build();
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
        page.addView(actions, new LinearLayout.LayoutParams(dp(380), -2));
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
        if (id == null) {
            status.setText("Choose the folder that holds your decrypted Fire Emblem Awakening ROM (.3ds, .cci or .cxi) and its Android title.so. "
                    + "Only use a title.so you trust: it runs inside this app.");
            actions.addView(button("Set up game files…", true, v -> pickGame()));
        } else {
            if (status.getText().length() == 0) status.setText(Exalt.prefs(this).getBoolean("launch_failed", false)
                    ? "The game stopped last time. Replace the game files if they are damaged or out of date, or try again." : "Ready.");
            actions.addView(button("Play", true, v -> { Exalt.prefs(this).edit().putBoolean("launch_failed", false).commit(); play(); }));
            actions.addView(button("Replace game files…", false, v -> pickGame()));
            actions.addView(button("Export in-game saves…", false, v -> pickSaves(true)));
            actions.addView(button("Restore in-game saves…", false, v -> pickSaves(false)));
        }
        View first = actions.getChildAt(0);
        if (first != null) first.post(first::requestFocusFromTouch);
    }

    private Button button(String label, boolean primary, View.OnClickListener l) {
        Button b = new Button(this);
        b.setText(label); b.setAllCaps(false); b.setTextSize(16);
        b.setTextColor(primary ? 0xFF1A1405 : Exalt.INK);
        b.setTypeface(Typeface.create("sans-serif-medium", Typeface.NORMAL));
        b.setStateListAnimator(null);
        android.graphics.drawable.StateListDrawable s = new android.graphics.drawable.StateListDrawable();
        s.addState(new int[]{android.R.attr.state_focused}, shape(primary ? 0xFFF0C766 : Exalt.PRESS, 0xFFFFFFFF));
        s.addState(new int[]{android.R.attr.state_pressed}, shape(primary ? 0xFFF0C766 : Exalt.PRESS, 0));
        s.addState(new int[]{}, shape(primary ? Exalt.GOLD : Exalt.FOCUS, 0));
        b.setBackground(s);
        b.setOnClickListener(l);
        b.setEnabled(!busy);
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(-1, dp(52)); lp.setMargins(0, dp(6), 0, dp(6));
        b.setLayoutParams(lp);
        return b;
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
        if (e.getKeyCode() == KeyEvent.KEYCODE_BUTTON_B) { if (e.getAction() == KeyEvent.ACTION_UP) finish(); return true; }
        return super.dispatchKeyEvent(e);
    }

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

    private void pickGame() {
        if (busy) return;
        startActivityForResult(new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_PREFIX_URI_PERMISSION), IMPORT);
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
        new Thread(() -> {
            String done;
            try { done = job.run(); } catch (Exception e) { done = "Failed: " + (e.getMessage() == null ? e.getClass().getSimpleName() : e.getMessage()); }
            String text = done;
            runOnUiThread(() -> { busy = false; progress.setVisibility(View.GONE); status.setText(text); refresh(); });
        }, "exalt-home").start();
    }

    @Override protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        if (result != RESULT_OK || data == null || data.getData() == null || status == null) return;
        Uri uri = data.getData();
        String id = Exalt.gameId(this);
        if (request == IMPORT) run("Checking the folder… No files copied yet.", () -> {
            String added = GameImport.install(getApplicationContext(), uri, false, (bytes, files, name) ->
                    runOnUiThread(() -> status.setText(String.format(Locale.ROOT, "Copying game files… %.0f MB", bytes / 1048576.0))));
            Exalt.prefs(this).edit().putBoolean("launch_failed", false).commit();
            Exalt.firstSetup(this, added);
            return "Game files ready (" + added + "). Mods, textures and drivers are in the sidebar: press Back in game.";
        });
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
