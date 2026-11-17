//! Recall Fate (Naga's Tear's rewind) for Fire Emblem Awakening USA, see NAGA_TEAR_PLAN.md in
//! Awakening-Port. NT-04 (turns) and NT-05 (actions).
//!
//! The battle structure at `[0x4D1850]` holds the turn as a u16 at +0xA (what `ev::TurnGet` reads)
//! and the phase as a u16 at +0x8: 0 player, 1 enemy; in the flags at +0x4, 0x20000 is set once
//! the player has control (after the opening scene) and bit 31 while the phase changes; +0x18
//! counts frames (found by watching it on the AYN Thor, 2026-11-16). Turn and phase already read
//! (1, player) while the map loads, so a player phase counts as started once it is the player's,
//! 0x20000 is set and bit 31 is clear.
//!
//! Units (from ev::ForceUnitGetFirst/Next, ev::UnitGetX/HP, and watched on the Thor): an array of
//! 0x110-byte units at `[0x4D1100]`, unit i (from 1) at +(i - 1) * 0x110; `[0x4D1104]` + army * 12
//! holds an army's first unit (0 the player's, 1 the enemy's), a unit's next at +0xC. In a unit:
//! +0x64 its person, +0x8C its index (byte), +0x8F HP (byte), +0x90 x and +0x91 y (bytes), +0x94
//! state, whose bit 0 is "has acted": it is set at the very end of an action (after Wait, or after
//! the combat, its experience and level up), and cleared when the army's phase starts.
//!
//! Checkpoints: the whole console (`System::save_state`, the save states' own format) is saved when
//! a player phase starts and again whenever a player unit's "has acted" bit goes from 0 to 1, the
//! end of an action and so the moment before the next one. Each action gets an entry: its turn, its
//! unit, its kind and the checkpoint taken before it. The kind is the unit command last chosen
//! (title.so reports Attack, Dance, Staff, Items, Pair Up, Separate, Rally, Talk, Visit and Wait
//! through `zakuro_recall_command`); for Wait or none, what the action changed (attack: an enemy
//! lost HP or a unit went; heal: another player unit gained HP; move: it moved; otherwise wait). Going back to an entry loads
//! that checkpoint and drops everything after it. All of it belongs to one battle: it starts over
//! when the structure moves (another map) or is gone, or when a save state is loaded.
//!
//! The game asks for a restore from its own map menu: title.so's override
//! `overrides/awakening-usa/recall_fate.c` (Awakening-Port) reads the entries with
//! `zakuro_recall_entries` and asks with `zakuro_recall_go` (the older `zakuro_recall_turns` and
//! `zakuro_recall_request` still work); the restore happens after the frame in which it was asked.
//!
//! Development switch: `adb shell setprop debug.zakuro.recall_watch 1` (read at launch) also logs
//! the structure's first 0x30 bytes whenever they change (the frame counter left out), the player
//! units' and the mind's words whenever they change, and each player unit's person once.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
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
/// at most this many bytes of checkpoints are kept (the oldest go first)
const MAX_BYTES: usize = 512 << 20;
const WATCH_WORDS: usize = 0x30 / 4;
const FRAME_COUNTER_WORD: usize = 0x18 / 4;

const UNITS: u32 = 0x004D_1100;
const ARMIES: u32 = 0x004D_1104;
const UNIT_SIZE: u32 = 0x110;
const UNIT_NEXT: u32 = 0xC;
const UNIT_PERSON: u32 = 0x64;
const UNIT_HP: u32 = 0x8F;
const UNIT_X: u32 = 0x90;
const UNIT_STATE: u32 = 0x94;
const HAS_ACTED: u32 = 1;
const PLAYER_ARMY: u32 = 0;
/// the armies looked at for a kind: player, enemy, other (allies)
const ARMIES_SEEN: u32 = 3;
/// the current action's context (ev::MindGetUnit reads its +0xC); only watched
const MIND: u32 = 0x004D_1120;
const MIND_WORDS: u32 = 0x40;

