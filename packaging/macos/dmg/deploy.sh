#!/bin/bash

set -euo pipefail

ARCH="${1:-x86_64}"
VERSION="${2:-dev}"
BUILD_DIR="build"
APP_PATH="${BUILD_DIR}/iDescriptor.app"


echo "Deploying iDescriptor DMG for ${ARCH} architecture (version: ${VERSION})"

# Determine the platform-specific suffix for the DMG name based on architecture
PLATFORM_SUFFIX=""
if [ "${ARCH}" == "x86_64" ]; then
  PLATFORM_SUFFIX="Apple_Intel"
elif [ "${ARCH}" == "arm64" ]; then
  PLATFORM_SUFFIX="Apple_Silicon"
else
  echo "Error: Unsupported architecture '${ARCH}'."
  exit 1
fi

# Stage the Cargo executable into a native application bundle.
CARGO_EXECUTABLE="target/release/idescriptor"
if [ ! -f "${CARGO_EXECUTABLE}" ]; then
  echo "Error: ${CARGO_EXECUTABLE} not found; run cargo build --release first."
  exit 1
fi

ACTUAL_ARCH="$(file "${CARGO_EXECUTABLE}")"
case "${ARCH}" in
  x86_64) EXPECTED_PATTERN="x86_64" ;;
  arm64) EXPECTED_PATTERN="arm64" ;;
esac
if ! grep -q "${EXPECTED_PATTERN}" <<<"${ACTUAL_ARCH}"; then
  echo "Error: Cargo executable architecture does not match ${ARCH}: ${ACTUAL_ARCH}"
  exit 1
fi

rm -rf "${APP_PATH}"
mkdir -p "${APP_PATH}/Contents/MacOS" "${APP_PATH}/Contents/Resources" "${APP_PATH}/Contents/Frameworks"
cp "${CARGO_EXECUTABLE}" "${APP_PATH}/Contents/MacOS/iDescriptor"
chmod +x "${APP_PATH}/Contents/MacOS/iDescriptor"

# ipatool loads Unicorn dynamically for SAP request signing.
UNICORN_DYLIB="$(brew --prefix unicorn)/lib/libunicorn.2.dylib"
if [ ! -f "${UNICORN_DYLIB}" ]; then
  echo "Error: Unicorn library not found at ${UNICORN_DYLIB}"
  exit 1
fi
cp -L "${UNICORN_DYLIB}" "${APP_PATH}/Contents/Frameworks/libunicorn.2.dylib"
install_name_tool -id "@rpath/libunicorn.2.dylib" "${APP_PATH}/Contents/Frameworks/libunicorn.2.dylib"
cp "packaging/macos/dmg/Info.plist" "${APP_PATH}/Contents/Info.plist"
cp "packaging/shared/resources/app-icon/icon.icns" "${APP_PATH}/Contents/Resources/iDescriptor.icns"
plutil -replace CFBundleShortVersionString -string "${VERSION#v}" "${APP_PATH}/Contents/Info.plist"
plutil -replace CFBundleVersion -string "${VERSION#v}" "${APP_PATH}/Contents/Info.plist"

GST_PLUGIN_DIR="${APP_PATH}/Contents/Frameworks/gstreamer"
mkdir -p "${GST_PLUGIN_DIR}"

PLUGINS=(
  "libgstapp"
  "libgstaudioconvert"
  "libgstaudioresample"
  "libgstautodetect"
  "libgstavi"
  "libgstcoreelements"
  "libgstimagefreeze"
  "libgstjpeg"
  "libgstlevel"
  "libgstlibav"
  "libgstosxaudio"
  "libgstplayback"
  "libgstvideobox"
  "libgstvideofilter"
  "libgstvideoparsersbad"
  "libgstvolume"
  "libgstvideoconvertscale"
  "libgstvideorate"
  # GL plugins
  "libgstopengl"
  "libgstqml6"
)

BREW_PREFIX="$(brew --prefix)"

# Copy GStreamer plugins
for plugin in "${PLUGINS[@]}"; do
  cp "${BREW_PREFIX}/lib/gstreamer-1.0/${plugin}.dylib" "${GST_PLUGIN_DIR}/"
done

# Copy gst-plugin-scanner
cp "$(brew --prefix gstreamer)/libexec/gstreamer-1.0/gst-plugin-scanner" "${APP_PATH}/Contents/Frameworks/"

# Bundle libjxl_cms.
# For some reason libjxl_cms is not bundled by macdeployqt, so we do it manually.
LIBJXL_CMS_PATH="$(find "${BREW_PREFIX}/lib" -name 'libjxl_cms.*.dylib' -print -quit)"
if [ -z "${LIBJXL_CMS_PATH}" ]; then
  echo "Error: libjxl_cms was not found under ${BREW_PREFIX}/lib"
  exit 1
