#!/bin/bash
set -euo pipefail


#Example
#mkdir -p "target/deploy"
#cd target/deploy
#cp ../release/idescriptor.exe ./
#bash ../../packaging/windows/portable/deploy.sh --executable="./idescriptor.exe" --qt-bin-path="C:\Qt\6.9.3\mingw_64\bin" --project-source-dir="../../" --qml-source-dir="/c/Users/uncore/Desktop/iDescriptor/src/ui"
#
#Optional:
#  --strict-dlls                 abort if any DLL pattern matches nothing (default: warn)
#  --qt-fixups-dir=<dir>         overlay locally rebuilt Qt modules (see ../qt-mingw-fixups/README.md)


# Parse arguments
EXECUTABLE_PATH=""
OUTPUT_DIR="."
QT_BIN_PATH=""
MSYS2_BIN_PATH="/c/msys64/mingw64/bin"  # default
QML_SOURCE_DIR=""
PROJECT_SOURCE_DIR=""
QT_FIXUPS_DIR=""      # optional: overlay locally rebuilt Qt module DLLs (see ../qt-mingw-fixups/README.md)
STRICT_DLLS=0         # 1 = abort when a pattern matches nothing (old behaviour)

for arg in "$@"; do
    case $arg in
        --executable=*) EXECUTABLE_PATH="${arg#*=}" ;;
        --output-dir=*) OUTPUT_DIR="${arg#*=}" ;;
        --qt-bin-path=*) QT_BIN_PATH="${arg#*=}" ;;
        --msys2-bin-path=*) MSYS2_BIN_PATH="${arg#*=}" ;;
        --qml-source-dir=*) QML_SOURCE_DIR="${arg#*=}" ;;
        --project-source-dir=*) PROJECT_SOURCE_DIR="${arg#*=}" ;;
        --qt-fixups-dir=*) QT_FIXUPS_DIR="${arg#*=}" ;;
        --strict-dlls) STRICT_DLLS=1 ;;
        *) echo "Unknown argument: $arg"; exit 1 ;;
    esac
done

# Validate required args
for var_name in EXECUTABLE_PATH OUTPUT_DIR QT_BIN_PATH QML_SOURCE_DIR PROJECT_SOURCE_DIR; do
    if [ -z "${!var_name}" ]; then
        echo "Error: --$(echo $var_name | tr '[:upper:]' '_' | tr '_' '-' | tr '[:upper:]' '[:lower:]') is required"
        exit 1
    fi
done

echo "=== Starting Windows deployment for: ${EXECUTABLE_PATH} ==="
echo "Debug info:"
echo "  EXECUTABLE_PATH: ${EXECUTABLE_PATH}"
echo "  OUTPUT_DIR: ${OUTPUT_DIR}"
echo "  QT_BIN_PATH: ${QT_BIN_PATH}"
echo "  MSYS2_BIN_PATH: ${MSYS2_BIN_PATH}"

if [ ! -f "${EXECUTABLE_PATH}" ]; then
    echo "Error: Executable not found: ${EXECUTABLE_PATH}"
    exit 1
fi

echo "SUCCESS: Executable found at: ${EXECUTABLE_PATH}"

echo "Running windeployqt6 to deploy Qt dependencies (without compiler runtime)..."

echo "Executing: ${QT_BIN_PATH}/windeployqt6.exe --qmldir ${QML_SOURCE_DIR} --dir ${OUTPUT_DIR} --plugindir ${OUTPUT_DIR}/plugins ${EXECUTABLE_PATH}"
"${QT_BIN_PATH}/windeployqt6.exe" \
    --qmldir "${QML_SOURCE_DIR}" \
    --dir "${OUTPUT_DIR}" \
    --plugindir "${OUTPUT_DIR}/plugins" \
    "${EXECUTABLE_PATH}"

echo "windeployqt6 completed successfully"

echo "Copying GStreamer plugins..."
GSTREAMER_PLUGIN_DIR="${MSYS2_BIN_PATH}/../lib/gstreamer-1.0"

