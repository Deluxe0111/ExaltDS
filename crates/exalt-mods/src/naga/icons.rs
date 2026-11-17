//! Recall Fate's clock icon: `systems/icon_E.ctpk.lz` with `menu.tga` made taller, and the HD
//! pack's sheet for the new texture.
//!
//! The map menu draws an item's icon with `0x2BF4D4(sheet, index, colour, ...)`: indices below 56
//! are 32x32 cells of `menu.tga` (256x256 RGBA4), 8 a row; 56-99 are 16x16 icons from y 224; 100
//! and up the 24x24 unit portraits at y 208 and 232. No cell is free (the bottom halves of cells
//! 52-55 hold portraits), so `menu.tga` grows to 256x512 and the clock goes at (0, 256), below
//! everything the game draws. The texture's first 256 rows stay byte for byte (tiles run across,
//! then down, so the new rows are appended), and `recall_fate.c` draws index `CLOCK_INDEX` from
//! there (its override of 0x2BF4D4).
//!
//! The tile is the game's own (Suspend's, cell 0x19) with its glyph painted out and recoloured
//! blue; the clock glyph is ours (`art/clock-32.png`, `art/clock-128.png`, from
//! `art/clock_glyph.py`). With the user's HD pack (`--hd <pack folder>`), its sheet for `menu.tga`
//! (found by the original texture's hash) gets the same 256 rows (x4) added, the HD tile made the
//! same way, and is written under the new texture's hash, once for each way packs hash, into the
//! layer's `textures` folder, which ExaltDS links into the game's texture pack.

use std::path::Path;

/// the 32x32 cell the clock is drawn from, in the 0-55 numbering continued downwards
pub const CLOCK_CELL: (u32, u32) = (0, 256);
const SOURCE_CELL: u32 = 0x19; // Suspend's tile
const CLOCK_32: &[u8] = include_bytes!("../../art/clock-32.png");
const CLOCK_128: &[u8] = include_bytes!("../../art/clock-128.png");
/// the hue the tile turns to (0-1), a deep blue
const BLUE: f32 = 0.60;
const RGBA4: u32 = 4;

struct Entry {
    at: usize,
    name: String,
    size: u32,
    offset: u32,
    format: u32,
    width: u16,
    height: u16,
    sizes_at: usize,
}

fn u16_at(d: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([d[at], d[at + 1]])
}
fn u32_at(d: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(d[at..at + 4].try_into().unwrap())
}

fn entries(ctpk: &[u8]) -> Result<(usize, Vec<Entry>), String> {
    if ctpk.get(..4) != Some(b"CTPK") {
        return Err("icon_E.ctpk: not a CTPK".into());
    }
    let count = u16_at(ctpk, 6) as usize;
    let data = u32_at(ctpk, 8) as usize;
    let mut out = Vec::new();
    for i in 0..count {
        let at = 0x20 + i * 0x20;
        let name_at = u32_at(ctpk, at) as usize;
        let end = ctpk[name_at..].iter().position(|&b| b == 0).ok_or("icon_E.ctpk: bad name")? + name_at;
        out.push(Entry {
            at,
            name: String::from_utf8_lossy(&ctpk[name_at..end]).into_owned(),
            size: u32_at(ctpk, at + 4),
            offset: u32_at(ctpk, at + 8),
            format: u32_at(ctpk, at + 0xC),
            width: u16_at(ctpk, at + 0x10),
            height: u16_at(ctpk, at + 0x12),
            sizes_at: u32_at(ctpk, at + 0x18) as usize * 4,
        });
    }
    Ok((data, out))
}

fn morton(x: u32, y: u32) -> u32 {
    (0..3).fold(0, |m, i| m | ((x >> i) & 1) << (2 * i) | ((y >> i) & 1) << (2 * i + 1))
}

/// RGBA4 texels (as memory holds them, 8x8 tiles across then down, from the top) to RGBA8 rows
fn decode(data: &[u8], w: u32, h: u32) -> Vec<[u8; 4]> {
    let mut px = vec![[0; 4]; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = ((y / 8 * (w / 8) + x / 8) * 64 + morton(x % 8, y % 8)) as usize;
            let v = u16_at(data, i * 2);
            px[(y * w + x) as usize] = [(v >> 12) as u8 * 17, (v >> 8 & 15) as u8 * 17, (v >> 4 & 15) as u8 * 17, (v & 15) as u8 * 17];
        }
    }
    px
}

