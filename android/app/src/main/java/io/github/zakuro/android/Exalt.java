package io.github.zakuro.android;

import android.content.ContentResolver;
import android.content.Context;
import android.content.SharedPreferences;
import android.database.Cursor;
import android.net.Uri;
import android.provider.DocumentsContract;
import java.io.File;
import java.io.IOException;
import java.nio.file.Files;
import java.util.*;

/**
 * ExaltDS: the same app built as its own package (io.github.exaltds) for Fire Emblem Awakening. It starts straight into
 * the game and replaces the pause grid with a sidebar (Sidebar.java). Everything here is only used by that build.
 *
 * Mods are kept as separate layers in files/exalt/mods/<title>/<name>/romfs, each switched on or off; applying them
 * hard-links the switched-on layers into the game's data/mods/<title>/romfs (the folder the runtime reads), so a toggle
 * costs no copying. A texture pack goes where Zakuroishi puts one (GameImport.replaceTextures). A layer may also hold a
 * textures folder (pictures for textures its romfs changes, e.g. Naga's Tear's icon sheet); applying links those into
 * the texture pack's folder under .exalt-layers/<layer>, which the pack reads with the rest.
 */
final class Exalt {
    static final String PACKAGE = "io.github.exaltds";
    static boolean is(Context c) { return PACKAGE.equals(c.getPackageName()); }

    // ---- colours: Chrom's navy and the Exalt's gold ----
    static final int PANEL_TOP = 0xF7152044, PANEL_BOTTOM = 0xF70A0F22;
    static final int GOLD = 0xFFE2B455, GOLD_SOFT = 0x66E2B455, BLUE = 0xFF3D63C8;
    static final int FOCUS = 0xFF223463, PRESS = 0xFF2D4482, INK = 0xFFFFFFFF, MUTED = 0xFFAFB9D6, DANGER = 0xFFFF8A94;

    // ---- enhancement names (crates/zakuro-core/src/enhancements.txt) ----
    static final String WIDE = "16:9", COMBAT_ORIGINAL = "Combat 60 fps (original speed)", COMBAT_FASTER = "Combat 60 fps (faster)";
    static final float WIDE_TOP = 426.66666f, NARROW_TOP = 400f;

    static SharedPreferences prefs(Context c) {
        @SuppressWarnings("deprecation") SharedPreferences p = c.getSharedPreferences("exalt", Context.MODE_MULTI_PROCESS);
        return p;
    }
    /** 0 off, 1 FPS, 2 FPS and frame times with a graph, 3 adds the CPU and GPU temperatures. */
    static int perfLevel(Context c) { return Math.max(0, Math.min(3, prefs(c).getInt("perf_level", 0))); }

    /** The one imported game (ExaltDS holds just Awakening), or null before setup. */
    static String gameId(Context c) {
        SortedSet<String> ids = GameImport.importedIds(c.getFilesDir());
        return ids.isEmpty() ? null : ids.first();
    }

    // ---- mod layers ----

    static File layers(Context c, String id) { return new File(c.getFilesDir(), "exalt/mods/" + id); }

    static final class Layer {
        final String name; final boolean on;
        Layer(String name, boolean on) { this.name = name; this.on = on; }
        boolean feet() { return name.toLowerCase(Locale.ROOT).contains("feet"); }
        /** the Naga's Tear layer (tools/naga-tear in Awakening-Port): its scripts add the world-map introduction */
        boolean naga() { return name.toLowerCase(Locale.ROOT).contains("naga"); }
    }

    static List<Layer> list(Context c, String id) {
        Set<String> off = prefs(c).getStringSet(id + ".mods_off", Collections.emptySet());
        List<Layer> out = new ArrayList<>();
        File[] dirs = layers(c, id).listFiles(f -> f.isDirectory() && !f.getName().startsWith("."));
        if (dirs != null) for (File d : dirs) out.add(new Layer(d.getName(), !off.contains(d.getName())));
        out.sort((a, b) -> a.name.compareToIgnoreCase(b.name));
        return out;
    }

