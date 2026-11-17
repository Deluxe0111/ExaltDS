//! The feet mod's models, from fe-awakening-recomp's builder (foot.rs, formats.rs, model.rs,
//! unchanged; see NOTICE.md): for each body model `b/uBody_*.m.lz`, a copy with feet, written to
//! `<out>/romfs/b/`. The builder works from the files' names and contents, so the USA release gets
//! them too. Keeping the feet on the ground is game code (a title.so override, feet.c).

#[allow(dead_code)]
pub mod foot;
#[allow(dead_code)]
mod formats;
#[allow(dead_code)]
mod model;

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::lz;

/// Builds the models; gives how many of the bodies got feet. `progress(done, total)`.
pub fn build(title: &zakuro_fs::Title, out: &Path, progress: &(dyn Fn(usize, usize) + Sync)) -> Result<usize, String> {
    if ![crate::USA, crate::EUR].contains(&title.program_id()) {
        return Err(format!("{:016X} is not Fire Emblem Awakening", title.program_id()));
    }
    let romfs = title.romfs.as_ref().ok_or("the game has no RomFS")?;
    let mut dir = romfs.root().map_err(|e| e.to_string())?;
    if let Some((_, nameless)) = romfs.subdirs(&dir).into_iter().find(|(_, d)| d.name.is_empty()) {
        dir = nameless;
    }
    let (_, b) = romfs.subdirs(&dir).into_iter().find(|(_, d)| d.name == "b").ok_or("the game has no b folder")?;
    let mut bodies: Vec<String> =
        romfs.files(&b).into_iter().map(|(_, f)| f.name).filter(|n| n.starts_with("uBody_") && n.ends_with(".m.lz")).collect();
    bodies.sort();

    let out = out.join("romfs").join("b");
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let read = |name: &str| -> Option<Vec<u8>> {
        let file = romfs.lookup(&format!("b/{name}")).ok()?;
        lz::unpack(title.read_romfs(&file, 0, file.data_size as usize)?).ok()
    };
    let one = |name: &str| -> Result<(), String> {
        let model = read(name).ok_or("its file can't be read")?;
        let texture = read(&name.replace(".m.lz", "_c0.t.lz")).and_then(|file| foot::colour_texture(&file));
        let (bytes, _style) = foot::build(&format!("b/{name}"), &model, texture.as_ref())?;
        let path = out.join(name);
        std::fs::write(&path, lz::lz13(&bytes)).map_err(|e| format!("{}: {e}", path.display()))
    };
    let (next, done, made) = (AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0));
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get()).min(bodies.len()).max(1);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= bodies.len() {
                    break;
                }
                // a body the builder can't fit keeps the game's legs
                if one(&bodies[i]).is_ok() {
                    made.fetch_add(1, Ordering::Relaxed);
                }
                progress(done.fetch_add(1, Ordering::Relaxed) + 1, bodies.len());
            });
        }
    });
    match made.into_inner() {
        0 => Err("feet for none of the bodies".into()),
        made => Ok(made),
    }
}
