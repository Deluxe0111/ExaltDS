//! HD movies: the player's own HD copy of a title's pre-rendered movies, drawn
//! in place of the frames the title decodes and converts through y2r into a
//! texture, while those frames are on screen.
//!
//! The movies folder (a title's mods/<id>/movies) is made by an importer from
//! the player's own video and holds, for each movie the title has:
//! - `<name>.thumbs`, a 20x12 thumbnail of every frame the title converts,
//!   both eyes, 240 bytes each, which tells which movie and frame a texture is;
//! - `<name>/<pair>.png`, the HD picture shown for frames 2*pair and
//!   2*pair+1, the two eyes of a stereoscopic movie, absent where the
//!   player's video has no picture for it (a fade an uploader put in, say);
//! - `movies.json`, the movies in order with their sizes;
//! - or, in place of the PNGs, `<name>.h264` and `<name>.units`, the
//!   pictures as one video, decoded by the platform's decoder, see video.rs.
//!
//! A movie frame is a 512x256 RGB8 texture with the picture upright in its
//! top-left 400x240, as Fire Emblem Awakening converts them. The HD picture
//! is placed the same way in a canvas the same multiple of 512x256, so that
//! the title's own texture coordinates draw it as they drew the frame.
//!
//! Nothing here holds a title's movies: they come from the player.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex, PoisonError, RwLock};

use crate::pack::{Level, Picture};

mod video;
pub use video::{rgba, set_decoder, Decoder, Opener, Yuv};

/// the size of the texture a movie frame is converted into, and of the
/// picture in its top-left corner.
const TEXTURE: (u32, u32) = (512, 256);
const SHOWN: (usize, usize) = (400, 240);
/// thumbnails are the mean of 20x20 blocks of the picture's luma.
const BLOCK: usize = 20;
const THUMB: usize = (SHOWN.0 / BLOCK) * (SHOWN.1 / BLOCK);

/// how far a thumbnail may be from a frame's, in mean luma steps, to be it:
/// a frame matches its own at well under 1, the next frame along at 1 to 3.
const NEAR: f32 = 2.5;
/// as far again near the frame last matched, where nothing else is looked at.
const NEAR_LOCKED: f32 = 3.0;
/// thumbnails flatter than this, a fade's black or white, tell nothing.
const FLAT: f32 = 3.0;
/// pairs of frames tried around the last one matched, behind it and ahead.
const BEHIND: u32 = 3;
const AHEAD: u32 = 15;
/// how much farther than the nearest the pair locked or the next may be
/// and still be kept.
const KEEP: f32 = 0.5;
/// textures in a row that match nothing near the last frame before it is
/// looked for everywhere again.
const MISSES: u32 = 12;
/// HD pictures kept read, and how many ahead of the one drawn are read.
const KEPT: usize = 16;
const READ_AHEAD: u32 = 6;

/// one HD picture, ready for the GPU, told apart by id: the picture alone,
/// which the renderer puts in the top-left of an image the size of canvas.
pub struct Frame {
    pub id: u64,
    pub picture: Arc<Picture>,
    pub canvas: (u32, u32),
}

struct Movie {
    name: String,
    /// thumbnails of each frame, THUMB values each.
    thumbs: Vec<u8>,
    /// whether each pair of frames has an HD picture.
    hd: Vec<bool>,
    /// the pictures as video, when there is one and a decoder for it.
    clip: Option<video::Clip>,
}

impl Movie {
    /// pairs of frames, the last one alone when there is an odd one out.
    fn pairs(&self) -> usize {
        (self.thumbs.len() / THUMB).div_ceil(2)
    }
}

pub struct Movies {
    dir: PathBuf,
    movies: Vec<Movie>,
    /// the canvas the picture goes in, and the picture's size.
    canvas: (u32, u32),
    picture: (u32, u32),
    state: Mutex<State>,
    /// pictures read, by movie and pair, and the reader thread's requests.
    read: Arc<Mutex<Read>>,
    reader: Mutex<Option<mpsc::Sender<(usize, u32)>>>,
    /// the thread decoding the video, when the pictures are video.
    player: video::Player,
}