fi
LIBJXL_CMS_NAME="$(basename "${LIBJXL_CMS_PATH}")"
cp "${LIBJXL_CMS_PATH}" "${APP_PATH}/Contents/Frameworks/"
install_name_tool -id "@rpath/${LIBJXL_CMS_NAME}" "${APP_PATH}/Contents/Frameworks/${LIBJXL_CMS_NAME}"

# Add RPATH to main executable
install_name_tool -add_rpath "@executable_path/../Frameworks" "${APP_PATH}/Contents/MacOS/iDescriptor"

# Bundle libsqlite3 - if not done macOS tries to load sqlite from system libs
cp "$(brew --prefix sqlite)/lib/libsqlite3.dylib" "${APP_PATH}/Contents/Frameworks/"
install_name_tool -change /usr/lib/libsqlite3.dylib @rpath/libsqlite3.dylib "${APP_PATH}/Contents/MacOS/iDescriptor"

# Copy GStreamer + GLib core libraries
GST_LIBS=(
  "libgstreamer-1.0.0.dylib"
  "libgstbase-1.0.0.dylib"
  "libgstaudio-1.0.0.dylib"
  "libgstvideo-1.0.0.dylib"
  "libgstapp-1.0.0.dylib"
  "libgstpbutils-1.0.0.dylib"
  "libgsttag-1.0.0.dylib"
  "libgstriff-1.0.0.dylib"
  "libgstcodecparsers-1.0.0.dylib"
  "libgstcodecs-1.0.0.dylib"
  "libgstrtp-1.0.0.dylib"
  "libgstsdp-1.0.0.dylib"
  "libglib-2.0.0.dylib"
  "libgobject-2.0.0.dylib"
  "libgmodule-2.0.0.dylib"
  "libgio-2.0.0.dylib"
  "libgthread-2.0.0.dylib"
  "libgstgl-1.0.0.dylib"
)

FRAMEWORKS_DIR="${APP_PATH}/Contents/Frameworks"

for lib in "${GST_LIBS[@]}"; do
  if [ -f "${BREW_PREFIX}/lib/${lib}" ]; then
    cp "${BREW_PREFIX}/lib/${lib}" "${FRAMEWORKS_DIR}/"
    install_name_tool -id "@rpath/${lib}" "${FRAMEWORKS_DIR}/${lib}"
    echo "Fixed rpath for ${lib}"
  fi
done

# Copy FFmpeg libraries
FFMPEG_LIB_DIR="$(brew --prefix ffmpeg)/lib"
FFMPEG_LIBS=(
  "libavformat"
  "libavcodec"
  "libavutil"
  "libswscale"
  "libavfilter"
)

for lib_base in "${FFMPEG_LIBS[@]}"; do
  # Use find to get the full versioned filename
  lib_path=$(find "${FFMPEG_LIB_DIR}" -name "${lib_base}.*.dylib" -print -quit)
  if [ -f "$lib_path" ]; then
    lib_name=$(basename "$lib_path")
    cp "$lib_path" "${FRAMEWORKS_DIR}/"
    #These maybe unneeded, macdeployqt already does this but just in case
    install_name_tool -id "@rpath/${lib_name}" "${FRAMEWORKS_DIR}/${lib_name}"
    echo "Fixed rpath for ${lib_name}"
  else
    echo "Warning: ${lib_base} library not found in ${FFMPEG_LIB_DIR}"
  fi
done

macdeployqt "${APP_PATH}" -qmldir=src/ui -verbose=2

if [ -n "${MACOS_SIGNING_IDENTITY:-}" ]; then
  codesign --force --deep --options runtime --timestamp --sign "${MACOS_SIGNING_IDENTITY}" "${APP_PATH}"
else
  codesign --force --deep --sign - "${APP_PATH}"
fi

DMG_NAME="iDescriptor-${VERSION}-${PLATFORM_SUFFIX}.dmg"
rm -f "${BUILD_DIR}/${DMG_NAME}"

create-dmg \
  --volname "iDescriptor" \
  --volicon "packaging/shared/resources/app-icon/icon.icns" \
  --window-pos 200 120 \
  --window-size 600 400 \
  --icon-size 100 \
  --icon "iDescriptor.app" 175 190 \
  --hide-extension "iDescriptor.app" \
  --app-drop-link 425 190 \
  "${BUILD_DIR}/${DMG_NAME}" \
  "${APP_PATH}"

if [ -n "${MACOS_SIGNING_IDENTITY:-}" ]; then
  codesign --force --timestamp --sign "${MACOS_SIGNING_IDENTITY}" "${BUILD_DIR}/${DMG_NAME}"
fi
