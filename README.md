<h1 align="center">ExaltDS</h1>

<p align="center"><b>Fire Emblem Awakening, rebuilt to run natively on dual-screen Android handhelds.</b></p>

<p align="center">
  <a href="https://github.com/teton-dev/ExaltDS/releases">Download</a> ·
  <a href="#recall-fate">Recall Fate</a> ·
  <a href="#hd-cutscenes-experimental">HD cutscenes</a> ·
  <a href="#install">Install</a> ·
  <a href="#where-its-going">Roadmap</a>
</p>

<p align="center"><img src="docs/screenshots/exaltds-recall-fate.jpg" alt="Recall Fate listing the actions of a turn" width="720"></p>

ExaltDS takes your own copy of Awakening and turns its code into native ARM64 on your handheld. There is no 3DS
processor being emulated while you play, so the game has room to run at up to 4× resolution and a steady 60 fps in
cutscenes and attack animations, on both screens, the way a handheld with two displays should.

It is built for two devices, and tuned for them: the **AYN Thor** and the **Retroid Pocket Duo**.

> **Status:** early, but playable. Everything here has been tested on the AYN Thor (Snapdragon 8 Gen 2) with the US
> release of the game. The Retroid Pocket Duo is a target device that hasn't been tested yet.

ExaltDS contains **no game files**. You need your own legally obtained, decrypted copy of Fire Emblem Awakening.

## Why ExaltDS

- **Made for two screens.** The top and bottom screens use the device's own two displays, with the top screen in
  true 16:9. No letterboxing, no stretching a single window across both.
- **Native, not emulated.** The game's code is recompiled once, on your device, from your own copy. Graphics still translated.
- **Sharper than the original.** Up to 4× resolution, optional HD texture packs, and a no-glow option so bright
  scenes stay clean instead of washing out.
- **Smooth.** Cutscenes and in-game attack animations run at 60 fps, at the original speed or faster.
- **Recall Fate.** Take back any move, not just the last one. More below.
- **One app, one setup.** Pick your game file, press **Set up**, press **Play**. No PC step, no extra tools.

## Recall Fate

<p align="center"><img src="docs/screenshots/exaltds-recall-fate-intro.jpg" alt="Robin and Chrom's introduction to Recall Fate" width="720"></p>

Fire Emblem is a game of consequences, and sometimes you want to see how it would have gone. Recall Fate adds a new
entry to the battle map menu, similar to Mila's Turnwheel or the Time Crystal, that lists every turn and every action of the current battle, each named after what
happened: Pick one and the battle returns to just before it.

- Go back to before any earlier action or turn, not only the last one.
- Rewind the last five turns of a battle.
- It sits in the game's own battle menu, in the game's own style.

A short scene between Robin and Chrom introduces it the first time you reach the world map.

