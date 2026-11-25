//! ExaltDS's mod layers for Fire Emblem Awakening, built on the device from the player's own copy
//! of the game (nothing of the game is shipped): the feet mod's models (USA and EUR) and Naga's
//! Tear (USA). Each writes a layer folder, `<out>/romfs/...`, which ExaltDS links into the game's
//! mods folder.

pub mod feet;
pub mod lz;
pub mod naga;

/// Fire Emblem Awakening, USA and EUR
pub const USA: u64 = 0x0004_0000_000A_0500;
pub const EUR: u64 = 0x0004_0000_0009_F100;

/// ExaltDS's game-code overrides for the USA release, compiled into title.so with the game's own
/// code (3dsrecomp's overrides): Recall Fate in the battle map menu, the feet ground hook, and the
/// SpotPass bonuses on by default.
pub const OVERRIDES: &[(&str, &str)] = &[
    ("recall_fate.c", include_str!("../overrides/recall_fate.c")),
    ("feet.c", include_str!("../overrides/feet.c")),
    ("bonus_box.c", include_str!("../overrides/bonus_box.c")),
];

/// Overrides for the supported revision-zero executable. Never apply USA addresses to EUR.
/// The two EUR feet addresses are from Zelkin1/fe-awakening-recomp at 9c9543b,
/// source/zakuro/crates/zakuro-core/src/feet.rs (MIT).
pub fn overrides_for(id: u64) -> Vec<(&'static str, String)> {
    match id {
        USA => OVERRIDES.iter().map(|&(name, source)| (name, source.to_owned())).collect(),
        EUR => vec![("feet.c", include_str!("../overrides/feet.c")
            .replace("0x00306300", "0x00306964")
            .replace("0x002602A0", "0x002607CC"))],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod region_tests {
    use super::*;
    #[test]
    fn only_regional_overrides_are_selected() {
        let us = overrides_for(USA);
        assert_eq!(us.len(), 3);
        let eu = overrides_for(EUR);
        assert_eq!(eu.len(), 1);
        assert_eq!(eu[0].0, "feet.c");
        assert!(eu[0].1.contains("RECOMP_OVERRIDE(0x00306964)"));
        assert!(eu[0].1.contains("RECOMP_ORIGINAL(0x00306964)"));
        assert!(eu[0].1.contains("#define FEET_LOOP_RETURN 0x002607CCu"));
        assert!(!eu[0].1.contains("0x00306300"));
        assert!(overrides_for(0).is_empty());
    }
}
