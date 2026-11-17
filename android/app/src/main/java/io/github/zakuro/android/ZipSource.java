package io.github.zakuro.android;

import java.io.*;
import java.nio.ByteBuffer;
import java.nio.channels.FileChannel;
import java.nio.channels.NonWritableChannelException;
import java.nio.channels.SeekableByteChannel;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.StandardOpenOption;
import java.util.*;
import java.util.zip.CRC32;
import java.util.zip.Inflater;
import java.util.zip.InflaterInputStream;

/**
 * A ZIP archive read in place as an import source, so a downloaded pack can be imported without extracting it first.
 * Entries are read straight from the archive: a stored entry is a window on it, a deflated one is inflated (in memory
 * when small, into a temporary file that goes when closed otherwise) and its CRC checked.
 *
 * Folder ids are "d:" plus the path with its trailing slash ("d:" alone is the root); file ids are "f:" and the entry's index.
 * Names are taken from the central directory: Windows backslashes become slashes, folders only implied by a file's
 * path exist too, and macOS metadata (__MACOSX, ._ files, .DS_Store) and Windows' Thumbs.db/desktop.ini are left out.
 */
final class ZipSource implements ImportPlan.Source, Closeable {
    static final String ROOT = "d:";
    static final int MAX_ENTRIES = 200000;
    /** Inflated entries up to this size stay in memory; larger ones go through a temporary file. */
    static final int MEMORY_LIMIT = 32 << 20;

    private static final class File { String name; long offset, compressed, size, crc; int method; }
    private final FileChannel archive;
    private final java.io.File temp;
    private final List<File> files = new ArrayList<>();
    private final Map<String, List<ImportPlan.Entry>> folders = new HashMap<>();

    /** Reads the archive's central directory; temp is where large deflated entries are inflated (it must exist). */
    ZipSource(FileChannel archive, java.io.File temp) throws IOException {
        this.archive = archive; this.temp = temp;
        folders.put(ROOT, new ArrayList<>());
        read();
    }

    @Override public List<ImportPlan.Entry> children(String id) throws IOException {
        List<ImportPlan.Entry> children = folders.get(id);
        if (children == null) throw new IOException("Unknown folder in the ZIP");
        return children;
    }

    @Override public SeekableByteChannel open(String id) throws IOException {
        File file;
        try {
            if (!id.startsWith("f:")) throw new IllegalArgumentException();
            file = files.get(Integer.parseInt(id.substring(2)));
        } catch (RuntimeException e) { throw new IOException("Unknown file in the ZIP"); }
        long data = dataStart(file);
        if (file.method == 0) {
            if (file.compressed != file.size) throw new IOException("Damaged ZIP entry: " + file.name);
            return new Window(archive, data, file.size);
        }
        try (InputStream in = new InflaterInputStream(new Range(archive, data, file.compressed), new Inflater(true), 65536)) {
            if (file.size <= MEMORY_LIMIT) {
                byte[] bytes = new byte[(int) file.size];
                int at = 0, n;
                while (at < bytes.length && (n = in.read(bytes, at, bytes.length - at)) > 0) at += n;
                if (at != bytes.length || in.read() != -1) throw new IOException("Damaged ZIP entry: " + file.name);
                CRC32 crc = new CRC32(); crc.update(bytes);
                if (crc.getValue() != file.crc) throw new IOException("Damaged ZIP entry (checksum): " + file.name);
                return new Window(new Memory(bytes), 0, bytes.length);
            }
            java.io.File out = java.io.File.createTempFile("zip-", ".part", temp);
            FileChannel channel = null;
            try {
                channel = FileChannel.open(out.toPath(), StandardOpenOption.READ, StandardOpenOption.WRITE, StandardOpenOption.DELETE_ON_CLOSE);
                // gone from the folder now on Android; Windows (host tests) removes it on close instead
                out.delete();
                byte[] buffer = new byte[65536]; CRC32 crc = new CRC32(); long total = 0; int n;
                while ((n = in.read(buffer)) > 0) {
                    total += n;
                    if (total > file.size) throw new IOException("Damaged ZIP entry: " + file.name);
                    crc.update(buffer, 0, n);
                    ByteBuffer chunk = ByteBuffer.wrap(buffer, 0, n);
                    while (chunk.hasRemaining()) channel.write(chunk);
                }
                if (total != file.size || crc.getValue() != file.crc) throw new IOException("Damaged ZIP entry (checksum): " + file.name);
                Window window = new Window(channel, 0, file.size);
                window.owned = true; channel = null;
                return window;
            } finally {
                if (channel != null) channel.close();
                out.delete();
            }
        } catch (EOFException | java.util.zip.ZipException e) { throw new IOException("Damaged ZIP entry: " + file.name, e); }
    }

