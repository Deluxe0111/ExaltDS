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
/// code (3dsrecomp's overrides): Recall Fate in the battle map menu, and the feet ground hook.
pub const OVERRIDES: &[(&str, &str)] =
    &[("recall_fate.c", include_str!("../overrides/recall_fate.c")), ("feet.c", include_str!("../overrides/feet.c"))];
