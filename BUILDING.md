# Building iDescriptor

iDescriptor is a Rust and Qt 6 application built with Cargo. Qt integration is provided by qmetaobject-rs and the existing `cpp!` bridge, while FFmpeg and libheif are consumed through Rust bindings.

## Common requirements

All platforms need:

- Git
- Rust stable (the project uses Rust edition 2024)
- CMake 3.16 or newer
- A C/C++ compiler with C++17 support
- `pkg-config`
- Qt 6 (version 6.9) with these components:
  - Core
  - GUI
  - QML and Qt Quick
  - Quick Controls 2
  - Multimedia
  - Location and Positioning
  - Serial Port
  - DBus on Linux
- OpenSSL
- libplist (version 2.7.0 if you want match the CI build) (the only required dependency from libimobiledevice)
- FFmpeg development libraries (`avformat`, `avcodec`, `avutil`, and `swscale`)
- GStreamer development libraries (`gstreamer`, app, audio, and video)
- libheif

Clone the repository with all submodules:

```bash
git clone --recurse-submodules https://github.com/iDescriptor/iDescriptor.git
cd iDescriptor
```

If the repository was cloned without submodules:

```bash
git submodule update --init --recursive
```

### Ubuntu/Debian dependencies

We develop on Arch Linux however you can also use Ubuntu/Debian.

The exact availability and names of Qt packages vary by Ubuntu/Debian release. 

**You can install Qt from the official installer or install it from the package manager. Both should work fine provided that you have at least version 6.9.2 of Qt installed.**

What do you need to install?

```bash
sudo apt-get update
sudo apt-get install -y \
  build-essential \
  autoconf \
  automake \
  cmake \
  git \
  libavahi-client-dev \
  libavahi-compat-libdnssd-dev \
  libavcodec-dev \
  libavformat-dev \
  libavutil-dev \
  libfuse3-dev \
  libglib2.0-dev \
  libgstreamer1.0-dev \
  libgstreamer-plugins-base1.0-dev \
  libheif-dev \
  libplist-dev \
  libssl-dev \
  libswscale-dev \
  libusb-1.0-0-dev \
  ninja-build \
  patchelf \
  pkg-config \
  python3-pip \
  qt6-base-dev \
  qt6-base-private-dev \
  qt6-declarative-dev \
  qt6-declarative-private-dev \
  qt6-multimedia-dev \
  qt6-positioning-dev \
  qt6-serialport-dev \
  qt6-tools-dev \
  wget
```

Install the QML runtime modules provided by your distribution. On Ubuntu/Debian these commonly include:

```bash
sudo apt-get install -y \
  qml6-module-qt-labs-platform \
  qml6-module-qt5compat-graphicaleffects \
  qml6-module-qtlocation \
  qml6-module-qtmultimedia \
  qml6-module-qtpositioning \
  qml6-module-qtquick \
  qml6-module-qtquick-controls \
  qml6-module-qtquick-dialogs \
  qml6-module-qtquick-layouts \
  qml6-module-qtquick-window
```

The application uses additional GStreamer plugins at runtime:

```bash
sudo apt-get install -y \
  gstreamer1.0-alsa \
  gstreamer1.0-gl \
  gstreamer1.0-libav \
  gstreamer1.0-pipewire \
  gstreamer1.0-plugins-bad \
  gstreamer1.0-plugins-good \
  gstreamer1.0-plugins-ugly \
  gstreamer1.0-pulseaudio \
  gstreamer1.0-tools
```

If your distribution provides the Qt 6 GStreamer plugin, install it as well (often named `gstreamer1.0-qt6`). The release workflow builds `qml6glsink` from `gst-plugins-good` because it is not consistently available on all runners; see `.github/workflows/build-linux.yml` for the exact CI procedure.

