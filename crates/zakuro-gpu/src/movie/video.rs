//! HD movies as video: each movie's pictures as one H.264 stream, decoded by
//! the platform (on Android, MediaCodec, see zakuro-android's movie_codec)
//! on a thread of their own, a few pairs ahead of the one on screen.
//!
//! A movie's clip is `<name>.h264`, raw Annex B with no B-frames, so that
//! the access units come out in the order they go in, access unit i being
//! the picture of pair i, and `<name>.units`, where each access unit starts,
//! a little-endian u64 each, the offset shifted left by one with the low bit
//! set on key frames, which a seek starts from, then the file's length.

use std::collections::VecDeque;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, PoisonError, RwLock, Weak};
use std::time::Duration;

use super::{Frame, Movies};
use crate::pack::{Level, Picture};

/// a decoded picture, the planes of 4:2:0 YUV in BT.709's limited range:
/// u and v are read every uv_step bytes, 2 for NV12's interleaved plane.
pub struct Yuv<'a> {
    pub width: usize,
    pub height: usize,
    pub y: &'a [u8],
    pub y_stride: usize,
    pub u: &'a [u8],
    pub v: &'a [u8],
    pub uv_stride: usize,
    pub uv_step: usize,
}

/// the platform's decoder, made and used on the movie thread alone.
pub trait Decoder {
    /// decodes an access unit, the index-th, giving out the pictures that
    /// come out meanwhile with the index each went in with.
    fn decode(&mut self, unit: &[u8], index: u32, out: &mut dyn FnMut(u32, &Yuv)) -> Result<(), String>;
    /// gives out the pictures still inside, at the end of a stream.
    fn finish(&mut self, out: &mut dyn FnMut(u32, &Yuv)) -> Result<(), String>;
    /// forgets everything decoded, before decoding from another key frame.
    fn flush(&mut self) -> Result<(), String>;
}

/// makes a decoder for pictures of a size.
pub type Opener = fn(width: u32, height: u32) -> Result<Box<dyn Decoder>, String>;

static OPENER: RwLock<Option<Opener>> = RwLock::new(None);

/// how the platform decodes HD movie clips, which it sets once at start.
pub fn set_decoder(opener: Opener) {
    *OPENER.write().unwrap_or_else(PoisonError::into_inner) = Some(opener);
}

pub(super) fn decoder() -> Option<Opener> {
    *OPENER.read().unwrap_or_else(PoisonError::into_inner)
}

/// a movie's clip, where its access units are.
pub(super) struct Clip {
    path: PathBuf,
    /// start and whether a key frame, then one past the last.
    units: Vec<(u64, bool)>,
}

impl Clip {
    pub(super) fn open(dir: &Path, name: &str) -> Option<Clip> {
        let path = dir.join(format!("{name}.h264"));
        let index = std::fs::read(dir.join(format!("{name}.units"))).ok()?;
        if !path.exists() || index.len() < 16 || index.len() % 8 != 0 {
            return None;
        }
        let units: Vec<(u64, bool)> = index
            .chunks_exact(8)
            .map(|word| u64::from_le_bytes(word.try_into().unwrap()))
            .map(|word| (word >> 1, word & 1 == 1))
            .collect();
        if units.windows(2).any(|pair| pair[1].0 < pair[0].0) || !units[0].1 {
            log::warn!("HD movies: {name}.units is out of order or doesn't start on a key frame");
            return None;
        }
        Some(Clip { path, units })
    }

    /// how many access units, pictures, it has.
    pub(super) fn len(&self) -> usize {
        self.units.len() - 1
    }

    /// the key frame a decode reaching unit starts from.
    fn key_before(&self, unit: usize) -> usize {
        (0..=unit.min(self.len() - 1)).rev().find(|&at| self.units[at].1).unwrap_or(0)
    }
}

/// pairs decoded ahead of the one on screen, then kept ahead of it (a
/// hardware decoder hands several out at once, past what was asked for)
/// and behind it.
const AHEAD: u32 = 8;
const KEPT_AHEAD: u32 = AHEAD + 8;
const BEHIND: u32 = 1;
/// how far past what is decoded a wanted pair may be before a seek to it
/// is quicker than decoding up to it.
const SKIP: u32 = 45;
/// seeks in a row that bring nothing wanted before the video is given up on.
const STALLS: u32 = 2;

