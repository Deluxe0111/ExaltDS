//! Naga's Tear introduction for Fire Emblem Awakening USA (00040000000A0500) rev0.
//!
//! Builds, from the user's own decrypted dump, the RomFS layer that adds the game-side part of the
//! Naga's Tear introduction (NAGA_TEAR_PLAN.md, NT-02/NT-03):
//!
//! - `scripts/Command.cmb`: its load event (the one that registers the game's five save-kept global
//!   flags) registers a sixth, `G_ナーガの涙`, whose bit in the save says that this campaign has
//!   Naga's Tear. A new script function `bev::NagaTear` returns at once when the flag is set;
//!   otherwise it suspends the world map, plays Robin and Chrom's conversation (`ev::Talk`), shows
//!   the two tutorial boxes (`ev::TutorialDialogMustShow`), sets the flag and resumes the map.
//! - `m/E/GMAP.bin.lz`: the world map's messages, plus the conversation and the two boxes.
//!
//! - The trigger is data too: `Command.cmb` also gets `bev::GMAP_ON_<chapter>` for every chapter
//!   script without an arrival scene of its own (all but 004, 007, 010, 011), each running
//!   `bev::NagaTear`; those four scripts get the same call in front of their own scene, so wherever
//!   the player is, the first arrival on the world map grants it. Arriving on the world map the game starts that name itself (0x1C29B8), so the
//!   scene plays on the first arrival after the layer is turned on. No title.so code is involved.
//!   (A first version started it from a title.so override of 0x399E58; it crashed the first
//!   arrival and was dropped, 2026-11-16.)
//!
//! The flag is set only after the scene has played, and the game saves it with the campaign the
//! next time the player saves.
//!
//! Used by ExaltDS's setup (`layer`) and by Awakening-Port's tools/naga-tear (`check`, `build`).

pub mod cmb;
pub mod icons;
pub mod mess;

use crate::lz;

use std::path::Path;

use crate::USA;

/// "G_ナーガの涙" in Shift-JIS: global flags must start with "G_" (ev::FlagEntryGlobal checks).
pub const FLAG: &[u8] = b"G_\x83\x69\x81\x5B\x83\x4B\x82\xCC\x97\xDC";
pub const FUNCTION: &[u8] = b"bev::NagaTear";
pub const MID_INTRO: &str = "MID_NAGA_TEAR_INTRO";
pub const MID_OBTAINED: &str = "MID_NAGA_TEAR_OBTAINED";
pub const MID_TUTORIAL: &str = "MID_NAGA_TEAR_TUTORIAL";

/// Robin (`username`, the player's own portrait) and Chrom (`クロム`). Same markup as the game's
/// map conversations: `$t1` map talk, `$Wm<who>|<slot>` loads a portrait (3 left, 7 right), `$w0`
/// waits for it, `$Ws<who>|$Wa` makes them the speaker, `$E<face>,|` the expression, `$k` waits for
/// a button, `$p` starts a new box. Two lines per box, about 40 characters each.
pub const INTRO: &str = concat!(
    "$t1$Wmusername|3$Wmクロム|7$w0|",
    "$Wsusername|$Wa$E通常,|Chrom, do you have a moment? I found\nsomething peculiar among my belongings.$k\n",
    "$Wsクロム|$Wa$E通常,|A crystal? Does it mean anything to you?$k\n",
    "$Wsusername|$Wa$E通常,|Not that I can remember.$k$pBut when I hold it, I can almost see\nthings that haven't happened yet.$k\n",
    "$Wsクロム|$Wa$E通常,|You think it's connected to\nyour memories?$k\n",
    "$Wsusername|$Wa$E通常,|Perhaps. There's a strange warmth to it.$k$pAs though it's trying to guide me\naway from something.$k\n",
    "$Wsクロム|$Wa$E笑,|Well, whatever it is, keep it close.\nWe could use all the help we can get.$k",
);
/// Plain text, as the game's own tutorial boxes (MID_X001_EV1_TUT). The player may have renamed
/// Robin, so the box does not name them.
pub const OBTAINED: &str = "Obtained Naga's Tear!";
pub const TUTORIAL: &str = "Select Recall Fate from the battle\nmenu to return to an earlier moment.\nNaga's Tear can be used ten times\nper battle.";