    @Override public void close() throws IOException { archive.close(); }

    // ---- the central directory ----

    private void read() throws IOException {
        long length = archive.size();
        if (length < 22) throw notZip();
        int tail = (int) Math.min(length, 22 + 65535);
        byte[] end = bytes(length - tail, tail);
        int at = -1;
        for (int p = tail - 22; p >= 0; p--)
            if (u32(end, p) == 0x06054b50L && p + 22 + u16(end, p + 20) == tail) { at = p; break; }
        if (at < 0) throw notZip();
        long count = u16(end, at + 10), size = u32(end, at + 12), start = u32(end, at + 16);
        if (u16(end, at + 4) != 0 || u16(end, at + 6) != 0) throw new IOException("Split (multi-part) ZIPs are not supported. Join the parts into one ZIP first.");
        long endRecord = length - tail + at;
        if (count == 0xFFFF || size == 0xFFFFFFFFL || start == 0xFFFFFFFFL) {
            // ZIP64: the locator sits right before the end record and names the ZIP64 end record
            if (endRecord < 20) throw damaged();
            byte[] locator = bytes(endRecord - 20, 20);
            if (u32(locator, 0) != 0x07064b50L) throw damaged();
            long record = u64(locator, 8);
            if (record < 0 || record > endRecord - 56) throw damaged();
            byte[] big = bytes(record, 56);
            if (u32(big, 0) != 0x06064b50L) throw damaged();
            count = u64(big, 32); size = u64(big, 40); start = u64(big, 48);
            endRecord = record;
        }
        if (count == 0) throw new IOException("The ZIP is empty.");
        if (count > MAX_ENTRIES) throw new IOException("The ZIP has too many files.");
        if (start < 0 || size < 0 || start > endRecord || size > endRecord - start || size > Integer.MAX_VALUE) throw damaged();
        byte[] directory = bytes(start, (int) size);
        Map<String, String> folderNames = new HashMap<>(); // lower-case path -> path as first spelled
        int p = 0;
        for (long i = 0; i < count; i++) {
            if (p + 46 > directory.length || u32(directory, p) != 0x02014b50L) throw damaged();
            int flags = u16(directory, p + 8), method = u16(directory, p + 10);
            File file = new File();
            file.crc = u32(directory, p + 16); file.compressed = u32(directory, p + 20); file.size = u32(directory, p + 24);
            int nameLength = u16(directory, p + 28), extra = u16(directory, p + 30), comment = u16(directory, p + 32);
            file.offset = u32(directory, p + 42);
            int next = p + 46 + nameLength + extra + comment;
            if (next > directory.length) throw damaged();
            byte[] raw = Arrays.copyOfRange(directory, p + 46, p + 46 + nameLength);
            // (bit 11 marks UTF-8; without it names are nearly always ASCII, which UTF-8 reads the same)
            String name = new String(raw, StandardCharsets.UTF_8);
            zip64(file, directory, p + 46 + nameLength, extra);
            p = next;
            name = name.replace('\\', '/');
            if (skipped(name)) continue;
            String path = clean(name);
            boolean folder = name.endsWith("/");
            if (folder) { if (!path.isEmpty()) folder(path, folderNames); continue; }
            if ((flags & 1) != 0) throw new IOException("The ZIP is password-protected. Extract it with its password first, then import the folder.");
            if (method != 0 && method != 8) throw new IOException("The ZIP uses a compression this app can't read (" + name + "). Extract it and import the folder, or re-zip it with normal compression.");
            if (file.offset < 0 || file.offset >= start || file.compressed < 0 || file.size < 0) throw damaged();
            int slash = path.lastIndexOf('/');
            String parent = slash < 0 ? ROOT : folder(path.substring(0, slash), folderNames);
            file.name = path.substring(slash + 1); file.method = method;
            folders.get(parent).add(new ImportPlan.Entry("f:" + files.size(), file.name, false, file.size));
            files.add(file);
        }
        if (p != directory.length) throw damaged();
        if (files.isEmpty()) throw new IOException("The ZIP holds no files.");
    }

