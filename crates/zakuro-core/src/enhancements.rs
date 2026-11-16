//! enhancements, the community's codes that make games run at 60 FPS and
//! the like, for the builds of games they were tested on, which a player
//! turns on by name instead of finding and pasting the code.

use crate::cheats::Cheat;

/// the codes, one game after another, its title id and name, then each
/// enhancement, its name, the build it is for, who made it and what to know
/// about it, and its code.
const LIST: &str = include_str!("enhancements.txt");

/// one enhancement for a game.
#[derive(Debug, Clone, PartialEq)]
pub struct Enhancement {
    pub name: String,
    pub author: String,
    /// what a player should know before turning it on, empty if nothing.
    pub note: String,
    pub lines: Vec<String>,
}

/// the build a title runs, which an enhancement's code has to be made for:
/// its update's version, or else its cartridge's revision.
pub fn build(title: &zakuro_fs::Title) -> String {
    match title.update() {
        Some(update) => format!("update {}.{}", update.version >> 10, update.version >> 4 & 0x3F),
        None => format!("rev{}", title.exheader.remaster_version),
    }
}

/// the enhancements for a game of program_id running build.
pub fn for_title(program_id: u64, build: &str) -> Vec<Enhancement> {
    let mut found: Vec<Enhancement> = Vec::new();
    let (mut game, mut taking) = (false, false);
    for line in LIST.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#')) {
        if let Some(header) = line.strip_prefix('=') {
            let id = header.split_whitespace().next().and_then(|id| u64::from_str_radix(id, 16).ok());
            game = id == Some(program_id);
            taking = false;
        } else if let Some((name, rest)) = line.split_once('|').filter(|_| game) {
            let mut fields = rest.split('|').map(str::trim);
            let made_for = fields.next().unwrap_or_default();
            taking = made_for == "any" || made_for == build;
            if taking {
                found.push(Enhancement {
                    name: name.trim().to_owned(),
                    author: fields.next().unwrap_or_default().to_owned(),
                    note: fields.next().unwrap_or_default().to_owned(),
                    lines: Vec::new(),
                });
            }
        } else if game && taking {
            if let Some(enhancement) = found.last_mut() {
                enhancement.lines.push(line.to_owned());
            }
        }
    }
    found
}

/// enhancements that leave out a title's own glow and have its 3D scene brightened in its
/// place: the title, the build, the enhancement, and the copy of the scene the title still
/// makes where the glow went, which marks the moment, after the 3D and before the 2D.
const TONED: [(u64, &str, &str, zakuro_gpu::ToneCopy); 1] = [(
    0x0004_0000_000A_0500,
    "rev0",
    "No glow",
    // Fire Emblem Awakening's scene, 256x416 RGBA8, into its glow's buffer
    // the lift, then 133% brightness, 102% contrast and 110% more saturation, chosen by eye
    zakuro_gpu::ToneCopy { input: 0x1800_0000, output: 0x1838_0000, size: 0x6_8000, lift: 1.0, gain: 1.33, contrast: 1.02, saturation: 1.375 },
)];

/// the enhancements of this build that leave out a glow, and the copy marking where the scene
/// is brightened in its place.
pub fn tones(program_id: u64, build: &str) -> Vec<(String, zakuro_gpu::ToneCopy)> {
    TONED.iter().filter(|&&(id, made_for, ..)| id == program_id && made_for == build).map(|&(_, _, name, tone)| (name.to_owned(), tone)).collect()
}

