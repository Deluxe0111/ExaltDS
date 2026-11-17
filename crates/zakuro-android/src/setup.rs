//! ExaltDS's setup, on the device: from the player's own Fire Emblem Awakening (USA) and,
//! optionally, its DLC, it builds the mod layers (exalt-mods: the feet models, Naga's Tear) and the
//! game's native code: 3dsrecomp writes the game's code as C with ExaltDS's overrides (Recall
//! Fate, the feet ground hook), and the clang the APK carries (android/exalt/clang-kit.py) compiles
//! and links it into title.so. ExaltSetup.java copies the files in, calls these, and installs the
//! result as the game. Nothing of the game is shipped with the app.
//!
//! The clang programs sit in the app's native library folder (the only place Android lets an app
//! run programs from), their libraries beside them (LD_LIBRARY_PATH), and their headers and
//! runtime in the files the kit's zip was unpacked to.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jintArray, jstring};
use jni::JNIEnv;

/// what setup is doing, for the start page: 0 idle, 1 feet, 2 Naga's Tear, 3 writing the C,
/// 4 compiling, 5 linking, 6 done
static STAGE: AtomicU32 = AtomicU32::new(0);
static DONE: AtomicU32 = AtomicU32::new(0);
static TOTAL: AtomicU32 = AtomicU32::new(0);
static CANCEL: AtomicBool = AtomicBool::new(false);

const USA: u64 = exalt_mods::USA;
/// clang's target: the API level ExaltDS starts at (Android 11)
const TARGET: &str = "--target=aarch64-linux-android30";

fn stage(stage: u32, done: u32, total: u32) {
    DONE.store(done, Ordering::Relaxed);
    TOTAL.store(total, Ordering::Relaxed);
    STAGE.store(stage, Ordering::Release);
}

fn string(env: &mut JNIEnv, s: &JString) -> String {
    if s.is_null() {
        return String::new();
    }
    env.get_string(s).map(Into::into).unwrap_or_default()
}

fn answer(env: &mut JNIEnv, text: Option<String>) -> jstring {
    match text {
        Some(text) => env.new_string(text).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut()),
        None => std::ptr::null_mut(),
    }
}

/// Whether the files are a game setup can use: null when they are, else why not.
fn inspect(rom: &Path, dlc: Option<&Path>) -> Result<(), String> {
    let title = zakuro_fs::Title::load(rom).map_err(|e| format!("The game file can't be read: {e}. It must be a decrypted .3ds, .cci or .cia."))?;
    match title.program_id() {
        USA => {}
        exalt_mods::EUR => return Err("This is the European release. ExaltDS supports the US release (00040000000A0500) for now.".into()),
        other => return Err(format!("This is not Fire Emblem Awakening (US) but title {other:016X}.")),
    }
    if let Some(dlc) = dlc {
        let dlc = zakuro_fs::Dlc::load(dlc).map_err(|e| format!("The DLC file can't be used: {e}. It must be the decrypted DLC .cia."))?;
        if dlc.title_id & 0xFFFF_FFFF != USA & 0xFFFF_FFFF {
            return Err(format!("The DLC file is DLC for another game ({:016X}).", dlc.title_id));
        }
    }
    Ok(())
}

#[no_mangle]
pub extern "system" fn Java_io_github_zakuro_android_ExaltSetup_nativeInspect<'a>(
    mut env: JNIEnv<'a>,
    _: JClass<'a>,
    rom: JString<'a>,
    dlc: JString<'a>,
) -> jstring {
    crate::platform::init_log();
    let (rom, dlc) = (string(&mut env, &rom), string(&mut env, &dlc));
    let result = inspect(Path::new(&rom), (!dlc.is_empty()).then(|| Path::new(&dlc)));
    answer(&mut env, result.err())
}

/// [stage, done, total]
#[no_mangle]
pub extern "system" fn Java_io_github_zakuro_android_ExaltSetup_nativeProgress<'a>(env: JNIEnv<'a>, _: JClass<'a>) -> jintArray {
    let values = [STAGE.load(Ordering::Acquire) as i32, DONE.load(Ordering::Relaxed) as i32, TOTAL.load(Ordering::Relaxed) as i32];
    match env.new_int_array(3) {
        Ok(array) => {
            let _ = env.set_int_array_region(&array, 0, &values);
            array.into_raw()
        }
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "system" fn Java_io_github_zakuro_android_ExaltSetup_nativeCancel<'a>(_: JNIEnv<'a>, _: JClass<'a>) {
    CANCEL.store(true, Ordering::Release);
}