fn encode(px: &[[u8; 4]], w: u32, h: u32) -> Vec<u8> {
    let mut out = vec![0; (w * h * 2) as usize];
    let q = |c: u8| (c as u16 * 15 + 127) / 255;
    for y in 0..h {
        for x in 0..w {
            let [r, g, b, a] = px[(y * w + x) as usize];
            let i = ((y / 8 * (w / 8) + x / 8) * 64 + morton(x % 8, y % 8)) as usize;
            out[i * 2..i * 2 + 2].copy_from_slice(&(q(r) << 12 | q(g) << 8 | q(b) << 4 | q(a)).to_le_bytes());
        }
    }
    out
}

/// an RGBA8 picture
#[derive(Clone)]
pub struct Picture {
    pub w: u32,
    pub h: u32,
    pub px: Vec<[u8; 4]>,
}

impl Picture {
    fn crop(&self, x0: u32, y0: u32, w: u32, h: u32) -> Picture {
        let px = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).map(|(x, y)| self.px[((y0 + y) * self.w + x0 + x) as usize]).collect();
        Picture { w, h, px }
    }
    fn paste(&mut self, other: &Picture, x0: u32, y0: u32) {
        for y in 0..other.h {
            for x in 0..other.w {
                self.px[((y0 + y) * self.w + x0 + x) as usize] = other.px[(y * other.w + x) as usize];
            }
        }
    }
    /// paints the rectangle [x0, x1) x [y0, y1) row by row between the texels just outside it
    fn blank(&mut self, x0: u32, x1: u32, y0: u32, y1: u32) {
        for y in y0..y1 {
            let a = self.px[(y * self.w + x0 - 1) as usize];
            let b = self.px[(y * self.w + x1) as usize];
            for x in x0..x1 {
                let t = (x - x0 + 1) as f32 / (x1 - x0 + 1) as f32;
                self.px[(y * self.w + x) as usize] = std::array::from_fn(|i| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round() as u8);
            }
        }
    }
    /// keeps each texel's saturation and value, at hue
    fn hue(&mut self, hue: f32) {
        for p in &mut self.px {
            let [r, g, b] = [p[0], p[1], p[2]].map(|c| c as f32 / 255.0);
            let (max, min) = (r.max(g).max(b), r.min(g).min(b));
            let s = if max > 0.0 { (max - min) / max } else { 0.0 };
            let (v, h6) = (max, hue * 6.0);
            let (i, f) = (h6.floor() as i32 % 6, h6 - h6.floor());
            let (pp, qq, tt) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
            let (r, g, b) = match i {
                0 => (v, tt, pp),
                1 => (qq, v, pp),
                2 => (pp, v, tt),
                3 => (pp, qq, v),
                4 => (tt, pp, v),
                _ => (v, pp, qq),
            };
            p[0] = (r * 255.0).round() as u8;
            p[1] = (g * 255.0).round() as u8;
            p[2] = (b * 255.0).round() as u8;
        }
    }
    /// other over this, alpha blended
    fn over(&mut self, other: &Picture) {
        for (d, s) in self.px.iter_mut().zip(&other.px) {
            let (sa, da) = (s[3] as f32 / 255.0, d[3] as f32 / 255.0);
            let a = sa + da * (1.0 - sa);
            if a <= 0.0 {
                continue;
            }
            for i in 0..3 {
                d[i] = ((s[i] as f32 * sa + d[i] as f32 * da * (1.0 - sa)) / a).round() as u8;
            }
            d[3] = (a * 255.0).round() as u8;
        }
    }
}

pub fn read_png(bytes: &[u8]) -> Result<Picture, String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16 | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or("png: too large")?];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let (w, h) = (info.width, info.height);
    let px = match info.color_type {
        png::ColorType::Rgba => buf.chunks_exact(4).map(|c| [c[0], c[1], c[2], c[3]]).collect(),
        png::ColorType::GrayscaleAlpha => buf.chunks_exact(2).map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        other => return Err(format!("png: unexpected colour type {other:?}")),
    };
    Ok(Picture { w, h, px })
}