#[derive(Default)]
struct State {
    /// the movie and pair of frames last matched, once two matches agreed on it.
    locked: Option<(usize, u32)>,
    /// a match from looking everywhere, waiting for the next to agree.
    pending: Option<(usize, u32)>,
    misses: u32,
    /// movie textures since the last match too flat to match, a fade's
    /// white or black, the movie going on meanwhile.
    skipped: u32,
    /// textures in a row matched to the pair locked.
    repeats: u32,
    /// the last textures asked about and what they were, kept alive so their
    /// addresses are not those of new texels.
    seen: VecDeque<(Arc<[[u8; 4]]>, Option<Arc<Frame>>)>,
    /// movie textures drawn as an HD picture, those whose picture was not
    /// decoded yet, and those of pairs with none.
    shown: u64,
    late: u64,
    missed: u64,
}

#[derive(Default)]
struct Read {
    pictures: HashMap<(usize, u32), Arc<Frame>>,
    order: VecDeque<(usize, u32)>,
    asked: std::collections::HashSet<(usize, u32)>,
}

#[derive(serde::Deserialize)]
struct Index {
    canvas: (u32, u32),
    picture: (u32, u32),
    movies: Vec<IndexMovie>,
}

#[derive(serde::Deserialize)]
struct IndexMovie {
    name: String,
    frames: usize,
    hd: Vec<bool>,
}

static ACTIVE: RwLock<Option<Arc<Movies>>> = RwLock::new(None);

/// the movies drawn in place of a title's own from now on, or none.
pub fn set(movies: Option<Arc<Movies>>) {
    *ACTIVE.write().unwrap_or_else(PoisonError::into_inner) = movies;
}

/// the HD picture for a texture, when it is a frame of a movie there is one for.
pub fn frame_for(texels: &Arc<[[u8; 4]]>, width: u32, height: u32) -> Option<Arc<Frame>> {
    if (width, height) != TEXTURE {
        return None;
    }
    let movies = ACTIVE.read().unwrap_or_else(PoisonError::into_inner).clone()?;
    movies.frame_for(texels)
}

impl Movies {
    /// the movies in dir, none when there are none or they can't be read.
    pub fn open(dir: &Path) -> Option<Movies> {
        let text = std::fs::read_to_string(dir.join("movies.json")).ok()?;
        let index: Index = match serde_json::from_str(&text) {
            Ok(index) => index,
            Err(error) => {
                log::warn!("HD movies: {} is not readable, {error}", dir.join("movies.json").display());
                return None;
            }
        };
        let scale = index.picture.0 / SHOWN.0 as u32;
        if scale == 0 || index.picture != (SHOWN.0 as u32 * scale, SHOWN.1 as u32 * scale) || index.canvas != (TEXTURE.0 * scale, TEXTURE.1 * scale) {
            log::warn!("HD movies: pictures of {:?} in {:?} are not a multiple of the frames", index.picture, index.canvas);
            return None;
        }
        let mut movies = Vec::new();
        for movie in index.movies {
            let thumbs = std::fs::read(dir.join(format!("{}.thumbs", movie.name))).unwrap_or_default();
            if thumbs.len() != movie.frames * THUMB {
                log::warn!("HD movies: {} has {} bytes of thumbnails for {} frames", movie.name, thumbs.len(), movie.frames);
                return None;
            }
            let clip = video::decoder().and_then(|_| video::Clip::open(dir, &movie.name));
            let mut hd = movie.hd;
            if let Some(clip) = &clip {
                // a video of part of the movie: the rest has no HD picture
                hd.iter_mut().skip(clip.len()).for_each(|hd| *hd = false);
            }
            movies.push(Movie { name: movie.name, thumbs, hd, clip });
        }
        let pictures: usize = movies.iter().map(|movie| movie.hd.iter().filter(|hd| **hd).count()).sum();
        let videos = movies.iter().filter(|movie| movie.clip.is_some()).count();
        log::info!("HD movies: {} movies ({videos} as video), {pictures} pictures of {:?}, from {}", movies.len(), index.picture, dir.display());
        Some(Movies {
            dir: dir.to_owned(),
            movies,
            canvas: index.canvas,
            picture: index.picture,
            state: Mutex::default(),
            read: Arc::default(),
            reader: Mutex::new(None),
            player: video::Player::default(),
        })
    }

