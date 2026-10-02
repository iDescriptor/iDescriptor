#!/bin/bash
set -euo pipefail


#Example
#mkdir -p "target/deploy"
#cd target/deploy
#cp ../release/idescriptor.exe ./
#bash ../../packaging/windows/portable/deploy.sh --executable="./idescriptor.exe" --qt-bin-path="C:\Qt\6.9.3\mingw_64\bin" --project-source-dir="../../" --qml-source-dir="/c/Users/uncore/Desktop/iDescriptor/src/ui"


# Parse arguments
EXECUTABLE_PATH=""
OUTPUT_DIR="."
QT_BIN_PATH=""
MSYS2_BIN_PATH="/c/msys64/mingw64/bin"  # default
QML_SOURCE_DIR=""
PROJECT_SOURCE_DIR=""
ARCHITECTURE="x64"

for arg in "$@"; do
    case $arg in
        --executable=*) EXECUTABLE_PATH="${arg#*=}" ;;
        --output-dir=*) OUTPUT_DIR="${arg#*=}" ;;
        --qt-bin-path=*) QT_BIN_PATH="${arg#*=}" ;;
        --msys2-bin-path=*) MSYS2_BIN_PATH="${arg#*=}" ;;
        --qml-source-dir=*) QML_SOURCE_DIR="${arg#*=}" ;;
        --project-source-dir=*) PROJECT_SOURCE_DIR="${arg#*=}" ;;
        --architecture=*) ARCHITECTURE="${arg#*=}" ;;
        *) echo "Unknown argument: $arg"; exit 1 ;;
    esac
done

if [ "${ARCHITECTURE}" != "x64" ] && [ "${ARCHITECTURE}" != "arm64" ]; then
    echo "Error: --architecture must be x64 or arm64"
    exit 1
fi

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

ADDITIONAL_DLLS=(
    # TODO
    # "libunicorn.dll"
    "libwinpthread-1.dll"
    "libgstreamer-1.0-0.dll"
    "libgstbase-1.0-0.dll"
    "libgstcodecparsers-1.0-0.dll"
    "libgstcodecs-1.0-0.dll"
    "libgobject-2.0-0.dll"
    "libglib-2.0-0.dll"
    "libintl-8.dll"
    "libiconv-2.dll"
    "libfdk-aac-2.dll"
    "libfaad-2.dll"
    "avcodec-61.dll"
    "avformat-61.dll"
    "avutil-59.dll"
    "swresample-5.dll"
    "swscale-8.dll"
    # "avfilter-11.dll"
    "avfilter-10.dll"
    "libopenal-1.dll"
    "libgstaudio-1.0-0.dll"
    "libgstvideo-1.0-0.dll"
    "liborc-0.4-0.dll"
    "libgstpbutils-1.0-0.dll"
    "libgsttag-1.0-0.dll"
    # "libgstlibav.dll"
    "libass-9.dll"
    "libfontconfig-1.dll"
    "libharfbuzz-0.dll"
    "libexpat-1.dll"
    "libfreetype-6.dll"
    "libpng16-16.dll"
    "libgraphite2.dll"
    "libfribidi-0.dll"
    "libunibreak-6.dll"
    "liblcms2-2.dll"
    "libvpl-2.dll"
    "libzimg-2.dll"
    "libdovi.dll"
    "libshaderc_shared.dll"
    "vulkan-1.dll"
    "libvidstab.dll"
    "postproc-58.dll"
    "libplacebo-351.dll"
    "libspirv-cross-c-shared.dll"
    "libva.dll"
    # "libxml2-16.dll"
    "libva_win32.dll"
    "libpcre2-8-0.dll"
    "libffi-8.dll"
    "libgmodule-2.0-0.dll"
    "libhwy.dll"
    "libmp3lame-0.dll"
    "librsvg-2-2.dll"
    "libwebp-7.dll"
    "libthai-0.dll"
    "libjxl.dll"
    "libdatrie-1.dll"
    "libwebpmux-3.dll"
    "libx264-164.dll"
    "libtasn1-6.dll"
    "libgsm.dll"
    "libcairo-gobject-2.dll"
    "libvorbis-0.dll"
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
    "libopencore-amrwb-0.dll"
    "libtheoradec-2.dll"
    "libvpx-1.dll"
    "libgme.dll"
    "libhogweed-6.dll"
    "liblc3-1.dll"
    "libpango-1.0-0.dll"
    "xvidcore.dll"
    "libopencore-amrnb-0.dll"
    "libtiff-6.dll"
    "libxml2-2.dll"
    "libjbig-0.dll"
    "libLerc.dll"
    "libjxl_cms.dll"
    "libgdk_pixbuf-2.0-0.dll"
    "libvorbisenc-2.dll"
    "libsoxr.dll"
    "librtmp-1.dll"
    "libcairo-2.dll"
    "libdeflate.dll"
    "libpangocairo-1.0-0.dll"
    "libpangoft2-1.0-0.dll"
    "libtheoraenc-2.dll"
    "libbluray-2.dll"
    "libnettle-8.dll"
    "libunistring-5.dll"
    "libidn2-0.dll"
    "libssh.dll"
    "libdav1d-7.dll"
    "liblzma-5.dll"
    "libopenjp2-7.dll"
    "libzstd.dll"
    "libSvtAv1Enc-3.dll"
    "libbrotlicommon.dll"
    "libjpeg-8.dll"
    "libb2-1.dll"
    "libarchive-13.dll"
    "libheif.dll"
    "libopenh264-7.dll"
    "libopenjph-0.21.dll"
    "zlib1.dll"
    "libbrotlienc.dll"
    "libkvazaar-7.dll"
    "liblz4.dll"
    "librav1e.dll"
    "libaom.dll"
    "libbrotlidec.dll"
    "libsharpyuv-0.dll"
    "libx265-215.dll"
    "libcryptopp.dll"
    "libde265-0.dll"
    "libbz2-1.dll"
    #gl plugins dependencies
    "libgstapp-1.0-0.dll"
    "libgstgl-1.0-0.dll"
    "libgstcontroller-1.0-0.dll"
    "libgraphene-1.0-0.dll"
)

