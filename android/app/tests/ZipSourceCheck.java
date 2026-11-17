package io.github.zakuro.android;

import java.io.*;
import java.nio.ByteBuffer;
import java.nio.channels.FileChannel;
import java.nio.channels.SeekableByteChannel;
import java.nio.file.*;
import java.util.*;
import java.util.zip.*;

/**
 * Host checks for importing straight from a ZIP (ZipSource) and for adding a texture pack to an installed one (PackMerge).
 * With ZIP paths as arguments, also inspects those real packs: each as a texture pack, then each against the ones before it.
 */
public final class ZipSourceCheck {
    private static final String TITLE = "000400000A0F8000";
    private static int checks;
    private static void check(boolean condition, String message) {
        if (!condition) throw new AssertionError(message);
        checks++;
    }
    private interface Attempt { void run() throws Exception; }
    private static void rejects(Attempt attempt, String needle, String label) throws Exception {
        try { attempt.run(); }
        catch (IOException expected) {
            check(needle == null || expected.getMessage().contains(needle), label + ": unexpected message " + expected.getMessage());
            return;
        }
        throw new AssertionError(label + " was accepted");
    }

    private static final class Spec { final String name; final byte[] data; final boolean stored; Spec(String n, byte[] d, boolean s) { name = n; data = d; stored = s; } }
    private static Path zip(Path dir, String file, Spec... specs) throws IOException {
        Path out = dir.resolve(file);
        try (ZipOutputStream zip = new ZipOutputStream(Files.newOutputStream(out))) {
            for (Spec spec : specs) {
                ZipEntry entry = new ZipEntry(spec.name);
                if (spec.stored) {
                    CRC32 crc = new CRC32(); crc.update(spec.data);
                    entry.setMethod(ZipEntry.STORED); entry.setSize(spec.data.length); entry.setCompressedSize(spec.data.length); entry.setCrc(crc.getValue());
                }
                zip.putNextEntry(entry); zip.write(spec.data); zip.closeEntry();
            }
        }
        return out;
    }
    private static ZipSource open(Path zip, Path temp) throws IOException {
        return new ZipSource(FileChannel.open(zip, StandardOpenOption.READ), temp.toFile());
    }
    private static byte[] read(ZipSource source, String id) throws IOException {
        try (SeekableByteChannel channel = source.open(id)) {
            ByteArrayOutputStream out = new ByteArrayOutputStream(); ByteBuffer buffer = ByteBuffer.allocate(70000);
            int n; while ((n = channel.read(buffer)) > 0) { out.write(buffer.array(), 0, n); buffer.clear(); }
            check(out.size() == channel.size(), "read size matches channel size");
            // seeking back works on every kind of entry, as the PNG check needs
            channel.position(1); buffer.clear(); buffer.limit(1);
            if (channel.size() > 1) { channel.read(buffer); check(buffer.get(0) == out.toByteArray()[1], "seek back"); }
            return out.toByteArray();
        }
    }
    private static ImportPlan.Entry child(ImportPlan.Source source, String parent, String name) throws IOException {
        for (ImportPlan.Entry entry : source.children(parent)) if (entry.name.equals(name)) return entry;
        throw new AssertionError("missing " + name + " in " + parent);
    }
    private static String picture(String hash) { return "tex1_64x64_" + hash + "_13.png"; }

