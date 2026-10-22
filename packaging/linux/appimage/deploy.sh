#!/bin/bash
# if you get errors try
# QMAKE=/usr/lib/qt6/bin/qmake NO_STRIP=1 ./packaging/linux/appimage/deploy.sh v1.0.0
# or even more explicit
#export QT_HOME=~/Qt/$YOUR_QT_VERSION/gcc_64
#export PATH="$QT_HOME/bin:$PATH"
#export LD_LIBRARY_PATH="$QT_HOME/lib"
#export QML2_IMPORT_PATH="$QT_HOME/qml"
#export QT_PLUGIN_PATH="$QT_HOME/plugins"
#QMAKE="$QT_HOME/bin/qmake6" ./packaging/linux/appimage/deploy.sh v0.1.0

set -euo pipefail

VERSION="${1:-}"
EXPECTED_ARCH="${2:-}"
if [ -z "$VERSION" ]; then
    echo "No version specified"
    exit 1
fi

case "$(uname -m)" in
    x86_64)
        LINUXDEPLOY_ARCH="x86_64"
        ASSET_ARCH="x86_64"
        ;;
    aarch64|arm64)
        LINUXDEPLOY_ARCH="aarch64"
        ASSET_ARCH="arm64"
        ;;
    *)
        echo "Unsupported AppImage build architecture: $(uname -m)"
        exit 1
        ;;
esac

if [ -n "$EXPECTED_ARCH" ] && [ "$EXPECTED_ARCH" != "$ASSET_ARCH" ]; then
    echo "Architecture mismatch: runner is $ASSET_ARCH but workflow requested $EXPECTED_ARCH"
    exit 1
fi

export VERSION=$VERSION
export APPDIR=$PWD/AppDir
export GSTREAMER_VERSION=1.0

rm -rf "$APPDIR"

# Download linuxdeploy and linuxdeploy-plugin-qt if not already present
LINUXDEPLOY="linuxdeploy-${LINUXDEPLOY_ARCH}.AppImage"
LINUXDEPLOY_QT="linuxdeploy-plugin-qt-${LINUXDEPLOY_ARCH}.AppImage"

if [ ! -f "$LINUXDEPLOY" ]; then
    wget -c -nv "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/${LINUXDEPLOY}"
    chmod a+x "$LINUXDEPLOY"
fi

if [ ! -f "$LINUXDEPLOY_QT" ]; then
    wget -c -nv "https://github.com/linuxdeploy/linuxdeploy-plugin-qt/releases/download/continuous/${LINUXDEPLOY_QT}"
    chmod a+x "$LINUXDEPLOY_QT"
fi

# Ensure patchelf is installed
if ! command -v patchelf &> /dev/null; then
    echo "ERROR: patchelf not found. Please install it with: sudo apt install patchelf"
    exit 1
fi

# Prepare AppDir structure
mkdir -p "$APPDIR/usr/bin"
mkdir -p "$APPDIR/usr/lib"
mkdir -p "$APPDIR/usr/share/applications"
# Only copy 512x512
# because of https://github.com/iDescriptor/iDescriptor/issues/123
# not sure if there is a better way to fix this
mkdir -p "$APPDIR/usr/share/icons/hicolor/512x512/apps"

cp target/release/idescriptor "$APPDIR/usr/bin/idescriptor"
cp packaging/shared/resources/app-icon/icon-512.png "$APPDIR/usr/share/icons/hicolor/512x512/apps/io.github.idescriptor.iDescriptor.png"

# ipatool's SAP signer loads Unicorn dynamically, so linuxdeploy cannot infer
# this dependency from the executable. Bundle it explicitly.
UNICORN_LIB="/usr/local/lib/libunicorn.so.2"
if [ ! -f "$UNICORN_LIB" ]; then
    UNICORN_LIB="$(ldconfig -p | awk '/libunicorn\.so\.2 / { print $NF; exit }')"
fi
if [ -z "$UNICORN_LIB" ] || [ ! -f "$UNICORN_LIB" ]; then
    echo "Error: libunicorn.so.2 was not found"
    exit 1
fi
cp -L "$UNICORN_LIB" "$APPDIR/usr/lib/libunicorn.so.2"
if [ -f /usr/local/share/licenses/unicorn/COPYING ]; then
    mkdir -p "$APPDIR/usr/share/licenses/unicorn"
    cp /usr/local/share/licenses/unicorn/COPYING "$APPDIR/usr/share/licenses/unicorn/COPYING"