/// the movie thread's side, shared with the renderer's.
#[derive(Default)]
pub(super) struct Player {
    shared: Arc<(Mutex<Wanted>, Condvar)>,
    started: Mutex<bool>,
}

#[derive(Default)]
struct Wanted {
    /// the movie and pair on screen.
    want: Option<(usize, u32)>,
    /// pictures decoded, by movie and pair.
    frames: VecDeque<(usize, u32, Arc<Frame>)>,
    /// seeks made, and those since the wanted picture last came out.
    seeks: u64,
    stalled: u32,
    failed: bool,
}

impl Player {
    /// the picture of a pair, when the movie thread has it, asking for it
    /// and those after it either way.
    pub(super) fn frame(&self, movies: &Arc<Movies>, movie: usize, pair: u32) -> Option<Arc<Frame>> {
        self.start(movies);
        let (lock, wake) = &*self.shared;
        let mut wanted = lock.lock().unwrap_or_else(PoisonError::into_inner);
        if wanted.failed {
            return None;
        }
        if wanted.want != Some((movie, pair)) {
            wanted.want = Some((movie, pair));
            wanted.frames.retain(|(m, p, _)| *m == movie && *p + BEHIND >= pair && *p <= pair + KEPT_AHEAD);
            wake.notify_all();
        }
        wanted.frames.iter().find(|(m, p, _)| (*m, *p) == (movie, pair)).map(|(.., frame)| frame.clone())
    }

    /// seeks made so far, and whether the video was given up on.
    pub(super) fn seeks(&self) -> (u64, bool) {
        let wanted = self.shared.0.lock().unwrap_or_else(PoisonError::into_inner);
        (wanted.seeks, wanted.failed)
    }

    fn start(&self, movies: &Arc<Movies>) {
        let mut started = self.started.lock().unwrap_or_else(PoisonError::into_inner);
        if *started {
            return;
        }
        *started = true;
        let (shared, movies) = (self.shared.clone(), Arc::downgrade(movies));
        let spawned = std::thread::Builder::new().name("HD movies".into()).spawn(move || {
            if let Err(error) = play(&shared, &movies) {
                log::warn!("HD movies: the video stopped, {error}");
                shared.0.lock().unwrap_or_else(PoisonError::into_inner).failed = true;
            }
        });
        if let Err(error) = spawned {
            log::warn!("HD movies: no thread to decode on, {error}");
        }
    }
}

/// what the movie thread is decoding.
struct Playing {
    movie: usize,
    file: std::fs::File,
    /// the next access unit to feed, and the last pair decoded.
    next: usize,
    decoded: Option<u32>,
}

/// whether a wanted pair is had: decoded and kept, or with no HD picture.
fn has(movies: &Movies, wanted: &Wanted, movie: usize, pair: u32) -> bool {
    !movies.movies[movie].hd.get(pair as usize).copied().unwrap_or(false) || wanted.frames.iter().any(|(m, p, _)| (*m, *p) == (movie, pair))
}