fn write_png(path: &Path, picture: &Picture) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), picture.w, picture.h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(picture.px.as_flattened()).map_err(|e| e.to_string())
}

/// the clock tile: Suspend's tile from sheet (scale 1 or 4), its glyph painted out, blue, the
/// clock on it
fn clock_tile(sheet: &Picture, scale: u32) -> Result<Picture, String> {
    let (x, y) = (SOURCE_CELL % 8 * 32 * scale, SOURCE_CELL / 8 * 32 * scale);
    let mut tile = sheet.crop(x, y, 32 * scale, 32 * scale);
    // the two bars and their dark rim: x 11-20, y 9-21 at 1x; the HD pack's softer shadow reaches
    // further
    match scale {
        1 => tile.blank(10, 21, 9, 22),
        _ => tile.blank(36, 90, 28, 94),
    }
    tile.hue(BLUE);
    tile.over(&read_png(if scale == 1 { CLOCK_32 } else { CLOCK_128 })?);
    Ok(tile)
}

/// the texture's hash as packs name it, both ways: (bytes as memory holds them, Citra's rows)
pub fn pack_hashes(data: &[u8], w: u32, h: u32) -> (u64, u64) {
    // Citra's rows: from the bottom, texels kept as two bytes
    let mut rows = vec![0u8; (w * h * 2) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = ((y / 8 * (w / 8) + x / 8) * 64 + morton(x % 8, y % 8)) as usize;
            let o = (((h - 1 - y) * w + x) * 2) as usize;
            rows[o..o + 2].copy_from_slice(&data[i * 2..i * 2 + 2]);
        }
    }
    (cityhasher::hash::<u64>(data), cityhasher::hash::<u64>(&rows))
}

pub struct Icons {
    /// icon_E.ctpk (not packed)
    pub ctpk: Vec<u8>,
    /// hashes of the original and the new menu.tga, (bytes, rows) each
    pub old_hashes: (u64, u64),
    pub new_hashes: (u64, u64),
    pub new_size: (u32, u32),
    tile_32: Picture,
}

/// icon_E.ctpk with menu.tga 256x512 and the clock at CLOCK_CELL
pub fn build(ctpk: &[u8]) -> Result<Icons, String> {
    let (data_at, list) = entries(ctpk)?;
    let menu = list.iter().find(|e| e.name == "menu.tga").ok_or("icon_E.ctpk has no menu.tga")?;
    if (menu.format, menu.width, menu.height, menu.size) != (RGBA4, 256, 256, 0x20000) {
        return Err(format!("menu.tga is {}x{} format {} ({} bytes), not the 256x256 RGBA4 expected", menu.width, menu.height, menu.format, menu.size));
    }
    let start = data_at + menu.offset as usize;
    let old = &ctpk[start..start + menu.size as usize];
    let (w, h) = (256u32, 512u32);
    let old_sheet = Picture { w, h: 256, px: decode(old, w, 256) };
    let mut sheet = Picture { w, h, px: vec![[0; 4]; (w * h) as usize] };
    sheet.paste(&old_sheet, 0, 0);
    let tile_32 = clock_tile(&old_sheet, 1)?;
    sheet.paste(&tile_32, CLOCK_CELL.0, CLOCK_CELL.1);
    let new = encode(&sheet.px, w, h);
    if new[..old.len()] != *old {
        return Err("menu.tga: the first 256 rows changed".into());
    }
    // the new file: menu.tga's data grows by `grow`, the textures after it move down
    let grow = (new.len() - old.len()) as u32;
    let mut out = Vec::with_capacity(ctpk.len() + grow as usize);
    out.extend_from_slice(&ctpk[..start]);
    out.extend_from_slice(&new);
    out.extend_from_slice(&ctpk[start + old.len()..]);
    let put = |out: &mut Vec<u8>, at: usize, v: u32| out[at..at + 4].copy_from_slice(&v.to_le_bytes());
    put(&mut out, 0xC, u32_at(ctpk, 0xC) + grow); // the data's size
    for e in &list {
        if e.offset > menu.offset {
            put(&mut out, e.at + 8, e.offset + grow);
        }
    }
    put(&mut out, menu.at + 4, new.len() as u32);
    out[menu.at + 0x12..menu.at + 0x14].copy_from_slice(&(h as u16).to_le_bytes());
    if u32_at(ctpk, menu.sizes_at) != menu.size {
        return Err("icon_E.ctpk: menu.tga's size table entry is not where expected".into());
    }
    put(&mut out, menu.sizes_at, new.len() as u32);
    // reads back
    let (_, again) = entries(&out)?;
    let m = again.iter().find(|e| e.name == "menu.tga").unwrap();
    if (m.width, m.height, m.size) != (256, 512, new.len() as u32) {
        return Err("icon_E.ctpk: menu.tga reads back wrong".into());
    }
    for (a, b) in list.iter().zip(&again) {
        if a.name != "menu.tga" {
            let (x, y) = (data_at + a.offset as usize, data_at + b.offset as usize);
            if ctpk[x..x + a.size as usize] != out[y..y + b.size as usize] {
                return Err(format!("icon_E.ctpk: {} moved wrong", a.name));
            }
        }
    }
    Ok(Icons { ctpk: out, old_hashes: pack_hashes(old, w, 256), new_hashes: pack_hashes(&new, w, h), new_size: (w, h), tile_32 })
}