fi


# Bundle GStreamer plugins and helpers
plugins_target_dir="$APPDIR/usr/lib/gstreamer-$GSTREAMER_VERSION"

# Detect plugin dirs based on architecture
if [ -d /usr/lib/$(uname -m)-linux-gnu/gstreamer-$GSTREAMER_VERSION ]; then
    plugins_dir="/usr/lib/$(uname -m)-linux-gnu/gstreamer-$GSTREAMER_VERSION"
else
    plugins_dir="/usr/lib/gstreamer-$GSTREAMER_VERSION"
fi

mkdir -p "$plugins_target_dir"

plugins=(
    "libgstalsa.so"
    "libgstpulse.so"
    "libgstpipewire.so"
    "libgstjack.so"
    "libgstaudioconvert.so"
    "libgstaudioresample.so"
    "libgstvolume.so"
    "libgstlevel.so"
    "libgstcoreelements.so"
    "libgstdecodebin.so"
    "libgstplayback.so"
    "libgstwavparse.so"
    "libgstmpg123.so"
    "libgstvorbis.so"
    "libgstogg.so"
    "libgstopus.so"
    "libgstflac.so"
    "libgstfaad.so"
    "libgstfdkaac.so"
    "libgstmatroska.so"
    "libgstlibav.so"
    "libgstapp.so"
    "libgstautodetect.so"
    "libgstaudioresample.so"
    "libgstvideoparsersbad.so"
    "libgstvaapi.so"
    "libgstva.so"
    "libgstvideo4linux2.so"
    "libgstvideoconvertscale.so"
    "libgstvideoconvert.so"
    "libgstvideoscale.so"
    "libgstvideofilter.so"
    "libgstjpeg.so"
    "libgstimagefreeze.so"
    "libgstximagesink.so"
    "libgstxvimagesink.so"
    "libgstgtk.so"
    "libgstopengl.so"
    "libgstqml6.so"
    "libgstrtp.so"
    "libgstrtpmanager.so"
    "libgsttypefindfunctions.so"
    "libgstisomp4.so"
)

for i in "${plugins[@]}"; do
    plugin_target_path="$plugins_target_dir/$i"
    plugin_path="$plugins_dir/$i"
    if [ -f "$plugin_path" ]; then
        echo "Copying plugin: $i"
        cp "$plugin_path" "$plugins_target_dir"
        echo "Manually setting RPATH for $plugin_target_path"
        patchelf --set-rpath '$ORIGIN/..:$ORIGIN' "$plugin_target_path"
    else
        echo "Warning: Plugin $i not found in $plugins_dir"
    fi
done

# Copy gst-plugin-scanner and gst-ptp-helper by searching for them
scanner_path=$(find /usr/lib -name gst-plugin-scanner 2>/dev/null | head -n 1)
if [ -n "$scanner_path" ] && [ -f "$scanner_path" ]; then
    echo "Copying gst-plugin-scanner from $scanner_path"
    cp "$scanner_path" "$plugins_target_dir/"
else
    echo "Warning: gst-plugin-scanner could not be found on the system."
fi

helper_path=$(find /usr/lib -name gst-ptp-helper 2>/dev/null | head -n 1)
if [ -n "$helper_path" ] && [ -f "$helper_path" ]; then
    echo "Copying gst-ptp-helper from $helper_path"
    cp "$helper_path" "$plugins_target_dir/"
else
    echo "Warning: gst-ptp-helper could not be found on the system."
fi

mkdir -p "$APPDIR/apprun-hooks"

cat <<'EOF' > "$APPDIR/apprun-hooks/linuxdeploy-plugin-env.sh"
#!/bin/bash

export GST_REGISTRY_REUSE_PLUGIN_SCANNER="no"
export GST_PLUGIN_SYSTEM_PATH_1_0="${APPDIR}/usr/lib/gstreamer-1.0"
export GST_PLUGIN_PATH_1_0="${APPDIR}/usr/lib/gstreamer-1.0"

export GST_PLUGIN_SCANNER_1_0="${APPDIR}/usr/lib/gstreamer-1.0/gst-plugin-scanner"
export GST_PTP_HELPER_1_0="${APPDIR}/usr/lib/gstreamer-1.0/gst-ptp-helper"

EOF

chmod +x "$APPDIR/apprun-hooks/linuxdeploy-plugin-env.sh"