/// the movie thread: decodes the wanted movie from a key frame at or before
/// the wanted pair and up to AHEAD pairs past it, then waits for the next.
fn play(shared: &Arc<(Mutex<Wanted>, Condvar)>, movies: &Weak<Movies>) -> Result<(), String> {
    let (lock, wake) = &**shared;
    let opener = decoder().ok_or("no decoder")?;
    let (width, height) = movies.upgrade().ok_or("gone")?.picture;
    let mut decoder = opener(width, height)?;
    log::info!("HD movies: decoding {width}x{height} video");
    let mut playing: Option<Playing> = None;
    loop {
        let Some(movies) = movies.upgrade() else { return Ok(()) };
        // what is wanted, waiting while everything wanted is decoded
        let (movie, pair, have) = {
            let wanted = lock.lock().unwrap_or_else(PoisonError::into_inner);
            let Some((movie, pair)) = wanted.want else {
                drop(movies);
                drop(wake.wait_timeout(wanted, Duration::from_millis(200)).unwrap_or_else(PoisonError::into_inner));
                continue;
            };
            let have = has(&movies, &wanted, movie, pair);
            let last = movies.movies[movie].clip.as_ref().map_or(0, |clip| clip.len() as u32).saturating_sub(1);
            let done = playing.as_ref().is_some_and(|at| at.movie == movie && have && at.decoded.is_some_and(|decoded| decoded >= (pair + AHEAD).min(last)));
            if done {
                drop(movies);
                drop(wake.wait_timeout(wanted, Duration::from_millis(200)).unwrap_or_else(PoisonError::into_inner));
                continue;
            }
            (movie, pair, have)
        };
        let Some(clip) = movies.movies[movie].clip.as_ref() else {
            lock.lock().unwrap_or_else(PoisonError::into_inner).want = None;
            continue;
        };
        // another movie, a pair decoded already but not kept, or one far
        // ahead: start again from the key frame before it
        let seek = match &playing {
            Some(at) if at.movie == movie => (!have && at.decoded.is_some_and(|decoded| decoded >= pair)) || at.next as u32 + SKIP < pair,
            _ => true,
        };
        if seek {
            {
                let mut wanted = lock.lock().unwrap_or_else(PoisonError::into_inner);
                wanted.seeks += 1;
                wanted.stalled += 1;
                if wanted.stalled > STALLS {
                    return Err(format!("{} seeks in a row brought no wanted picture", wanted.stalled - 1));
                }
            }
            decoder.flush()?;
            let file = std::fs::File::open(&clip.path).map_err(|e| format!("{}: {e}", clip.path.display()))?;
            let next = clip.key_before(pair as usize);
            log::debug!("HD movies: {} from unit {next} for pair {pair}", movies.movies[movie].name);
            playing = Some(Playing { movie, file, next, decoded: None });
        }
        let at = playing.as_mut().expect("playing after a seek");
        if at.next >= clip.len() {
            // past the end, the last pictures are out
            at.decoded = Some(clip.len() as u32 - 1);
            continue;
        }
        let (start, end) = (clip.units[at.next].0, clip.units[at.next + 1].0);
        let mut unit = vec![0u8; (end - start) as usize];
        at.file.seek(SeekFrom::Start(start)).and_then(|_| at.file.read_exact(&mut unit)).map_err(|e| e.to_string())?;
        let index = at.next as u32;
        at.next += 1;
        let last = at.next == clip.len();
        let hd = &movies.movies[movie].hd;
        let mut decoded = at.decoded;
        let mut out = |done: u32, yuv: &Yuv| {
            decoded = Some(decoded.map_or(done, |d| d.max(done)));
            let want = lock.lock().unwrap_or_else(PoisonError::into_inner).want;
            let Some((_, wanted_pair)) = want.filter(|(m, _)| *m == movie) else { return };
            if done + BEHIND < wanted_pair || done > wanted_pair + KEPT_AHEAD || !hd.get(done as usize).copied().unwrap_or(false) {
                return;
            }
            let picture = Arc::new(rgba(yuv, movies.picture));
            let frame = Arc::new(Frame { id: 1 << 63 | (movie as u64) << 32 | done as u64, picture, canvas: movies.canvas });
            let mut wanted = lock.lock().unwrap_or_else(PoisonError::into_inner);
            if wanted.want.is_some_and(|(m, p)| m == movie && done + BEHIND >= p && done <= p + KEPT_AHEAD) {
                if wanted.want == Some((movie, done)) {
                    wanted.stalled = 0;
                }
                wanted.frames.push_back((movie, done, frame));
            }
        };
        decoder.decode(&unit, index, &mut out)?;
        if last {
            decoder.finish(&mut out)?;
        }
        at.decoded = decoded.or(at.decoded);
        if last {
            // everything is out: nothing more comes from this movie until a seek
            at.decoded = Some(clip.len() as u32 - 1);
        }
        // a wanted pair with no HD picture, or one already kept, is progress too
        let mut wanted = lock.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((m, p)) = wanted.want {
            if m == movie && has(&movies, &wanted, m, p) {
                wanted.stalled = 0;
            }
        }
    }
}

