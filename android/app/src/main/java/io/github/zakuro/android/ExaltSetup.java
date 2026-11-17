package io.github.zakuro.android;

import android.content.ContentResolver;
import android.content.Context;
import android.database.Cursor;
import android.net.Uri;
import android.os.StatFs;
import android.provider.OpenableColumns;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.file.Files;
import java.util.Locale;
import java.util.zip.ZipEntry;
import java.util.zip.ZipInputStream;

/**
 * ExaltDS's setup from the player's own files: Fire Emblem Awakening (US) as a decrypted .3ds/.cci/.cia and, optionally,
 * its DLC .cia. The files are copied into the app; the native side (crates/zakuro-android/src/setup.rs) builds the feet and
 * Naga's Tear layers from them and recompiles the game's code into title.so with the compiler the APK carries; then the
 * game is installed (GameImport.installBuilt) with its layers. Nothing of the game comes with the app.
 */
final class ExaltSetup {
    static { System.loadLibrary("zakuro_android"); }

    private static native String nativeInspect(String rom, String dlc);
    private static native String nativeBuild(String rom, String work, String kit, String programs, String layers, boolean feet, boolean naga);
    private static native int[] nativeProgress();
    static native void nativeCancel();

    /** Fire Emblem Awakening, US. */
    static final String TITLE = "00040000000A0500";
    static final String FEET = "Feet", NAGA = "Naga's Tear";
    /** about what setup needs besides the game and DLC: the C (~200 MB), objects, the layers and some room */
    private static final long WORK_BYTES = 1L << 30;

    interface Ui { void say(String text); }

    private ExaltSetup() { }

    static String displayName(Context c, Uri uri) {
        try (Cursor cursor = c.getContentResolver().query(uri, new String[]{OpenableColumns.DISPLAY_NAME}, null, null, null)) {
            if (cursor != null && cursor.moveToFirst()) return cursor.getString(0);
        } catch (Exception ignored) { }
        return uri.getLastPathSegment();
    }

    private static long size(Context c, Uri uri) {
        try (Cursor cursor = c.getContentResolver().query(uri, new String[]{OpenableColumns.SIZE}, null, null, null)) {
            if (cursor != null && cursor.moveToFirst() && !cursor.isNull(0)) return cursor.getLong(0);
        } catch (Exception ignored) { }
        return -1;
    }

    private static void copy(Context c, Uri from, File to, String what, Ui ui) throws IOException {
        long total = size(c, from), done = 0, shown = 0;
        ContentResolver r = c.getContentResolver();
        try (InputStream in = r.openInputStream(from); OutputStream out = new FileOutputStream(to)) {
            if (in == null) throw new IOException("The " + what + " can't be read.");
            byte[] buffer = new byte[1 << 20];
            for (int n; (n = in.read(buffer)) > 0; ) {
                out.write(buffer, 0, n);
                done += n;
                long now = System.currentTimeMillis();
                if (now - shown > 250) {
                    shown = now;
                    ui.say(String.format(Locale.ROOT, "Copying the %s… %d%s", what, total > 0 ? done * 100 / total : done >> 20, total > 0 ? "%" : " MB"));
                }
            }
        } catch (IOException e) {
            if (GameImport.outOfSpace(e)) throw new IOException("The device ran out of space while copying the " + what + ".");
            throw e;
        }
    }

    /** The compiler's headers and runtime (assets/clang-kit.zip), unpacked once per app version. */
    private static File kit(Context c, Ui ui) throws Exception {
        File kit = new File(c.getFilesDir(), "clang-kit"), stamp = new File(kit, ".version");
        android.content.pm.PackageInfo info = c.getPackageManager().getPackageInfo(c.getPackageName(), 0);
        String version = info.getLongVersionCode() + "/" + info.lastUpdateTime;
        if (stamp.isFile() && version.equals(new String(Files.readAllBytes(stamp.toPath())))) return kit;
        ui.say("Unpacking the compiler…");
        GameImport.remove(kit);
        GameImport.mkdir(kit);
        String root = kit.getCanonicalPath() + File.separator;
        try (ZipInputStream zip = new ZipInputStream(c.getAssets().open("clang-kit.zip"))) {
            byte[] buffer = new byte[1 << 16];
            for (ZipEntry e; (e = zip.getNextEntry()) != null; ) {
                File out = new File(kit, e.getName()).getCanonicalFile();
                if (!out.getPath().startsWith(root)) throw new IOException("Bad compiler kit");
                if (e.isDirectory()) { GameImport.mkdir(out); continue; }
                GameImport.mkdir(out.getParentFile());
                try (OutputStream o = new FileOutputStream(out)) { for (int n; (n = zip.read(buffer)) > 0; ) o.write(buffer, 0, n); }
            }
        }
        Files.write(stamp.toPath(), version.getBytes());
        return kit;
    }