Recall Fate (Naga's Tear) is on by default. You can turn it off during setup, or later in the sidebar (this needs a restart).
It's still experimental, so keep regular saves as you normally would.

## HD cutscenes (experimental)

Awakening's anime cutscenes are 400×240 video on the 3DS. If you have your own HD copy of them, such as a rip of the
*Fire Emblem: HD Movie Collection* Blu-ray (from Japan's *Valentia Complete* set) or another recording, ExaltDS can
play your HD version in their place. The 3D cutscenes stay as they are.

- **Nothing is included.** ExaltDS ships no cutscenes, video files or pictures from the game. It only uses the video you
  choose and your own game file.
- **Import once.** In the sidebar, open **Mods / textures**, choose **HD movies from your video…** and pick the video.
  ExaltDS decodes the game's own movies from your copy and searches your video for each of the 14. It finds where each
  one starts, any black bars, how the picture is framed, the frame rate and the brightness, then matches it to the game
  frame by frame. It then renders the matched frames into HD clips on your device, with the device's own video
  encoder, and checks each clip. On the AYN Thor this takes about 8 minutes for a 15-minute video. Then use **Apply and
  restart**.
- **While you play,** ExaltDS recognises each frame of a movie as the game plays it and draws your HD frame of the same
  moment over it. The game's own sound and subtitles stay. Anything your video doesn't have, such as a missing movie or
  a fade added by whoever recorded it, plays as the original.
- **What video works:** one file holding the movies, in any order. It can have intros or other footage between them,
  and some movies can be missing. MP4, MKV, TS or AVI with H.264, HEVC, MPEG-2 or VP9 video, progressive or interlaced.
- **Experimental.** It has been tested on the AYN Thor with a 1080p recording. Disc rips (interlaced or 23.976 fps)
  haven't been tested yet. The HD clips take about 0.6 GB; **Remove HD movies…** deletes them.

## Clean, bright 3D

The game's glow effect adds a haze around bright areas, and at higher resolutions it gets heavier. The **No glow**
option removes it, so characters and grass stay crisp without a halo and without the scene going dull.

<p align="center"><img src="docs/screenshots/exaltds-no-glow.jpg" alt="The same scene with the original glow and with No glow" width="900"></p>

## Everything else

- **Widescreen 16:9** and resolution up to 4×.
- **Feet on the ground:** the feet mod from the
  [Fire Emblem: Awakening recompilation project](https://github.com/Zelkin1/fe-awakening-recomp), built from your copy.
- **HD texture packs**, if you have one (not included).
- **HD cutscenes** from your own video (experimental, not included). See [above](#hd-cutscenes-experimental).
- **A sidebar** (press Back in game) with save states, Quick Save and Quick Load, a performance monitor, a clock, mods,
  and GPU drivers.
- **Updates from inside the app.**

<sub>The screenshots here use the [AAHD texture pack](https://gamebanana.com/mods/400809) (not included).</sub>

## Where it's going

ExaltDS targets the Thor and the Retroid Pocket Duo first, and it will stay that way until the main game and
performance are fully polished on them. Support for single-screen devices and phones is planned after that, not before.
It isn't supported today, and it's better to do it properly later than to do it partly now.

## Requirements

| | |
| --- | --- |
| **Device** | AYN Thor (tested) or Retroid Pocket Duo (target). 64-bit ARM, Android 11 or newer, Vulkan 1.3. |
| **Game** | Fire Emblem Awakening, US release (`00040000000A0500`), as a decrypted `.3ds`/`.cci` or `.cia`. |
| **DLC** | Optional: the decrypted DLC `.cia`. |
| **Storage** | The game's size (about 2 GB) plus about 1 GB free during setup; the game's size plus about 0.3 GB after. |
| **Memory** | 6 GB of RAM minimum; 8 GB or more recommended. Setup runs a compiler on every core. Tested with 16 GB. |

## Install

1. Download the latest APK from the [Releases page](https://github.com/teton-dev/ExaltDS/releases).
2. Install it. Later versions update from the sidebar (**Check for updates**); install over the app, **don't uninstall
   first** or you lose your saves.
3. Open ExaltDS, choose your game file and, if you have it, the DLC file. Then press **Set up**.
4. Setup copies the game into the app, builds the mods from it and recompiles the game's code for your device. On the
   AYN Thor this takes about 5 minutes; keep ExaltDS open and the device charging. Your saves are kept, and you can
   delete your own copy of the game file afterwards.
5. Press **Play**. Settings, mods, texture packs and save states are in the sidebar: press Back in game.

## Privacy

ExaltDS asks once whether to share **anonymous diagnostics**: device model, chipset and GPU names, Android version, RAM,
frame-rate totals and crash locations (library and offset), tied to a random ID. Never your name, accounts, IP address,
files, saves or game data. Change it any time in the sidebar. Update checks ask GitHub for the latest release and send
nothing else.

## Building

ExaltDS is built from this repository (a fork of [Zakuroishi](https://github.com/Sekyleky/Zakuroishi), whose own
README is [docs/zakuroishi.md](docs/zakuroishi.md)) on Windows with `android/scripts/build-windows.ps1 -Flavor exalt`,
the Android SDK and NDK r30, a JDK and Rust 1.95. The on-device compiler comes from `android/exalt/clang-kit.py`.
HD cutscene import needs FFmpeg for Android: build it with `android/scripts/build-ffmpeg.sh` and pass its folder to
`build-windows.ps1` as `-FFmpeg <folder>`.

## Credits and licenses

ExaltDS is built on [Zakuroishi](https://github.com/Sekyleky/Zakuroishi) and [Zakuro](https://github.com/fearkov/zakuro)
(MIT), with game code recompiled by [3dsrecomp](https://github.com/fearkov/3dsrecomp) (MIT). The feet mod is from the
[Fire Emblem: Awakening recompilation project](https://github.com/Zelkin1/fe-awakening-recomp) (MIT). The on-device
compiler is [LLVM/Clang](https://llvm.org) (Apache-2.0 with LLVM exceptions), as built for Android by
[Termux](https://github.com/termux/termux-packages). HD cutscene import decodes video with [FFmpeg](https://ffmpeg.org)
(LGPL-2.1-or-later, shipped as its own unmodified libraries). All notices, including the libraries the compiler needs, are in
[`android/exalt/THIRD_PARTY_NOTICES.md`](android/exalt/THIRD_PARTY_NOTICES.md) and in the app.

Not affiliated with Nintendo or Intelligent Systems. Fire Emblem is a trademark of Nintendo.