pub fn all_files(romfs: &zakuro_fs::romfs::RomFs, dir: &zakuro_fs::romfs::DirEntry, prefix: &str, out: &mut Vec<String>) {
    for (_, file) in romfs.files(dir) {
        out.push(format!("{prefix}{}", file.name));
    }
    for (_, sub) in romfs.subdirs(dir) {
        let next = if sub.name.is_empty() { prefix.to_string() } else { format!("{prefix}{}/", sub.name) };
        all_files(romfs, &sub, &next, out);
    }
}

pub fn check(romfs: &zakuro_fs::romfs::RomFs, read: &dyn Fn(&str) -> Result<Vec<u8>, String>) -> Result<(), String> {
    let mut files = Vec::new();
    all_files(romfs, &romfs.root().map_err(|e| e.to_string())?, "", &mut files);
    let (mut scripts, mut archives) = (0, 0);
    for path in &files {
        if path.starts_with("scripts/") && path.ends_with(".cmb") {
            let data = read(path)?;
            let script = cmb::Script::parse(&data).map_err(|e| format!("{path}: {e}"))?;
            if script.write_rebased(&data).map_err(|e| format!("{path}: {e}"))? != data {
                return Err(format!("{path}: does not write back the same"));
            }
            scripts += 1;
        } else if path.starts_with("m/") && path.ends_with(".bin.lz") {
            let data = lz::unpack(&read(path)?).map_err(|e| format!("{path}: {e}"))?;
            let archive = mess::Archive::parse(&data).map_err(|e| format!("{path}: {e}"))?;
            if archive.write() != data {
                return Err(format!("{path}: does not write back the same"));
            }
            archives += 1;
        }
    }
    println!("{scripts} scripts and {archives} message archives read and written back unchanged");
    Ok(())
}

/// Command.cmb with the flag registered at load and `bev::NagaTear` added.
pub fn command_script(original: &[u8], arrivals: &[String]) -> Result<Vec<u8>, String> {
    let mut script = cmb::Script::parse(original).map_err(|e| format!("Command.cmb: {e}"))?;
    if script.find(FUNCTION).is_some() {
        return Err("Command.cmb already has bev::NagaTear".into());
    }
    // the load event: unnamed, type 5, registers the global flags with ev::FlagEntryGlobal
    let load = script
        .functions
        .iter()
        .position(|f| f.name.is_none() && f.kind & 0xFF == 5)
        .ok_or("Command.cmb has no load event")?;
    let mut code = cmb::Code::new(&mut script);
    code.push_string(FLAG).run(b"ev::FlagEntryGlobal", 1);
    let register = code.bytes;
    let event = &mut script.functions[load];
    let end = event.code.iter().rposition(|&b| b == 0x54).ok_or("the load event has no return")?;
    if !event.code[end + 1..].iter().all(|&b| b == 0) {
        return Err("the load event has code after its return".into());
    }
    event.code.splice(end..end, register);

    let mut code = cmb::Code::new(&mut script);
    code.push_string(FLAG).call(b"ev::FlagGet", 1).return_if_set();
    code.run(b"ev::GMapSuspend", 0);
    code.push_string(MID_INTRO.as_bytes()).run(b"ev::Talk", 1);
    code.push_string(MID_OBTAINED.as_bytes()).run(b"ev::TutorialDialogMustShow", 1);
    code.push_string(MID_TUTORIAL.as_bytes()).run(b"ev::TutorialDialogMustShow", 1);
    code.push_string(FLAG).run(b"ev::FlagSet", 1);
    code.run(b"ev::GMapResume", 0).ret();
    let body = code.bytes;
    add_function(&mut script, FUNCTION, body);

    // The trigger: arriving on the world map, the game starts `bev::GMAP_ON_<chapter script>`
    // (0x1C29B8, after loading scripts/<chapter>.cmb) when the chapter has one. Only 004, 007, 010
    // and 011 do; for the others the name is looked up and nothing plays. Command.cmb is loaded at
    // boot and its names stay, so giving those chapters an arrival scene that plays Naga's Tear lets
    // the game itself start it, on the first arrival after the layer is turned on.
    for chapter in arrivals {
        let mut code = cmb::Code::new(&mut script);
        code.run(FUNCTION, 0).ret();
        let body = code.bytes;
        add_function(&mut script, arrival_name(chapter).as_bytes(), body);
    }
    script.write_rebased(original).map_err(|e| format!("Command.cmb: {e}"))
}