# .desktop file
cp io.github.idescriptor.iDescriptor.desktop "$APPDIR/usr/share/applications/"

# Manually deploy geoservices plugins (workaround for linuxdeploy-plugin-qt not finding them)
if [ -n "${Qt6_DIR:-}" ] && [ -d "$Qt6_DIR/plugins/geoservices" ]; then
    echo "Manually deploying geoservices plugins from $Qt6_DIR/plugins/geoservices"
    mkdir -p "$APPDIR/usr/plugins/geoservices"
    cp -v "$Qt6_DIR/plugins/geoservices"/*.so "$APPDIR/usr/plugins/geoservices/" || echo "Warning: Could not copy geoservices plugins"

    echo "Setting RPATH for geoservices plugins"
    for plugin in "$APPDIR/usr/plugins/geoservices"/*.so; do
        if [ -f "$plugin" ]; then
            echo "Setting rpath for $plugin"
            patchelf --set-rpath '$ORIGIN/../../lib' "$plugin"
        fi
    done
else
    echo "Warning: Could not find geoservices plugins directory"
    echo "Qt6_DIR=${Qt6_DIR:-}"
    echo "QT_HOME=${QT_HOME:-}"
fi

export LD_LIBRARY_PATH="$APPDIR/usr/local/lib:${LD_LIBRARY_PATH:-}"
export LINUXDEPLOY_EXCLUDED_LIBRARIES="*sql*"
export QML_SOURCES_PATHS="./src/ui"
export EXTRA_QT_MODULES="geoservices;position;multimedia"


 "./${LINUXDEPLOY}" \
            --appdir ./AppDir \
            --desktop-file AppDir/usr/share/applications/io.github.idescriptor.iDescriptor.desktop \
            --executable "$APPDIR/usr/lib/gstreamer-1.0/gst-plugin-scanner" \
            --executable "$APPDIR/usr/lib/gstreamer-1.0/gst-ptp-helper" \
            --plugin qt \
            --exclude-library libGL,libGLX,libEGL,libOpenGL,libdrm,libva,libvdpau,libxcb,libxcb-glx,libxcb-dri2,libxcb-dri3,libX11,libXext,libXrandr,libXrender,libXfixes,libXau,libXdmcp,libqsqlmimer,libmysqlclient,libmysqlclient \
            --output appimage

# Find the generated AppImage and rename it
mapfile -t APPIMAGE_FILES < <(find . -maxdepth 1 -type f -name 'iDescriptor*.AppImage')
if [ "${#APPIMAGE_FILES[@]}" -eq 1 ]; then
    APPIMAGE_FILE="${APPIMAGE_FILES[0]}"
    OUTPUT="iDescriptor-${VERSION}-Linux_${ASSET_ARCH}.AppImage"
    mv "$APPIMAGE_FILE" "$OUTPUT"
    chmod +x "$OUTPUT"
    case "$ASSET_ARCH" in
        x86_64) FILE_PATTERN='x86-64|x86_64' ;;
        arm64) FILE_PATTERN='aarch64|ARM aarch64' ;;
    esac
    file "$OUTPUT" | grep -Eq "$FILE_PATTERN" || {
        echo "Generated AppImage architecture does not match $ASSET_ARCH"
        exit 1
    }

    # best effort to make sure .DirIcon resolves to 512x512 icon
    rm -rf squashfs-root
    "./$OUTPUT" --appimage-extract .DirIcon >/dev/null
    DIR_ICON_TARGET=$(readlink squashfs-root/.DirIcon)
    rm -rf squashfs-root
    "./$OUTPUT" --appimage-extract "$DIR_ICON_TARGET" >/dev/null
    ROOT_ICON_TARGET=$(readlink "squashfs-root/$DIR_ICON_TARGET")
    rm -rf squashfs-root

    EXPECTED_ICON_TARGET="usr/share/icons/hicolor/512x512/apps/io.github.idescriptor.iDescriptor.png"
    if [ "$ROOT_ICON_TARGET" != "$EXPECTED_ICON_TARGET" ]; then
        echo "Generated AppImage root icon points to '$ROOT_ICON_TARGET', expected '$EXPECTED_ICON_TARGET'"
        exit 1
    fi

    echo "AppImage created: $OUTPUT"
else
    echo "Error: Expected exactly one generated iDescriptor AppImage, found ${#APPIMAGE_FILES[@]}."
    exit 1
fi