/// an entry's kind, as `zakuro_recall_entries` gives it
pub const TURN_START: u8 = 0;
pub const ATTACK: u8 = 1;
pub const HEAL: u8 = 2;
pub const MOVE: u8 = 3;
pub const WAIT: u8 = 4;
pub const DANCE: u8 = 5;
pub const STAFF: u8 = 6;
pub const ITEM: u8 = 7;
pub const PAIR_UP: u8 = 8;
pub const SEPARATE: u8 = 9;
pub const RALLY: u8 = 10;
pub const TALK: u8 = 11;
pub const VISIT: u8 = 12;

/// one line of the history, as title.so reads it (newest first)
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Entry {
    /// what `zakuro_recall_go` takes to come back here
    pub id: u32,
    pub turn: u16,
    /// the acting unit's index (from 1), 0 for a turn start
    pub unit: u16,
    pub kind: u8,
    pub pad: [u8; 3],
    /// the acting unit's person (guest address), 0 for a turn start
    pub person: u32,
}

/// what the game's menu reads, newest first
static ENTRIES: Mutex<Vec<Entry>> = Mutex::new(Vec::new());

/// the turns that can be recalled, newest first, for the Java side (`nativeRecallTurns`)
pub fn turns() -> Vec<u16> {
    ENTRIES.lock().unwrap_or_else(|e| e.into_inner()).iter().filter(|e| e.kind == TURN_START).map(|e| e.turn).collect()
}

/// a request from the game's menu: an entry id + 1, or (turn | TURN_REQUEST) for the older call
static REQUEST: AtomicU32 = AtomicU32::new(0);
const TURN_REQUEST: u32 = 0x8000_0000;

/// For title.so: the history, newest first, into `out`; gives how many entries there are.
///
/// # Safety
/// `out` points to `length` writable entries (or `length` is 0).
#[no_mangle]
pub unsafe extern "C" fn zakuro_recall_entries(out: *mut Entry, length: usize) -> usize {
    let entries = ENTRIES.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if !out.is_null() {
        for (i, entry) in entries.iter().take(length).enumerate() {
            // SAFETY: the caller gives room for `length` entries and i < length
            unsafe { *out.add(i) = *entry };
        }
    }
    entries.len()
}

/// the unit command last chosen in the game's command menu (an entry kind), 0 for none
static COMMAND: AtomicU8 = AtomicU8::new(0);

/// For title.so: a unit command was chosen (an entry kind: Attack, Wait, Dance, Staff, Items, Pair
/// Up, Separate, Rally, Talk, Visit). The last one chosen before the unit has acted names the action.
#[no_mangle]
pub extern "C" fn zakuro_recall_command(kind: u8) {
    COMMAND.store(kind, Ordering::Release);
}

/// For title.so: go back to that entry (its id) after this frame.
#[no_mangle]
pub extern "C" fn zakuro_recall_go(id: u32) {
    REQUEST.store(id.wrapping_add(1) & !TURN_REQUEST, Ordering::Release);
}

/// For title.so: the turns that can be recalled, newest first, into `out`; gives how many.
///
/// # Safety
/// `out` points to `length` writable u16s (or `length` is 0).
#[no_mangle]
pub unsafe extern "C" fn zakuro_recall_turns(out: *mut u16, length: usize) -> usize {
    let turns = turns();
    if !out.is_null() {
        for (i, turn) in turns.iter().take(length).enumerate() {
            // SAFETY: the caller gives room for `length` values and i < length
            unsafe { *out.add(i) = *turn };
        }
    }
    turns.len()
}

/// For title.so: go back to the start of that turn's player phase after this frame.
#[no_mangle]
pub extern "C" fn zakuro_recall_request(turn: u16) {
    REQUEST.store(TURN_REQUEST | u32::from(turn), Ordering::Release);
}

/// a unit as last seen: enough to tell what an action did
#[derive(Clone, Copy, PartialEq, Eq)]
struct Seen {
    army: u32,
    hp: u8,
    x: u8,
    y: u8,
    acted: bool,
    person: u32,
    index: u16,
}