/// A chapter script that has an arrival scene of its own (004, 007, 010, 011) with `bev::NagaTear` run at the
/// start of that scene, so the first world-map arrival grants Naga's Tear there too. Jumps are relative, so
/// the code in front of the scene's own moves nothing.
pub fn chapter_script(original: &[u8], chapter: &str) -> Result<Vec<u8>, String> {
    let mut script = cmb::Script::parse(original).map_err(|e| format!("{chapter}.cmb: {e}"))?;
    let at = script.find(arrival_name(chapter).as_bytes()).ok_or(format!("{chapter}.cmb has no arrival scene"))?;
    let mut code = cmb::Code::new(&mut script);
    code.run(FUNCTION, 0);
    let prefix = code.bytes;
    script.functions[at].code.splice(0..0, prefix);
    script.write_rebased(original).map_err(|e| format!("{chapter}.cmb: {e}"))
}

pub fn arrival_name(chapter: &str) -> String {
    format!("bev::GMAP_ON_{chapter}")
}

/// chapter scripts (000-026, P001, P002, X001-X023): the names the world map builds arrival scenes from
pub fn is_chapter_script(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() == 4 && matches!(b[0], b'P' | b'X') && b[1..].iter().all(u8::is_ascii_digit)
        || b.len() == 3 && b.iter().all(u8::is_ascii_digit)
}

fn add_function(script: &mut cmb::Script, name: &[u8], code: Vec<u8>) {
    let index = script.functions.len() as u32;
    // the code before a following function is padded to 4 bytes in the game's files
    if let Some(last) = script.functions.last_mut() {
        while (last.code.len() + last.params.len() + last.name.as_ref().map_or(0, |n| n.len() + 1)) % 4 != 0 {
            last.code.push(0);
        }
    }
    script.functions.push(cmb::Function { kind: 0, index, name: Some(name.to_vec()), extra_offset: 0, params: vec![], code });
}

/// GMAP.bin (unpacked) with the three messages added.
pub fn gmap_messages(original: &[u8]) -> Result<Vec<u8>, String> {
    let mut archive = mess::Archive::parse(original).map_err(|e| format!("GMAP.bin: {e}"))?;
    archive.set(MID_INTRO.as_bytes(), INTRO);
    archive.set(MID_OBTAINED.as_bytes(), OBTAINED);
    archive.set(MID_TUTORIAL.as_bytes(), TUTORIAL);
    Ok(archive.write())
}