Install Rust if needed:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup toolchain install stable
rustup default stable
```

### Build and run

```bash
cargo build
cargo run
```

If Qt was installed outside the system prefix, make its tools and CMake files discoverable. For example:

```bash
export PATH="/path/to/Qt/6.x.x/gcc_64/bin:$PATH"
export CMAKE_PREFIX_PATH="/path/to/Qt/6.x.x/gcc_64/lib/cmake:$CMAKE_PREFIX_PATH"
export PKG_CONFIG_PATH="/path/to/Qt/6.x.x/gcc_64/lib/pkgconfig:$PKG_CONFIG_PATH"
```

### Build an AppImage

The AppImage build requires `patchelf`, `wget`, the runtime GStreamer plugins listed above, and a working `qml6glsink` plugin.

Build with the AppImage feature, then run the deployment script from the repository root:

```bash
cargo build --release --features appimage
bash packaging/linux/appimage/deploy.sh v0.6.0 x86_64
```

For ARM64:

```bash
cargo build --release --features appimage
bash packaging/linux/appimage/deploy.sh v0.6.0 arm64
```

The output is named like:

```text
iDescriptor-v0.6.0-Linux_x86_64.AppImage
iDescriptor-v0.6.0-Linux_arm64.AppImage
```


The deployment script downloads `linuxdeploy` and its Qt plugin, creates the AppDir, installs the `.desktop` file and application icons, bundles Qt/GStreamer dependencies, and produces the AppImage.

## Build for a package manager

To build for a package manager, use the `package` feature:

```bash
# Set a custom package manager message, this will be shown to the user when there is an update available
export IDESCRIPTOR_PACKAGE_MANAGER_MESSAGE="Please update iDescriptor using yay or paru."
cargo build --release --features package_manager
```

## Windows

The Windows release workflow uses:

- Windows 2022 runner
- MSYS2/MinGW64
- Rust target `stable-x86_64-pc-windows-gnu` (you cannot use MSVC due to UxPlay)
- Qt 6.9.3 MinGW 64-bit (we prefer the MinGW runtime over UCRT)
- Bonjour SDK
- WinFsp

### Install Qt

Use the Qt Online Installer or the Qt Maintenance Tool installed with Qt Creator. Install Qt 6.9.3 (or a compatible Qt 6 version) for **MinGW 64-bit**, including:

- Qt Multimedia
- Qt Location
- Qt Positioning
- Qt Serial Port
- Qt 5 Compatibility Module
- Qt Shader Tools

### Install MSYS2 dependencies

```bash
pacman -S --needed --noconfirm \
  base-devel \
  coreutils \
  git \
  make \
  libtool \
  autoconf \
  automake-wrapper \
  p7zip \
  mingw-w64-x86_64-clang \
  mingw-w64-x86_64-cmake \
  mingw-w64-x86_64-curl \
  mingw-w64-x86_64-gcc \
  mingw-w64-x86_64-gst-libav \
  mingw-w64-x86_64-gst-plugins-bad \
  mingw-w64-x86_64-gst-plugins-base \
  mingw-w64-x86_64-gst-plugins-good \
  mingw-w64-x86_64-gst-plugins-ugly \
  mingw-w64-x86_64-gstreamer \
  mingw-w64-x86_64-libarchive \
  mingw-w64-x86_64-libheif \
  mingw-w64-x86_64-libzip \
  mingw-w64-x86_64-meson \
  mingw-w64-x86_64-ninja \
  mingw-w64-x86_64-openssl \
  mingw-w64-x86_64-pkgconf \
  mingw-w64-x86_64-rustup
```

CI builds libplist 2.7.0 from source so every platform uses the same version. You can either reproduce the `Build libplist` step in `.github/workflows/build-windows.yml` or install a compatible MinGW `libplist` development package if available.

### Install Bonjour and WinFsp

Install:

- [Bonjour runtime & Bonjour SDK](https://github.com/tempx-x/bonjour-sdk/raw/refs/heads/main/bonjoursdksetup.exe) (or install iTunes or the Bonjour SDK from the Apple Developer website)
- [WinFsp](https://github.com/winfsp/winfsp)

By default, the build looks for the Bonjour SDK at:

```text
C:\Program Files\Bonjour SDK
```

If it is installed elsewhere, set `BONJOUR_SDK` to the SDK directory.

### Configure Qt and Rust

In the MSYS2 MinGW x64 shell, adjust the Qt path to your installation:

```bash
export QMAKE="C:/Qt/6.9.3/mingw_64/bin/qmake.exe"
export CMAKE_PREFIX_PATH="C:/Qt/6.9.3/mingw_64"
export PKG_CONFIG_EXECUTABLE="C:/msys64/mingw64/bin/pkg-config.exe"
export BONJOUR_SDK="C:/Program Files/Bonjour SDK"

rustup toolchain install stable-x86_64-pc-windows-gnu
rustup default stable-x86_64-pc-windows-gnu
```

Make sure the Qt and MinGW binary directories are on `PATH`:

```bash
export PATH="/c/Qt/6.9.3/mingw_64/bin:/mingw64/bin:$PATH"
```

### Install FluentUI

The Windows QML interface imports FluentUI. The release workflow installs the project's FluentUI fork into Qt's QML directory:

```bash
git clone https://github.com/uncor3/FluentUI.git
cd FluentUI
cmake -B build -S . \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_PREFIX_PATH="C:/Qt/6.9.3/mingw_64" \
  -DFLUENTUI_QML_PLUGIN_DIRECTORY="C:/Qt/6.9.3/mingw_64/qml/FluentUI"