    static void setOn(Context c, String id, String name, boolean on) {
        Set<String> off = new HashSet<>(prefs(c).getStringSet(id + ".mods_off", Collections.emptySet()));
        if (on) off.remove(name); else off.add(name);
        prefs(c).edit().putStringSet(id + ".mods_off", off).commit();
    }

    static Layer feet(Context c, String id) {
        for (Layer l : list(c, id)) if (l.feet()) return l;
        return null;
    }

    static Layer naga(Context c, String id) {
        for (Layer l : list(c, id)) if (l.naga()) return l;
        return null;
    }

    /**
     * Mods that were in the game's romfs before ExaltDS managed it (an import that carried them) become the layer
     * "Imported mods", so the first Apply keeps them.
     */
    private static void adopt(Context c, String id, File romfs) throws IOException {
        File home = layers(c, id), marker = new File(home, ".managed");
        if (marker.isFile()) return;
        GameImport.mkdir(home);
        if (romfs.isDirectory()) {
            File layer = new File(home, "Imported mods");
            GameImport.mkdir(layer);
            Files.move(romfs.toPath(), new File(layer, "romfs").toPath());
        }
        if (!marker.createNewFile() && !marker.isFile()) throw new IOException("Cannot mark the mods folder");
    }

    /** Puts the switched-on layers into the game's romfs (later names win where two hold the same file). */
    static void apply(Context c, String id) throws Exception {
        File version = GameImport.active(c, id), parent = GameImport.modsDir(version, id);
        GameImport.mkdir(parent);
        File romfs = new File(parent, "romfs"), stage = new File(parent, "romfs.exalt-new"), old = new File(parent, "romfs.exalt-old");
        synchronized (GameImport.POINTER_LOCK) {
            adopt(c, id, romfs);
            GameImport.remove(stage); GameImport.remove(old);
            boolean any = false;
            for (Layer l : list(c, id)) {
                if (!l.on) continue;
                File from = new File(new File(layers(c, id), l.name), "romfs");
                if (from.isDirectory()) { link(from, stage); any = true; }
            }
            if (romfs.exists()) Files.move(romfs.toPath(), old.toPath());
            if (any) Files.move(stage.toPath(), romfs.toPath());
            GameImport.remove(old);
            // the layers' pictures, beside the texture pack's own
            File pictures = new File(new File(parent, "textures"), LAYER_TEXTURES);
            GameImport.remove(pictures);
            for (Layer l : list(c, id)) {
                if (!l.on) continue;
                File from = new File(new File(layers(c, id), l.name), "textures");
                if (from.isDirectory()) link(from, new File(pictures, l.name));
            }
        }
    }
    /** The folder, inside the texture pack's, that holds the switched-on layers' pictures. */
    static final String LAYER_TEXTURES = ".exalt-layers";

    private static void link(File from, File to) throws IOException {
        GameImport.mkdir(to);
        File[] kids = from.listFiles();
        if (kids == null) return;
        for (File k : kids) {
            File target = new File(to, k.getName());
            if (k.isDirectory()) { link(k, target); continue; }
            Files.deleteIfExists(target.toPath());
            try { Files.createLink(target.toPath(), k.toPath()); }
            catch (IOException | UnsupportedOperationException e) { Files.copy(k.toPath(), target.toPath()); }
        }
    }

    static void delete(Context c, String id, String name) {
        GameImport.remove(new File(layers(c, id), name));
        setOn(c, id, name, true);
    }

    // ---- importing a mod or a texture pack from a chosen folder ----

    private static final class Doc {
        final String id, name; final boolean dir; final long size;
        Doc(String id, String name, boolean dir, long size) { this.id = id; this.name = name; this.dir = dir; this.size = size; }
    }

    private static List<Doc> children(ContentResolver r, Uri tree, String parent) throws IOException {
        Uri uri = DocumentsContract.buildChildDocumentsUriUsingTree(tree, parent);
        List<Doc> out = new ArrayList<>();
        try (Cursor cur = r.query(uri, new String[]{DocumentsContract.Document.COLUMN_DOCUMENT_ID, DocumentsContract.Document.COLUMN_DISPLAY_NAME,
                DocumentsContract.Document.COLUMN_MIME_TYPE, DocumentsContract.Document.COLUMN_SIZE}, null, null, null)) {
            if (cur == null) throw new IOException("Cannot read the chosen folder");
            while (cur.moveToNext()) out.add(new Doc(cur.getString(0), cur.getString(1),
                    DocumentsContract.Document.MIME_TYPE_DIR.equals(cur.getString(2)), cur.isNull(3) ? -1 : cur.getLong(3)));
        }
        return out;
    }

