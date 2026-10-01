// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

#![recursion_limit = "4096"]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use crate::qquickimageprovider_imp::AddImageProvider;
use ::log::info;
use once_cell::sync::Lazy;
use qmetaobject::*;
use std::future::Future;
use std::sync::mpsc;
use tokio::runtime::Runtime;
use tracing_subscriber::{EnvFilter, filter::LevelFilter, prelude::*};

pub mod afc_services;
pub mod airplay;
mod airplay_receiver;
pub mod apps;
pub mod backup_manager;
pub mod companion_protocol;
pub mod constants;
pub mod core;
pub mod dev_imgs;
pub mod dev_imgs_manager;
pub mod device_ctx;
pub mod device_db;
#[cfg(not(target_os = "macos"))]
pub mod diagnose;
pub mod gallery;
pub mod gallery_fs_provider;
pub mod gallery_sqlite_provider;
pub mod gallery_sqlite_vfs;
#[cfg(not(target_os = "macos"))]
pub mod ifuse;
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub mod ifuse_manager;
pub mod image_cache;
pub mod image_loader;
pub mod image_provider;
pub mod io_manager;
pub mod jailbroken;
pub mod list_model;
pub mod media_decoder;
pub mod media_streamer;
pub mod native;
pub mod network_device_provider;
pub mod platform;
pub mod qml_image;
pub mod qml_utils;
pub mod qquickimageprovider_imp;
pub mod qrc;
pub mod qt_threading;
pub mod screenshot;
pub mod service_factory;
pub mod service_manager;
pub mod settings_manager;
pub mod springboard_services;
pub mod status_window_controller;
pub mod system_appearance;
pub mod transfer_speed_tester;
#[cfg(not(debug_assertions))]
pub mod ui_qrc;
pub mod updater;
pub mod utils;
pub mod web_wireless_gallery_import;
pub mod wired_gallery_import;

pub const IMAGE_LIST_URL: &str = "https://raw.githubusercontent.com/iDescriptor/iDescriptor/refs/heads/main/DeveloperDiskImages.json";
pub const POSSIBLE_ROOT: &str = "../../../../";
pub const APP_LABEL: &str = "iDescriptor";
pub const EV_CONNECTED: u32 = 1;
pub const EV_DISCONNECTED: u32 = 2;
pub const EV_PAIRING_PENDING: u32 = 3;
pub const EV_FAIL: u32 = 4;

static RUNTIME: Lazy<Runtime> = Lazy::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
});

pub fn run_sync<F, R>(fut: F) -> R
where
    F: Future<Output = R> + Send + 'static,
    R: Send + 'static,
{
    let (tx, rx) = mpsc::sync_channel(1);

    RUNTIME.spawn(async move {
        let res = fut.await;
        let _ = tx.send(res);
    });

    rx.recv().expect("Tokio runtime worker panicked")
}

