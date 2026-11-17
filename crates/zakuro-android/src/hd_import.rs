//! ExaltDS's HD movie import, on the device: from the player's own video of the game's anime movies (a disc rip or
//! an upload, any size, rate or framing) and the game's own movies in their ROM, it makes the HD movies
//! zakuro_gpu::movie draws in place of the game's (`<name>.thumbs`, `<name>.h264`, `<name>.units`, `movies.json`).
//! The same steps as tools/hd-movies/import_hd.py:
//!
//! 1. the game's movies, decoded from the ROM, as a 20x12 thumbnail of every frame;
//! 2. one pass over the whole video at 6 pictures a second finds the black bars, how the game's 5:3 picture sits
//!    in the video and where each movie starts, by the game's own frames (a movie not in it is left out);
//! 3. per movie, its stretch of the video at full rate is matched picture by picture (a path through how far each
//!    pair of game frames is from each picture near where it should be, never backwards), brightness fitted, and
//!    pairs where the video shows something else keep the game's own picture;
//! 4. the pictures, cropped and scaled to 1600x960, are encoded with the device's H.264 encoder (MediaCodec),
//!    one access unit a pair, then decoded back and checked against the game.
//!
//! Decoding goes through FFmpeg (LGPL), shipped as libraries beside the app's own and reached through
//! libexalt_ffmpeg.so (crates/zakuro-android/ffmpeg/hdmovies.c), loaded only when an import runs. Nothing of the
//! game's movies is shipped: everything comes from the player's ROM and video.

use std::ffi::{c_char, c_double, c_int, c_void, CString};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use jni::objects::{JClass, JString};
use jni::sys::jstring;
use jni::JNIEnv;
use ndk::media::media_codec::{DequeuedInputBufferResult, DequeuedOutputBufferInfoResult, MediaCodec, MediaCodecDirection};
use ndk::media::media_format::MediaFormat;

const NAMES: [&str; 14] = [
    "MovieOP", "Movie01", "Movie02", "Movie03", "Movie04", "Movie05", "Movie06", "Movie07", "Movie08", "Movie09", "Movie10", "Movie11",
    "Movie12", "Movie13",
];
const PICTURE: (usize, usize) = (1600, 960);
const CANVAS: (usize, usize) = (2048, 1024);
const TW: usize = 20;
const TH: usize = 12;
const THUMB: usize = TW * TH;
const COARSE_FPS: f64 = 6.0;
const COARSE_WIDTH: usize = 160;
/// seconds searched either side of where a movie's picture should be
const WINDOW: f64 = 1.5;
/// game frames a movie is located by
const SAMPLES: usize = 30;

type Thumb = [f32; THUMB];

static STATUS: Mutex<String> = Mutex::new(String::new());
static CANCEL: AtomicBool = AtomicBool::new(false);

fn status(text: impl Into<String>) {
    let text = text.into();
    log::info!("HD movie import: {text}");
    *STATUS.lock().unwrap_or_else(|e| e.into_inner()) = text;
}

fn cancelled() -> Result<(), String> {
    match CANCEL.load(Ordering::Acquire) {
        true => Err("Cancelled.".into()),
        false => Ok(()),
    }
}

// ---- FFmpeg, through libexalt_ffmpeg.so ----

type InfoFn = unsafe extern "C" fn(*const c_char, *mut c_int, *mut c_int, *mut c_double, *mut c_int, *mut c_double, *mut c_char, c_int) -> c_int;
type OpenFn = unsafe extern "C" fn(*const c_char, c_double, *const c_char, c_int, c_int, c_int, *mut c_char, c_int) -> *mut c_void;
type NextFn = unsafe extern "C" fn(*mut c_void, *mut u8, *mut c_double) -> c_int;
type CloseFn = unsafe extern "C" fn(*mut c_void);

struct Ffmpeg {
    _library: libloading::Library,
    info: InfoFn,
    open: OpenFn,
    next: NextFn,
    close: CloseFn,
}