    /** A folder named name (romfs: the mod's files; textures: its pictures) at most depth levels in. */
    private static Doc findNamed(ContentResolver r, Uri tree, Doc at, int depth, String name) throws IOException {
        for (Doc d : children(r, tree, at.id)) if (d.dir && d.name.equalsIgnoreCase(name)) return d;
        if (depth == 0) return null;
        for (Doc d : children(r, tree, at.id)) if (d.dir) { Doc found = findNamed(r, tree, d, depth - 1, name); if (found != null) return found; }
        return null;
    }

    private static boolean hasPng(ContentResolver r, Uri tree, Doc at, int depth) throws IOException {
        for (Doc d : children(r, tree, at.id)) {
            if (!d.dir && d.name.toLowerCase(Locale.ROOT).endsWith(".png")) return true;
            if (d.dir && depth > 0 && hasPng(r, tree, d, depth - 1)) return true;
        }
        return false;
    }

    static String safeName(String s) {
        s = s == null ? "" : s.replaceAll("[^A-Za-z0-9 _().-]+", "_").replaceAll("^[ ._]+", "").trim();
        if (s.length() > 60) s = s.substring(0, 60);
        return s.isEmpty() ? "Mod" : s;
    }

    /** Imports the chosen folder: a romfs mod becomes a new layer (switched on), anything holding pictures a texture pack. */
    static String importFolder(Context c, String id, Uri tree, GameImport.Progress progress) throws Exception {
        ContentResolver r = c.getContentResolver();
        String rootId = DocumentsContract.getTreeDocumentId(tree);
        String rootName = rootId;
        try (Cursor cur = r.query(DocumentsContract.buildDocumentUriUsingTree(tree, rootId), new String[]{DocumentsContract.Document.COLUMN_DISPLAY_NAME}, null, null, null)) {
            if (cur != null && cur.moveToFirst()) rootName = cur.getString(0);
        }
        Doc root = new Doc(rootId, rootName, true, -1);
        Doc romfs = rootName.equalsIgnoreCase("romfs") ? root : findNamed(r, tree, root, 2, "romfs");
        Doc textures = romfs == root ? null : findNamed(r, tree, root, 2, "textures");
        if (romfs == null) {
            if (!hasPng(r, tree, root, 6)) throw new IOException("This folder holds neither a romfs folder (a mod) nor PNG pictures (a texture pack).");
            GameImport.replaceTextures(c, id, tree, progress);
            return "Texture pack imported.";
        }
        String name = safeName(romfs == root ? "Mod" : rootName);
        File home = layers(c, id); GameImport.mkdir(home);
        File stage = new File(home, ".import"), target = new File(home, name);
        GameImport.remove(stage); GameImport.mkdir(stage);
        long[] done = {0, 0};
        try {
            copy(r, tree, romfs, new File(stage, "romfs"), done, progress, 0);
            if (done[1] == 0) throw new IOException("The romfs folder is empty.");
            if (textures != null) copy(r, tree, textures, new File(stage, "textures"), done, progress, 0);
            GameImport.remove(target);
            Files.move(stage.toPath(), target.toPath());
        } finally { GameImport.remove(stage); }
        setOn(c, id, name, true);
        return "Mod “" + name + "” imported (" + done[1] + " files).";
    }