struct Checkpoint {
    id: u32,
    turn: u16,
    state: Vec<u8>,
    /// the units when it was taken
    units: HashMap<u32, Seen>,
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
    checkpoints: Vec<Checkpoint>,
    /// actions, oldest first: (entry, the checkpoint id before it)
    actions: Vec<Entry>,
    next_id: u32,
    /// the units last frame (address -> what was seen), during a player phase
    units: HashMap<u32, Seen>,
    watched: [u32; WATCH_WORDS],
    watched_units: HashMap<u32, Vec<u32>>,
    watched_mind: Vec<u32>,
    persons_logged: HashSet<u32>,
}

impl Recall {
    pub fn new() -> Recall {
        let watch = crate::platform::system_property(c"debug.zakuro.recall_watch").as_deref() == Some("1");
        let recall = Recall {
            on: true,
            checked_title: false,
            watch,
            battle: 0,
            saved: None,
            looked: false,
            checkpoints: Vec::new(),
            actions: Vec::new(),
            next_id: 0,
            units: HashMap::new(),
            watched: [0; WATCH_WORDS],
            watched_units: HashMap::new(),
            watched_mind: Vec::new(),
            persons_logged: HashSet::new(),
        };
        recall.publish();
        recall
    }

    /// after each emulated frame: a restore the game's menu asked for is done (and the message for
    /// it given back), else checkpoints are taken
    pub fn after_frame(&mut self, system: &mut System) -> Option<Result<String, String>> {
        let request = REQUEST.swap(0, Ordering::AcqRel);
        if !self.on {
            return None;
        }
        if request != 0 && self.checked_title {
            return Some(if request & TURN_REQUEST != 0 {
                let turn = request as u16;
                self.recall(system, turn).map(|()| format!("Back to turn {turn}"))
            } else {
                self.go(system, request - 1)
            });
        }
        self.saving(system);
        None
    }

    fn saving(&mut self, system: &mut System) {
        if !self.checked_title {
            self.checked_title = true;
            let id = system.title.as_ref().map(|t| t.program_id()).unwrap_or(0);
            if id != AWAKENING_USA {
                self.on = false;
                return;
            }
            log::info!("Recall Fate: saving each player phase and action{}", if self.watch { " (and watching the battle)" } else { "" });
        }
        let battle = system.memory.peek32(BATTLE_INFO);
        if self.watch {
            self.log_changes(system, battle);
            self.log_units(system);
        }
        if battle != self.battle {
            if !self.checkpoints.is_empty() {
                log::info!("Recall Fate: battle {:08X} -> {battle:08X}, {} checkpoints dropped", self.battle, self.checkpoints.len());
            }
            self.battle = battle;
            self.clear();
        }
        if battle == 0 {
            return;
        }
        let flags = system.memory.peek32(battle + FLAGS);
        let word = system.memory.peek32(battle + PHASE);
        let (phase, turn) = (word as u16, (word >> 16) as u16);
        debug_assert_eq!(TURN, PHASE + 2);
        let playing = phase == PLAYER_PHASE && turn != 0 && flags & IN_CONTROL != 0 && flags & CHANGING_PHASE == 0;
        let first_look = !self.looked;
        self.looked = true;
        if !playing {
            self.units.clear();
            COMMAND.store(0, Ordering::Release);
            return;
        }
        let units = read_units(system);
        if self.saved != Some(turn) {
            self.saved = Some(turn);
            self.units = units;
            // a battle already in a player phase when first seen (a state loaded there) is not
            // saved halfway through the phase; its next player phase is
            if !first_look {
                self.checkpoint(system, turn, "turn");
            }
            COMMAND.store(0, Ordering::Release);
            return;
        }
        // an action has ended: a player unit has acted, or one has gone (fell in its own combat)
        let actor = units
            .iter()
            .find(|(address, now)| now.army == PLAYER_ARMY && now.acted && self.units.get(*address).is_some_and(|before| before.army == PLAYER_ARMY && !before.acted))
            .map(|(address, now)| (*address, *now))
            .or_else(|| {
                self.units.iter().find(|(address, before)| before.army == PLAYER_ARMY && !before.acted && !units.contains_key(*address)).map(|(a, s)| (*a, *s))
            });
        let previous = std::mem::replace(&mut self.units, units);
        let Some((address, actor)) = actor else { return };
        let command = COMMAND.swap(0, Ordering::AcqRel);
        // the newest checkpoint is the moment before this action (a battle seen first mid-phase
        // has none yet)
        let Some(before) = self.checkpoints.last() else { return };
        if before.turn != turn {
            return;
        }
        let kind = named(command, kind_of(address, &before.units, &self.units));
        let entry = Entry { id: before.id, turn, unit: actor.index, kind, pad: [0; 3], person: actor.person };
        self.actions.push(entry);
        log::info!("Recall Fate: turn {turn}, unit {} {} (from checkpoint {})", actor.index, kind_name(kind), before.id);
        let _ = previous;
        self.checkpoint(system, turn, "action");
    }

