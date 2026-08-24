#!/bin/bash
# ============================================================================
# Rebuilds Qt6ShaderTools.dll with the local MSYS2 MinGW64 toolchain.
# The official mingw binary imports __emutls_v.* symbols that newer
# libstdc++ removed -> PROC_NOT_FOUND at load time. See README.md (Defect 2).
#
# Usage:
#   ./build-qtshadertools.sh /path/to/qtshadertools-everywhere-src-<ver>
#
# Env overrides:
#   QT_BIN_PATH   (default C:/Qt/6.9.3/mingw_64)
#   STAGE_DIR     (default <this dir>/stage)
#
# Output: $STAGE_DIR/qtshadertools/bin/Qt6ShaderTools.dll
# ============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
QT_BIN_PATH="${QT_BIN_PATH:-C:/Qt/6.9.3/mingw_64}"
STAGE_DIR="${STAGE_DIR:-${SCRIPT_DIR}/stage}"

SRC="${1:-}"
[ -n "$SRC" ] || { echo "usage: $0 /path/to/qtshadertools-everywhere-src-<ver>"; exit 1; }
[ -d "$SRC" ] || { echo "FATAL: source not found: $SRC"; exit 1; }

export MSYSTEM=MINGW64
export PATH="/mingw64/bin:${QT_BIN_PATH}/bin:$PATH"

BUILD="$SRC/build-mingw-fixup"

echo "=== configuring qtshadertools (Ninja, Release) ==="
cmake -S "$SRC" -B "$BUILD" -G Ninja \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_PREFIX_PATH="$QT_BIN_PATH" \
    -DCMAKE_INSTALL_PREFIX="$BUILD/install"

echo "=== building ==="
ninja -C "$BUILD" qtshadertools

DLL="$(find "$BUILD" -name 'Qt6ShaderTools.dll' -type f | head -1)"
[ -n "$DLL" ] || { echo "FATAL: Qt6ShaderTools.dll not found in build"; exit 1; }

mkdir -p "$STAGE_DIR/qtshadertools/bin"
cp -f "$DLL" "$STAGE_DIR/qtshadertools/bin/Qt6ShaderTools.dll"

echo "=== verifying no legacy emutls imports remain ==="
if objdump -p "$STAGE_DIR/qtshadertools/bin/Qt6ShaderTools.dll" | grep -q '__emutls_v'; then
    echo "FATAL: rebuilt Qt6ShaderTools.dll still imports __emutls_v.* symbols"
    exit 1
fi
echo "OK: no __emutls_v.* imports"

echo "=== DONE OK: $STAGE_DIR/qtshadertools ==="