    public static void main(String[] args) throws Exception {
        if (args.length > 0) { realPacks(args); return; }
        Path dir = Files.createTempDirectory("zip-source-check"), temp = Files.createDirectory(dir.resolve("temp"));
        byte[] png = NativeFixture.png(), big = new byte[ZipSource.MEMORY_LIMIT + 12345];
        for (int i = 0; i < big.length; i += 4096) big[i] = (byte) i;

        // a pack as made on Windows (backslashes) inside a wrapper folder, with macOS leftovers
        Path pack = zip(dir, "pack.zip",
                new Spec("My Pack/", new byte[0], false),
                new Spec("My Pack\\UI\\" + picture("00000000000000AA"), png, false),
                new Spec("My Pack/UI/" + picture("00000000000000BB"), png, true),
                new Spec("My Pack/Big/large.bin", big, false),
                new Spec("My Pack/Readme.txt", "hi".getBytes(), false),
                new Spec("__MACOSX/My Pack/._Readme.txt", new byte[]{1}, false),
                new Spec("My Pack/.DS_Store", new byte[]{1}, false));
        try (ZipSource source = open(pack, temp)) {
            List<ImportPlan.Entry> top = source.children(ZipSource.ROOT);
            check(top.size() == 1 && top.get(0).directory && top.get(0).name.equals("My Pack"), "one wrapper folder, metadata left out: " + top.size());
            ImportPlan.Entry root = top.get(0), ui = child(source, root.id, "UI");
            check(source.children(root.id).size() == 3, "UI, Big, Readme.txt");
            check(source.children(ui.id).size() == 2, "both pictures under UI (backslash and slash)");
            check(Arrays.equals(read(source, child(source, ui.id, picture("00000000000000AA")).id), png), "deflated entry reads back");
            check(Arrays.equals(read(source, child(source, ui.id, picture("00000000000000BB")).id), png), "stored entry reads back");
            check(Arrays.equals(read(source, child(source, child(source, root.id, "Big").id, "large.bin").id), big), "large entry inflated through a file");
            String[] left = temp.toFile().list();
            check(left != null && left.length == 0, "temporary file removed after close");
            ImportPlan plan = ImportPlan.texturePack(source, root.id, TITLE);
            check(plan.files.size() == 2, "texture pack plan takes the two pictures: " + plan.files.size());
            for (ImportPlan.Item item : plan.files) check(item.destination.startsWith("data/mods/" + TITLE + "/textures/UI/tex1_64x64_"), item.destination);
            rejects(() -> source.open("f:99"), "Unknown", "missing entry");
            rejects(() -> source.open("d:My Pack/"), "Unknown", "folder opened as a file");
        }

        // a pack at the ZIP's top level, and a mod
        try (ZipSource source = open(zip(dir, "flat.zip", new Spec(picture("CC"), png, false), new Spec("romfs/a/b.bin", new byte[]{9}, false)), temp)) {
            check(source.children(ZipSource.ROOT).size() == 2, "flat ZIP top level");
            ImportPlan mod = ImportPlan.romfsMod(source, ZipSource.ROOT, "", TITLE);
            check(mod.files.size() == 1 && mod.files.get(0).destination.equals("data/mods/" + TITLE + "/romfs/a/b.bin"), "romfs mod from a ZIP");
        }

        // refused: escaping paths, damage, encryption-like flags, not a ZIP
        rejects(() -> open(zip(dir, "up.zip", new Spec("../evil.png", png, false)), temp).close(), "Unsafe path", "parent path");
        rejects(() -> open(zip(dir, "abs.zip", new Spec("/evil.png", png, false)), temp).close(), "Unsafe path", "absolute path");
        rejects(() -> open(zip(dir, "drive.zip", new Spec("C:/evil.png", png, false)), temp).close(), "Unsafe path", "drive path");
        Path text = dir.resolve("text.zip"); Files.write(text, "not a zip at all, just some words".getBytes());
        rejects(() -> open(text, temp).close(), "not a ZIP", "plain text");
        Path cut = dir.resolve("cut.zip"); byte[] whole = Files.readAllBytes(pack);
        Files.write(cut, Arrays.copyOf(whole, whole.length - 30));
        rejects(() -> open(cut, temp).close(), null, "truncated archive");
        // flip a byte of the deflated picture's data: the CRC check catches it
        Path bad = zip(dir, "crc.zip", new Spec("p/" + picture("DD"), png, true));
        byte[] bytes = Files.readAllBytes(bad); int at = indexOf(bytes, new byte[]{(byte) 137, 'P', 'N', 'G'});
        bytes[at + 40] ^= 1; Files.write(bad, bytes);
        try (ZipSource source = open(bad, temp)) {
            ImportPlan.Entry p = child(source, ZipSource.ROOT, "p");
            rejects(() -> ImportPlan.texturePack(source, p.id, TITLE), null, "damaged stored picture");
        }
        Path deflatedBad = zip(dir, "crc2.zip", new Spec("big.bin", big, false));
        bytes = Files.readAllBytes(deflatedBad); bytes[bytes.length / 2] ^= 0x55; Files.write(deflatedBad, bytes);
        try (ZipSource source = open(deflatedBad, temp)) {
            rejects(() -> read(source, "f:0"), "Damaged", "damaged deflated entry");
        }
        // encrypted flag set in the central directory
        Path locked = zip(dir, "locked.zip", new Spec("a.png", png, true));
        bytes = Files.readAllBytes(locked); int central = lastIndexOf(bytes, new byte[]{'P', 'K', 1, 2});
        bytes[central + 8] |= 1; Files.write(locked, bytes);
        rejects(() -> open(locked, temp).close(), "password", "encrypted entry");

        // ZIP64: more entries than a plain ZIP can count
        Spec[] many = new Spec[70000];
        for (int i = 0; i < many.length; i++) many[i] = new Spec("m/" + i + ".bin", new byte[]{(byte) i}, true);
        try (ZipSource source = open(zip(dir, "many.zip", many), temp)) {
            ImportPlan.Entry m = child(source, ZipSource.ROOT, "m");
            check(source.children(m.id).size() == 70000, "ZIP64 entry count");
            check(read(source, child(source, m.id, "69999.bin").id)[0] == (byte) 69999, "ZIP64 last entry");
        }

        // ---- PackMerge ----
        check(PackMerge.nameHash("tex1_256x128_00AB12CD34EF5678_13") == 0x00AB12CD34EF5678L, "hash");
        check(PackMerge.nameHash("tex1_256x128_00ab12cd34ef5678_13_mip0.png") == 0x00AB12CD34EF5678L, "hash, lower case, suffix");
        check(PackMerge.nameHash("tex1_8x8_1F_0anything") == 0x1F, "hash, anything after");
        check(PackMerge.nameHash("tex1_8x8_0x1F_0.png") == 0x1F, "hash with 0x");
        check(PackMerge.nameHash("tex1_8x8_FFFFFFFFFFFFFFFF_0.png") == -1L, "full 64-bit hash");
        check(PackMerge.nameHash("tex1_8x8_1F") == null, "no format");
        check(PackMerge.nameHash("tex1_8x8__0") == null, "no hash");
        check(PackMerge.nameHash("tex1_8_1F_0") == null, "no height");
        check(PackMerge.nameHash("tex1_8x8_10000000000000000_0") == null, "hash too long");
        check(PackMerge.key("A/B/" + picture("00000000000000AA")).equals(PackMerge.key("C/" + picture("aa").replace("_aa_", "_00AA_"))), "same hash, other folder clashes");
        check(PackMerge.key("pack.json").equals("p:pack.json") && !PackMerge.key("x/pack.json").equals("p:pack.json"), "other files by path");

        Path installed = Files.createDirectories(dir.resolve("installed"));
        Files.createDirectories(installed.resolve("Old/Deep"));
        Files.write(installed.resolve("Old/Deep/" + picture("00000000000000AA")), png);
        Files.write(installed.resolve("Old/" + picture("00000000000000EE")), png);
        Files.createDirectories(installed.resolve(PackMerge.LAYERS + "/Naga"));
        Files.write(installed.resolve(PackMerge.LAYERS + "/Naga/" + picture("00000000000000BB")), png);
        Map<String, List<String>> keys = PackMerge.installed(installed.toFile());
        check(keys.size() == 2, "layer pictures are not the pack's: " + keys.keySet());
        check(keys.containsKey(PackMerge.key(picture("AA").replace("_AA_", "_00000000000000AA_"))), "installed key");
        int removed = PackMerge.removeKeys(installed.toFile(), new HashSet<>(Collections.singletonList(PackMerge.key(picture("00000000000000AA")))));
        check(removed == 1 && !Files.exists(installed.resolve("Old/Deep")), "overwritten picture removed with its emptied folder");
        check(Files.exists(installed.resolve("Old/" + picture("00000000000000EE"))), "other pictures stay");
        check(Files.exists(installed.resolve(PackMerge.LAYERS + "/Naga/" + picture("00000000000000BB"))), "layer pictures stay");

        System.out.println("ZIP source and pack merge checks passed: " + checks + " checks.");
    }