fn main() {
    let reset_settings =
        std::env::args_os().any(|argument| argument == std::ffi::OsStr::new("--reset-settings"));
    let application_version = QString::from(env!("CARGO_PKG_VERSION"));

    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::DEBUG.into())
        .from_env_lossy()
        // debug logs from idevice crate is so frequent and it even logs read bytes etc.
        // so we filter it out for now
        // also filter fuse debug logs -> DEBUG fuser::request: FUSE(1482)
        .add_directive("idevice=warn".parse().expect("valid idevice log filter"))
        .add_directive(
            "fuser::request=warn"
                .parse()
                .expect("valid fuser request log filter"),
        );
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().compact())
        .with(filter)
        .init();

    let ui_live_reload = utils::env_flag("IDESCRIPTOR_UI_LIVE_RELOAD");
    let qml_from_fs =
        cfg!(debug_assertions) || ui_live_reload || utils::env_flag("IDESCRIPTOR_QML_FROM_FS");

    // TODO: report crashs logs
    // however we currently log sensitive info, we need to handle that first
    // let _ = util::install_crash_handler();
    qmetaobject::log::init_qt_to_rust();

    native::configure_application(application_version);

    #[cfg(target_os = "linux")]
    unsafe {
        std::env::set_var("QT_QPA_PLATFORM", "xcb");
    }

    crate::qrc::rsrc();
    #[cfg(not(debug_assertions))]
    crate::ui_qrc::qml();
    #[cfg(all(not(debug_assertions), target_os = "linux"))]
    crate::ui_qrc::linux_qml();

    #[cfg(target_os = "macos")]
    {
        crate::qrc::macos_rsrc();
        #[cfg(not(debug_assertions))]
        crate::ui_qrc::macos_qml();
    }

    // workaround for gstreamer plugins not being loaded on Windows
    #[cfg(target_os = "windows")]
    {
        // in the release build we bundle gstreamer plugins
        if !cfg!(debug_assertions) {
            // unsafe is needed because of env::set_var
            unsafe {
                use std::env;

                let exe_dir = std::env::current_exe()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .to_path_buf();

                let gst_plugin_path = exe_dir.join("gstreamer-1.0");

                env::set_var(
                    "GST_PLUGIN_PATH",
                    gst_plugin_path.to_string_lossy().to_string(),
                );
            }
        }

        #[cfg(not(debug_assertions))]
        crate::ui_qrc::windows_qml();
    }

    #[cfg(target_os = "macos")]
    {
        if !cfg!(debug_assertions) {
            let executable_path =
                std::env::current_exe().expect("failed to determine the application path");
            let application_dir = executable_path
                .parent()
                .expect("application executable has no parent directory");
            let frameworks_path = application_dir
                .parent()
                .expect("application directory has no parent directory")
                .join("Frameworks");
            let gst_plugin_path = frameworks_path.join("gstreamer");
            let gst_plugin_scanner_path = frameworks_path.join("gst-plugin-scanner");

            unsafe {
                std::env::set_var("GST_PLUGIN_PATH", &gst_plugin_path);
                std::env::set_var("GST_PLUGIN_SYSTEM_PATH", &gst_plugin_path);
                std::env::set_var("GST_PLUGIN_SCANNER", &gst_plugin_scanner_path);
            }
        }
    }

    qml_register_type::<screenshot::ScreenshotBackend>(
        cstr::cstr!("iDescriptor"),
        1,
        0,
        cstr::cstr!("ScreenshotBackend"),
    );
    qml_register_type::<qml_image::QmlImage>(
        cstr::cstr!("iDescriptor"),
        1,
        0,
        cstr::cstr!("QmlImage"),
    );
    // FIXME: should be singleton
    qml_register_type::<jailbroken::Jailbroken>(
        cstr::cstr!("iDescriptor"),
        1,
        0,
        cstr::cstr!("JailbrokenImp"),
    );

    let mut engine = QmlEngine::new();
    let engine_ptr = engine.cpp_ptr();

    #[cfg(not(target_os = "windows"))]
    native::add_resource_style_import_path(&mut engine);

    #[cfg(all(debug_assertions, not(target_os = "windows")))]
    if ui_live_reload || qml_from_fs {
        let style_import_path = QString::from(utils::source_qml_path("src/ui/styles"));
        native::add_style_import_path(&mut engine, style_import_path);
    }

    if reset_settings {
        settings_manager::SettingsManager::clear_all();
        native::show_settings_reset_message();
    }

    #[cfg(target_os = "windows")]
    native::set_default_font();

    let settings_manager_impl = settings_manager::SettingsManager::default();
    let initial_language = settings_manager_impl.language();
    let network_discovery_backend = settings_manager_impl.network_discovery_backend();
    let z_linux_window_enabled =
        cfg!(target_os = "linux") && settings_manager_impl.z_linux_window();
    qml_utils::QmlUtils::apply_language_to_engine(engine_ptr, initial_language);
    let settings_manager = QObjectBox::new(settings_manager_impl);
    engine.set_object_property("settingsManager".into(), settings_manager.pinned());

    let updater = QObjectBox::new(updater::Updater::new_with_state());
    engine.set_object_property("UpdaterImp".into(), updater.pinned());

    let core_obj = QObjectBox::new(core::Core::default());
    engine.set_object_property("core".into(), core_obj.pinned());

    let obj = QObjectBox::new(image_loader::ImageLoader::default());
    engine.set_object_property("imageLoader".into(), obj.pinned());

    let apps_impl = QObjectBox::new(apps::Apps::new_with_state());
    engine.set_object_property("apps".into(), apps_impl.pinned());

    let provider_ref_cell = QObjectBox::new(image_provider::ImageProvider::default(obj));
    engine.add_image_provider("thumb", provider_ref_cell);

    let io_manager = QObjectBox::new(io_manager::IOManager::default());
    engine.set_object_property("ioManager".into(), io_manager.pinned());

    let airplay = QObjectBox::new(airplay::Airplay::default());
    engine.set_object_property("AirplayImp".into(), airplay.pinned());

    let dev_imgs_manager = QObjectBox::new(dev_imgs_manager::DevImgsManager::default());
    engine.set_object_property("DevImgsManager".into(), dev_imgs_manager.pinned());

    let wireless_import =
        QObjectBox::new(web_wireless_gallery_import::WebWirelessGalleryImport::new_with_state());
    engine.set_object_property("WebWirelessGalleryImport".into(), wireless_import.pinned());

    let wired_import = QObjectBox::new(wired_gallery_import::WiredGalleryImport::new_with_state());
    engine.set_object_property("wiredGalleryImportBackend".into(), wired_import.pinned());

    let backup_manager = QObjectBox::new(backup_manager::BackupManager::new_with_state());
    engine.set_object_property("backupManager".into(), backup_manager.pinned());

    #[cfg(not(target_os = "macos"))]
    let ifuse = QObjectBox::new(ifuse::IFuse::new_with_state());
    #[cfg(not(target_os = "macos"))]
    engine.set_object_property("iFuse".into(), ifuse.pinned());
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    let ifuse_manager = QObjectBox::new(ifuse_manager::IFuseManager::default());
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    engine.set_object_property("IFuseManager".into(), ifuse_manager.pinned());
    #[cfg(not(target_os = "macos"))]
    let diagnose = QObjectBox::new(diagnose::Diagnose::new_with_state());
    #[cfg(not(target_os = "macos"))]
    engine.set_object_property("DiagnoseImpl".into(), diagnose.pinned());

    let qml_utils = QObjectBox::new(qml_utils::QmlUtils::new(engine_ptr));
    engine.set_object_property("QmlUtils".into(), qml_utils.pinned());

    let status_window_controller =
        QObjectBox::new(status_window_controller::StatusWindowController::default());
    engine.set_object_property(
        "StatusWindowController".into(),
        status_window_controller.pinned(),
    );

    let network_device_provider = QObjectBox::new(
        network_device_provider::NetworkDeviceProvider::new(network_discovery_backend),
    );
    engine.set_object_property(
        "NetworkDeviceProvider".into(),
        network_device_provider.pinned(),
    );

    let system_appearance = QObjectBox::new(system_appearance::SystemAppearance::default());
    engine.set_object_property("SystemAppearance".into(), system_appearance.pinned());
    system_appearance.pinned().borrow_mut().initialize();

    native::initialize_engine(&mut engine);

    // FIXME: workaround to find FluentUI
    // in dev builds
    #[cfg(all(debug_assertions, target_os = "windows"))]
    native::add_development_import_path(&mut engine);

    let service_factory = QObjectBox::new(crate::service_factory::ServiceFactory::new(engine_ptr));
    engine.set_object_property("serviceFactory".into(), service_factory.pinned());

    let windows_qml_entry = "src/ui/platform/windows/Main.qml";
    let macos_qml_entry = "src/ui/platform/macos/Main.qml";
    let linux_qml_entry = "src/ui/ZLinuxWindow.qml";
    let default_qml_entry = "src/ui/Main.qml";

    let entry = if cfg!(target_os = "windows") {
        windows_qml_entry
    } else if cfg!(target_os = "macos") {
        macos_qml_entry
    } else if z_linux_window_enabled {
        linux_qml_entry
    } else {
        default_qml_entry
    };

    if ui_live_reload {
        let ui_path = QString::from(utils::source_qml_path("src/ui"));
        let entry_path = QString::from(utils::source_qml_path(entry));

        info!("QML live reload enabled: {}", entry_path.to_string());
        engine.load_file(entry_path.clone().into());

        native::initialize_live_reload(&mut engine, ui_path, entry_path);
    } else if qml_from_fs {
        let path = utils::deployed_qml_path(entry).unwrap_or_else(|| utils::source_qml_path(entry));
        info!("Loading QML from filesystem: {path}");
        engine.load_file(path.into());
    } else if let Some(path) = utils::deployed_qml_path(entry) {
        info!("Loading deployed QML from filesystem: {path}");
        engine.load_file(path.into());
    } else {
        info!("Loading QML from resources: qrc:/{entry}");
        engine.load_url(QString::from(format!("qrc:/{}", entry)).into());
    }

    engine.exec();

    qmetaobject::log::install_message_handler(None);

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    ifuse::shutdown_all_mounts(settings_manager::SettingsManager::unmount_ifuse_on_exit_enabled());
}