    private static void copy(ContentResolver r, Uri tree, Doc from, File to, long[] done, GameImport.Progress progress, int depth) throws IOException {
        if (depth > 16) throw new IOException("The mod's folders nest too deeply.");
        GameImport.mkdir(to);
        byte[] buffer = new byte[1 << 16];
        for (Doc d : children(r, tree, from.id)) {
            if (d.name.equals(".") || d.name.equals("..") || d.name.contains("/") || d.name.contains("\\")) throw new IOException("Unsafe name in the mod: " + d.name);
            File out = new File(to, d.name);
            if (d.dir) { copy(r, tree, d, out, done, progress, depth + 1); continue; }
            try (java.io.InputStream in = r.openInputStream(DocumentsContract.buildDocumentUriUsingTree(tree, d.id));
                 java.io.OutputStream o = new java.io.FileOutputStream(out)) {
                if (in == null) throw new IOException("Cannot read " + d.name);
                int n;
                while ((n = in.read(buffer)) > 0) { o.write(buffer, 0, n); done[0] += n; }
            }
            done[1]++;
            if (progress != null && done[1] % 16 == 0) progress.update(done[0], (int) done[1], d.name);
        }
    }

    // ---- settings the sidebar changes (saved; they apply when the game restarts) ----

    static void setWide(GameSettings s, boolean on) {
        if (on) s.enhancements.add(WIDE); else s.enhancements.remove(WIDE);
        s.values.put("top_width", on ? WIDE_TOP : NARROW_TOP);
    }
    static boolean wide(GameSettings s) { return s.enhancements.contains(WIDE); }

    /** 0 original 30 fps combat, 1 60 fps at the original speed, 2 60 fps and faster. */
    static int combat(GameSettings s) { return s.enhancements.contains(COMBAT_ORIGINAL) ? 1 : s.enhancements.contains(COMBAT_FASTER) ? 2 : 0; }
    static void setCombat(GameSettings s, int mode) {
        s.enhancements.remove(COMBAT_ORIGINAL); s.enhancements.remove(COMBAT_FASTER);
        // the test names an earlier build used
        s.enhancements.remove("Original combat 60 test"); s.enhancements.remove("Faster combat 60 test");
        if (mode == 1) s.enhancements.add(COMBAT_ORIGINAL);
        if (mode == 2) s.enhancements.add(COMBAT_FASTER);
    }

    /** ExaltDS's recommended settings, once, when the game is first set up: 16:9, no glow, 60 fps combat at its own speed. */
    static void firstSetup(Context c, String id) {
        if (prefs(c).getBoolean(id + ".defaults", false)) return;
        GameSettings s = GameSettings.load(c, id);
        setWide(s, true);
        s.enhancements.add("No glow");
        setCombat(s, 1);
        s.save(c);
        prefs(c).edit().putBoolean(id + ".defaults", true).commit();
    }

    // ---- temperatures: the hottest CPU zone and the hottest GPU zone (thermal zone names differ by chip maker) ----

    private static List<File> cpuZones, gpuZones;

    private static String read(File f) {
        try { return new String(Files.readAllBytes(f.toPath())).trim(); } catch (Exception e) { return null; }
    }

    private static void findZones() {
        cpuZones = new ArrayList<>(); gpuZones = new ArrayList<>();
        File[] zones = new File("/sys/class/thermal").listFiles(f -> f.getName().startsWith("thermal_zone"));
        if (zones == null) return;
        for (File z : zones) {
            String type = read(new File(z, "type"));
            File temp = new File(z, "temp");
            if (type == null || read(temp) == null) continue;
            type = type.toLowerCase(Locale.ROOT);
            if (type.contains("gpu") || type.contains("g3d") || type.contains("mali")) gpuZones.add(temp);
            else if (type.startsWith("cpu") || type.contains("cluster") || type.equals("big") || type.equals("mid") || type.equals("little")) cpuZones.add(temp);
        }
    }

    private static int hottest(List<File> zones) {
        int best = -1;
        for (File f : zones) {
            String s = read(f);
            if (s == null) continue;
            try {
                long v = Long.parseLong(s);
                if (v >= 1000) v /= 1000;
                if (v > 0 && v < 150 && v > best) best = (int) v;
            } catch (NumberFormatException ignored) { }
        }
        return best;
    }

    /** {CPU, GPU} in °C; -1 where the device does not say. */
    static int[] temperatures() {
        if (cpuZones == null) findZones();
        return new int[]{hottest(cpuZones), hottest(gpuZones)};
    }
}