    private static int indexOf(byte[] in, byte[] what) {
        outer: for (int i = 0; i + what.length <= in.length; i++) { for (int j = 0; j < what.length; j++) if (in[i + j] != what[j]) continue outer; return i; }
        throw new AssertionError("pattern not found");
    }
    private static int lastIndexOf(byte[] in, byte[] what) {
        outer: for (int i = in.length - what.length; i >= 0; i--) { for (int j = 0; j < what.length; j++) if (in[i + j] != what[j]) continue outer; return i; }
        throw new AssertionError("pattern not found");
    }

    /** Each real pack as ExaltDS would see it: its pictures, and how many clash with the packs before it. */
    private static void realPacks(String[] zips) throws Exception {
        Path temp = Files.createTempDirectory("zip-source-real");
        Map<String, String> seen = new HashMap<>(); // key -> pack it came from
        int total = 0;
        for (String file : zips) {
            long started = System.nanoTime();
            try (ZipSource source = open(Paths.get(file), temp)) {
                List<ImportPlan.Entry> top = source.children(ZipSource.ROOT);
                String root = top.size() == 1 && top.get(0).directory ? top.get(0).id : ZipSource.ROOT;
                ImportPlan plan = ImportPlan.texturePack(source, root, TITLE);
                int clashes = 0; Map<String, Integer> from = new TreeMap<>();
                for (ImportPlan.Item item : plan.files) {
                    String path = item.destination.substring(("data/mods/" + TITLE + "/textures/").length());
                    String key = PackMerge.key(path), before = seen.get(key);
                    if (before != null && !before.equals(file)) { clashes++; from.merge(Paths.get(before).getFileName().toString(), 1, Integer::sum); }
                }
                for (ImportPlan.Item item : plan.files) seen.putIfAbsent(PackMerge.key(item.destination.substring(("data/mods/" + TITLE + "/textures/").length())), file);
                // copy every file out through the source, as the import does, and check each PNG again
                long bytes = 0; ByteBuffer buffer = ByteBuffer.allocate(65536);
                for (ImportPlan.Item item : plan.files) {
                    try (SeekableByteChannel in = source.open(item.source.id)) {
                        item.checkHeader(in); ImportPlan.png(in, item.source.name); in.position(0);
                        int n; while ((n = in.read(buffer)) > 0) { bytes += n; buffer.clear(); }
                    }
                }
                total += plan.files.size();
                System.out.printf(Locale.ROOT, "%s: %d files (%.1f MB) read and checked in %.1f s; %d clash with earlier packs %s%n",
                        Paths.get(file).getFileName(), plan.files.size(), bytes / 1048576.0, (System.nanoTime() - started) / 1e9, clashes, from);
            }
        }
        System.out.println("All packs: " + total + " files, " + seen.size() + " distinct textures.");
    }
}
