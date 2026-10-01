// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();

    println!("cargo:rerun-if-changed=src/native.rs");
    println!("cargo:rerun-if-changed=src/media_decoder.rs");
    println!("cargo:rerun-if-changed=src/status_window_controller.rs");
    println!("cargo:rerun-if-changed=src/system_appearance.rs");
    println!("cargo:rerun-if-env-changed=IDESCRIPTOR_PACKAGE_MANAGER_MESSAGE");

    println!("cargo:rerun-if-changed=src/live_reload.cpp");

    println!("cargo:rerun-if-changed=packaging/shared/resources/app-icon/icon.ico");
    println!("cargo:rerun-if-changed=packaging/shared/resources/app-icon/icon.png");
    println!("cargo:rerun-if-changed=packaging/windows/idescriptor.rc");

    let qt_include_path = env::var("DEP_QT_INCLUDE_PATH").unwrap();
    let qt_library_path = env::var("DEP_QT_LIBRARY_PATH").unwrap();

    let qt_version = env::var("DEP_QT_VERSION").unwrap();
    let flatpak_build = env::var_os("CARGO_FEATURE_FLATPAK").is_some();

    //TODO
    // if target_os == "linux" {
    //     let linker_dir = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("scripts");
    //     println!(
    //         "cargo:rustc-link-arg-bin=idescriptor=-B{}",
    //         linker_dir.display()
    //     );
    //     println!("cargo:rustc-link-arg-bin=idescriptor=-fuse-ld=lld");
    // }

    // compile_translations(&qt_library_path);

    // ------------------------------------------------------------------
    // cpp_build — scans the crate root and compiles its cpp! macro modules
    // ------------------------------------------------------------------
    let mut config = cpp_build::Config::new();
    // GCC 16 diagnoses a SFINAE pattern used by the Qt headers. This is
    // third-party compatibility noise, so suppress it only when supported.
    config.flag_if_supported("-Wno-sfinae-incomplete");

    for f in env::var("DEP_QT_COMPILE_FLAGS")
        .unwrap()
        .split_terminator(';')
    {
        config.flag(f);
    }

    let mut public_include = |name: &str| {
        if target_os == "macos" {
            config.include(format!("{}/{}.framework/Headers/", qt_library_path, name));
        }
        config.include(format!("{}/{}", qt_include_path, name));
    };
    public_include("QtCore");
    public_include("QtGui");
    public_include("QtQuick");
    public_include("QtQml");
    public_include("QtQuickControls2");
    public_include("QtWidgets");
    if target_os == "linux" && flatpak_build {
        config.define("IDESCRIPTOR_FLATPAK", None);
    }

    let mut private_include = |name: &str| {
        if target_os == "macos" {
            config.include(format!(
                "{}/{}.framework/Headers/{}",
                qt_library_path, name, qt_version
            ));
            config.include(format!(
                "{}/{}.framework/Headers/{}/{}",
                qt_library_path, name, qt_version, name
            ));
        }
        config
            .include(format!("{}/{}/{}", qt_include_path, name, qt_version))
            .include(format!(
                "{}/{}/{}/{}",
                qt_include_path, name, qt_version, name
            ));
    };
    private_include("QtCore");
    private_include("QtQuick");
    private_include("QtQml");

    let mut add_pkg_includes = |pkg: &str| {
        if let Ok(lib) = pkg_config::Config::new().cargo_metadata(false).probe(pkg) {
            for p in lib.include_paths {
                config.include(p);
            }
        }
    };
    add_pkg_includes("gstreamer-1.0");
    add_pkg_includes("gstreamer-app-1.0");
    add_pkg_includes("gstreamer-video-1.0");
    add_pkg_includes("gstreamer-audio-1.0");
    add_pkg_includes("glib-2.0");
    add_pkg_includes("gobject-2.0");

    if target_os == "macos" {
        config.flag(&format!("-F{}", qt_library_path));
    }

    if let Ok(time) = std::time::SystemTime::now().duration_since(std::time::SystemTime::UNIX_EPOCH)
    {
        println!(
            "cargo:rustc-env=BUILD_TIME={}",
            (time.as_secs() - 1642516578) / 600
        );
    }

    if target_os == "windows" {
        embed_resource::compile("packaging/windows/idescriptor.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("failed to compile Windows executable resources");
    }

    config.include(&qt_include_path).build("src/main.rs");

    pkg_config::Config::new().probe("glib-2.0").unwrap();
    pkg_config::Config::new().probe("gobject-2.0").unwrap();

    // GStreamer
    for pkg in &[
        "gstreamer-1.0",
        "gstreamer-app-1.0",
        "gstreamer-video-1.0",
        "gstreamer-audio-1.0",
    ] {
        pkg_config::Config::new().probe(pkg).unwrap();
    }

    // Qt (macOS needs framework search path; Linux/Windows via pkg-config)
    if target_os == "macos" {
        println!("cargo:rustc-link-search=framework={}", qt_library_path);
        for fw in &["QtCore", "QtGui", "QtQml", "QtQuick", "QtQuickControls2"] {
            println!("cargo:rustc-link-lib=framework={}", fw);
        }
    } else {
        // pkg_config::Config::new().probe("Qt6Core").unwrap();
    }
}

