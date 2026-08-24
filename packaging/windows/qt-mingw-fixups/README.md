# Qt mingw_64 + MSYS2 MinGW64 launch fixes

The official Qt `win64_mingw` binaries (verified with 6.9.3) are built with an
older GCC than current MSYS2 ships. If you build the app with MSYS2's MinGW64
toolchain, the two runtimes cannot fully coexist, and three distinct
launch-breaking mismatches have been observed. All are fixed the same way:
rebuild the affected Qt module/plugin from matching-version source with your
local toolchain and stamp the results over the deployed copies.

Symptom in every case: the app starts but stays headless (no window), and the
QML engine log shows plugins failing with
"The specified procedure could not be found" — sometimes naming a plugin that
is actually innocent (the real culprit is one of *its* dependencies).

## Defect 1 — phantom thunk imports in GraphicalEffects private plugin

`qml/Qt5Compat/GraphicalEffects/private/qtgraphicaleffectsprivateplugin.dll`
imports two non-virtual adjustor thunks from Qt6Quick.dll:

- `_ZThn16_N10QQuickItem10classBeginEv`
- `_ZThn16_N10QQuickItem17componentCompleteEv`

Qt6Quick.dll does not export them (verify at PE level: identical hashes
between the installed copy and every deploy). Without them,
`DropShadow` is unavailable → `AnimatedDialog`/`ClosingHandler` unavailable →
Main.qml fails to load.

**Fix:** `build-qtgraphicaleffects.sh` rebuilds the plugin and injects
`qt5compat-shim/qtgraphicaleffects-thunks.cpp`, which defines the two thunks
locally as byte-exact assembly (`subq $16,%rcx; jmp <real method>`), so the
plugin satisfies its own imports at link time.

## Defect 2 — emutls symbols removed from newer libstdc++

Stock `Qt6ShaderTools.dll` and `Qt6Multimedia.dll` import from
libstdc++-6.dll:

- `__emutls_v._ZSt11__once_call`
- `__emutls_v._ZSt15__once_callable`

Old GCC's libstdc++ exported these; current MSYS2 GCC16 libstdc++ removed
them. Any process where the new libstdc++ wins the name binding (which is
unavoidable when your own exe is built with GCC16) fails to load these DLLs.
Because the GraphicalEffects private plugin links ShaderTools directly, this
defect masks itself as Defect 1 even after that fix is applied.

**Fix:** `build-qtshadertools.sh` and `build-qtmultimedia.sh` rebuild those
modules against the local runtime; the phantom imports disappear.

> qtmultimedia gotcha: configure enables the `vulkan` feature on finding just
> `vulkan-1.dll.a`, but `QRhiVulkanInitParams` is only declared under
> `(QT_CONFIG(vulkan) && __has_include(<vulkan/vulkan.h>))` — install
> `mingw-w64-x86_64-vulkan-headers` first and delete any stale build dir.

## Defect 3 — no single C++ runtime serves both sides

Do NOT "fix" the above by swapping the deployed libstdc++/libgcc/libwinpthread
trio for Qt's older bundled copies: current MSYS2 dependency DLLs (ffmpeg,
gnutls, gstreamer core, openal, libheif, ...) import symbols that only exist
in the NEW trio (`clock_gettime64`, `nanosleep64`, `pthread_cond_timedwait64`,
`std::__detail __wait_impl`, ...). Neither runtime satisfies the whole tree;
the only consistent policy is to keep the new trio and rebuild any official
Qt binary that still needs old-runtime symbols.

## Usage

```bash
# 1) Download + extract the module sources you need, e.g.:
#    https://download.qt.io/official_releases/qt/<ver>/<ver>.<patch>/submodules/qtshadertools-everywhere-src-<ver>.zip

# 2) Build each fixup (source dir as argument; stages next to the script):
./build-qtgraphicaleffects.sh /path/to/qt5compat-everywhere-src-6.9.3
./build-qtshadertools.sh      /path/to/qtshadertools-everywhere-src-6.9.3
./build-qtmultimedia.sh       /path/to/qtmultimedia-everywhere-src-6.9.3

# 3) Point the portable deploy at the staging tree:
bash ../portable/deploy.sh ... --qt-fixups-dir="$(pwd)/stage"
```

Environment overrides: `QT_BIN_PATH` (default `C:/Qt/6.9.3/mingw_64`),
`MSYSTEM=MINGW64` assumed. Each script verifies its output (objdump) before
staging, so a failed fix cannot silently ship.

Staging layout consumed by `deploy.sh --qt-fixups-dir`:

```
stage/
  qtgraphicaleffects/{qtgraphicaleffectsplugin.dll,private/qtgraphicaleffectsprivateplugin.dll}
  qtshadertools/bin/Qt6ShaderTools.dll
  qtmultimedia/bin/*.dll
  qtmultimedia/plugins/multimedia/*.dll
```