if [ "${ARCHITECTURE}" = "arm64" ]; then
    ADDITIONAL_DLLS+=(
        "libc++.dll"
        "libunwind.dll"
        "libcrypto-3-arm64.dll"
        # libssl for openssl (idevice crate uses the system openssl)
        "libssl-3-arm64.dll"
    )
else
    ADDITIONAL_DLLS+=(
        "libgcc_s_seh-1.dll"
        "libstdc++-6.dll"
        "libgomp-1.dll"
        "libcrypto-3-x64.dll"
        # libssl for openssl (idevice crate uses the system openssl)
        "libssl-3-x64.dll"
    )
fi

echo "Copying additional MinGW runtime DLLs from MSYS2..."
for DLL_NAME in "${ADDITIONAL_DLLS[@]}"; do
    DLL_PATH="${MSYS2_BIN_PATH}/${DLL_NAME}"
    if [ -f "${DLL_PATH}" ]; then
        echo "Copying additional DLL: ${DLL_NAME}"
        cp "${DLL_PATH}" "${OUTPUT_DIR}/"
    else
        echo "Error: Additional DLL not found: ${DLL_NAME} (searched ${MSYS2_BIN_PATH})"
        exit 1
    fi
done

echo "Copying GStreamer helper executables..."
GST_LIBEXEC_PATH="${MSYS2_BIN_PATH}/../libexec/gstreamer-1.0"
mkdir -p "${OUTPUT_DIR}/gstreamer-1.0/libexec"
cp "${GST_LIBEXEC_PATH}/gst-plugin-scanner.exe" "${OUTPUT_DIR}/gstreamer-1.0/libexec/"


echo "Copying required scripts"
# TODO: Bonjour is not required anymore, consider removing
cp "${PROJECT_SOURCE_DIR}/install-bonjour.ps1" "${OUTPUT_DIR}/"
cp "${PROJECT_SOURCE_DIR}/install-apple-drivers.ps1" "${OUTPUT_DIR}/"
cp "${PROJECT_SOURCE_DIR}/install-win-fsp.silent.bat" "${OUTPUT_DIR}/"

if [ "${ARCHITECTURE}" = "arm64" ]; then
    WINFSP_DLL="winfsp-a64.dll"
else
    WINFSP_DLL="winfsp-x64.dll"
fi
echo "Copying ${WINFSP_DLL}"
cp "/c/Program Files (x86)/WinFsp/bin/${WINFSP_DLL}" "${OUTPUT_DIR}/"

echo "=== Windows deployment completed ==="
