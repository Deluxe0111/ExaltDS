//! Recall Fate (Naga's Tear's rewind) for Fire Emblem Awakening USA, see NAGA_TEAR_PLAN.md in
//! Awakening-Port. NT-04: the turn-level proof of concept.
//!
//! The battle structure at `[0x4D1850]` holds the turn as a u16 at +0xA (what `ev::TurnGet` reads)
//! and the phase as a u16 at +0x8: 0 player, 1 enemy; in the flags at +0x4, 0x20000 is set once
//! the player has control (after the opening scene) and bit 31 while the phase changes; +0x18
//! counts frames (found by watching it on the AYN Thor, 2026-11-16). Turn and phase already read
//! (1, player) while the map loads, so a player phase counts as started once it is the player's,
//! 0x20000 is set and bit 31 is clear. Each time a player phase starts the whole console is saved (`System::save_state`, the save states' own format)
//! into a list kept in memory, one per turn, so that the battle can be taken back to the start of
//! an earlier player phase. The list belongs to one battle: it starts over when the structure moves
//! (another map) or is gone.
//!
//! Development switch: `adb shell setprop debug.zakuro.recall_watch 1` (read at launch) also logs
//! the structure's first 0x30 bytes whenever they change, the frame counter left out.

use std::sync::Mutex;

use zakuro_core::System;

const AWAKENING_USA: u64 = 0x0004_0000_000A_0500;
const BATTLE_INFO: u32 = 0x004D_1850;
const FLAGS: u32 = 0x4;
const PHASE: u32 = 0x8;
const TURN: u32 = 0xA;
const PLAYER_PHASE: u16 = 0;
const IN_CONTROL: u32 = 0x2_0000;
const CHANGING_PHASE: u32 = 0x8000_0000;
/// the newest turns kept, and at most this many bytes of them
const MAX_TURNS: usize = 10;
const MAX_BYTES: usize = 512 << 20;
const WATCH_WORDS: usize = 0x30 / 4;
const FRAME_COUNTER_WORD: usize = 0x18 / 4;

/// the turns that can be recalled, newest first, for the Java side (`nativeRecallTurns`)
static TURNS: Mutex<Vec<u16>> = Mutex::new(Vec::new());