    /** The folder for path (and its parents), made on first use; returns its id. Names match ignoring case, as Windows does. */
    private String folder(String path, Map<String, String> names) throws IOException {
        String known = names.get(path.toLowerCase(Locale.ROOT));
        if (known != null) return "d:" + known + "/";
        int slash = path.lastIndexOf('/');
        String parent = slash < 0 ? ROOT : folder(path.substring(0, slash), names);
        String id = "d:" + path + "/";
        names.put(path.toLowerCase(Locale.ROOT), path);
        folders.get(parent).add(new ImportPlan.Entry(id, path.substring(slash + 1), true, -1));
        folders.put(id, new ArrayList<>());
        return id;
    }

    static boolean skipped(String name) {
        String trimmed = name.endsWith("/") ? name.substring(0, name.length() - 1) : name;
        String base = trimmed.substring(trimmed.lastIndexOf('/') + 1);
        return name.startsWith("__MACOSX/") || name.contains("/__MACOSX/") || base.startsWith("._") || base.equals(".DS_Store")
                || base.equalsIgnoreCase("Thumbs.db") || base.equalsIgnoreCase("desktop.ini");
    }

    /** The entry's path without a leading "./" or slashes; any "..", drive letter or empty part is refused. */
    static String clean(String name) throws IOException {
        String path = name;
        while (path.startsWith("./")) path = path.substring(2);
        if (path.endsWith("/")) path = path.substring(0, path.length() - 1);
        if (path.startsWith("/") || path.matches("^[A-Za-z]:.*")) throw new IOException("Unsafe path in the ZIP: " + name);
        if (path.isEmpty()) return path;
        for (String part : path.split("/", -1)) {
            if (part.isEmpty() || part.equals(".") || part.equals("..") || part.indexOf('\0') >= 0) throw new IOException("Unsafe path in the ZIP: " + name);
            ImportPlan.safeName(part);
        }
        return path;
    }

    private static void zip64(File file, byte[] b, int at, int length) throws IOException {
        if (file.size != 0xFFFFFFFFL && file.compressed != 0xFFFFFFFFL && file.offset != 0xFFFFFFFFL) return;
        int end = at + length;
        while (at + 4 <= end) {
            int tag = u16(b, at), size = u16(b, at + 2), p = at + 4;
            if (p + size > end) break;
            if (tag == 1) {
                if (file.size == 0xFFFFFFFFL) { if (p + 8 > at + 4 + size) throw damaged(); file.size = u64(b, p); p += 8; }
                if (file.compressed == 0xFFFFFFFFL) { if (p + 8 > at + 4 + size) throw damaged(); file.compressed = u64(b, p); p += 8; }
                if (file.offset == 0xFFFFFFFFL) { if (p + 8 > at + 4 + size) throw damaged(); file.offset = u64(b, p); }
                return;
            }
            at = p + size;
        }
        throw damaged();
    }

