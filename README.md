# ExaltDS

**Fire Emblem Awakening, recompiled for Android handhelds.** ExaltDS turns your own copy of the game into native ARM64
code on your device and runs it with a Vulkan renderer, at up to 4× resolution and 60 fps in combat.

> Early, experimental software. Tested on an AYN Thor (Snapdragon 8 Gen 2) with the US release of the game.

ExaltDS contains **no game files**. You need your own legally obtained, decrypted copy of Fire Emblem Awakening.

## What you get

- **Native game code**, built once from your ROM on the device. No CPU emulation while you play.
- **Widescreen 16:9**, resolution up to 4×, and combat at 60 fps (original speed or faster).
- **Feet on the ground**: the feet mod from the
  [Fire Emblem: Awakening recompilation project](https://github.com/Zelkin1/fe-awakening-recomp), built from your copy.
- **Naga's Tear** (experimental): Recall Fate in the battle map menu takes you back to before any earlier action or
  turn of the battle.
- **HD texture packs**, if you have one (not included).
- A sidebar (press Back) with save states, a performance monitor, the clock, mods and GPU drivers.

## Requirements

| | |
| --- | --- |
| **Device** | 64-bit ARM, Android 11 or newer, Vulkan 1.3. Tested: AYN Thor (Snapdragon 8 Gen 2). |
| **Game** | Fire Emblem Awakening, US release (`00040000000A0500`), as a decrypted `.3ds`/`.cci` or `.cia`. |
| **DLC** | Optional: the decrypted DLC `.cia`. |
| **Storage** | The game's size (about 2 GB) plus about 1 GB free during setup; the game's size plus about 0.3 GB after. |
| **Memory** | 8 GB of RAM or more recommended: setup runs a compiler on every core. Tested with 16 GB. |

## Install

1. Download the latest APK from the [Releases page](https://github.com/teton-dev/ExaltDS/releases).
2. Install it. Later versions update from the sidebar (**Check for updates**); install over the app, **don't uninstall
   first** or you lose your saves.
3. Open ExaltDS, choose your game file and, if you have it, the DLC file. Tick **Naga's Tear** if you want it, then
   **Set up**.
4. Setup copies the game into the app, builds the feet and Naga's Tear from it and recompiles the game's code for your
   device. On the AYN Thor this takes about 5 minutes; keep ExaltDS open and the device charging. Your saves are kept,
   and you can delete your own copy of the game file afterwards.
5. Press **Play**. Settings, mods, texture packs and save states are in the sidebar: press Back in game.

## Naga's Tear

When you turn it on, Robin and Chrom find Naga's Tear the next time you enter the world map. From then on, **Recall
Fate** in the battle map menu lists every turn and action of the battle; choose one to return to just before it. It is
experimental. It is a box in setup; turning it on or off later, in the sidebar, needs a restart (Apply and restart).

## Privacy

ExaltDS asks once whether to share **anonymous diagnostics**: device model, chipset and GPU names, Android version, RAM,
frame-rate totals and crash locations (library and offset), tied to a random ID. Never your name, accounts, IP address,
files, saves or game data. Change it any time in the sidebar. Update checks ask GitHub for the latest release and send
nothing else.

## Building

ExaltDS is built from this repository (a fork of [Zakuroishi](https://github.com/Sekyleky/Zakuroishi), whose own
README is [docs/zakuroishi.md](docs/zakuroishi.md)) on Windows with `android/scripts/build-windows.ps1 -Flavor exalt`,
the Android SDK and NDK r30, a JDK and Rust 1.95. The on-device compiler comes from `android/exalt/clang-kit.py`.

## Credits and licenses

ExaltDS is built on [Zakuroishi](https://github.com/Sekyleky/Zakuroishi) and [Zakuro](https://github.com/fearkov/zakuro)
(MIT), with game code recompiled by [3dsrecomp](https://github.com/fearkov/3dsrecomp) (MIT). The feet mod is from the
[Fire Emblem: Awakening recompilation project](https://github.com/Zelkin1/fe-awakening-recomp) (MIT). The on-device
compiler is [LLVM/Clang](https://llvm.org) (Apache-2.0 with LLVM exceptions), as built for Android by
[Termux](https://github.com/termux/termux-packages). All notices, including the libraries the compiler needs, are in
[`android/exalt/THIRD_PARTY_NOTICES.md`](android/exalt/THIRD_PARTY_NOTICES.md) and in the app.

Not affiliated with Nintendo or Intelligent Systems. Fire Emblem is a trademark of Nintendo.