pub fn turns() -> Vec<u16> {
    TURNS.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

struct Snapshot {
    turn: u16,
    state: Vec<u8>,
}

pub struct Recall {
    on: bool,
    checked_title: bool,
    watch: bool,
    battle: u32,
    /// the turn whose player phase was last saved (or found already under way)
    saved: Option<u16>,
    /// whether this battle has been looked at since it appeared
    looked: bool,
    snapshots: Vec<Snapshot>,
    watched: [u32; WATCH_WORDS],
}

impl Recall {
    pub fn new() -> Recall {
        let watch = crate::platform::system_property(c"debug.zakuro.recall_watch").as_deref() == Some("1");
        publish(&[]);
        Recall { on: true, checked_title: false, watch, battle: 0, saved: None, looked: false, snapshots: Vec::new(), watched: [0; WATCH_WORDS] }
    }

    /// after each emulated frame: a new player phase is saved
    pub fn after_frame(&mut self, system: &mut System) {
        if !self.on {
            return;
        }
        if !self.checked_title {
            self.checked_title = true;
            let id = system.title.as_ref().map(|t| t.program_id()).unwrap_or(0);
            if id != AWAKENING_USA {
                self.on = false;
                return;
            }
            log::info!("Recall Fate: saving the start of each player phase{}", if self.watch { " (and watching the battle structure)" } else { "" });
        }
        let battle = system.memory.peek32(BATTLE_INFO);
        if self.watch {
            self.log_changes(system, battle);
        }
        if battle != self.battle {
            if !self.snapshots.is_empty() {
                log::info!("Recall Fate: battle {:08X} -> {battle:08X}, {} saved turns dropped", self.battle, self.snapshots.len());
            }
            self.battle = battle;
            self.saved = None;
            self.looked = false;
            self.snapshots.clear();
            publish(&self.snapshots);
        }
        if battle == 0 {
            return;
        }
        let flags = system.memory.peek32(battle + FLAGS);
        let word = system.memory.peek32(battle + PHASE);
        let (phase, turn) = (word as u16, (word >> 16) as u16);
        debug_assert_eq!(TURN, PHASE + 2);
        let started = phase == PLAYER_PHASE && turn != 0 && flags & IN_CONTROL != 0 && flags & CHANGING_PHASE == 0;
        let first_look = !self.looked;
        self.looked = true;
        if !started || self.saved == Some(turn) {
            return;
        }
        self.saved = Some(turn);
        // a battle already in a player phase when first seen (a state loaded there) is not saved
        // halfway through the phase; its next player phase is
        if first_look {
            return;
        }
        let start = std::time::Instant::now();
        match system.save_state() {
            Ok(state) => {
                let bytes = state.len();
                self.snapshots.retain(|s| s.turn < turn);
                self.snapshots.push(Snapshot { turn, state });
                while self.snapshots.len() > MAX_TURNS || self.snapshots.iter().map(|s| s.state.len()).sum::<usize>() > MAX_BYTES {
                    self.snapshots.remove(0);
                }
                publish(&self.snapshots);
                log::info!(
                    "Recall Fate: saved turn {turn} ({:.1} MB in {:.0} ms), {} turns kept ({:.0} MB)",
                    bytes as f64 / 1048576.0,
                    start.elapsed().as_secs_f64() * 1000.0,
                    self.snapshots.len(),
                    self.snapshots.iter().map(|s| s.state.len()).sum::<usize>() as f64 / 1048576.0
                );
            }
            Err(error) => log::warn!("Recall Fate: turn {turn} not saved: {error}"),
        }
    }

    /// a save state was loaded: the turns saved so far may be from another time line
    pub fn reset(&mut self) {
        self.battle = 0;
        self.saved = None;
        self.looked = false;
        self.snapshots.clear();
        publish(&self.snapshots);
    }

    /// takes the battle back to the start of that turn's player phase; later turns are dropped
    pub fn recall(&mut self, system: &mut System, turn: u16) -> Result<(), String> {
        let at = self.snapshots.iter().position(|s| s.turn == turn).ok_or(format!("Turn {turn} cannot be recalled"))?;
        let start = std::time::Instant::now();
        system.load_state(&self.snapshots[at].state)?;
        self.snapshots.truncate(at + 1);
        publish(&self.snapshots);
        // as it was when the snapshot was taken: that turn's player phase already saved
        self.saved = Some(turn);
        self.looked = true;
        log::info!("Recall Fate: back to turn {turn} in {:.0} ms", start.elapsed().as_secs_f64() * 1000.0);
        Ok(())
    }

    fn log_changes(&mut self, system: &mut System, battle: u32) {
        let mut words = [0u32; WATCH_WORDS];
        if battle != 0 {
            for (i, word) in words.iter_mut().enumerate() {
                *word = system.memory.peek32(battle.wrapping_add(4 * i as u32));
            }
        }
        let changed: Vec<String> = (0..WATCH_WORDS)
            .filter(|&i| i != FRAME_COUNTER_WORD && words[i] != self.watched[i])
            .map(|i| format!("+{:02X}:{:08X}->{:08X}", 4 * i, self.watched[i], words[i]))
            .collect();
        if !changed.is_empty() || battle != self.battle {
            log::info!("Recall Fate watch: battle={battle:08X} {}", changed.join(" "));
        }
        self.watched = words;
    }
}

fn publish(snapshots: &[Snapshot]) {
    *TURNS.lock().unwrap_or_else(|e| e.into_inner()) = snapshots.iter().rev().map(|s| s.turn).collect();
}