fn ffmpeg() -> Result<&'static Ffmpeg, String> {
    static LOADED: OnceLock<Result<Ffmpeg, String>> = OnceLock::new();
    LOADED
        .get_or_init(|| {
            // SAFETY: the library is ours (hdmovies.c), its functions have these signatures
            unsafe {
                let library = libloading::Library::new("libexalt_ffmpeg.so").map_err(|e| format!("This build has no video decoding ({e})."))?;
                let info = *library.get::<InfoFn>(b"hd_info\0").map_err(|e| e.to_string())?;
                let open = *library.get::<OpenFn>(b"hd_open\0").map_err(|e| e.to_string())?;
                let next = *library.get::<NextFn>(b"hd_next\0").map_err(|e| e.to_string())?;
                let close = *library.get::<CloseFn>(b"hd_close\0").map_err(|e| e.to_string())?;
                Ok(Ffmpeg { _library: library, info, open, next, close })
            }
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn c_path(path: &Path) -> Result<CString, String> {
    CString::new(path.to_string_lossy().as_bytes()).map_err(|_| "a path with a NUL in it".to_string())
}

fn error_text(buffer: &[u8]) -> String {
    let end = buffer.iter().position(|&b| b == 0).unwrap_or(buffer.len());
    String::from_utf8_lossy(&buffer[..end]).into_owned()
}

struct Info {
    width: usize,
    height: usize,
    fps: f64,
    interlaced: bool,
    seconds: f64,
}

fn info(path: &Path) -> Result<Info, String> {
    let ff = ffmpeg()?;
    let (mut w, mut h, mut fps, mut interlaced, mut seconds) = (0, 0, 0.0, 0, 0.0);
    let mut error = [0u8; 256];
    // SAFETY: plain out-parameters and a buffer of the length given
    let code = unsafe { (ff.info)(c_path(path)?.as_ptr(), &mut w, &mut h, &mut fps, &mut interlaced, &mut seconds, error.as_mut_ptr().cast(), error.len() as c_int) };
    if code != 0 {
        return Err(format!("The video can't be read: {}", error_text(&error)));
    }
    let interlaced = interlaced != 0;
    Ok(Info { width: w as usize, height: h as usize, fps: if interlaced { 2.0 * fps } else { fps }, interlaced, seconds })
}

/// a video's pictures from a time on, through a filter chain ending at width x height, RGB24 or NV12.
struct Pictures {
    handle: *mut c_void,
    next: NextFn,
    close: CloseFn,
    pub picture: Vec<u8>,
}

impl Pictures {
    fn open(path: &Path, start: f64, filters: &str, width: usize, height: usize, nv12: bool) -> Result<Pictures, String> {
        let ff = ffmpeg()?;
        let filters = CString::new(filters).map_err(|_| "bad filters")?;
        let mut error = [0u8; 256];
        // SAFETY: as hd_open takes them
        let handle = unsafe {
            (ff.open)(c_path(path)?.as_ptr(), start, filters.as_ptr(), width as c_int, height as c_int, nv12 as c_int, error.as_mut_ptr().cast(), error.len() as c_int)
        };
        if handle.is_null() {
            return Err(format!("The video can't be decoded: {}", error_text(&error)));
        }
        let size = if nv12 { width * height * 3 / 2 } else { width * height * 3 };
        Ok(Pictures { handle, next: ff.next, close: ff.close, picture: vec![0; size] })
    }

    /// the next picture into self.picture, and its time; none at the end.
    fn next(&mut self) -> Result<Option<f64>, String> {
        let mut at = 0.0;
        // SAFETY: the buffer is the size the handle was opened for
        match unsafe { (self.next)(self.handle, self.picture.as_mut_ptr(), &mut at) } {
            1 => Ok(Some(at)),
            0 => Ok(None),
            code => Err(format!("decoding the video failed ({code})")),
        }
    }
}

impl Drop for Pictures {
    fn drop(&mut self) {
        // SAFETY: opened by hd_open, closed once
        unsafe { (self.close)(self.handle) };
    }
}

// ---- thumbnails ----

fn luma(r: u8, g: u8, b: u8) -> u32 {
    (77 * r as u32 + 150 * g as u32 + 29 * b as u32) >> 8
}

/// the thumbnail of a 400x240 RGB picture, as zakuro_gpu::movie makes them of the game's textures.
fn thumb(rgb: &[u8]) -> Thumb {
    let mut sums = [0u32; THUMB];
    for y in 0..240 {
        let row = &rgb[y * 400 * 3..][..400 * 3];
        let base = y / 20 * TW;
        for (x, px) in row.chunks_exact(3).enumerate() {
            sums[base + x / 20] += luma(px[0], px[1], px[2]);
        }
    }
    sums.map(|s| s as f32 / 400.0)
}

fn mean(t: &[f32]) -> f32 {
    t.iter().sum::<f32>() / t.len() as f32
}

fn spread(t: &[f32]) -> f32 {
    let m = mean(t);
    (t.iter().map(|v| (v - m) * (v - m)).sum::<f32>() / t.len() as f32).sqrt()
}

/// a thumbnail with its mean and contrast taken out, none when flat.
fn normalized(t: &[f32]) -> Option<Thumb> {
    let (m, s) = (mean(t), spread(t));
    if s < 8.0 {
        return None;
    }
    let mut out = [0f32; THUMB];
    for (o, v) in out.iter_mut().zip(t) {
        *o = (v - m) / s;
    }
    Some(out)
}

fn distance(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).sum::<f32>() / a.len() as f32
}

/// a plane's summed-area table, for the means of rectangles of it.
struct Integral {
    width: usize,
    height: usize,
    sums: Vec<f64>,
}

impl Integral {
    fn new(plane: &[u8], width: usize, height: usize) -> Integral {
        let mut sums = vec![0f64; (width + 1) * (height + 1)];
        for y in 0..height {
            let mut row = 0f64;
            for x in 0..width {
                row += plane[y * width + x] as f64;
                sums[(y + 1) * (width + 1) + x + 1] = sums[y * (width + 1) + x + 1] + row;
            }
        }
        Integral { width, height, sums }
    }

    /// the 20x12 thumbnail of a rectangle, each cell the mean of its share.
    fn thumb(&self, x: f64, y: f64, w: f64, h: f64) -> Thumb {
        let xs: Vec<usize> = (0..=TW).map(|i| ((x + i as f64 * w / TW as f64).round().max(0.0) as usize).min(self.width)).collect();
        let ys: Vec<usize> = (0..=TH).map(|i| ((y + i as f64 * h / TH as f64).round().max(0.0) as usize).min(self.height)).collect();
        let s = |x: usize, y: usize| self.sums[y * (self.width + 1) + x];
        let mut out = [0f32; THUMB];
        for j in 0..TH {
            for i in 0..TW {
                let area = ((xs[i + 1] - xs[i]) * (ys[j + 1] - ys[j])).max(1) as f64;
                let sum = s(xs[i + 1], ys[j + 1]) - s(xs[i], ys[j + 1]) - s(xs[i + 1], ys[j]) + s(xs[i], ys[j]);
                out[j * TW + i] = (sum / area) as f32;
            }
        }
        out
    }
}

fn luma_plane(rgb: &[u8]) -> Vec<u8> {
    rgb.chunks_exact(3).map(|p| luma(p[0], p[1], p[2]) as u8).collect()
}

// ---- the game's movies ----

fn game_movies(rom: &Path, cache: &Path) -> Result<Vec<Vec<Thumb>>, String> {
    let title = zakuro_fs::Title::load(rom).map_err(|e| format!("The game file can't be read: {e}"))?;
    let romfs = title.romfs.as_ref().ok_or("The game has no RomFS.")?;
    std::fs::create_dir_all(cache).map_err(|e| e.to_string())?;
    let mut movies = Vec::new();
    for (i, name) in NAMES.iter().enumerate() {
        cancelled()?;
        status(format!("Reading the game's movies ({} of {})…", i + 1, NAMES.len()));
        let entry = romfs.lookup(&format!("movie/{name}.moflex")).map_err(|e| format!("The game has no {name}: {e}"))?;
        let bytes = title.read_romfs(&entry, 0, entry.data_size as usize).ok_or("The game file is cut short.")?;
        let file = cache.join(format!("{name}.moflex"));
        std::fs::write(&file, bytes).map_err(|e| e.to_string())?;
        let mut pictures = Pictures::open(&file, 0.0, "scale=400:240:flags=area", 400, 240, false)?;
        let mut thumbs = Vec::new();
        while pictures.next()?.is_some() {
            thumbs.push(thumb(&pictures.picture));
        }
        drop(pictures);
        let _ = std::fs::remove_file(&file);
        if thumbs.is_empty() {
            return Err(format!("The game's {name} has no pictures."));
        }
        movies.push(thumbs);
    }
    Ok(movies)
}

// ---- locating the movies in the video ----

struct Located {
    starts: Vec<Option<f64>>,
    /// the video's rectangle the game's picture is: x, y, width, height
    rect: (f64, f64, f64, f64),
}

fn locate(game: &[Vec<Thumb>], video: &Path, pre: &str, v: &Info) -> Result<Located, String> {
    let (cw, ch) = (COARSE_WIDTH, ((v.height as f64 * COARSE_WIDTH as f64 / v.width as f64 / 2.0).round() as usize * 2).max(2));
    let mut pictures = Pictures::open(video, 0.0, &format!("{pre}scale={cw}:{ch}:flags=area"), cw, ch, false)?;
    let (mut planes, mut times) = (Vec::new(), Vec::new());
    let mut next = 0.0;
    while let Some(at) = pictures.next()? {
        if at + 1e-6 < next {
            continue;
        }
        next = at + 1.0 / COARSE_FPS;
        planes.push(luma_plane(&pictures.picture));
        times.push(at);
        if planes.len() % 60 == 0 {
            cancelled()?;
            status(format!("Looking through your video… {:.0} of {:.0} s", at, v.seconds));
        }
    }
    drop(pictures);
    if planes.is_empty() {
        return Err("The video has no pictures.".into());
    }
    // black bars: rows and columns never brighter than black
    let mut bright = vec![false; cw * ch];
    for plane in &planes {
        for (b, &p) in bright.iter_mut().zip(plane) {
            *b |= p > 32;
        }
    }
    let rows: Vec<usize> = (0..ch).filter(|&y| (0..cw).any(|x| bright[y * cw + x])).collect();
    let cols: Vec<usize> = (0..cw).filter(|&x| (0..ch).any(|y| bright[y * cw + x])).collect();
    let (Some(&top), Some(&bottom), Some(&left), Some(&right)) = (rows.first(), rows.last(), cols.first(), cols.last()) else {
        return Err("The video is black throughout.".into());
    };
    let (ax, ay, aw, ah) = (left as f64, top as f64, (right + 1 - left) as f64, (bottom + 1 - top) as f64);
    let shapes = [("sides cropped", ah * 5.0 / 3.0, ah), ("top and bottom cropped", aw, aw * 3.0 / 5.0), ("squeezed", aw, ah)];
    // the game's frames to find: textured eye-0 frames spread over each movie
    let samples: Vec<Vec<(usize, Thumb)>> = game
        .iter()
        .map(|g| {
            let eye: Vec<usize> = (0..g.len()).step_by(2).filter(|&f| spread(&g[f]) > 20.0).collect();
            let count = SAMPLES.min(eye.len());
            (0..count)
                .map(|i| eye[if count > 1 { i * (eye.len() - 1) / (count - 1) } else { 0 }])
                .filter_map(|f| normalized(&g[f]).map(|n| (f, n)))
                .collect()
        })
        .collect();
    let flat = [99f32; THUMB];
    let mut best: Option<(f32, &str, (f64, f64, f64, f64), Vec<Vec<(usize, usize, f32)>>)> = None;
    for (shape, sw, sh) in shapes {
        if sw > aw + 1.0 || sh > ah + 1.0 {
            continue;
        }
        cancelled()?;
        let (x, y) = (ax + (aw - sw) / 2.0, ay + (ah - sh) / 2.0);
        let thumbs: Vec<Thumb> = planes.iter().map(|p| normalized(&Integral::new(p, cw, ch).thumb(x, y, sw, sh)).unwrap_or(flat)).collect();
        let found: Vec<Vec<(usize, usize, f32)>> = samples
            .iter()
            .map(|s| {
                s.iter()
                    .map(|(f, t)| {
                        let (at, d) = thumbs.iter().enumerate().map(|(i, r)| (i, distance(r, t))).min_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
                        (*f, at, d)
                    })
                    .collect()
            })
            .collect();
        let mut all: Vec<f32> = found.iter().flatten().map(|h| h.2).collect();
        all.sort_by(f32::total_cmp);
        let score = all.get(all.len() / 2).copied().unwrap_or(f32::INFINITY);
        log::info!("HD movie import: the game's picture {shape}: median distance {score:.3}");
        if best.as_ref().is_none_or(|b| score < b.0) {
            best = Some((score, shape, (x, y, sw, sh), found));
        }
    }
    let (_, shape, (x, y, sw, sh), found) = best.ok_or("The video's pictures can't hold the game's.")?;
    let mut starts = vec![None; game.len()];
    let mut quality = vec![9f32; game.len()];
    for (m, hits) in found.iter().enumerate() {
        let guesses: Vec<(f64, f32)> = hits.iter().filter(|h| h.2 < 0.5).map(|&(f, at, d)| (times[at] - f as f64 / 60.0, d)).collect();
        let Some((count, guess)) = guesses.iter().map(|&(g, _)| (guesses.iter().filter(|o| (o.0 - g).abs() < 0.5).count(), g)).max_by(|a, b| a.0.cmp(&b.0)) else {
            continue;
        };
        if count < 5.max((0.4 * hits.len() as f64).ceil() as usize) {
            log::info!("HD movie import: {} not found ({count} of {} frames agree)", NAMES[m], hits.len());
            continue;
        }
        let mut agree: Vec<(f64, f32)> = guesses.iter().copied().filter(|o| (o.0 - guess).abs() < 0.5).collect();
        agree.sort_by(|a, b| a.0.total_cmp(&b.0));
        starts[m] = Some(agree[agree.len() / 2].0);
        let mut ds: Vec<f32> = agree.iter().map(|a| a.1).collect();
        ds.sort_by(f32::total_cmp);
        quality[m] = ds[ds.len() / 2];
        log::info!("HD movie import: {} at {:.2} s ({count} of {} frames agree, distance {:.3})", NAMES[m], starts[m].unwrap(), hits.len(), quality[m]);
    }
    // a movie can reuse another's footage: where two claim the same stretch, the nearer has it
    let lengths: Vec<f64> = game.iter().map(|g| g.len() as f64 / 60.0).collect();
    for a in 0..game.len() {
        for b in 0..game.len() {
            let (Some(sa), Some(sb)) = (starts[a], starts[b]) else { continue };
            if a == b {
                continue;
            }
            let shared = (sa + lengths[a]).min(sb + lengths[b]) - sa.max(sb);
            if shared > 0.5 * lengths[a].min(lengths[b]) && quality[a] > quality[b] {
                log::info!("HD movie import: {} left out, {}'s frames match its stretch better", NAMES[a], NAMES[b]);
                starts[a] = None;
            }
        }
    }
    let (fx, fy) = (v.width as f64 / cw as f64, v.height as f64 / ch as f64);
    let rect = (x * fx, y * fy, sw * fx, sh * fy);
    log::info!("HD movie import: the game's picture is {shape}, {:.0}x{:.0} at ({:.0}, {:.0})", rect.2, rect.3, rect.0, rect.1);
    Ok(Located { starts, rect })
}

/// the rectangle within a few percent of the first guess that matches the game's frames best.
fn refine(game: &[Vec<Thumb>], video: &Path, pre: &str, v: &Info, located: &Located) -> Result<(f64, f64, f64, f64), String> {
    let (x, y, sw, sh) = located.rect;
    let steps = [-0.01, -0.005, 0.0, 0.005, 0.01];
    let mut tries: Vec<(f64, f64, f64)> = Vec::new();
    for i in 0..17 {
        for &dx in &steps {
            for &dy in &steps {
                tries.push((0.95 + 0.005 * i as f64, dx, dy));
            }
        }
    }
    let mut totals = vec![0f64; tries.len()];
    let mut used = 0;
    for (m, start) in located.starts.iter().enumerate() {
        let Some(start) = start else { continue };
        if used >= 6 {
            break;
        }
        cancelled()?;
        let g = &game[m];
        let f = (0..g.len()).step_by(2).max_by(|&a, &b| spread(&g[a]).total_cmp(&spread(&g[b]))).unwrap_or(0);
        let Some(t) = normalized(&g[f]) else { continue };
        let mut best = vec![f64::INFINITY; tries.len()];
        let mut pictures = Pictures::open(video, (start + f as f64 / 60.0 - 0.25).max(0.0), &format!("{pre}null"), v.width, v.height, false)?;
        for _ in 0..30 {
            if pictures.next()?.is_none() {
                break;
            }
            let plane = luma_plane(&pictures.picture);
            let whole = Integral::new(&plane, v.width, v.height);
            for (i, &(s, dx, dy)) in tries.iter().enumerate() {
                let (cw, chh) = (sw * s, sh * s);
                let (cx, cy) = (x + (sw - cw) / 2.0 + dx * sw, y + (sh - chh) / 2.0 + dy * sh);
                if let Some(n) = normalized(&whole.thumb(cx, cy, cw, chh)) {
                    best[i] = best[i].min(distance(&n, &t) as f64);
                }
            }
        }
        for (total, b) in totals.iter_mut().zip(best) {
            *total += b;
        }
        used += 1;
    }
    if used == 0 {
        return Ok(located.rect);
    }
    let (s, dx, dy) = tries[totals.iter().enumerate().min_by(|a, b| a.1.total_cmp(b.1)).unwrap().0];
    let (cw, chh) = (sw * s, sh * s);
    let rect = (x + (sw - cw) / 2.0 + dx * sw, y + (sh - chh) / 2.0 + dy * sh, cw, chh);
    log::info!("HD movie import: refined to {:.0}x{:.0} at ({:.0}, {:.0}) from {used} movies", rect.2, rect.3, rect.0, rect.1);
    Ok(rect)
}

// ---- matching one movie ----

/// the video picture for each pair: the cheapest path through how far each pair is from each picture within WINDOW
/// seconds of where it should be (lead seconds in, then 30 pairs a second), never backwards, each picture of
/// change costing a little; pictures compared by shape alone, so the video's levels don't move it.
fn walk(g: &[Thumb], r: &[Thumb], lead: f64, pairs: usize, fps: f64) -> Vec<usize> {
    let span = (WINDOW * fps).round() as i64;
    let shifts: Vec<i64> = (-span..=span).collect();
    let n = shifts.len();
    let expected: Vec<i64> = (0..pairs).map(|k| (lead * fps + k as f64 * fps / 30.0).round() as i64).collect();
    let last = r.len() as i64 - 1;
    let shapes: Vec<(Thumb, f32)> = r.iter().map(|t| shape(t)).collect();
    let mut cost = vec![0f32; pairs * n];
    for k in 0..pairs {
        let game = &g[2 * k];
        let (gs, spread_g) = shape(game);
        let row = &mut cost[k * n..(k + 1) * n];
        for (c, s) in row.iter_mut().zip(&shifts) {
            let at = (expected[k] + s).clamp(0, last) as usize;
            *c = distance(&shapes[at].0, &gs) * spread_g.max(1.0);
        }
        // a flat picture (a fade's black or white) says nothing about timing
        if spread(game) < 3.0 {
            let low = row.iter().copied().fold(f32::INFINITY, f32::min);
            row.fill(low);
        }
    }
    let reach = 6.max((3.0 * fps / 30.0).ceil() as i64);
    let mut total: Vec<f32> = cost[..n].to_vec();
    let mut back = vec![0i16; pairs * n];
    for k in 1..pairs {
        let advance = expected[k] - expected[k - 1];
        let mut best = vec![f32::INFINITY; n];
        for step in -advance..=reach {
            for s in 0..n as i64 {
                let from = s - step;
                if from < 0 || from >= n as i64 {
                    continue;
                }
                let came = total[from as usize] + 0.3 * step.abs() as f32;
                if came < best[s as usize] {
                    best[s as usize] = came;
                    back[k * n + s as usize] = step as i16;
                }
            }
        }
        for s in 0..n {
            total[s] = best[s] + cost[k * n + s];
        }
    }
    let mut s = total.iter().enumerate().min_by(|a, b| a.1.total_cmp(b.1)).map(|(i, _)| i as i64).unwrap_or(0);
    let mut chosen = vec![0i64; pairs];
    for k in (0..pairs).rev() {
        chosen[k] = s;
        s -= back[k * n + s as usize] as i64;
    }
    let mut source = Vec::with_capacity(pairs);
    let mut highest = 0usize;
    for k in 0..pairs {
        let at = (expected[k] + shifts[chosen[k] as usize]).clamp(0, last) as usize;
        highest = highest.max(at);
        source.push(highest);
    }
    source
}

/// a thumbnail's shape: mean and contrast taken out, and the contrast.
fn shape(t: &[f32]) -> (Thumb, f32) {
    let (m, s) = (mean(t), spread(t));
    let mut out = [0f32; THUMB];
    for (o, v) in out.iter_mut().zip(t) {
        *o = (v - m) / s.max(1.0);
    }
    (out, s)
}

/// least squares y = a x + b over pairs of samples.
fn line(points: impl Iterator<Item = (f32, f32)>) -> Option<(f32, f32)> {
    let (mut n, mut sx, mut sy, mut sxx, mut sxy) = (0f64, 0f64, 0f64, 0f64, 0f64);
    for (x, y) in points {
        let (x, y) = (x as f64, y as f64);
        n += 1.0;
        sx += x;
        sy += y;
        sxx += x * x;
        sxy += x * y;
    }
    let d = n * sxx - sx * sx;
    (n > 2.0 && d.abs() > 1e-9).then(|| (((n * sxy - sx * sy) / d) as f32, ((sy * sxx - sx * sxy) / d) as f32))
}

/// whether each pair has an HD picture, and the brightness fit: the video's picture is the game's when, its
/// brightness fitted over the whole movie or over the second around the pair, it is about as near as the rest.
fn decide(g: &[Thumb], r: &[Thumb], source: &[usize], pairs: usize) -> (Vec<bool>, (f32, f32), f32) {
    let textured: Vec<usize> = (0..pairs).filter(|&k| spread(&g[2 * k]) > 20.0).collect();
    let mut fit = (1.0, 0.0);
    if textured.len() > 20 {
        if let Some((a, b)) = line(textured.iter().flat_map(|&k| r[source[k]].iter().copied().zip(g[2 * k].iter().copied()))) {
            if a > 0.6 && a < 1.6 {
                fit = (a, b);
            }
        }
    }
    let far = |a: f32, b: f32, k: usize| r[source[k]].iter().zip(&g[2 * k]).map(|(x, y)| ((x * a + b).clamp(0.0, 255.0) - y).abs()).sum::<f32>() / THUMB as f32;
    let mut mad: Vec<f32> = (0..pairs).map(|k| far(fit.0, fit.1, k)).collect();
    for k in 0..pairs {
        let ks = k.saturating_sub(15)..(k + 16).min(pairs);
        let points = ks.clone().flat_map(|j| r[source[j]].iter().copied().zip(g[2 * j].iter().copied()));
        let (xs, ys): (Vec<f32>, Vec<f32>) = ks.clone().flat_map(|j| r[source[j]].iter().copied().zip(g[2 * j].iter().copied())).unzip();
        if spread(&xs) < 3.0 || spread(&ys) < 3.0 {
            continue;
        }
        if let Some((a, b)) = line(points) {
            if a > 0.7 && a < 1.4 && b.abs() < 40.0 {
                mad[k] = mad[k].min(far(a, b, k));
            }
        }
    }
    let mut sorted = mad.clone();
    sorted.sort_by(f32::total_cmp);
    let median = sorted[sorted.len() / 2];
    let mut hd: Vec<bool> = mad.iter().map(|&m| m < 6f32.max(2.5 * median)).collect();
    // a gap of a few pairs between HD ones would flash the game's own picture: draw HD through it
    let mut i = 0;
    while i < hd.len() {
        if hd[i] {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < hd.len() && !hd[j] {
            j += 1;
        }
        if i > 0 && j < hd.len() && j - i <= 3 {
            hd[i..j].fill(true);
        }
        i = j;
    }
    (hd, fit, median)
}

// ---- encoding ----

const KEY_FRAME: u32 = 1;
const CODEC_CONFIG: u32 = 2;
const END_OF_STREAM: u32 = 4;
const FRAME_US: u64 = 33_333;

/// H.264 with the device's encoder: an access unit a picture, a key frame (with its SPS and PPS before it) every
/// 30, written to `<base>.h264` with `<base>.units`.
struct Encoder {
    codec: MediaCodec,
    stride: usize,
    slice: usize,
    file: std::io::BufWriter<std::fs::File>,
    written: u64,
    config: Vec<u8>,
    units: Vec<u64>,
    queued: u64,
}

impl Encoder {
    fn new(base: &Path) -> Result<Encoder, String> {
        let codec = MediaCodec::from_encoder_type("video/avc").ok_or("This device has no H.264 encoder.")?;
        let mut format = MediaFormat::new();
        format.set_str("mime", "video/avc");
        format.set_i32("width", PICTURE.0 as i32);
        format.set_i32("height", PICTURE.1 as i32);
        format.set_i32("color-format", 21); // NV12
        format.set_i32("bitrate", 10_000_000);
        format.set_i32("bitrate-mode", 1); // variable
        format.set_i32("frame-rate", 30);
        format.set_i32("i-frame-interval", 1);
        format.set_i32("max-bframes", 0);
        codec.configure(&format, None, MediaCodecDirection::Encoder).map_err(|e| format!("The H.264 encoder can't be set up: {e:?}"))?;
        codec.start().map_err(|e| format!("The H.264 encoder won't start: {e:?}"))?;
        let mut input = codec.input_format();
        let stride = input.i32("stride").unwrap_or(0).max(PICTURE.0 as i32) as usize;
        let slice = input.i32("slice-height").unwrap_or(0).max(PICTURE.1 as i32) as usize;
        log::info!("HD movie import: encoding with {} (input stride {stride}, slice {slice}, {:?})", codec.name().unwrap_or_default(), input.str("mime"));
        let file = std::fs::File::create(base.with_extension("h264")).map_err(|e| e.to_string())?;
        Ok(Encoder { codec, stride, slice, file: std::io::BufWriter::new(file), written: 0, config: Vec::new(), units: Vec::new(), queued: 0 })
    }

    /// queues an NV12 picture (PICTURE size, packed) or, with none, the end.
    fn push(&mut self, nv12: Option<&[u8]>) -> Result<(), String> {
        for _ in 0..1000 {
            match self.codec.dequeue_input_buffer(Duration::from_millis(5)).map_err(|e| format!("encoding: {e:?}"))? {
                DequeuedInputBufferResult::Buffer(mut buffer) => {
                    let (w, h) = PICTURE;
                    let room = buffer.buffer_mut();
                    let size = match nv12 {
                        Some(picture) => {
                            // the last row of each plane need only be as wide as the picture
                            let needed = self.stride * self.slice + self.stride * (h / 2 - 1) + w;
                            if room.len() < needed {
                                return Err(format!("the encoder's buffer is {} bytes, a picture needs {needed}", room.len()));
                            }
                            for y in 0..h {
                                for (to, &from) in room[y * self.stride..][..w].iter_mut().zip(&picture[y * w..][..w]) {
                                    to.write(from);
                                }
                            }
                            let uv = self.stride * self.slice;
                            for y in 0..h / 2 {
                                for (to, &from) in room[uv + y * self.stride..][..w].iter_mut().zip(&picture[w * h + y * w..][..w]) {
                                    to.write(from);
                                }
                            }
                            uv + self.stride * (h / 2)
                        }
                        None => 0,
                    };
                    let flags = if nv12.is_none() { END_OF_STREAM } else { 0 };
                    self.codec.queue_input_buffer(buffer, 0, size, self.queued * FRAME_US, flags).map_err(|e| format!("encoding: {e:?}"))?;
                    if nv12.is_some() {
                        self.queued += 1;
                    }
                    return self.drain(false).map(|_| ());
                }
                DequeuedInputBufferResult::TryAgainLater => {
                    self.drain(false)?;
                }
            }
        }
        Err("the encoder takes no more pictures".into())
    }

    /// writes out what the encoder has done, waiting for the end of the stream when finishing.
    fn drain(&mut self, finishing: bool) -> Result<bool, String> {
        loop {
            let wait = if finishing { Duration::from_millis(20) } else { Duration::ZERO };
            match self.codec.dequeue_output_buffer(wait).map_err(|e| format!("encoding: {e:?}"))? {
                DequeuedOutputBufferInfoResult::TryAgainLater => {
                    if finishing {
                        continue;
                    }
                    return Ok(false);
                }
                DequeuedOutputBufferInfoResult::OutputFormatChanged | DequeuedOutputBufferInfoResult::OutputBuffersChanged => {}
                DequeuedOutputBufferInfoResult::Buffer(buffer) => {
                    let info = *buffer.info();
                    let data = buffer.buffer()[info.offset().max(0) as usize..][..info.size().max(0) as usize].to_vec();
                    let flags = info.flags();
                    self.codec.release_output_buffer(buffer, false).map_err(|e| format!("encoding: {e:?}"))?;
                    if flags & CODEC_CONFIG != 0 {
                        self.config = data;
                    } else if !data.is_empty() {
                        let key = flags & KEY_FRAME != 0;
                        self.units.push(self.written << 1 | key as u64);
                        if key {
                            self.file.write_all(&self.config).map_err(|e| e.to_string())?;
                            self.written += self.config.len() as u64;
                        }
                        self.file.write_all(&data).map_err(|e| e.to_string())?;
                        self.written += data.len() as u64;
                    }
                    if flags & END_OF_STREAM != 0 {
                        return Ok(true);
                    }
                }
            }
        }
    }

    /// ends the stream and writes the units; how many pictures came out.
    fn finish(mut self, base: &Path) -> Result<usize, String> {
        self.push(None)?;
        self.drain(true)?;
        let _ = self.codec.stop();
        self.file.flush().map_err(|e| e.to_string())?;
        let count = self.units.len();
        let mut words: Vec<u8> = self.units.iter().flat_map(|w| w.to_le_bytes()).collect();
        words.extend((self.written << 1).to_le_bytes());
        std::fs::write(base.with_extension("units"), words).map_err(|e| e.to_string())?;
        if self.units.first().is_some_and(|w| w & 1 == 0) {
            return Err("the encoder's first picture is not a key frame".into());
        }
        Ok(count)
    }
}

// ---- the whole import ----

fn import(rom: &Path, video: &Path, out: &Path, cache: &Path) -> Result<String, String> {
    let started = std::time::Instant::now();
    ffmpeg()?;
    status("Reading your video…");
    let v = info(video)?;
    if v.width == 0 || v.height == 0 || v.fps <= 0.0 {
        return Err("The video's size or frame rate can't be read.".into());
    }
    log::info!("HD movie import: {}x{}, {} pictures a second{}, {:.0} s", v.width, v.height, v.fps, if v.interlaced { " (deinterlaced)" } else { "" }, v.seconds);
    let pre = if v.interlaced { "bwdif=mode=send_field," } else { "" };
    let game = game_movies(rom, cache)?;
    let located = locate(&game, video, pre, &v)?;
    if located.starts.iter().all(Option::is_none) {
        return Err("None of the game's movies were found in this video.".into());
    }
    status("Measuring how the picture sits in your video…");
    let (x, y, sw, sh) = refine(&game, video, pre, &v, &located)?;
    let even = |v: f64| ((v / 2.0).round() as usize * 2).max(2);
    let crop = format!("{pre}crop={}:{}:{}:{}", even(sw), even(sh), x.round().max(0.0) as usize, y.round().max(0.0) as usize);
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let mut index = Vec::new();
    let (mut total_hd, mut total_pairs, mut found) = (0usize, 0usize, 0usize);
    let mut checks = Vec::new();
    for (m, name) in NAMES.iter().enumerate() {
        cancelled()?;
        let g = &game[m];
        let n = g.len();
        let all_pairs = n.div_ceil(2);
        std::fs::write(out.join(format!("{name}.thumbs")), g.iter().flatten().map(|v| v.round().clamp(0.0, 255.0) as u8).collect::<Vec<u8>>())
            .map_err(|e| e.to_string())?;
        let Some(start) = located.starts[m] else {
            index.push(serde_json::json!({"name": name, "frames": n, "hd": vec![false; all_pairs]}));
            continue;
        };
        found += 1;
        status(format!("Matching {name} ({found} found so far)…"));
        let pairs = all_pairs;
        let first = (start - WINDOW).max(0.0);
        let lead = start - first;
        let count = ((lead + pairs as f64 / 30.0 + WINDOW) * v.fps).ceil() as usize;
        let mut pictures = Pictures::open(video, first, &format!("{crop},scale=400:240:flags=area"), 400, 240, false)?;
        let mut r = Vec::with_capacity(count);
        while r.len() < count {
            match pictures.next()? {
                Some(_) => r.push(thumb(&pictures.picture)),
                None => break,
            }
        }
        drop(pictures);
        if r.is_empty() {
            index.push(serde_json::json!({"name": name, "frames": n, "hd": vec![false; all_pairs]}));
            continue;
        }
        let source = walk(g, &r, lead, pairs, v.fps);
        let (hd, fit, median) = decide(g, &r, &source, pairs);
        let hd_count = hd.iter().filter(|h| **h).count();
        log::info!("HD movie import: {name} at {start:.2} s, brightness x{:.2}{:+.1}, median {median:.2}, HD {hd_count}/{pairs}", fit.0, fit.1);
        // the chosen pictures, at the picture's size, into the encoder
        let base = out.join(name);
        let mut encoder = Encoder::new(&base)?;
        let mut pictures = Pictures::open(video, first, &format!("{crop},scale={}:{}:flags=lanczos", PICTURE.0, PICTURE.1), PICTURE.0, PICTURE.1, true)?;
        let (mut wanted, mut at) = (0usize, 0usize);
        while wanted < pairs {
            if pictures.next()?.is_none() {
                break;
            }
            while wanted < pairs && source[wanted] == at {
                encoder.push(Some(&pictures.picture))?;
                wanted += 1;
                if wanted % 150 == 0 {
                    cancelled()?;
                    status(format!("Encoding {name}… {wanted} of {pairs} pictures"));
                }
            }
            at += 1;
        }
        drop(pictures);
        // past the video's end, the last picture again: pairs with no HD picture anyway
        let mut last = vec![16u8; PICTURE.0 * PICTURE.1 * 3 / 2]; // black: Y 16, U and V 128
        last[PICTURE.0 * PICTURE.1..].fill(128);
        while wanted < pairs {
            encoder.push(Some(&last))?;
            wanted += 1;
        }
        let written = encoder.finish(&base)?;
        if written != pairs {
            return Err(format!("{name}: the encoder gave {written} pictures for {pairs}"));
        }
        checks.push((name.to_string(), check(&base.with_extension("h264"), g, &hd, pairs, fit)?));
        total_hd += hd_count;
        total_pairs += pairs;
        index.push(serde_json::json!({"name": name, "frames": n, "hd": hd}));
    }
    let json = serde_json::json!({"canvas": [CANVAS.0, CANVAS.1], "picture": [PICTURE.0, PICTURE.1], "thumb": [TW, TH], "movies": index});
    std::fs::write(out.join("movies.json"), json.to_string()).map_err(|e| e.to_string())?;
    for (name, (pictures, right)) in &checks {
        log::info!("HD movie import: check {name}: {pictures} pictures, {:.1}% of HD pairs at their own frame", right * 100.0);
    }
    let worst = checks.iter().map(|c| c.1 .1).fold(1.0f64, f64::min);
    let minutes = started.elapsed().as_secs_f64() / 60.0;
    Ok(format!(
        "{found} of {} movies found in your video; {total_hd} of {total_pairs} moments in HD ({:.1}%). Checked: at least {:.0}% of HD pictures are their own moment. Took {minutes:.0} min.",
        NAMES.len(),
        100.0 * total_hd as f64 / total_pairs.max(1) as f64,
        worst * 100.0
    ))
}

/// decodes a clip back: how many pictures, and the share of HD pairs whose picture is nearest its own pair's frame
/// (or as near as the pairs either side, as still pictures are).
fn check(clip: &Path, g: &[Thumb], hd: &[bool], pairs: usize, fit: (f32, f32)) -> Result<(usize, f64), String> {
    let mut pictures = Pictures::open(clip, 0.0, "scale=400:240:flags=area", 400, 240, false)?;
    let (mut count, mut right, mut checked) = (0usize, 0usize, 0usize);
    while pictures.next()?.is_some() {
        let i = count;
        count += 1;
        if i >= pairs || !hd[i] {
            continue;
        }
        let t = thumb(&pictures.picture).map(|v| (v * fit.0 + fit.1).clamp(0.0, 255.0));
        let d = |j: usize| distance(&t, &g[2 * j]);
        let own = d(i);
        let near = [i.checked_sub(1), Some(i + 1).filter(|&j| 2 * j < g.len())].into_iter().flatten().map(d).fold(f32::INFINITY, f32::min);
        right += (own <= near + 0.25) as usize;
        checked += 1;
    }
    if count != pairs {
        return Err(format!("{}: {count} pictures decoded back for {pairs} pairs", clip.display()));
    }
    Ok((count, right as f64 / checked.max(1) as f64))
}

// ---- JNI: io.github.zakuro.android.ExaltMovies ----

fn string(env: &mut JNIEnv, s: &JString) -> String {
    if s.is_null() {
        return String::new();
    }
    env.get_string(s).map(Into::into).unwrap_or_default()
}

fn answer(env: &mut JNIEnv, text: &str) -> jstring {
    env.new_string(text).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
}

/// Makes the HD movies into `out` from the game in `rom` and the player's `video`, working in `cache`. Returns
/// "ok:" and a summary, or "error:" and what went wrong.
#[no_mangle]
pub extern "system" fn Java_io_github_zakuro_android_ExaltMovies_nativeImport<'a>(
    mut env: JNIEnv<'a>,
    _: JClass<'a>,
    rom: JString<'a>,
    video: JString<'a>,
    out: JString<'a>,
    cache: JString<'a>,
) -> jstring {
    crate::platform::init_log();
    CANCEL.store(false, Ordering::Release);
    let [rom, video, out, cache] = [&rom, &video, &out, &cache].map(|s| PathBuf::from(string(&mut env, s)));
    let result = std::panic::catch_unwind(|| import(&rom, &video, &out, &cache)).unwrap_or_else(|_| Err("the import failed unexpectedly".into()));
    let text = match result {
        Ok(summary) => format!("ok:{summary}"),
        Err(error) => {
            log::error!("HD movie import: {error}");
            format!("error:{error}")
        }
    };
    status("");
    answer(&mut env, &text)
}

#[no_mangle]
pub extern "system" fn Java_io_github_zakuro_android_ExaltMovies_nativeStatus<'a>(mut env: JNIEnv<'a>, _: JClass<'a>) -> jstring {
    let text = STATUS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    answer(&mut env, &text)
}

#[no_mangle]
pub extern "system" fn Java_io_github_zakuro_android_ExaltMovies_nativeCancel<'a>(_: JNIEnv<'a>, _: JClass<'a>) {
    CANCEL.store(true, Ordering::Release);
}

/// null when this build can import HD movies, else why not.
#[no_mangle]
pub extern "system" fn Java_io_github_zakuro_android_ExaltMovies_nativeAvailable<'a>(mut env: JNIEnv<'a>, _: JClass<'a>) -> jstring {
    match ffmpeg() {
        Ok(_) => std::ptr::null_mut(),
        Err(error) => answer(&mut env, &error),
    }
}