    fn frame_for(self: &Arc<Self>, texels: &Arc<[[u8; 4]]>) -> Option<Arc<Frame>> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((_, frame)) = state.seen.iter().find(|(seen, _)| Arc::ptr_eq(seen, texels)) {
            return frame.clone();
        }
        let found = self.identify(&mut state, texels);
        let frame = found.and_then(|(movie, pair)| self.picture(movie, pair));
        match (found, frame.is_some()) {
            (Some(_), true) => state.shown += 1,
            (Some((movie, pair)), false) if self.movies[movie].hd.get(pair as usize).copied().unwrap_or(false) => state.late += 1,
            (Some(_), false) => state.missed += 1,
            (None, _) => {}
        }
        if found.is_some() && (state.shown + state.missed + state.late) % 300 == 299 {
            let (seeks, failed) = self.player.seeks();
            log::info!(
                "HD movies: {} textures drawn as HD pictures, {} late, {} with none; {seeks} seeks{}",
                state.shown,
                state.late,
                state.missed,
                if failed { ", the video stopped" } else { "" }
            );
        }
        if state.seen.len() == 4 {
            state.seen.pop_front();
        }
        state.seen.push_back((texels.clone(), frame.clone()));
        frame
    }

    /// which movie and pair of frames the texture is, once sure. the two
    /// eyes of a pair come one after the other, then the next pair, so the
    /// pair locked or the one after it is kept whenever it is about as near
    /// as the nearest: stereo pictures and still shots are near enough alike
    /// that nearest alone would step back and forth.
    fn identify(&self, state: &mut State, texels: &[[u8; 4]]) -> Option<(usize, u32)> {
        let Some(thumb) = thumbnail(texels) else {
            if state.locked.is_some() {
                state.skipped = state.skipped.saturating_add(1);
            }
            return None;
        };
        if let Some((movie, at)) = state.locked {
            let pairs = self.movies[movie].pairs();
            let from = at.saturating_sub(BEHIND) as usize;
            // the movie went on through the textures that matched nothing, two a pair
            let gone = (state.skipped + state.misses) as usize / 2;
            let to = (at as usize + AHEAD as usize + gone + 1).min(pairs);
            let nearest = self.nearest(&thumb, movie, from..to);
            // the other eye of the pair locked, then the next pair: once both
            // eyes were seen, the next, so that a still shot, where every pair
            // is as near as another, does not hold the movie back
            let next = if state.repeats >= 2 { at as usize + 1 } else { at as usize };
            let predicted = self.nearest(&thumb, movie, next.min(pairs)..(at as usize + 2).min(pairs));
            // and never back, when staying is about as near
            let stay = self.nearest(&thumb, movie, at as usize..(at as usize + 1).min(pairs));
            let chosen = match (nearest, predicted, stay) {
                (Some(best), Some(next), _) if next.1 <= best.1 + KEEP => Some(next),
                (Some(best), _, Some(stay)) if best.0 < at && stay.1 <= best.1 + KEEP => Some(stay),
                (best, ..) => best,
            };
            match chosen {
                Some((found, distance)) if distance < NEAR_LOCKED => {
                    log::debug!("HD movies: {} pair {found} after {at}, at {distance:.2}", self.movies[movie].name);
                    state.repeats = if found == at { state.repeats + 1 } else { 1 };
                    state.locked = Some((movie, found));
                    (state.misses, state.skipped) = (0, 0);
                    return Some((movie, found));
                }
                missed => {
                    log::debug!("HD movies: {} after pair {at}, nearest {missed:?}", self.movies[movie].name);
                    state.misses += 1;
                    if state.misses < MISSES {
                        return None;
                    }
                    log::info!("HD movies: lost {} at pair {at} ({} pictures drawn, {} not)", self.movies[movie].name, state.shown, state.missed);
                    state.locked = None;
                    (state.misses, state.skipped) = (0, 0);
                }
            }
        }
        // everywhere, two matches in a row agreeing before it counts
        let (movie, found, distance) = (0..self.movies.len())
            .filter_map(|movie| {
                let pairs = self.movies[movie].pairs();
                self.nearest(&thumb, movie, 0..pairs).map(|(found, distance)| (movie, found, distance))
            })
            .min_by(|a, b| a.2.total_cmp(&b.2))?;
        log::debug!("HD movies: nearest everywhere {} pair {found} at {distance:.2}", self.movies[movie].name);
        if distance >= NEAR {
            state.pending = None;
            return None;
        }
        let agrees = state.pending.is_some_and(|(was, at)| was == movie && (at..=at + AHEAD).contains(&found));
        state.pending = Some((movie, found));
        if !agrees {
            return None;
        }
        log::info!("HD movies: playing {} from pair {found}", self.movies[movie].name);
        state.repeats = 1;
        state.locked = Some((movie, found));
        state.pending = None;
        Some((movie, found))
    }

    /// the pair of movie in range nearest the thumbnail, by its nearer eye,
    /// and how near.
    fn nearest(&self, thumb: &[f32; THUMB], movie: usize, pairs: std::ops::Range<usize>) -> Option<(u32, f32)> {
        let thumbs = &self.movies[movie].thumbs;
        let frames = thumbs.len() / THUMB;
        let distance = |frame: usize| {
            let stored = &thumbs[frame * THUMB..(frame + 1) * THUMB];
            stored.iter().zip(thumb).map(|(&a, &b)| (a as f32 - b).abs()).sum::<f32>() / THUMB as f32
        };
        pairs
            .map(|pair| {
                let eyes = (2 * pair..(2 * pair + 2).min(frames)).map(distance);
                (pair as u32, eyes.fold(f32::INFINITY, f32::min))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// the HD picture of a pair of frames, read now if the reader has not
    /// read it yet, and those after it asked for.
    fn picture(self: &Arc<Self>, movie: usize, pair: u32) -> Option<Arc<Frame>> {
        let has = |pair: u32| self.movies[movie].hd.get(pair as usize).copied().unwrap_or(false);
        if self.movies[movie].clip.is_some() {
            return has(pair).then(|| self.player.frame(self, movie, pair)).flatten();
        }
        for ahead in 1..=READ_AHEAD {
            if has(pair + ahead) {
                self.ask(movie, pair + ahead);
            }
        }
        if !has(pair) {
            return None;
        }
        if let Some(frame) = self.read.lock().unwrap_or_else(PoisonError::into_inner).pictures.get(&(movie, pair)) {
            return Some(frame.clone());
        }
        let frame = Arc::new(self.load(movie, pair)?);
        keep(&mut self.read.lock().unwrap_or_else(PoisonError::into_inner), (movie, pair), frame.clone());
        Some(frame)
    }

    /// has the reader thread read a picture, starting the thread the first time.
    fn ask(self: &Arc<Self>, movie: usize, pair: u32) {
        {
            let mut read = self.read.lock().unwrap_or_else(PoisonError::into_inner);
            if read.pictures.contains_key(&(movie, pair)) || !read.asked.insert((movie, pair)) {
                return;
            }
        }
        let mut reader = self.reader.lock().unwrap_or_else(PoisonError::into_inner);
        if reader.is_none() {
            let (send, receive) = mpsc::channel::<(usize, u32)>();
            let movies = Arc::downgrade(self);
            let spawned = std::thread::Builder::new().name("HD movies".into()).spawn(move || {
                while let Ok((movie, pair)) = receive.recv() {
                    let Some(movies) = movies.upgrade() else { return };
                    let frame = movies.load(movie, pair).map(Arc::new);
                    let mut read = movies.read.lock().unwrap_or_else(PoisonError::into_inner);
                    read.asked.remove(&(movie, pair));
                    if let Some(frame) = frame {
                        keep(&mut read, (movie, pair), frame);
                    }
                }
            });
            match spawned {
                Ok(_) => *reader = Some(send),
                Err(error) => log::warn!("HD movies: no thread to read pictures on, {error}"),
            }
        }
        if let Some(send) = reader.as_ref() {
            let _ = send.send((movie, pair));
        }
    }

    /// reads a picture.
    fn load(&self, movie: usize, pair: u32) -> Option<Frame> {
        let path = self.dir.join(&self.movies[movie].name).join(format!("{pair:06}.png"));
        let rgba = match read_png(&path) {
            Ok(rgba) => rgba,
            Err(error) => {
                log::warn!("HD movies: {} is not readable, {error}", path.display());
                return None;
            }
        };
        let (width, height) = self.picture;
        if rgba.0 != (width, height) {
            log::warn!("HD movies: {} is {:?}, not {:?}", path.display(), rgba.0, self.picture);
            return None;
        }
        let picture = Picture { width, height, texels: rgba.1, levels: vec![Level { offset: 0, width, height }] };
        Some(Frame { id: 1 << 63 | (movie as u64) << 32 | pair as u64, picture: Arc::new(picture), canvas: self.canvas })
    }
}

fn keep(read: &mut Read, key: (usize, u32), frame: Arc<Frame>) {
    if read.pictures.insert(key, frame).is_none() {
        read.order.push_back(key);
    }
    while read.order.len() > KEPT {
        if let Some(old) = read.order.pop_front() {
            read.pictures.remove(&old);
        }
    }
}

/// the thumbnail of the picture in a movie frame's texture, none when it is
/// too flat to tell frames apart by.
fn thumbnail(texels: &[[u8; 4]]) -> Option<[f32; THUMB]> {
    let columns = SHOWN.0 / BLOCK;
    let mut thumb = [0f32; THUMB];
    let mut sums = [0u32; THUMB];
    for y in 0..SHOWN.1 {
        let line = &texels[y * TEXTURE.0 as usize..][..SHOWN.0];
        let base = y / BLOCK * columns;
        for (x, texel) in line.iter().enumerate() {
            sums[base + x / BLOCK] += (77 * texel[0] as u32 + 150 * texel[1] as u32 + 29 * texel[2] as u32) >> 8;
        }
    }
    for (value, sum) in thumb.iter_mut().zip(sums) {
        *value = sum as f32 / (BLOCK * BLOCK) as f32;
    }
    let mean = thumb.iter().sum::<f32>() / THUMB as f32;
    let spread = (thumb.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / THUMB as f32).sqrt();
    (spread >= FLAT).then_some(thumb)
}

/// a PNG as its size and RGBA8 rows from the top.
fn read_png(path: &Path) -> Result<((u32, u32), Vec<u8>), String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buffer = vec![0; reader.output_buffer_size().ok_or("too large")?];
    let info = reader.next_frame(&mut buffer).map_err(|e| e.to_string())?;
    let (width, height) = (info.width, info.height);
    let pixels = (width * height) as usize;
    let rgba = match info.color_type {
        png::ColorType::Rgba => buffer[..pixels * 4].to_vec(),
        png::ColorType::Rgb => buffer[..pixels * 3].chunks_exact(3).flat_map(|c| [c[0], c[1], c[2], 0xFF]).collect(),
        png::ColorType::Grayscale => buffer[..pixels].iter().flat_map(|&g| [g, g, g, 0xFF]).collect(),
        other => return Err(format!("{other:?} pictures are not read")),
    };
    Ok(((width, height), rgba))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texture(shade: impl Fn(usize, usize) -> u8) -> Vec<[u8; 4]> {
        (0..TEXTURE.1 as usize).flat_map(|y| (0..TEXTURE.0 as usize).map(move |x| (x, y))).map(|(x, y)| { let v = shade(x, y); [v, v, v, 0xFF] }).collect()
    }

    #[test]
    fn thumbnails_average_the_shown_corner() {
        // the texels past 400x240 are not the picture
        let thumb = thumbnail(&texture(|x, y| if x >= 400 || y >= 240 { 255 } else { (x / 20 * 10) as u8 })).unwrap();
        assert_eq!(thumb[0], 0.0);
        assert_eq!(thumb[19], 190.0);
        assert_eq!(thumb[20], 0.0);
    }

    #[test]
    fn flat_pictures_have_no_thumbnail() {
        assert!(thumbnail(&texture(|_, _| 0)).is_none());
        assert!(thumbnail(&texture(|_, _| 250)).is_none());
    }
}

