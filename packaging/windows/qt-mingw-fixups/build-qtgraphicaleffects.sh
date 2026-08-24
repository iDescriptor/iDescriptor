#!/bin/bash
# ============================================================================
# Rebuilds Qt5Compat's GraphicalEffects QML plugin (DropShadow et al.) with
# the local MSYS2 MinGW64 toolchain and injects local definitions for the two
# adjustor thunks the official private plugin wrongly imports from
# Qt6Quick.dll. See README.md (Defect 1) in this directory.
#
# Usage:
#   ./build-qtgraphicaleffects.sh /path/to/qt5compat-everywhere-src-<ver>
#
# Env overrides:
#   QT_BIN_PATH   (default C:/Qt/6.9.3/mingw_64)
#   STAGE_DIR     (default <this dir>/stage)
#
# Output: $STAGE_DIR/qtgraphicaleffects/{qtgraphicaleffectsplugin.dll,
#         private/qtgraphicaleffectsprivateplugin.dll}
# ============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
QT_BIN_PATH="${QT_BIN_PATH:-C:/Qt/6.9.3/mingw_64}"
STAGE_DIR="${STAGE_DIR:-${SCRIPT_DIR}/stage}"
SHIM_SRC="${SCRIPT_DIR}/qt5compat-shim/qtgraphicaleffects-thunks.cpp"

SRC="${1:-}"
[ -n "$SRC" ] || { echo "usage: $0 /path/to/qt5compat-everywhere-src-<ver>"; exit 1; }
[ -d "$SRC" ] || { echo "FATAL: source not found: $SRC"; exit 1; }
[ -f "$SHIM_SRC" ] || { echo "FATAL: shim missing: $SHIM_SRC"; exit 1; }

export MSYSTEM=MINGW64
export PATH="/mingw64/bin:${QT_BIN_PATH}/bin:$PATH"

BUILD="$SRC/build-mingw-fixup"
PRIVATE_DIR="$SRC/src/imports/graphicaleffects5/private"

echo "=== injecting thunk shim (idempotent) ==="
cp -f "$SHIM_SRC" "$PRIVATE_DIR/qtgraphicaleffects-thunks.cpp"
if ! grep -q 'qtgraphicaleffects-thunks.cpp' "$PRIVATE_DIR/CMakeLists.txt"; then
    {
        echo ""
        echo "# Windows mingw fixup (see packaging/windows/qt-mingw-fixups/README.md)"
        echo "target_sources(qtgraphicaleffectsprivate PRIVATE qtgraphicaleffects-thunks.cpp)"
    } >> "$PRIVATE_DIR/CMakeLists.txt"
    echo "  patched private/CMakeLists.txt"
else
    echo "  already patched"
fi

echo "=== configuring qt5compat (Ninja, Release) ==="
cmake -S "$SRC" -B "$BUILD" -G Ninja \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_PREFIX_PATH="$QT_BIN_PATH" \
    -DCMAKE_INSTALL_PREFIX="$BUILD/install"

echo "=== building ==="
ninja -C "$BUILD"

PUB="$(find "$BUILD" -name 'qtgraphicaleffectsplugin.dll' -type f | head -1)"
PRIV="$(find "$BUILD" -path '*private*' -name 'qtgraphicaleffectsprivateplugin.dll' -type f | head -1)"
[ -n "$PUB" ]  || { echo "FATAL: public plugin dll not found"; exit 1; }
[ -n "$PRIV" ] || { echo "FATAL: private plugin dll not found"; exit 1; }

mkdir -p "$STAGE_DIR/qtgraphicaleffects/private"
cp -f "$PUB"  "$STAGE_DIR/qtgraphicaleffects/qtgraphicaleffectsplugin.dll"
cp -f "$PRIV" "$STAGE_DIR/qtgraphicaleffects/private/qtgraphicaleffectsprivateplugin.dll"

echo "=== verifying override plugin no longer imports the phantom thunks ==="
if objdump -p "$STAGE_DIR/qtgraphicaleffects/private/qtgraphicaleffectsprivateplugin.dll" | grep -q '_ZThn16_N10QQuickItem'; then
    echo "FATAL: rebuilt plugin STILL imports _ZThn16 thunks - shim did not link"
    exit 1
fi
echo "OK: no _ZThn16 QQuickItem imports"

echo "=== DONE OK: $STAGE_DIR/qtgraphicaleffects ==="