    private static String stage(int[] p) {
        switch (p[0]) {
            case 1: return p[2] > 0 ? String.format(Locale.ROOT, "Building the feet… %d of %d models", p[1], p[2]) : "Building the feet…";
            case 2: return "Building Naga's Tear…";
            case 3: return "Writing the game's code as C… (about a minute)";
            case 4: return String.format(Locale.ROOT, "Compiling the game's code for this device… %d of %d files", p[1], p[2]);
            case 5: return "Linking the game's code…";
            default: return null;
        }
    }

    /** Sets the game up; gives the title ID. Blocking: a worker thread. naga: switch Naga's Tear on. */
    static String run(Context c, Uri rom, Uri dlc, boolean naga, Ui ui) throws Exception {
        File setup = new File(c.getFilesDir(), "setup");
        GameImport.remove(setup);
        GameImport.mkdir(setup);
        try {
            long need = Math.max(0, size(c, rom)) + (dlc == null ? 0 : Math.max(0, size(c, dlc))) + WORK_BYTES;
            long free = new StatFs(c.getFilesDir().getAbsolutePath()).getAvailableBytes();
            if (free < need) throw new IOException(String.format(Locale.ROOT, "Setup needs about %.1f GB free; this device has %.1f GB.", need / 1e9, free / 1e9));
            File romFile = new File(setup, "game.3ds"), dlcFile = dlc == null ? null : new File(setup, "dlc.cia");
            copy(c, rom, romFile, "game", ui);
            if (dlc != null) copy(c, dlc, dlcFile, "DLC", ui);
            ui.say("Checking the game…");
            String problem = nativeInspect(romFile.getAbsolutePath(), dlcFile == null ? "" : dlcFile.getAbsolutePath());
            if (problem != null) throw new IOException(problem);
            File kit = kit(c, ui);
            File work = new File(setup, "work"), layers = new File(setup, "layers");
            GameImport.mkdir(work); GameImport.mkdir(layers);

            // progress from the native side while it builds
            String[] error = {null};
            Thread builder = new Thread(() -> error[0] = nativeBuild(romFile.getAbsolutePath(), work.getAbsolutePath(), kit.getAbsolutePath(),
                    c.getApplicationInfo().nativeLibraryDir, layers.getAbsolutePath(), true, true), "exalt-setup-build");
            long started = System.currentTimeMillis();
            builder.start();
            while (builder.isAlive()) {
                builder.join(500);
                String text = stage(nativeProgress());
                long minutes = (System.currentTimeMillis() - started) / 60000;
                if (text != null) ui.say(text + (minutes > 0 ? "\n" + minutes + " min so far" : ""));
            }
            if (error[0] != null) throw new IOException(error[0]);

            ui.say("Installing…");
            GameImport.installBuilt(c, TITLE, "Fire Emblem Awakening", romFile, new File(work, "title.so"), dlcFile);
            // the layers: the feet on, Naga's Tear as chosen
            File home = Exalt.layers(c, TITLE);
            GameImport.mkdir(home);
            for (Exalt.Layer l : Exalt.list(c, TITLE)) if (l.feet() || l.naga()) Exalt.delete(c, TITLE, l.name);
            for (String name : new String[]{FEET, NAGA}) {
                File built = new File(layers, name);
                if (built.isDirectory()) Files.move(built.toPath(), new File(home, name).toPath());
            }
            Exalt.setOn(c, TITLE, FEET, true);
            Exalt.setOn(c, TITLE, NAGA, naga);
            Exalt.apply(c, TITLE);
            Exalt.firstSetup(c, TITLE);
            Exalt.prefs(c).edit().putBoolean("launch_failed", false).commit();
            return TITLE;
        } finally {
            GameImport.remove(setup);
        }
    }
}