pub fn build(files: &[String], read: &dyn Fn(&str) -> Result<Vec<u8>, String>, out: &Path) -> Result<(), String> {
    // the chapters whose script has no arrival scene of its own
    let mut arrivals = Vec::new();
    let mut own = Vec::new(); // chapters with a scene of their own: the call goes in front of it
    for path in files {
        let Some(chapter) = path.strip_prefix("scripts/").and_then(|p| p.strip_suffix(".cmb")) else { continue };
        if !is_chapter_script(chapter) {
            continue;
        }
        let script = cmb::Script::parse(&read(path)?).map_err(|e| format!("{path}: {e}"))?;
        if script.find(arrival_name(chapter).as_bytes()).is_none() {
            arrivals.push(chapter.to_string());
        } else {
            own.push(chapter.to_string());
        }
    }
    arrivals.sort();
    let command = command_script(&read("scripts/Command.cmb")?, &arrivals)?;
    let gmap = lz::unpack(&read("m/E/GMAP.bin.lz")?)?;
    let gmap = gmap_messages(&gmap)?;
    // check what was written reads back as meant
    let script = cmb::Script::parse(&command).map_err(|e| e.to_string())?;
    let added = script.find(FUNCTION).ok_or("bev::NagaTear missing after writing")?;
    let archive = mess::Archive::parse(&gmap).map_err(|e| e.to_string())?;
    for (mid, text) in [(MID_INTRO, INTRO), (MID_OBTAINED, OBTAINED), (MID_TUTORIAL, TUTORIAL)] {
        let i = archive.find(mid.as_bytes()).ok_or(format!("{mid} missing after writing"))?;
        if archive.text(i).as_deref() != Some(text) {
            return Err(format!("{mid} reads back differently"));
        }
    }
    let packed = lz::lz11(&gmap, 64);
    if lz::unpack(&packed)? != gmap {
        return Err("GMAP.bin.lz does not unpack to what was packed".into());
    }
    let romfs = out.join("romfs");
    for (path, bytes) in [("scripts/Command.cmb", &command), ("m/E/GMAP.bin.lz", &packed)] {
        let file = romfs.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(&file, bytes).map_err(|e| format!("{}: {e}", file.display()))?;
        println!("{} ({} bytes)", file.display(), bytes.len());
    }
    for chapter in &own {
        let path = format!("scripts/{chapter}.cmb");
        let bytes = chapter_script(&read(&path)?, chapter)?;
        let parsed = cmb::Script::parse(&bytes).map_err(|e| format!("{path}: {e}"))?;
        let at = parsed.find(arrival_name(chapter).as_bytes()).ok_or(format!("{path}: arrival scene missing after writing"))?;
        if !parsed.functions[at].code.starts_with(&{
            let mut s = cmb::Script::parse(&bytes).map_err(|e| e.to_string())?;
            let mut c = cmb::Code::new(&mut s);
            c.run(FUNCTION, 0);
            c.bytes
        }) {
            return Err(format!("{path}: the call is not in front of the arrival scene"));
        }
        let file = romfs.join(&path);
        std::fs::create_dir_all(file.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(&file, &bytes).map_err(|e| format!("{}: {e}", file.display()))?;
        println!("{} ({} bytes)", file.display(), bytes.len());
    }
    for chapter in &arrivals {
        script.find(arrival_name(chapter).as_bytes()).ok_or(format!("{} missing after writing", arrival_name(chapter)))?;
    }
    println!("bev::NagaTear is function {added} of Command.cmb; the flag is registered at load");
    println!("arrival scenes playing it: {} chapters ({})", arrivals.len(), arrivals.join(" "));
    Ok(())
}

/// Recall Fate's clock icon (icons.rs): `systems/icon_E.ctpk.lz`, and with an HD pack its sheet
/// for the new menu.tga in `textures/`.
pub fn build_icons(read: &dyn Fn(&str) -> Result<Vec<u8>, String>, out: &Path, hd: Option<&Path>) -> Result<(), String> {
    let ctpk = lz::unpack(&read("systems/icon_E.ctpk.lz")?)?;
    let icons = icons::build(&ctpk)?;
    let packed = lz::lz13(&icons.ctpk);
    if lz::unpack(&packed)? != icons.ctpk {
        return Err("icon_E.ctpk.lz does not unpack to what was packed".into());
    }
    let file = out.join("romfs/systems/icon_E.ctpk.lz");
    std::fs::create_dir_all(file.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&file, &packed).map_err(|e| format!("{}: {e}", file.display()))?;
    println!("{} ({} bytes): menu.tga 256x512, the clock at {:?}", file.display(), packed.len(), icons::CLOCK_CELL);
    println!("menu.tga hashes: was {:016X} (bytes) / {:016X} (rows), now {:016X} / {:016X}",
        icons.old_hashes.0, icons.old_hashes.1, icons.new_hashes.0, icons.new_hashes.1);
    if let Some(pack) = hd {
        match icons::hd(&icons, pack, &out.join("textures"))? {
            Some(from) => println!("HD sheet made from {from}"),
            None => println!("no HD sheet for menu.tga in {}; the game's own resolution is used", pack.display()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flag_name_is_shift_jis_for_g_naga_no_namida() {
        assert_eq!(FLAG, b"G_\x83\x69\x81\x5B\x83\x4B\x82\xCC\x97\xDC");
        assert!(FLAG.starts_with(b"G_"));
    }

    #[test]
    fn no_conversation_line_is_longer_than_the_games_own() {
        // the longest line in the game's chapter 1 conversations is about 41 characters
        for line in INTRO.split(['\n']).flat_map(|l| l.split("$p")) {
            let spoken = line.rsplit('|').next().unwrap().trim_end_matches("$k");
            assert!(spoken.chars().count() <= 42, "too long: {spoken}");
        }
        for line in TUTORIAL.lines() {
            assert!(line.chars().count() <= 38, "too long: {line}");
        }
    }
}

/// The whole layer from the game: the scripts, the messages and the icon sheet (no HD sheet).
pub fn layer(title: &zakuro_fs::Title, out: &Path) -> Result<(), String> {
    if title.program_id() != USA {
        return Err(format!("{:016X} is not Fire Emblem Awakening USA (00040000000A0500)", title.program_id()));
    }
    let romfs = title.romfs.as_ref().ok_or("the game has no RomFS")?;
    let read = |path: &str| -> Result<Vec<u8>, String> {
        let file = romfs.lookup(path).map_err(|e| format!("{path}: {e}"))?;
        Ok(title.read_romfs(&file, 0, file.data_size as usize).ok_or(format!("{path}: unreadable"))?.to_vec())
    };
    let mut files = Vec::new();
    all_files(romfs, &romfs.root().map_err(|e| e.to_string())?, "", &mut files);
    build(&files, &read, out)?;
    build_icons(&read, out, None)
}