/// pictures for the layer's textures folder from the user's HD pack, when it has menu.tga's sheet:
/// the sheet made taller with the HD clock tile, under the new hash (both ways)
pub fn hd(icons: &Icons, pack: &Path, out: &Path) -> Result<Option<String>, String> {
    let names = [icons.old_hashes.0, icons.old_hashes.1].map(|h| format!("_{h:016X}_"));
    let mut found = None;
    let mut stack = vec![pack.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_uppercase();
            if path.is_dir() {
                stack.push(path);
            } else if name.ends_with(".PNG") && names.iter().any(|n| name.contains(n.as_str())) {
                found = Some(path);
            }
        }
    }
    let Some(path) = found else { return Ok(None) };
    let old = read_png(&std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?)?;
    if old.w != old.h || old.w % 256 != 0 {
        return Err(format!("{}: {}x{}, not a square multiple of 256", path.display(), old.w, old.h));
    }
    let scale = old.w / 256;
    let tile = if scale == 4 {
        clock_tile(&old, 4)?
    } else {
        // another scale: the 1x tile, enlarged
        let k = scale;
        let px = (0..32 * k).flat_map(|y| (0..32 * k).map(move |x| (x, y))).map(|(x, y)| icons.tile_32.px[((y / k) * 32 + x / k) as usize]).collect();
        Picture { w: 32 * k, h: 32 * k, px }
    };
    let mut sheet = Picture { w: old.w, h: old.h * 2, px: vec![[0; 4]; (old.w * old.h * 2) as usize] };
    sheet.paste(&old, 0, 0);
    sheet.paste(&tile, CLOCK_CELL.0 * scale, CLOCK_CELL.1 * scale);
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let (w, h) = icons.new_size;
    for hash in [icons.new_hashes.0, icons.new_hashes.1] {
        let file = out.join(format!("tex1_{w}x{h}_{hash:016X}_{RGBA4}.png"));
        write_png(&file, &sheet)?;
        println!("{} ({}x{})", file.display(), sheet.w, sheet.h);
    }
    Ok(Some(path.display().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba4_round_trips() {
        let px: Vec<[u8; 4]> = (0..64 * 16).map(|i| [(i * 17 % 256) as u8 / 17 * 17, 0x11, 0xEE, 0xFF]).collect();
        assert_eq!(decode(&encode(&px, 32, 32), 32, 32), px);
    }

    #[test]
    fn the_glyphs_are_the_cell_sizes() {
        assert_eq!((read_png(CLOCK_32).unwrap().w, read_png(CLOCK_128).unwrap().w), (32, 128));
    }
}