// TODO: allow dead_code until we decide whether this is the right approach for translations
#[allow(dead_code)]
fn compile_translations(qt_library_path: &str) {
    let lrelease = find_lrelease(qt_library_path)
        .unwrap_or_else(|| panic!("lrelease not found for Qt library path {}", qt_library_path));

    let translations_dir = std::path::Path::new("translations");
    let target_translations_dir = std::path::Path::new("target").join("translations");
    fs::create_dir_all(&target_translations_dir).unwrap();

    for entry in fs::read_dir(&target_translations_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("qm") {
            fs::remove_file(path).unwrap();
        }
    }

    println!("cargo:rerun-if-changed={}", translations_dir.display());

    for entry in fs::read_dir(translations_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("ts") {
            continue;
        }

        println!("cargo:rerun-if-changed={}", path.display());

        let output = target_translations_dir
            .join(path.file_stem().unwrap())
            .with_extension("qm");
        let status = Command::new(&lrelease)
            .arg(&path)
            .arg("-qm")
            .arg(&output)
            .status()
            .unwrap_or_else(|err| {
                panic!("failed to run {}: {}", lrelease.display(), err);
            });

        if !status.success() {
            panic!(
                "{} failed while compiling {}",
                lrelease.display(),
                path.display()
            );
        }
    }
}

fn find_lrelease(qt_library_path: &str) -> Option<PathBuf> {
    let executable = if cfg!(windows) {
        "lrelease.exe"
    } else {
        "lrelease"
    };

    for bin_dir in qt_bin_dirs(qt_library_path) {
        let candidate = bin_dir.join(executable);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    None
}

fn qt_bin_dirs(qt_library_path: &str) -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    for query in ["QT_HOST_BINS", "QT_INSTALL_BINS"] {
        if let Some(dir) = qmake_query(query) {
            dirs.push(dir);
        }
    }

    let qt_library_path = Path::new(qt_library_path);
    if let Some(parent) = qt_library_path.parent() {
        dirs.push(parent.join("bin"));
    }

    dirs.push(qt_library_path.join("bin"));
    dirs.push(PathBuf::from("/usr/lib/qt6/bin"));
    dirs.push(PathBuf::from("/usr/lib/qt5/bin"));
    dirs.push(PathBuf::from("/usr/bin"));

    dirs
}

fn qmake_query(var: &str) -> Option<PathBuf> {
    let qmake = env::var("QMAKE")
        .ok()
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .map_or_else(
            || vec!["qmake6".into(), "qmake".into(), "qmake-qt5".into()],
            |qmake| vec![qmake],
        );

    for candidate in qmake {
        let output = match Command::new(candidate).arg("-query").arg(var).output() {
            Ok(output) => output,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => continue,
        };

        if !output.status.success() {
            continue;
        }

        let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !value.is_empty() {
            return Some(PathBuf::from(value));
        }
    }

    None
}
