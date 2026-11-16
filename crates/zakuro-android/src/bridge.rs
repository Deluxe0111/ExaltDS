//! Java calls only enqueue commands and input. The emulator stays on android_main.
use android_activity::{AndroidApp, AndroidAppWaker};
use jni::{
    objects::{JObject, JString},
    sys::{jboolean, jfloat, jint},
    JNIEnv,
};
use std::{collections::VecDeque, sync::Mutex};

#[derive(Clone, Copy)]
pub enum Command {
    Pause,
    Resume,
    Restart,
    /// Slot 0 is the quick state; 1 to 9 are the numbered slots.
    Save(u8),
    Load(u8),
    Recalibrate,
    Swap,
    Exit,
    /// Recall Fate: back to the start of that turn's player phase (Fire Emblem Awakening)
    Recall(u16),
}
use crate::input::VirtualInput;
struct State {
    commands: VecDeque<Command>,
    input: VirtualInput,
    waker: Option<AndroidAppWaker>,
    second_window: Option<ndk::native_window::NativeWindow>,
    second_changed: bool,
    second_touch: Option<(f32, f32, u32, u32)>,
    /// the software keyboard's text and the button pressed, once typed.
    keyboard: Option<(String, usize)>,
}
static STATE: Mutex<State> = Mutex::new(State {
    commands: VecDeque::new(),
    input: VirtualInput {
        buttons: 0,
        touch: None,
        sticks: [None; 2],
        keys: std::collections::BTreeSet::new(),
    },
    waker: None,
    second_window: None,
    second_changed: false,
    second_touch: None,
    keyboard: None,
});

pub struct Session;
impl Session {
    pub fn start(app: &AndroidApp) -> Self {
        let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
        state.commands.clear();
        state.input = VirtualInput::default();
        state.waker = Some(app.create_waker());
        state.keyboard = None;
        Self
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
        state.waker = None;
        state.second_window = None;
        state.second_changed = false;
        state.second_touch = None;
        state.input = VirtualInput::default();
        state.commands.clear();
    }
}
pub fn pending() -> Vec<Command> {
    STATE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .commands
        .drain(..)
        .collect()
}
pub fn input() -> VirtualInput {
    STATE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .input
        .clone()
}
pub fn clear_input() {
    let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
    state.input = VirtualInput::default();
    state.second_touch = None;
}

/// the software keyboard's answer, once the dialog closed.
pub fn keyboard_answer() -> Option<(String, usize)> {
    STATE.lock().unwrap_or_else(|e| e.into_inner()).keyboard.take()
}
#[no_mangle]
pub extern "system" fn Java_io_github_zakuro_android_Game_nativeKeyboard(
    mut env: JNIEnv<'_>,
    _: JObject<'_>,
    text: JString<'_>,
    button: jint,
) {
    let text: String = env.get_string(&text).map(Into::into).unwrap_or_default();
    let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
    state.keyboard = Some((text, button.max(0) as usize));
    if let Some(waker) = &state.waker {
        waker.wake();
    }
}

/// Low 4 bits are the command id, the rest is the save-state slot (0 for plain SAVE/LOAD).
fn decode(command: i32) -> Option<Command> {
    if command & 15 == 8 {
        return u16::try_from(command >> 4).ok().filter(|turn| *turn > 0).map(Command::Recall);
    }
    let slot = u8::try_from(command >> 4).ok().filter(|slot| *slot <= 9)?;
    Some(match command & 15 {
        0 => Command::Pause,
        1 => Command::Save(slot),
        2 => Command::Load(slot),
        3 => Command::Restart,
        4 => Command::Recalibrate,
        5 => Command::Swap,
        6 => Command::Resume,
        7 => Command::Exit,
        _ => return None,
    })
}
#[no_mangle]
pub extern "system" fn Java_io_github_zakuro_android_Game_nativeCommand(
    _: JNIEnv<'_>,
    _: JObject<'_>,
    command: jint,
) {
    let Some(command) = decode(command) else {
        return;
    };
    let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
    if state.commands.len() >= 32 {
        state.commands.pop_front();
    }
    state.commands.push_back(command);
    if let Some(waker) = &state.waker {
        waker.wake();
    }
}
#[no_mangle]
pub extern "system" fn Java_io_github_zakuro_android_Game_nativeTouch(
    _: JNIEnv<'_>,
    _: JObject<'_>,
    mask: jint,
    x: jfloat,
    y: jfloat,
) {
    let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
    state.input.buttons = mask as u32 & 0xCFFF;
    state.input.touch =
        (x.is_finite() && y.is_finite() && (0.0..320.0).contains(&x) && (0.0..240.0).contains(&y))
            .then_some((x as u16, y as u16));
    if let Some(waker) = &state.waker {
        waker.wake();
    }
}
#[no_mangle]
pub extern "system" fn Java_io_github_zakuro_android_Game_nativeStick(
    _: JNIEnv<'_>,
    _: JObject<'_>,
    stick: jint,
    x: jfloat,
    y: jfloat,
) {
    if !(0..2).contains(&stick) {
        return;
    }
    let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
    state.input.sticks[stick as usize] =
        (x.is_finite() && y.is_finite()).then_some([x.clamp(-1.0, 1.0), y.clamp(-1.0, 1.0)]);
    if let Some(waker) = &state.waker {
        waker.wake();
    }
}
#[no_mangle]
pub extern "system" fn Java_io_github_zakuro_android_Game_nativeButton(
    _: JNIEnv<'_>,
    _: JObject<'_>,
    code: jint,
    pressed: jboolean,
) {
    if !(0..=1024).contains(&code) {
        return;
    }
    let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
    if pressed != 0 {
        state.input.keys.insert(code);
    } else {
        state.input.keys.remove(&code);
    }
    if let Some(waker) = &state.waker {
        waker.wake();
    }
}