WANTED_PLUGINS=(
    "libgstaudioconvert"
    "libgstvolume"
    "libgstcoreelements"
    "libgstautodetect"
    "libgstdirectsound"
    "libgstlibav"
    "libgstapp"
    "libgstlevel"
    "libgstwasapi"
    "libgstplayback"
    "libgstaudioresample"
    "libgstaudiomixer"
    "libgstaudiotestsrc"
    # "libgstmediafoundation"
    # "libgstdecodebin"
    "libgsttypefindfunctions"
    # "libgstvideoscale"
    "libgstvideoconvert"
    "libgstvideorate"
    "libgstoverlaycomposition"
    "libgstfaad"
    "libgstvideoparsersbad"
    "libgstvideofilter"
    "libgstvideoconvertscale"
    "libgstmultifile"
    "libgstjpeg"
    # GL plugin
    "libgstqml6"
    "libgstopengl"
)

mkdir -p "${OUTPUT_DIR}/gstreamer-1.0"
COPIED_PLUGIN_COUNT=0
for BASENAME in "${WANTED_PLUGINS[@]}"; do
    # match any versioned filename starting with the basename
    MATCHES=("${GSTREAMER_PLUGIN_DIR}/${BASENAME}"*.dll)
    if [ -e "${MATCHES[0]}" ]; then
        for PLUGIN_PATH in "${MATCHES[@]}"; do
            PLUGIN_NAME=$(basename "${PLUGIN_PATH}")
            echo "Copying GStreamer plugin: ${PLUGIN_NAME}"
            cp "${PLUGIN_PATH}" "${OUTPUT_DIR}/gstreamer-1.0/"
            COPIED_PLUGIN_COUNT=$((COPIED_PLUGIN_COUNT + 1))
        done
    else
        echo "Error: Requested GStreamer plugin not found: ${BASENAME} (searched ${GSTREAMER_PLUGIN_DIR})"
        exit 1
    fi
done

echo "Successfully copied ${COPIED_PLUGIN_COUNT} requested GStreamer plugins"

