#!/bin/bash
# ============================================================================
# Rebuilds Qt6Multimedia (+ built-with-us backend plugins) with the local
# MSYS2 MinGW64 toolchain. The official mingw binary imports __emutls_v.*
# symbols that newer libstdc++ removed -> PROC_NOT_FOUND at load time.
# See README.md (Defect 2).
#
# FFmpeg and GStreamer backends are disabled on purpose so only the Windows
# Media Foundation backend is produced; stock ffmpegmediaplugin/gstreamer
# plugins that don't import the removed symbols can stay as deployed.
#
# PREREQ: install vulkan headers first, or compilation fails in
# qvideowindow.cpp (QRhiVulkanInitParams is gated on __has_include):
#   pacman -S --needed mingw-w64-x86_64-vulkan-headers
#
# Usage:
#   ./build-qtmultimedia.sh /path/to/qtmultimedia-everywhere-src-<ver>
#
# Env overrides:
#   QT_BIN_PATH   (default C:/Qt/6.9.3/mingw_64)
#   STAGE_DIR     (default <this dir>/stage)
#
# Output: $STAGE_DIR/qtmultimedia/bin/*.dll
#         $STAGE_DIR/qtmultimedia/plugins/multimedia/*.dll
# ============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
QT_BIN_PATH="${QT_BIN_PATH:-C:/Qt/6.9.3/mingw_64}"
STAGE_DIR="${STAGE_DIR:-${SCRIPT_DIR}/stage}"

SRC="${1:-}"
[ -n "$SRC" ] || { echo "usage: $0 /path/to/qtmultimedia-everywhere-src-<ver>"; exit 1; }
[ -d "$SRC" ] || { echo "FATAL: source not found: $SRC"; exit 1; }

export MSYSTEM=MINGW64
export PATH="/mingw64/bin:${QT_BIN_PATH}/bin:$PATH"

BUILD="$SRC/build-mingw-fixup"

echo "=== configuring qtmultimedia (Ninja, Release) ==="
cmake -S "$SRC" -B "$BUILD" -G Ninja \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_PREFIX_PATH="$QT_BIN_PATH" \
    -DCMAKE_INSTALL_PREFIX="$BUILD/install" \
    -DFEATURE_ffmpeg=off \
    -DFEATURE_gstreamer=off

echo "=== building ==="
ninja -C "$BUILD"

mkdir -p "$STAGE_DIR/qtmultimedia/bin" "$STAGE_DIR/qtmultimedia/plugins/multimedia"

found=0
for d in "$BUILD"/bin/*.dll; do
    [ -e "$d" ] || continue
    cp -f "$d" "$STAGE_DIR/qtmultimedia/bin/"
    echo "  staged bin/$(basename "$d")"
    found=1
done
[ "$found" = 1 ] || { echo "FATAL: no module DLLs found under $BUILD/bin"; exit 1; }

for p in "$BUILD"/plugins/multimedia/*.dll; do
    [ -e "$p" ] || continue
    cp -f "$p" "$STAGE_DIR/qtmultimedia/plugins/multimedia/"
    echo "  staged plugins/multimedia/$(basename "$p")"
done

echo "=== verifying no legacy emutls imports remain ==="
FAIL=0
for d in "$STAGE_DIR"/qtmultimedia/bin/*.dll "$STAGE_DIR"/qtmultimedia/plugins/multimedia/*.dll; do
    [ -e "$d" ] || continue
    if objdump -p "$d" | grep -q '__emutls_v'; then
        echo "FATAL: $d still imports __emutls_v.* symbols"; FAIL=1
    fi
done
[ "$FAIL" = 0 ] || exit 1
echo "OK: no __emutls_v.* imports"

echo "=== DONE OK: $STAGE_DIR/qtmultimedia ==="
