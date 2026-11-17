#!/usr/bin/env bash

set -euo pipefail

VERSION="2.1.4"
# Use the official release libraries, not a local source build. Both require
# glibc 2.34 (our oldest runner, Ubuntu 22.04, provides 2.35).
case "$(uname -m)" in
    x86_64)
        ARCHIVE="ubuntu-cmake-shared-x64.7z"
        SHA256="07841e2df55a6d4ac063017bdfd861f1712083c6f2ffe5fe96973c834cd772dc"
        EXTRACTOR_ARCH="x64"
        EXTRACTOR_SHA256="4ca3b7c6f2f67866b92622818b58233dc70367be2f36b498eb0bdeaaa44b53f4"
        ;;
    aarch64|arm64)
        ARCHIVE="ubuntu-cmake-aarch64.7z"
        SHA256="348d5714e07d5b7fcc3ed56ae8749bc2afece9f5ae7556677fb8f4d3ea714352"
        EXTRACTOR_ARCH="arm64"
        EXTRACTOR_SHA256="39c5140f02ce4436599303c59a149f654cb1bbc47cdc105a942120d747ae040d"
        ;;
    *)
        echo "Unsupported Unicorn host architecture: $(uname -m)" >&2
        exit 1
        ;;
esac

PREFIX="${UNICORN_INSTALL_PREFIX:-/usr/local}"
WORK_DIR="$(mktemp -d)"

cleanup() {
    rm -rf "$WORK_DIR"
}
trap cleanup EXIT

download_verified() {
    local url="$1" output="$2" checksum="$3"
    curl --fail --location --retry 3 --connect-timeout 20 --max-time 180 \
        --output "$output" "$url"
    echo "$checksum  $output" | sha256sum --check
}

download_verified \
    "https://github.com/unicorn-engine/unicorn/releases/download/${VERSION}/${ARCHIVE}" \
    "$WORK_DIR/$ARCHIVE" "$SHA256"

# The release archives use the ARM64 BCJ filter, which Ubuntu 22.04's p7zip
# cannot decode. Use a pinned, prebuilt static extractor on both runners.
download_verified \
    "https://www.7-zip.org/a/7z2501-linux-${EXTRACTOR_ARCH}.tar.xz" \
    "$WORK_DIR/7zip.tar.xz" "$EXTRACTOR_SHA256"
tar -xJf "$WORK_DIR/7zip.tar.xz" -C "$WORK_DIR" 7zzs
"$WORK_DIR/7zzs" x -y "-o$WORK_DIR/extracted" "$WORK_DIR/$ARCHIVE" lib/libunicorn.so.2

# The binary archives omit the top-level license. Fetch it from the exact
# source commit associated with the release, and verify it as well.
download_verified \
    "https://raw.githubusercontent.com/unicorn-engine/unicorn/1730bd8d4ce6dbca658637f4b7dec81a7eff3a12/COPYING" \
    "$WORK_DIR/COPYING" "6f04ae8364d0079a192b14635f4b1da294ce18724c034c39a6a41d1b09df6100"

INSTALL=(install)
if [ "$PREFIX" = /usr/local ] && [ "$(id -u)" -ne 0 ]; then
    INSTALL=(sudo install)
fi
"${INSTALL[@]}" -Dm0755 "$WORK_DIR/extracted/lib/libunicorn.so.2" "$PREFIX/lib/libunicorn.so.2"
"${INSTALL[@]}" -Dm0644 "$WORK_DIR/COPYING" "$PREFIX/share/licenses/unicorn/COPYING"
if [ "$PREFIX" = /usr/local ]; then
    sudo ldconfig
fi