/// a decoded picture as RGBA of a size, BT.709 limited range, black past
/// what the decoder gave.
pub fn rgba(yuv: &Yuv, (picture_width, picture_height): (u32, u32)) -> Picture {
    let (width, height) = ((picture_width as usize).min(yuv.width), (picture_height as usize).min(yuv.height));
    let row = picture_width as usize * 4;
    let mut texels = vec![0u8; row * picture_height as usize];
    for y in 0..height {
        let luma = &yuv.y[y * yuv.y_stride..][..width];
        let chroma = (y / 2) * yuv.uv_stride;
        let (u, v) = (&yuv.u[chroma..], &yuv.v[chroma..]);
        let line = &mut texels[y * row..][..width * 4];
        for (x, (pixel, &l)) in line.chunks_exact_mut(4).zip(luma).enumerate() {
            let c = (x / 2) * yuv.uv_step;
            // 1.164 (Y-16), then 1.793 V, -0.213 U -0.533 V, 2.112 U, in 1/1024ths
            let l = (l as i32 - 16) * 1192;
            let (cb, cr) = (u[c] as i32 - 128, v[c] as i32 - 128);
            pixel[0] = clamp((l + 1836 * cr) >> 10);
            pixel[1] = clamp((l - 218 * cb - 546 * cr) >> 10);
            pixel[2] = clamp((l + 2163 * cb) >> 10);
            pixel[3] = 0xFF;
        }
    }
    Picture { width: picture_width, height: picture_height, texels, levels: vec![Level { offset: 0, width: picture_width, height: picture_height }] }
}