cmake --build build
cd ..
```

### Build and run

From the iDescriptor repository in the MSYS2 MinGW x64 shell:

```bash
cargo build
cargo run
```

Release build:

```bash
cargo build --release
./target/release/idescriptor.exe
```

### Create a portable directory

After building in release mode:

```bash
rm -rf target/deploy
mkdir -p target/deploy
cp target/release/idescriptor.exe target/deploy/

bash packaging/windows/portable/deploy.sh \
  --executable="target/deploy/idescriptor.exe" \
  --output-dir="target/deploy" \
  --qt-bin-path="C:/Qt/6.9.3/mingw_64/bin" \
  --project-source-dir="." \
  --qml-source-dir="$(pwd)/src/ui"
```

The deployment script uses `windeployqt6`, then copies the required GStreamer plugins, MinGW DLLs, WinFsp DLL, and helper scripts. Its dependency list is intentionally strict, so use the MSYS2 package versions expected by the current workflow when preparing release artifacts.

MSI and MSIX packaging is performed by the PowerShell scripts under `packaging/windows/msi/` and `packaging/windows/msix/`. See `.github/workflows/build-windows.yml` for the complete release sequence, including optional code signing.

## macOS

Install Rust:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup toolchain install stable
rustup default stable
```

Install Qt and native dependencies through Homebrew:

```bash
brew install \
  autoconf \
  automake \
  cmake \
  create-dmg \
  curl \
  ffmpeg \
  gst-libav \
  gst-plugins-bad \
  gst-plugins-base \
  gst-plugins-good \
  gst-plugins-ugly \
  gstreamer \
  jpeg-xl \
  libheif \
  libplist \
  meson \
  ninja \
  openssl \
  pkg-config \
  qt \
  sqlite
```

Make Homebrew Qt and dependencies discoverable (sometimes required):

```bash
export PATH="$(brew --prefix qt)/bin:$PATH"
export CMAKE_PREFIX_PATH="$(brew --prefix qt)/lib/cmake:${CMAKE_PREFIX_PATH:-}"
export PKG_CONFIG_PATH="$(brew --prefix)/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
```

### Build and run

```bash
cargo build
cargo run
```

Release build:

```bash
cargo build --release
./target/release/idescriptor
```

### Create a DMG

Build in release mode, then run the deployment script with the current architecture and a version:

Apple Silicon:

```bash
cargo build --release
bash packaging/macos/dmg/deploy.sh arm64 v0.6.0
```

Intel:

```bash
cargo build --release
bash packaging/macos/dmg/deploy.sh x86_64 v0.6.0
```

The script creates a native `.app` bundle, runs `macdeployqt`, bundles GStreamer/FFmpeg/SQLite dependencies, signs the app (ad-hoc if no identity is provided), and creates a DMG using `create-dmg`.

Expected outputs:

```text
build/iDescriptor-v0.6.0-Apple_Silicon.dmg
build/iDescriptor-v0.6.0-Apple_Intel.dmg
```

For a signed build, set `MACOS_SIGNING_IDENTITY` before running the deployment script. Release notarization additionally requires Apple credentials; see `.github/workflows/build-macos.yml` for the CI variables and notarization commands.

## Cargo features

The root crate currently defines these build features:

- `appimage` — adjusts native build behavior for AppImage packaging
- `flatpak` — enables Flatpak-specific behavior
- `package_manager` — marks builds managed by a system package manager
- `windows_store` — builds the Microsoft Store variant

Examples:

```bash
cargo build --release --features appimage
cargo build --release --features flatpak
cargo build --release --features package_manager
cargo build --release --features windows_store
```

Use platform-appropriate features only; for example, `windows_store` is intended for Windows packaging.

## Troubleshooting

### Qt cannot be found

Verify that `qmake` and CMake can locate Qt:

```bash
qmake -query
cmake --version
```

Then ensure Qt's `bin` directory is on `PATH` and its prefix is in `CMAKE_PREFIX_PATH`.

### A native package is missing

The Cargo build and dependency scripts use `pkg-config` for native libraries such as OpenSSL, libplist, libheif, GLib/GObject, FFmpeg, GStreamer, and Qt DBus. Check a package directly, for example:

```bash
pkg-config --modversion libplist-2.0
pkg-config --modversion gstreamer-1.0
pkg-config --modversion libavformat
pkg-config --modversion libheif
```

### QML module is missing at runtime

Ensure the Qt QML modules listed above are installed and that `QML_IMPORT_PATH` points to the QML directory for the same Qt installation used at build time. Windows additionally needs the FluentUI plugin.

### Match CI exactly

When a local release build differs from CI, compare against:

- `.github/workflows/build-linux.yml`
- `.github/workflows/build-windows.yml`
- `.github/workflows/build-macos.yml`
- `build.rs`
- `Cargo.toml`
