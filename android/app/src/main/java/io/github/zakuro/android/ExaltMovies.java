package io.github.zakuro.android;

import android.content.Context;
import android.net.Uri;
import android.os.ParcelFileDescriptor;

import java.io.File;
import java.nio.file.Files;

/**
 * ExaltDS's HD movie import: from a video of the game's anime movies the player has (a disc rip or an upload, any
 * size, frame rate or framing), the native side (crates/zakuro-android/src/hd_import.rs) finds each movie in it by the
 * game's own movies in the player's ROM, matches it picture by picture, and encodes the HD movies the game then draws
 * in place of its own (mods/<id>/movies). The video is read where it is, through the picker's file descriptor; no
 * video ships with the app. Decoding uses FFmpeg (LGPL), built into libexalt_ffmpeg.so and its libraries.
 */
final class ExaltMovies {
    static { System.loadLibrary("zakuro_android"); }

    private static native String nativeImport(String rom, String video, String out, String cache);
    private static native String nativeStatus();
    private static native void nativeCancel();
    private static native String nativeAvailable();

    private ExaltMovies() { }

    /** Null when this build can import HD movies, else why not. */
    static String unavailable() {
        try { return nativeAvailable(); }
        catch (UnsatisfiedLinkError e) { return "This build can't import HD movies."; }
    }

    /** What the import is doing, for a status line. */
    static String status() { return nativeStatus(); }

    static void cancel() { nativeCancel(); }

    /**
     * Imports HD movies from the picked video into the game's mods folder, replacing any there. Takes many minutes;
     * call it off the main thread. Returns a summary; throws with what went wrong.
     */
    static String importFrom(Context c, String id, Uri video) throws Exception {
        try (ParcelFileDescriptor fd = c.getContentResolver().openFileDescriptor(video, "r")) {
            if (fd == null) throw new java.io.IOException("The video can't be opened.");
            return importPath(c, id, "/proc/self/fd/" + fd.getFd());
        }
    }

    /** The same from a file the app can read by its path (DebugSettings: one in its transfer folder). */
    static String importPath(Context c, String id, String video) throws Exception {
        File version = GameImport.active(c, id);
        File rom = new File(version, "game/game.3ds");
        if (!rom.isFile()) throw new java.io.IOException("The game isn't set up.");
        File parent = GameImport.modsDir(version, id);
        GameImport.mkdir(parent);
        File stage = new File(parent, "movies.exalt-new"), target = new File(parent, "movies"), cache = new File(c.getCacheDir(), "hd-movies");
        GameImport.remove(stage);
        GameImport.remove(cache);
        try {
            String result = nativeImport(rom.getPath(), video, stage.getPath(), cache.getPath());
            if (result == null || !result.startsWith("ok:")) throw new java.io.IOException(result == null ? "The import failed." : result.substring(result.indexOf(':') + 1));
            synchronized (GameImport.POINTER_LOCK) {
                GameImport.remove(target);
                Files.move(stage.toPath(), target.toPath());
            }
            return result.substring(3);
        } finally {
            GameImport.remove(stage);
            GameImport.remove(cache);
        }
    }

    /** Removes the game's HD movies. */
    static boolean remove(Context c, String id) {
        try {
            File movies = new File(GameImport.modsDir(GameImport.active(c, id), id), "movies");
            if (!movies.exists()) return false;
            GameImport.remove(movies);
            return true;
        } catch (java.io.IOException e) { return false; }
    }

    static boolean installed(Context c, String id) {
        try { return new File(GameImport.modsDir(GameImport.active(c, id), id), "movies/movies.json").isFile(); }
        catch (java.io.IOException e) { return false; }
    }
}