/// Builds the layers into `layers/<name>` and the game code into `work/title.so`. Null when done,
/// else what went wrong.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_io_github_zakuro_android_ExaltSetup_nativeBuild<'a>(
    mut env: JNIEnv<'a>,
    _: JClass<'a>,
    rom: JString<'a>,
    work: JString<'a>,
    kit: JString<'a>,
    programs: JString<'a>,
    layers: JString<'a>,
    feet: jboolean,
    naga: jboolean,
) -> jstring {
    let [rom, work, kit, programs, layers] = [&rom, &work, &kit, &programs, &layers].map(|s| PathBuf::from(string(&mut env, s)));
    crate::platform::init_log();
    CANCEL.store(false, Ordering::Release);
    let result = build(&rom, &work, &kit, &programs, &layers, feet != 0, naga != 0);
    if let Err(error) = &result {
        log::error!("ExaltDS setup: {error}");
    }
    stage(if result.is_ok() { 6 } else { 0 }, 0, 0);
    answer(&mut env, result.err())
}

fn build(rom: &Path, work: &Path, kit: &Path, programs: &Path, layers: &Path, feet: bool, naga: bool) -> Result<(), String> {
    let started = std::time::Instant::now();
    let title = zakuro_fs::Title::load(rom).map_err(|e| format!("The game file can't be read: {e}"))?;
    if feet {
        stage(1, 0, 0);
        let made = exalt_mods::feet::build(&title, &layers.join("Feet"), &|done, total| stage(1, done as u32, total as u32))?;
        log::info!("ExaltDS setup: feet for {made} bodies");
    }
    if naga {
        stage(2, 0, 0);
        exalt_mods::naga::layer(&title, &layers.join("Naga's Tear"))?;
        log::info!("ExaltDS setup: Naga's Tear layer built");
    }
    drop(title);
    if CANCEL.load(Ordering::Acquire) {
        return Err("Setup was stopped.".into());
    }

    // the overrides, as files 3dsrecomp reads
    let overrides = work.join("overrides");
    std::fs::create_dir_all(&overrides).map_err(|e| e.to_string())?;
    for (name, text) in exalt_mods::OVERRIDES {
        std::fs::write(overrides.join(name), text).map_err(|e| format!("{name}: {e}"))?;
    }
    let generated = work.join("generated");
    let _ = std::fs::remove_dir_all(&generated);

    // clang's libraries are found beside it (the app's own are loaded already: the variable only
    // reaches the programs setup starts, clang and lld)
    std::env::set_var("LD_LIBRARY_PATH", programs);
    let flags = [
        TARGET.to_owned(),
        format!("--sysroot={}", kit.join("sysroot").display()),
        format!("-resource-dir={}", kit.join("lib/clang/21").display()),
        format!("--ld-path={}", programs.join("libexalt-ld-.so").display()),
        "-Qunused-arguments".to_owned(),
        "-Wl,-z,max-page-size=16384".to_owned(),
        "-Wl,--no-undefined".to_owned(),
        "-Wl,--no-as-needed".to_owned(),
        "-lm".to_owned(),
    ];
    let flags: Vec<&str> = flags.iter().map(String::as_str).collect();
    let compiler = recomp3ds::compile::Compiler::new(programs.join("libexalt-clang.so"), &flags);
    if !compiler.runs() {
        return Err("The compiler that comes with ExaltDS does not run on this device.".into());
    }
    stage(3, 0, 0);
    let options = recomp3ds::build::Options {
        dir: Some(&generated),
        overrides: Some(&overrides),
        cancel: Some(&CANCEL),
        compiler: Some(compiler),
        ..Default::default()
    };
    let library = recomp3ds::build::build(rom, &options, &|event| match event {
        recomp3ds::build::Event::Generated { files, bytes, overrides } => {
            log::info!("ExaltDS setup: game code written as C, {files} files, {} MB, {overrides} overrides", bytes >> 20);
            stage(4, 0, files as u32);
        }
        recomp3ds::build::Event::Compiled { done, total } => stage(if done == total { 5 } else { 4 }, done as u32, total as u32),
        recomp3ds::build::Event::Note(note) => log::info!("ExaltDS setup: {note}"),
        other => log::info!("ExaltDS setup: {other:?}"),
    })
    .map_err(|e| if e == "stopped" { "Setup was stopped.".to_owned() } else { format!("Building the game code failed: {e}") })?;
    std::fs::rename(&library, work.join("title.so"))
        .or_else(|_| std::fs::copy(&library, work.join("title.so")).map(drop))
        .map_err(|e| format!("title.so: {e}"))?;
    // the C and the objects are big and no longer needed
    let _ = std::fs::remove_dir_all(&generated);
    log::info!("ExaltDS setup: built in {:.0} s", started.elapsed().as_secs_f64());
    Ok(())
}