/// the game's enhancements as cheats of its own, the ones named in on
/// turned on, which go nowhere near the player's cheat file.
pub fn cheats(program_id: u64, build: &str, on: &[String]) -> Vec<Cheat> {
    for_title(program_id, build)
        .into_iter()
        .map(|enhancement| {
            let credit = if enhancement.name == "No Outlines" {
                "Community code from the CTRPF code archive; matches the existing Ultra Moon v1.0 override.".to_owned()
            } else {
                format!("Code by {}, from the 60 FPS thread on GBAtemp.", enhancement.author)
            };
            let notes = [enhancement.note, credit].into_iter().filter(|line| !line.is_empty()).collect();
            let mut cheat = Cheat::new(&enhancement.name, enhancement.lines, notes);
            cheat.builtin = true;
            cheat.enabled = on.contains(&enhancement.name);
            cheat
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// each enhancement is for its own game and build, with its note and
    /// who made it, and only its own code.
    #[test]
    fn a_game_gets_the_enhancements_for_its_build() {
        let enhancements = for_title(0x0004_0000_0008_C300, "rev2");
        assert_eq!(enhancements.len(), 1);
        let fps = &enhancements[0];
        assert_eq!((fps.name.as_str(), fps.author.as_str()), ("60 FPS", "Hazerou"));
        assert!(!fps.note.is_empty());
        assert_eq!(fps.lines, ["D3000000 08000000", "2020A748 0000003C", "D2000000 00000000"]);
        assert!(for_title(0x0004_0000_0008_C300, "rev0").is_empty());
        assert!(for_title(0x0004_0000_0008_C300, "update 1.1").is_empty());

        let cheats = cheats(0x0004_0000_0008_C300, "rev2", &["60 FPS".to_owned()]);
        assert!(cheats[0].enabled && cheats[0].builtin);
        assert_eq!(cheats[0].notes.len(), 2);
    }

    #[test]
    fn ultra_moon_outlines_patch_is_limited_to_the_original_build() {
        let id = 0x0004_0000_001B_5100;
        let original = for_title(id, "rev0");
        let outlines = original.iter().find(|code| code.name == "No Outlines").unwrap();
        assert_eq!(outlines.lines, ["0042E2B8 E320F000"]);
        assert!(!for_title(id, "rev1").iter().any(|code| code.name == "No Outlines"));
        assert!(!for_title(id, "update 1.2").iter().any(|code| code.name == "No Outlines"));
        let cheats = cheats(id, "rev0", &["No Outlines".into()]);
        assert!(cheats.iter().find(|cheat| cheat.name == "No Outlines").unwrap().enabled);
        assert!(!cheats.iter().find(|cheat| cheat.name == "60 FPS").unwrap().enabled);
    }

    /// Awakening USA's 16:9, relocated from the EUR code: its sites, and a cave in the text
    /// mapping's padding past the static-constructor table the game walks at boot.
    #[test]
    fn awakening_usa_widescreen_writes_its_relocated_sites() {
        let id = 0x0004_0000_000A_0500;
        let wide = for_title(id, "rev0").into_iter().find(|code| code.name == "16:9").unwrap();
        assert!(for_title(id, "rev1").is_empty());
        assert_eq!(wide.lines.len(), 43);
        let words: Vec<(u32, u32)> = wide
            .lines
            .iter()
            .map(|line| (u32::from_str_radix(&line[..8], 16).unwrap(), u32::from_str_radix(&line[9..], 16).unwrap()))
            .collect();
        let sites: Vec<u32> = words.iter().map(|&(addr, _)| addr).filter(|addr| !(0x0045_9C00..0x0045_9CE0).contains(addr)).collect();
        assert_eq!(sites, [0x002F_DCAC, 0x003D_4DFC, 0x002E_9FAC, 0x002D_13AC]);
        // the map camera's 5:3 becomes 16:9, and the hooks branch into the cave
        assert!(words.contains(&(0x003D_4DFC, (16.0f32 / 9.0).to_bits())));
        assert!(words.iter().all(|&(addr, _)| addr < 0x0010_0000 + 0x0035_A000), "inside the text mapping");
        assert!(words.iter().filter(|&&(addr, _)| addr >= 0x0045_9B88).all(|&(addr, _)| addr >= 0x0045_9C00), "past the constructor table");
    }

    /// Awakening USA without its glow: the pass makes its copy of the scene, the moment the
    /// scene is brightened in its place, and then leaves for its own return at 0x18D98C.
    #[test]
    fn awakening_usa_without_glow_copies_the_scene_and_leaves() {
        let id = 0x0004_0000_000A_0500;
        let plain = for_title(id, "rev0").into_iter().find(|code| code.name == "No glow").unwrap();
        assert_eq!(plain.lines, ["0018D3F4 EA000164"]);
        // b 0x18D98C from 0x18D3F4, right after the call that copies the scene
        assert_eq!(0x0018_D3F4 + 8 + 4 * (0xEA00_0164u32 & 0x00FF_FFFF), 0x0018_D98C);
        let found = tones(id, "rev0");
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].0.as_str(), found[0].1.input, found[0].1.output, found[0].1.size), ("No glow", 0x1800_0000, 0x1838_0000, 0x6_8000));
        assert!(tones(id, "rev1").is_empty());
    }

    #[test]
    fn every_listed_code_is_code() {
        for line in LIST.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with('=') && !line.contains('|')) {
            assert!(crate::cheats::is_code(line), "{line}");
        }
    }
}