# NOTE: entries may contain globs. MSYS2 bumps DLL sonames on routine updates
# (avcodec-61 -> avcodec-63 etc.), so anything version-suffixed should be a
# pattern like "avcodec-*.dll" instead of a hard pin that silently stops
# matching after an update.
ADDITIONAL_DLLS=(
    "libgcc_s_seh-1.dll"
    "libstdc++-6.dll"
    "libwinpthread-1.dll"
    "libgstreamer-1.0-0.dll"
    "libgstbase-1.0-0.dll"
    "libgstcodecparsers-1.0-0.dll"
    "libgstcodecs-1.0-0.dll"
    "libgobject-2.0-*.dll"
    "libglib-2.0-*.dll"
    "libintl-8.dll"
    "libiconv-2.dll"
    "libfdk-aac-2.dll"
    "libfaad-2.dll"
    "avcodec-*.dll"
    "avformat-*.dll"
    "avutil-*.dll"
    "swresample-*.dll"
    "swscale-*.dll"
    # "avfilter-11.dll"
    "avfilter-*.dll"
    "libopenal-1.dll"
    "libgstaudio-1.0-0.dll"
    "libgstvideo-1.0-0.dll"
    "liborc-0.4-0.dll"
    "libgstpbutils-1.0-0.dll"
    "libgsttag-1.0-0.dll"
    # "libgstlibav.dll"
    "libass-*.dll"
    "libfontconfig-1.dll"
    "libharfbuzz-0.dll"
    "libexpat-1.dll"
    "libfreetype-6.dll"
    "libpng16-16.dll"
    "libgraphite2.dll"
    "libfribidi-0.dll"
    "libunibreak-*.dll"
    "liblcms2-2.dll"
    "libvpl-2.dll"
    "libzimg-2.dll"
    "libdovi.dll"
    "libshaderc_shared.dll"
    "vulkan-1.dll"
    "libvidstab.dll"
    "libgomp-1.dll"
    "postproc-*.dll"
    "libplacebo-*.dll"
    "libspirv-cross-c-shared.dll"
    "libva.dll"
    "libva_win32.dll"
    "libpcre2-8-0.dll"
    "libffi-8.dll"
    "libgmodule-2.0-0.dll"
    "libhwy.dll"
    "libmp3lame-0.dll"
    "librsvg-2-2.dll"
    "libwebp-*.dll"
    "libthai-0.dll"
    "libjxl.dll"
    "libdatrie-1.dll"
    "libwebpmux-*.dll"
    "libx264-*.dll"
    "libtasn1-6.dll"
    "libgsm.dll"
    "libcairo-gobject-2.dll"
    "libvorbis-0.dll"
    # libvorbisenc is NOT matched by libvorbis-* (different basename!)
    "libvorbisenc-*.dll"
    "libgio-2.0-0.dll"
    "libgmp-10.dll"
    "libmodplug-1.dll"
    "libopus-0.dll"
    "libpangowin32-1.0-0.dll"
    "libspeex-1.dll"
    "libogg-0.dll"
    "libzvbi-0.dll"
    "libpixman-1-0.dll"
    "libsrt.dll"
    "libjxl_threads.dll"
    "libgnutls-30.dll"
    "libp11-kit-0.dll"
    "libopencore-amrwb-*.dll"
    "libtheoradec-*.dll"
    "libvpx-1.dll"
    "libgme.dll"
    "libhogweed-*.dll"
    "liblc3-*.dll"
    "libpango-1.0-0.dll"
    "xvidcore.dll"
    "libopencore-amrnb-*.dll"
    "libtiff-6.dll"
    # mingw64 currently ships libxml2-16.dll; older pins (libxml2-2) went stale
    "libxml2-*.dll"
    "libjbig-0.dll"
    "libLerc.dll"
    "libjxl_cms.dll"
    "libgdk_pixbuf-2.0-0.dll"
    "libsoxr.dll"
    "librtmp-1.dll"
    "libcairo-2.dll"
    "libdeflate.dll"
    "libpangocairo-1.0-0.dll"
    "libpangoft2-1.0-0.dll"
    "libtheoraenc-*.dll"
    "libbluray-*.dll"
    "libnettle-*.dll"
    "libunistring-*.dll"
    "libidn2-*.dll"
    "libssh.dll"
    "libdav1d-*.dll"
    "liblzma-5.dll"
    # openjpeg was renamed to openjph in newer MSYS2; match either
    "libopenjp2-*.dll"
    "libopenjph-*.dll"
    "libzstd.dll"
    "libSvtAv1Enc-*.dll"
    "libbrotlicommon.dll"
    "libjpeg-8.dll"
    "libb2-1.dll"
    "libarchive-13.dll"
    "libheif.dll"
    "libopenh264-*.dll"
    "libcrypto-3-x64.dll"
    "zlib1.dll"
    "libbrotlienc.dll"
    "libkvazaar-*.dll"
    "liblz4.dll"
    "librav1e.dll"
    "libaom.dll"
    "libbrotlidec.dll"
    "libsharpyuv-*.dll"
    "libx265-*.dll"
    "libcryptopp.dll"
    "libde265-0.dll"
    "libbz2-1.dll"
    # libplist for uxplay
    "libplist-2.0.dll"
    # libssl for openssl (idevice crate uses the system openssl)
    "libssl-3-x64.dll"
    #gl plugins dependencies
    "libgstapp-1.0-0.dll"
    "libgstgl-1.0-0.dll"
    "libgstcontroller-1.0-0.dll"
    "libgraphene-1.0-0.dll"
)