    /** Where the entry's data starts: after its local header, whose name and extra lengths may differ from the directory's. */
    private long dataStart(File file) throws IOException {
        byte[] local = bytes(file.offset, 30);
        if (u32(local, 0) != 0x04034b50L) throw damaged();
        long data = file.offset + 30 + u16(local, 26) + u16(local, 28);
        if (data > archive.size() || file.compressed > archive.size() - data) throw damaged();
        return data;
    }

    private byte[] bytes(long at, int count) throws IOException {
        ByteBuffer buffer = ByteBuffer.allocate(count);
        while (buffer.hasRemaining()) {
            int n = archive.read(buffer, at + buffer.position());
            if (n <= 0) throw damaged();
        }
        return buffer.array();
    }

    private static IOException notZip() { return new IOException("This file is not a ZIP. Extract it (7z, RAR...) and import the folder instead."); }
    private static IOException damaged() { return new IOException("The ZIP is damaged or incomplete. Download it again."); }
    private static int u16(byte[] b, int at) { return (b[at] & 255) | (b[at + 1] & 255) << 8; }
    private static long u32(byte[] b, int at) { return u16(b, at) | (long) u16(b, at + 2) << 16; }
    private static long u64(byte[] b, int at) { return u32(b, at) | u32(b, at + 4) << 32; }

    // ---- channels ----

    /** A read-only window [start, start+size) on a channel read by position, so several can share one archive. */
    private static final class Window implements SeekableByteChannel {
        final Object source; final long start, size; long position; boolean open = true;
        Window(Object source, long start, long size) { this.source = source; this.start = start; this.size = size; }
        @Override public int read(ByteBuffer target) throws IOException {
            if (!open) throw new IOException("closed");
            if (position >= size) return -1;
            if (!target.hasRemaining()) return 0;
            int count = (int) Math.min(target.remaining(), size - position);
            ByteBuffer slice = target.duplicate(); slice.limit(slice.position() + count);
            int n = source instanceof FileChannel ? ((FileChannel) source).read(slice, start + position) : ((Memory) source).read(slice, start + position);
            if (n <= 0) throw new IOException("Truncated ZIP entry");
            target.position(target.position() + n); position += n;
            return n;
        }
        @Override public int write(ByteBuffer source) { throw new NonWritableChannelException(); }
        @Override public long position() { return position; }
        @Override public SeekableByteChannel position(long value) { if (value < 0) throw new IllegalArgumentException(); position = value; return this; }
        @Override public long size() { return size; }
        @Override public SeekableByteChannel truncate(long size) { throw new NonWritableChannelException(); }
        @Override public boolean isOpen() { return open; }
        // the archive itself stays open (ZipSource.close); a temporary file is the window's own
        @Override public void close() throws IOException { open = false; if (owned) ((FileChannel) source).close(); }
        boolean owned;
    }

    private static final class Memory {
        final byte[] bytes;
        Memory(byte[] bytes) { this.bytes = bytes; }
        int read(ByteBuffer target, long at) {
            int count = (int) Math.min(target.remaining(), bytes.length - at);
            if (count <= 0) return -1;
            target.put(bytes, (int) at, count); return count;
        }
    }

    /** The compressed bytes of an entry as a stream, read by position. */
    private static final class Range extends InputStream {
        final FileChannel channel; long at; final long end;
        Range(FileChannel channel, long start, long length) { this.channel = channel; this.at = start; this.end = start + length; }
        @Override public int read() throws IOException { byte[] one = new byte[1]; return read(one, 0, 1) < 0 ? -1 : one[0] & 255; }
        @Override public int read(byte[] b, int off, int len) throws IOException {
            if (at >= end) return -1;
            int n = channel.read(ByteBuffer.wrap(b, off, (int) Math.min(len, end - at)), at);
            if (n <= 0) return -1;
            at += n; return n;
        }
    }
}