    fn checkpoint(&mut self, system: &mut System, turn: u16, why: &str) {
        let start = std::time::Instant::now();
        match system.save_state() {
            Ok(state) => {
                let bytes = state.len();
                let id = self.next_id;
                self.next_id += 1;
                self.checkpoints.push(Checkpoint { id, turn, state, units: self.units.clone() });
                while self.checkpoints.iter().map(|c| c.state.len()).sum::<usize>() > MAX_BYTES && self.checkpoints.len() > 1 {
                    let gone = self.checkpoints.remove(0).id;
                    self.actions.retain(|a| a.id != gone);
                }
                self.publish();
                log::info!(
                    "Recall Fate: checkpoint {id} ({why}, turn {turn}): {:.1} MB in {:.0} ms, {} kept ({:.0} MB)",
                    bytes as f64 / 1048576.0,
                    start.elapsed().as_secs_f64() * 1000.0,
                    self.checkpoints.len(),
                    self.checkpoints.iter().map(|c| c.state.len()).sum::<usize>() as f64 / 1048576.0
                );
            }
            Err(error) => log::warn!("Recall Fate: turn {turn} {why} not saved: {error}"),
        }
    }

    fn clear(&mut self) {
        self.saved = None;
        self.looked = false;
        self.checkpoints.clear();
        self.actions.clear();
        self.units.clear();
        self.publish();
    }

    /// a save state was loaded: the history so far may be from another time line
    pub fn reset(&mut self) {
        self.battle = 0;
        self.clear();
    }

    /// takes the battle back to the start of that turn's player phase; later history is dropped
    pub fn recall(&mut self, system: &mut System, turn: u16) -> Result<(), String> {
        let id = self.turn_checkpoint(turn).ok_or(format!("Turn {turn} cannot be recalled"))?;
        self.restore(system, id)
    }

    /// takes the battle back to an entry (a turn's start, or the moment before an action)
    fn go(&mut self, system: &mut System, id: u32) -> Result<String, String> {
        let at = self.checkpoints.iter().find(|c| c.id == id).ok_or("That moment cannot be recalled")?;
        let turn = at.turn;
        let message = match self.actions.iter().find(|a| a.id == id) {
            Some(action) => format!("Back to turn {turn}, before that {}", kind_name(action.kind)),
            None => format!("Back to turn {turn}"),
        };
        self.restore(system, id)?;
        Ok(message)
    }

    fn turn_checkpoint(&self, turn: u16) -> Option<u32> {
        self.checkpoints.iter().find(|c| c.turn == turn).map(|c| c.id)
    }

    fn restore(&mut self, system: &mut System, id: u32) -> Result<(), String> {
        let at = self.checkpoints.iter().position(|c| c.id == id).ok_or("That moment cannot be recalled")?;
        let start = std::time::Instant::now();
        system.load_state(&self.checkpoints[at].state)?;
        let turn = self.checkpoints[at].turn;
        self.checkpoints.truncate(at + 1);
        // the actions from this checkpoint on are undone
        self.actions.retain(|a| a.id < id);
        self.units = self.checkpoints[at].units.clone();
        // as it was when the checkpoint was taken: that turn's player phase already saved
        self.saved = Some(turn);
        self.looked = true;
        self.publish();
        log::info!("Recall Fate: back to checkpoint {id} (turn {turn}) in {:.0} ms", start.elapsed().as_secs_f64() * 1000.0);
        Ok(())
    }