echo "Copying additional MinGW runtime DLLs from MSYS2..."
shopt -s nullglob
UNMATCHED_PATTERNS=0
for DLL_PATTERN in "${ADDITIONAL_DLLS[@]}"; do
    MATCHES=("${MSYS2_BIN_PATH}/${DLL_PATTERN}")
    if [ ${#MATCHES[@]} -gt 0 ]; then
        for MATCHED_PATH in "${MATCHES[@]}"; do
            echo "Copying additional DLL: $(basename "${MATCHED_PATH}")"
            cp "${MATCHED_PATH}" "${OUTPUT_DIR}/"
        done
    else
        UNMATCHED_PATTERNS=$((UNMATCHED_PATTERNS + 1))
        echo "WARN: no DLL matched pattern '${DLL_PATTERN}' (searched ${MSYS2_BIN_PATH})"
        if [ "${STRICT_DLLS}" = 1 ]; then
            echo "Error: strict mode enabled (--strict-dlls), aborting."
            exit 1
        fi
    fi
done
if [ "${UNMATCHED_PATTERNS}" -gt 0 ]; then
    echo "NOTE: ${UNMATCHED_PATTERNS} pattern(s) matched nothing - usually just an MSYS2 package rename; check the WARNs above."
fi

echo "Copying GStreamer helper executables..."
GST_LIBEXEC_PATH="${MSYS2_BIN_PATH}/../libexec/gstreamer-1.0"
mkdir -p "${OUTPUT_DIR}/gstreamer-1.0/libexec"
cp "${GST_LIBEXEC_PATH}/gst-plugin-scanner.exe" "${OUTPUT_DIR}/gstreamer-1.0/libexec/"

echo "Copying executables"
# cp "${MSYS2_BIN_PATH}/iproxy.exe" "${OUTPUT_DIR}/"

echo "Copying required scripts"
cp "${PROJECT_SOURCE_DIR}/install-bonjour.ps1" "${OUTPUT_DIR}/"
cp "${PROJECT_SOURCE_DIR}/install-apple-drivers.ps1" "${OUTPUT_DIR}/"
cp "${PROJECT_SOURCE_DIR}/install-win-fsp.silent.bat" "${OUTPUT_DIR}/"

echo "Copying winfsp-x64.dll"
cp "/c/Program Files (x86)/WinFsp/bin/winfsp-x64.dll" "${OUTPUT_DIR}/"

# ---------------------------------------------------------------------------
# Optional: overlay locally rebuilt Qt module DLLs (--qt-fixups-dir=<dir>).
#
# The official Qt mingw binaries are built with an older GCC than current
# MSYS2 ships. Two launch-breaking mismatches have been observed with
# Qt 6.9.3 mingw_64 + MSYS2 MinGW64 (root causes + rebuild instructions in
# ../qt-mingw-fixups/README.md):
#   1) qml/Qt5Compat/GraphicalEffects/private/qtgraphicaleffectsprivateplugin.dll
#      imports adjustor thunks (_ZThn16_N10QQuickItem*) that its own
#      Qt6Quick.dll does not export;
#   2) Qt6ShaderTools.dll / Qt6Multimedia.dll import __emutls_v.* symbols
#      that newer libstdc++-6.dll removed; they fail with PROC_NOT_FOUND,
#      taking every plugin that links them down with them.
# Layout expected under the fixups dir:
#   <dir>/<module>/bin/*.dll                     -> ${OUTPUT_DIR}/
#   <dir>/qtmultimedia/plugins/multimedia/*.dll  -> ${OUTPUT_DIR}/plugins/multimedia/
# ---------------------------------------------------------------------------
if [ -n "${QT_FIXUPS_DIR}" ]; then
    if [ ! -d "${QT_FIXUPS_DIR}" ]; then
        echo "WARN: --qt-fixups-dir given but directory not found: ${QT_FIXUPS_DIR}"
    else
        shopt -s nullglob
        for MOD_DIR in "${QT_FIXUPS_DIR}"/*/; do
            for FIXUP_DLL in "${MOD_DIR}"bin/*.dll; do
                cp -f "${FIXUP_DLL}" "${OUTPUT_DIR}/"
                echo "Applied qt-fixup: $(basename "${FIXUP_DLL}")"
            done
            for FIXUP_PLUGIN in "${MOD_DIR}"plugins/multimedia/*.dll; do
                mkdir -p "${OUTPUT_DIR}/plugins/multimedia"
                cp -f "${FIXUP_PLUGIN}" "${OUTPUT_DIR}/plugins/multimedia/"
                echo "Applied qt-fixup: plugins/multimedia/$(basename "${FIXUP_PLUGIN}")"
            done
        done
    fi
fi

echo "=== Windows deployment completed ==="
