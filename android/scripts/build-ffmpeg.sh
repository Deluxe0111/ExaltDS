#!/usr/bin/env bash
# Builds the FFmpeg libraries ExaltDS's HD movie import decodes with (crates/zakuro-android/ffmpeg/hdmovies.c,
# src/hd_import.rs): a minimal, LGPL-2.1-or-later, shared build for arm64 Android from unmodified FFmpeg source.
# Pass the result to build-windows.ps1 as -FFmpeg <prefix>.
#
#   build-ffmpeg.sh <ffmpeg source dir> <prefix> <ndk> [make]
#
# On Windows: run in Git Bash with GNU make (make 4.4.1 builds with MSVC: build_w32.bat --without-guile), and TMPDIR
# set to a folder configure can run files from. Tested with FFmpeg 7.1.1 (ffmpeg.org, sha256 7339843...).
set -euo pipefail
src=$1 prefix=$2 ndk=$3 make=${4:-make}
bin=$ndk/toolchains/llvm/prebuilt/windows-x86_64/bin
[ -d "$bin" ] || bin=$ndk/toolchains/llvm/prebuilt/linux-x86_64/bin
exe=; [ -f "$bin/clang.exe" ] && exe=.exe
target=aarch64-linux-android30
cd "$src"
# the host compiler is only checked, never run: the target's will do
./configure --prefix="$prefix" --target-os=android --arch=aarch64 --cpu=armv8-a --enable-cross-compile \
  --cc="$bin/clang$exe" --cxx="$bin/clang++$exe" --ld="$bin/clang$exe" --ar="$bin/llvm-ar$exe" --nm="$bin/llvm-nm$exe" \
  --ranlib="$bin/llvm-ranlib$exe" --strip="$bin/llvm-strip$exe" \
  --extra-cflags="--target=$target -O2" --extra-ldflags="--target=$target -Wl,-z,max-page-size=16384" \
  --host-cc="$bin/clang$exe" --host-cflags="--target=$target" --host-ldflags="--target=$target" \
  --disable-everything --disable-programs --disable-doc --disable-network --disable-autodetect --disable-debug \
  --enable-shared --disable-static --enable-pic --disable-swresample --disable-postproc --disable-avdevice \
  --enable-protocol=file --enable-demuxer=moflex,mov,matroska,mpegts,avi,flv,h264,hevc \
  --enable-decoder=mobiclip,h264,hevc,mpeg2video,vp9 --enable-parser=h264,hevc,mpegvideo,vp9 \
  --enable-filter=bwdif,yadif,crop,scale,format,buffer,buffersink,null
"$make" -j"$(nproc 2>/dev/null || echo 8)"
"$make" install
# the license that ships with the libraries (assets/licenses/FFMPEG.txt)
{
  echo "FFmpeg $(cat VERSION 2>/dev/null || git describe 2>/dev/null || echo) - https://ffmpeg.org"
  echo "libavformat, libavcodec, libavfilter, libswscale and libavutil, built unmodified from the FFmpeg source"
  echo "(https://ffmpeg.org/releases/) by android/scripts/build-ffmpeg.sh, configured with:"
  grep -m1 'FFMPEG_CONFIGURATION' config.h | sed 's/#define FFMPEG_CONFIGURATION //'
  echo "They are licensed under the GNU Lesser General Public License, version 2.1 or later, which follows, and are"
  echo "linked dynamically: they can be replaced with other builds of the same FFmpeg version."
  echo
  cat COPYING.LGPLv2.1
} > "$prefix/LICENSE-FFMPEG.txt"
echo "FFmpeg for Android in $prefix"