    /// the history for the game's menu, newest first: each turn's start, its actions after it
    fn publish(&self) {
        let mut entries = Vec::new();
        for checkpoint in &self.checkpoints {
            // a turn's first checkpoint is its start (and also the moment before its first action)
            let is_turn_start = self.checkpoints.iter().find(|c| c.turn == checkpoint.turn).map(|c| c.id) == Some(checkpoint.id);
            if is_turn_start {
                entries.push(Entry { id: checkpoint.id, turn: checkpoint.turn, unit: 0, kind: TURN_START, pad: [0; 3], person: 0 });
            }
            if let Some(action) = self.actions.iter().find(|a| a.id == checkpoint.id) {
                entries.push(*action);
            }
        }
        entries.reverse();
        *ENTRIES.lock().unwrap_or_else(|e| e.into_inner()) = entries;
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

    /// development watch: changes in the player army's units and in the mind, word by word, and
    /// each player unit's person once (its words, and any text its words point at)
    fn log_units(&mut self, system: &mut System) {
        let units = system.memory.peek32(UNITS);
        let armies = system.memory.peek32(ARMIES);
        if units == 0 || armies == 0 {
            return;
        }
        let mut unit = system.memory.peek32(armies);
        let mut seen = 0;
        while unit != 0 && seen < 64 {
            seen += 1;
            let words: Vec<u32> = (0..UNIT_SIZE / 4).map(|i| system.memory.peek32(unit + 4 * i)).collect();
            let index = (unit.wrapping_sub(units) / UNIT_SIZE) + 1;
            if let Some(old) = self.watched_units.get(&unit) {
                let changed: Vec<String> =
                    (0..words.len()).filter(|&i| words[i] != old[i]).map(|i| format!("+{:02X}:{:08X}->{:08X}", 4 * i, old[i], words[i])).collect();
                if !changed.is_empty() {
                    log::info!("Recall Fate watch: unit {index} @{unit:08X} {}", changed.join(" "));
                }
            }
            let person = words[(UNIT_PERSON / 4) as usize];
            if person != 0 && self.persons_logged.insert(person) {
                let person_words: Vec<u32> = (0..0x20).map(|i| system.memory.peek32(person + 4 * i)).collect();
                let texts: Vec<String> = person_words
                    .iter()
                    .enumerate()
                    .filter(|(_, &w)| (0x0010_0000..0x3000_0000).contains(&w))
                    .filter_map(|(i, &w)| {
                        let bytes: Vec<u8> = (0..32).map(|k| (system.memory.peek32((w + k) & !3) >> (8 * ((w + k) & 3))) as u8).take_while(|&b| b != 0).collect();
                        (bytes.len() >= 2 && bytes.iter().all(|&b| b >= 0x20)).then(|| format!("+{:02X}->{:?}", 4 * i, String::from_utf8_lossy(&bytes)))
                    })
                    .collect();
                log::info!(
                    "Recall Fate watch: unit {index} person @{person:08X} {} | {}",
                    person_words.iter().map(|w| format!("{w:08X}")).collect::<Vec<_>>().join(" "),
                    texts.join(" ")
                );
            }
            let next = words[(UNIT_NEXT / 4) as usize];
            self.watched_units.insert(unit, words);
            unit = next;
        }
        let mind = system.memory.peek32(MIND);
        if mind != 0 {
            let words: Vec<u32> = (0..MIND_WORDS / 4).map(|i| system.memory.peek32(mind + 4 * i)).collect();
            if words.len() == self.watched_mind.len() {
                let changed: Vec<String> = (0..words.len())
                    .filter(|&i| words[i] != self.watched_mind[i])
                    .map(|i| format!("+{:02X}:{:08X}->{:08X}", 4 * i, self.watched_mind[i], words[i]))
                    .collect();
                if !changed.is_empty() {
                    log::info!("Recall Fate watch: mind @{mind:08X} {}", changed.join(" "));
                }
            }
            self.watched_mind = words;
        }
    }
}

fn peek8(system: &mut System, address: u32) -> u8 {
    (system.memory.peek32(address & !3) >> (8 * (address & 3))) as u8
}

/// the units of the first armies, by address
fn read_units(system: &mut System) -> HashMap<u32, Seen> {
    let mut out = HashMap::new();
    let units = system.memory.peek32(UNITS);
    let armies = system.memory.peek32(ARMIES);
    if units == 0 || armies == 0 {
        return out;
    }
    for army in 0..ARMIES_SEEN {
        let mut unit = system.memory.peek32(armies + 12 * army);
        let mut count = 0;
        while unit != 0 && count < 128 {
            count += 1;
            out.insert(
                unit,
                Seen {
                    army,
                    hp: peek8(system, unit + UNIT_HP),
                    x: peek8(system, unit + UNIT_X),
                    y: peek8(system, unit + UNIT_X + 1),
                    acted: system.memory.peek32(unit + UNIT_STATE) & HAS_ACTED != 0,
                    person: system.memory.peek32(unit + UNIT_PERSON),
                    index: (unit.wrapping_sub(units) / UNIT_SIZE + 1) as u16,
                },
            );
            unit = system.memory.peek32(unit + UNIT_NEXT);
        }
    }
    out
}

/// what an action did, from the units before and after it
fn kind_of(actor: u32, before: &HashMap<u32, Seen>, after: &HashMap<u32, Seen>) -> u8 {
    let hurt = before.iter().any(|(address, b)| b.army != PLAYER_ARMY && after.get(address).is_none_or(|a| a.hp < b.hp));
    let healed = before.iter().any(|(address, b)| *address != actor && b.army == PLAYER_ARMY && after.get(address).is_some_and(|a| a.hp > b.hp));
    let moved = match (before.get(&actor), after.get(&actor)) {
        (Some(b), Some(a)) => (b.x, b.y) != (a.x, a.y),
        _ => false,
    };
    if hurt || !after.contains_key(&actor) {
        ATTACK
    } else if healed {
        HEAL
    } else if moved {
        MOVE
    } else {
        WAIT
    }
}

/// an action's kind from the command chosen for it (0 for none seen) and what it changed: Wait
/// (or no command) leaves it to the change, so a unit that moved first reads as a move
fn named(command: u8, changed: u8) -> u8 {
    match command {
        ATTACK | DANCE..=VISIT => command,
        _ => changed,
    }
}

pub fn kind_name(kind: u8) -> &'static str {
    match kind {
        TURN_START => "turn",
        ATTACK => "attack",
        HEAL => "heal",
        MOVE => "move",
        DANCE => "dance",
        STAFF => "staff",
        ITEM => "item",
        PAIR_UP => "pair up",
        SEPARATE => "separate",
        RALLY => "rally",
        TALK => "talk",
        VISIT => "visit",
        _ => "wait",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(army: u32, hp: u8, x: u8) -> Seen {
        Seen { army, hp, x, y: 0, acted: false, person: 0, index: 1 }
    }

    #[test]
    fn an_action_is_named_by_what_it_changed() {
        let before = HashMap::from([(1, seen(0, 20, 1)), (2, seen(1, 20, 5)), (3, seen(0, 10, 2))]);
        let mut after = before.clone();
        assert_eq!(kind_of(1, &before, &after), WAIT);
        after.insert(1, seen(0, 20, 2));
        assert_eq!(kind_of(1, &before, &after), MOVE);
        after.insert(3, seen(0, 15, 2));
        assert_eq!(kind_of(1, &before, &after), HEAL);
        after.insert(2, seen(1, 12, 5));
        assert_eq!(kind_of(1, &before, &after), ATTACK);
        after.remove(&2);
        assert_eq!(kind_of(1, &before, &after), ATTACK);
    }

    #[test]
    fn a_chosen_command_names_the_action_unless_it_is_wait() {
        assert_eq!(named(0, MOVE), MOVE);
        assert_eq!(named(WAIT, MOVE), MOVE);
        assert_eq!(named(WAIT, WAIT), WAIT);
        assert_eq!(named(ATTACK, WAIT), ATTACK);
        assert_eq!(named(STAFF, HEAL), STAFF);
        assert_eq!(named(PAIR_UP, ATTACK), PAIR_UP);
        assert_eq!(named(99, MOVE), MOVE);
    }

    #[test]
    fn entries_are_laid_out_as_title_so_reads_them() {
        assert_eq!(std::mem::size_of::<Entry>(), 16);
        assert_eq!(std::mem::offset_of!(Entry, kind), 8);
        assert_eq!(std::mem::offset_of!(Entry, person), 12);
    }
}
