# ExaltDS third-party notices

ExaltDS contains no game files, firmware or keys. Everything taken from the game (its code, the
feet models, Naga's Tear's scripts and messages) is built on the player's device from the player's
own copy.

| Component | Where | License |
| --- | --- | --- |
| [Zakuro](https://github.com/fearkov/zakuro) | the emulator runtime | MIT |
| [Zakuroishi](https://github.com/Sekyleky/Zakuroishi) | the Android app ExaltDS is built from | MIT |
| [3dsrecomp](https://github.com/fearkov/3dsrecomp) (730fbac) | recompiles the game's code on the device | MIT |
| [Fire Emblem: Awakening recompilation project](https://github.com/Zelkin1/fe-awakening-recomp) 1.0.17 | the feet mod's model builder (`crates/exalt-mods/src/feet`, `src/lz.rs`, unchanged; see `crates/exalt-mods/NOTICE.md`) | MIT |
| [LLVM/Clang and LLD](https://llvm.org) 21.1.8, built for Android by [Termux](https://github.com/termux/termux-packages) | the on-device compiler (`lib/arm64-v8a/libexalt-clang.so`, `libexalt-ld-.so`, `libLLVM.so`, `libclang-cpp.so`, `libc++_shared.so`, clang's headers and builtins in `assets/clang-kit.zip`) | Apache-2.0 with LLVM exceptions (`assets/licenses/LLVM.txt`) |
| Android NDK r30 sysroot (C headers, crt objects, libc/libm/libdl stubs, libunwind) | `assets/clang-kit.zip` | Apache-2.0 and BSD-style (AOSP), Apache-2.0 with LLVM exceptions (libunwind) |
| [libffi](https://github.com/libffi/libffi) 3.8.1 | `libffi.so` | MIT |
| [zlib](https://zlib.net) 1.4 | `libz_1.so` (renamed from libz.so.1) | zlib |
| [Zstandard](https://github.com/facebook/zstd) 1.5.7 | `libzstd_1.so` (renamed from libzstd.so.1) | BSD-3-Clause |
| [libxml2](https://gitlab.gnome.org/GNOME/libxml2) 2.15.4 | `libxml2_16.so` (renamed from libxml2.so.16) | MIT |
| [GNU libiconv](https://www.gnu.org/software/libiconv/) 1.19.1 | `libiconv.so`, `libcharset.so` | LGPL-2.1-or-later |
| [libandroid-glob](https://github.com/termux/termux-packages/tree/master/packages/libandroid-glob) 0.6 | `libandroid-glob.so` | BSD-3-Clause |

The compiler's libraries are Termux's packages (`android/exalt/clang-kit.py` lists them with their
SHA-256), unchanged except that the library names `libz.so.1`, `libzstd.so.1` and `libxml2.so.16`
are rewritten, where files name them, to names of the same length, because Android installs only
`lib*.so` files from an app.

GNU libiconv is under the LGPL: it is shipped as its own shared libraries, unmodified, which can be
replaced. Its source is at <https://ftp.gnu.org/pub/gnu/libiconv/libiconv-1.19.1.tar.gz>, and Termux's
build recipe at <https://github.com/termux/termux-packages/tree/master/packages/libiconv>.

Each Rust crate ExaltDS links keeps its own license; `Cargo.lock` lists them.

Fire Emblem is a trademark of Nintendo. ExaltDS is not affiliated with Nintendo or Intelligent
Systems.