/// Replacing a surface retains an ANativeWindow reference until android_main consumes it.
pub fn second_surface() -> Option<Option<ndk::native_window::NativeWindow>> {
    let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
    if !state.second_changed {
        return None;
    }
    state.second_changed = false;
    Some(state.second_window.take())
}
pub fn second_touch() -> Option<(f32, f32, u32, u32)> {
    STATE.lock().unwrap_or_else(|e| e.into_inner()).second_touch
}
#[no_mangle]
pub extern "system" fn Java_io_github_zakuro_android_Game_nativeSecondSurface(
    env: JNIEnv<'_>,
    _: JObject<'_>,
    surface: JObject<'_>,
) {
    let window = if surface.is_null() {
        None
    } else {
        unsafe {
            ndk::native_window::NativeWindow::from_surface(
                env.get_native_interface().cast(),
                surface.as_raw().cast(),
            )
        }
    };
    let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
    state.second_window = window;
    state.second_changed = true;
    state.second_touch = None;
    if let Some(waker) = &state.waker {
        waker.wake();
    }
}
#[no_mangle]
pub extern "system" fn Java_io_github_zakuro_android_Game_nativeSecondTouch(
    _: JNIEnv<'_>,
    _: JObject<'_>,
    pressed: jboolean,
    x: jfloat,
    y: jfloat,
    width: jint,
    height: jint,
) {
    let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
    state.second_touch = (pressed != 0
        && x.is_finite()
        && y.is_finite()
        && width > 0
        && height > 0)
        .then_some((x, y, width.max(0) as u32, height.max(0) as u32));
    if let Some(waker) = &state.waker {
        waker.wake();
    }
}

/// The time between the game's last pictures, in microseconds, for the performance overlay. Written by
/// the emulation thread once per game picture; read by the overlay's view a few times a second.
pub const FRAME_RING: usize = 256;
static FRAME_US: [std::sync::atomic::AtomicU32; FRAME_RING] =
    [const { std::sync::atomic::AtomicU32::new(0) }; FRAME_RING];
static FRAMES_RECORDED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
pub fn record_frame(time: std::time::Duration) {
    use std::sync::atomic::Ordering;
    let n = FRAMES_RECORDED.load(Ordering::Relaxed);
    let us = u32::try_from(time.as_micros()).unwrap_or(u32::MAX);
    FRAME_US[n as usize % FRAME_RING].store(us, Ordering::Relaxed);
    FRAMES_RECORDED.store(n.wrapping_add(1), Ordering::Release);
}
/// Recall Fate's turns, newest first (see recall.rs), into `out`; gives how many there are.
#[no_mangle]
pub extern "system" fn Java_io_github_zakuro_android_Game_nativeRecallTurns(
    env: JNIEnv<'_>,
    _: JObject<'_>,
    out: jni::objects::JIntArray<'_>,
) -> jint {
    let turns: Vec<i32> = crate::recall::turns().into_iter().map(i32::from).collect();
    let len = (env.get_array_length(&out).unwrap_or(0).max(0) as usize).min(turns.len());
    let _ = env.set_int_array_region(&out, 0, &turns[..len]);
    turns.len() as jint
}

/// Copies the ring into `out` (at most FRAME_RING ints) and returns how many pictures were recorded so
/// far: entry `n % FRAME_RING` holds picture `n`.
#[no_mangle]
pub extern "system" fn Java_io_github_zakuro_android_Game_nativeFrameTimes(
    env: JNIEnv<'_>,
    _: JObject<'_>,
    out: jni::objects::JIntArray<'_>,
) -> jint {
    use std::sync::atomic::Ordering;
    let n = FRAMES_RECORDED.load(Ordering::Acquire);
    let ring: Vec<i32> = FRAME_US
        .iter()
        .map(|t| t.load(Ordering::Relaxed).min(i32::MAX as u32) as i32)
        .collect();
    let len = env.get_array_length(&out).unwrap_or(0).clamp(0, FRAME_RING as i32) as usize;
    let _ = env.set_int_array_region(&out, 0, &ring[..len]);
    n as jint
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn command_ids_carry_the_state_slot() {
        assert!(matches!(decode(1), Some(Command::Save(0))));
        assert!(matches!(decode(2), Some(Command::Load(0))));
        assert!(matches!(decode(1 | 3 << 4), Some(Command::Save(3))));
        assert!(matches!(decode(2 | 9 << 4), Some(Command::Load(9))));
        assert!(decode(1 | 10 << 4).is_none() && decode(8).is_none() && decode(-1).is_none());
        assert!(matches!(decode(7), Some(Command::Exit)));
        assert!(matches!(decode(8 | 12 << 4), Some(Command::Recall(12))));
        assert!(decode(8).is_none());
    }
}
