package io.github.zakuro.android;

import java.io.File;
import java.util.*;

/**
 * Adding a texture pack to the one already installed. Two pictures clash when they replace the same texture: the runtime
 * (zakuro-gpu pack.rs) reads the hash from a picture's name, tex1_<width>x<height>_<hash>_<format>, and keeps the first
 * picture it finds for each hash, wherever it sits in the pack. Files not named that way (pack.json, pictures it maps by
 * name) clash only at the same path.
 */
final class PackMerge {
    /** The folder inside the pack that ExaltDS fills with mod layers' pictures; they are not part of the pack. */
    static final String LAYERS = ".exalt-layers";

    /** What a picture replaces: "h:" and its hash, or "p:" and its path (lower case) for anything else. */
    static String key(String path) {
        String name = path.substring(path.lastIndexOf('/') + 1);
        Long hash = nameHash(name);
        return hash != null ? String.format(Locale.ROOT, "h:%016X", hash) : "p:" + path.toLowerCase(Locale.ROOT);
    }

    /** pack.rs name_hash: tex1_, decimal width, x, decimal height, _, hex hash (0x allowed), _, a decimal, then anything. */
    static Long nameHash(String name) {
        if (!name.startsWith("tex1_")) return null;
        int at = digits(name, 5);
        if (at < 0 || at >= name.length() || name.charAt(at) != 'x') return null;
        at = digits(name, at + 1);
        if (at < 0 || at >= name.length() || name.charAt(at) != '_') return null;
        at++;
        if (name.startsWith("0x", at) || name.startsWith("0X", at)) at += 2;
        int end = at;
        while (end < name.length() && Character.digit(name.charAt(end), 16) >= 0 && name.charAt(end) < 128) end++;
        if (end == at) return null;
        java.math.BigInteger value = new java.math.BigInteger(name.substring(at, end), 16);
        if (value.bitLength() > 64) return null;
        long hash = value.longValue();
        if (end >= name.length() || name.charAt(end) != '_' || digits(name, end + 1) < 0) return null;
        return hash;
    }
    /** Index after the ASCII digits starting at from, or -1 when there are none. */
    private static int digits(String s, int from) {
        int at = from;
        while (at < s.length() && s.charAt(at) >= '0' && s.charAt(at) <= '9') at++;
        return at == from ? -1 : at;
    }

    /** Every file of an installed pack by key (the layers' pictures left out), as paths relative to the pack. */
    static Map<String, List<String>> installed(File pack) {
        Map<String, List<String>> keys = new HashMap<>();
        walk(pack, "", keys, 0);
        return keys;
    }
    private static void walk(File dir, String relative, Map<String, List<String>> keys, int depth) {
        File[] children = dir.listFiles();
        if (children == null || depth > 64) return;
        for (File child : children) {
            if (relative.isEmpty() && child.getName().equals(LAYERS)) continue;
            if (java.nio.file.Files.isSymbolicLink(child.toPath())) continue;
            String path = relative + child.getName();
            if (child.isDirectory()) walk(child, path + "/", keys, depth + 1);
            else keys.computeIfAbsent(key(path), k -> new ArrayList<>()).add(path);
        }
    }

    /** Takes out of pack every file with one of these keys (before laying new pictures over it), leaving empty folders gone too. */
    static int removeKeys(File pack, Set<String> keys) {
        int removed = 0;
        for (Map.Entry<String, List<String>> entry : installed(pack).entrySet()) {
            if (!keys.contains(entry.getKey())) continue;
            for (String path : entry.getValue()) {
                File file = new File(pack, path);
                if (file.delete()) removed++;
                for (File up = file.getParentFile(); up != null && !up.equals(pack); up = up.getParentFile()) {
                    String[] left = up.list();
                    if (left == null || left.length > 0 || !up.delete()) break;
                }
            }
        }
        return removed;
    }
}