fn clamp(value: i32) -> u8 {
    value.clamp(0, 255) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_limited_range_bt709() {
        // black, white and a red of BT.709 limited range, NV12, 4x2
        let y = [16, 16, 235, 235, 63, 63, 63, 63];
        let uv = [128, 128, 102, 240];
        let yuv = Yuv { width: 4, height: 2, y: &y, y_stride: 4, u: &uv, v: &uv[1..], uv_stride: 4, uv_step: 2 };
        let picture = rgba(&yuv, (4, 2));
        assert_eq!(picture.texels.len(), 4 * 2 * 4);
        let at = |x: usize, y: usize| &picture.texels[(y * 4 + x) * 4..][..4];
        assert_eq!(at(0, 0), [0, 0, 0, 255]);
        // grey 63: 47 steps of 1.164
        assert_eq!(at(0, 1)[..3], [54, 54, 54]);
        let red = at(2, 1);
        assert!(red[0] > 240 && red[1] < 10 && red[2] < 10, "{red:?}");
    }

    /// a decoder that keeps three pictures in and gives them out two at a
    /// time, as hardware decoders do, each unit carrying its own index.
    struct Bursty {
        inside: VecDeque<u32>,
    }

    impl Decoder for Bursty {
        fn decode(&mut self, unit: &[u8], index: u32, out: &mut dyn FnMut(u32, &Yuv)) -> Result<(), String> {
            assert_eq!(u32::from_le_bytes(unit.try_into().unwrap()), index, "units are fed in order");
            self.inside.push_back(index);
            if self.inside.len() > 3 {
                for _ in 0..2.min(self.inside.len()) {
                    let done = self.inside.pop_front().unwrap();
                    out(done, &grey());
                }
            }
            Ok(())
        }
        fn finish(&mut self, out: &mut dyn FnMut(u32, &Yuv)) -> Result<(), String> {
            while let Some(done) = self.inside.pop_front() {
                out(done, &grey());
            }
            Ok(())
        }
        fn flush(&mut self) -> Result<(), String> {
            self.inside.clear();
            Ok(())
        }
    }

    fn grey() -> Yuv<'static> {
        static PLANE: [u8; 4] = [128; 4];
        Yuv { width: 2, height: 2, y: &PLANE, y_stride: 2, u: &PLANE, v: &PLANE, uv_stride: 2, uv_step: 2 }
    }

    #[test]
    fn plays_a_movie_through_with_one_seek() {
        let dir = std::env::temp_dir().join(format!("zakuro-movie-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pairs = 100u32;
        // pairs 40 to 44 have no HD picture, as where a rip fades
        let hd: Vec<bool> = (0..pairs).map(|pair| !(40..45).contains(&pair)).collect();
        let index = serde_json::json!({"canvas": [512, 256], "picture": [400, 240], "movies": [{"name": "M", "frames": 2 * pairs, "hd": hd}]});
        std::fs::write(dir.join("movies.json"), index.to_string()).unwrap();
        std::fs::write(dir.join("M.thumbs"), vec![0u8; 2 * pairs as usize * super::super::THUMB]).unwrap();
        let units: Vec<u8> = (0..pairs).flat_map(|i| i.to_le_bytes()).collect();
        std::fs::write(dir.join("M.h264"), &units).unwrap();
        let words: Vec<u8> = (0..=pairs as u64).flat_map(|i| (i * 4 << 1 | (i % 30 == 0 && i < pairs as u64) as u64).to_le_bytes()).collect();
        std::fs::write(dir.join("M.units"), words).unwrap();
        set_decoder(|_, _| Ok(Box::new(Bursty { inside: VecDeque::new() })));
        let movies = Arc::new(Movies::open(&dir).unwrap());
        for pair in 0..pairs {
            let started = std::time::Instant::now();
            let frame = loop {
                match movies.player.frame(&movies, 0, pair) {
                    Some(frame) => break Some(frame),
                    None if !hd[pair as usize] || started.elapsed() > Duration::from_secs(2) => break None,
                    None => std::thread::sleep(Duration::from_millis(1)),
                }
            };
            match hd[pair as usize] {
                true => assert_eq!(frame.map(|frame| frame.id & 0xFFFF_FFFF), Some(pair as u64), "pair {pair}"),
                false => assert!(frame.is_none(), "pair {pair} has no HD picture"),
            }
        }
        assert_eq!(movies.player.seeks(), (1, false));
        drop(movies);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn switches_between_movies_with_a_seek_each() {
        let dir = std::env::temp_dir().join(format!("zakuro-movies-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pairs = 60u32;
        let movie = |name: &str| serde_json::json!({"name": name, "frames": 2 * pairs, "hd": vec![true; pairs as usize]});
        let index = serde_json::json!({"canvas": [512, 256], "picture": [400, 240], "movies": [movie("A"), movie("B")]});
        std::fs::write(dir.join("movies.json"), index.to_string()).unwrap();
        for name in ["A", "B"] {
            std::fs::write(dir.join(format!("{name}.thumbs")), vec![0u8; 2 * pairs as usize * super::super::THUMB]).unwrap();
            std::fs::write(dir.join(format!("{name}.h264")), (0..pairs).flat_map(|i| i.to_le_bytes()).collect::<Vec<u8>>()).unwrap();
            let words: Vec<u8> = (0..=pairs as u64).flat_map(|i| (i * 4 << 1 | (i % 30 == 0 && i < pairs as u64) as u64).to_le_bytes()).collect();
            std::fs::write(dir.join(format!("{name}.units")), words).unwrap();
        }
        set_decoder(|_, _| Ok(Box::new(Bursty { inside: VecDeque::new() })));
        let movies = Arc::new(Movies::open(&dir).unwrap());
        // half of A, all of B, then back into A part-way, as a skip and the next movie, then a replay, would
        for (movie, range) in [(0usize, 0..30), (1, 0..pairs), (0, 20..pairs)] {
            for pair in range {
                let started = std::time::Instant::now();
                let frame = loop {
                    match movies.player.frame(&movies, movie, pair) {
                        Some(frame) => break Some(frame),
                        None if started.elapsed() > Duration::from_secs(2) => break None,
                        None => std::thread::sleep(Duration::from_millis(1)),
                    }
                };
                assert_eq!(frame.map(|frame| frame.id & !(1 << 63)), Some((movie as u64) << 32 | pair as u64), "movie {movie} pair {pair}");
            }
        }
        assert_eq!(movies.player.seeks(), (3, false));
        drop(movies);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn clips_start_from_key_frames() {
        let dir = std::env::temp_dir().join(format!("zakuro-clip-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("M.h264"), [0u8; 40]).unwrap();
        let words: Vec<u8> = [(0u64, true), (10, false), (20, true), (30, false), (40, false)]
            .iter()
            .flat_map(|&(at, key)| (at << 1 | key as u64).to_le_bytes())
            .collect();
        std::fs::write(dir.join("M.units"), words).unwrap();
        let clip = Clip::open(&dir, "M").unwrap();
        assert_eq!(clip.len(), 4);
        assert_eq!(clip.key_before(1), 0);
        assert_eq!(clip.key_before(3), 2);
        assert_eq!(clip.key_before(9), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
