// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::device_ctx::DeviceServices;
use crate::{RUNTIME, qt_threading::QtThreading, run_sync, utils};
use idevice::afc::opcode::AfcFopenMode;
use idevice::services::core_device_proxy::CoreDeviceProxy;
use idevice::{
    IdeviceService, RsdService, amfi,
    dvt::{location_simulation::LocationSimulationClient, remote_server::RemoteServerClient},
    installation_proxy::InstallationProxyClient,
    mobile_image_mounter::ImageMounter,
    provider::IdeviceProvider,
    rsd::RsdHandshake,
    simulate_location::LocationSimulationService,
};
use macros::QtThreading;
use plist::Value;
use qmetaobject::prelude::*;
use qttypes::{QStringList, QVariantMap};

use ::log::error;
use plist_macro::plist;
use serde_json;
use std::sync::Arc;
use std::{io::Read, time::Duration};
use tokio::sync::Mutex;

#[allow(non_snake_case)]
#[derive(Default, QObject, QtThreading)]
pub struct ServiceManager {
    base: qt_base_class!(trait QObject),

    get_cable_info: qt_method!(fn(&self)),
    reveal_developer_mode_option_in_ui: qt_method!(fn(&self)),
    query_mobilegestalt: qt_method!(fn(&self, keys: QStringList)),
    mount_dev_image: qt_method!(fn(&self, version: QString, image_path: QString, sig: QString)),
    get_mounted_image: qt_method!(fn(&self)),
    prepare_developer_image:
        qt_method!(fn(&self, selection: QString, directory: QString, revision: QString)),
    developerImageProgress: qt_signal!(stage: QString, progress: f64),
    developerImagePrepared: qt_signal!(success: bool, error: QString),
    developerImageStatus: qt_signal!(success: bool, mounted: bool, error: QString),
    check_developer_image: qt_method!(fn(&self)),
    fetch_installed_apps: qt_method!(fn(&self)),
    check_developer_mode_status: qt_method!(fn(&self)),
    set_location: qt_method!(fn(&self, latitude: QString, longitude: QString)),
    clear_location: qt_method!(fn(&self)),
    fetch_apps_disk_usage: qt_method!(fn(&self)),
    restart: qt_method!(fn(&self) -> bool),
    shutdown: qt_method!(fn(&self) -> bool),
    enter_recovery_mode: qt_method!(fn(&self) -> bool),
    unpair: qt_method!(fn(&self)),
    install_ipa: qt_method!(fn(&self, ipa_path: QString)),
    enable_wifi_connections: qt_method!(fn(&self)),
    get_battery_info: qt_method!(fn(&self, raw_product_type: QString, ios_major: u32)),

    // Signals
    cableInfoRetrieved: qt_signal!(info: QString),
    mobilegestaltInfoRetrieved: qt_signal!(info: QVariantMap),
    devImageMounted: qt_signal!(version: QString, success: bool, is_locked: bool),
    developerModeOptionRevealed: qt_signal!(success: bool),
    developerModeStatusChecked: qt_signal!(enabled: bool),
    mountedImageRetrieved: qt_signal!(
        success: bool,
        is_locked: bool,
        sig: QByteArray,
        sig_length: u64
    ),
    installedAppsRetrieved: qt_signal!(success : bool,apps: QVariantMap),
    batteryInfoUpdated: qt_signal!(info: QVariantMap),
    batteryInfoUpdateFailed: qt_signal!(error: QString),
    appsDiskUsageRetrieved: qt_signal!(success: bool, apps_usage: u64),
    installIpaInit: qt_signal!(started: bool, state: QString),
    installIpaProgress: qt_signal!(progress: f64, state: QString),
    enableWifiConnectionsResult: qt_signal!(success: bool),
    locationSimulationCompleted: qt_signal!(success: bool, code: i32, action: QString),
    unpairCompleted: qt_signal!(success: bool, error: QString),

    udid: String,
    ios_version: u32,
    device: Option<DeviceServices>,
}

impl ServiceManager {
    /* unwrap on self.device must
     *  be safe as from_device literally
     *  receives a device not an option */
    pub fn from_device(device: DeviceServices, udid: String, ios_version: u32) -> Self {
        let mut s = Self::default();
        s.device = Some(device);
        s.udid = udid;
        s.ios_version = ios_version;
        s
    }

    fn query_mobilegestalt(&self, keys: QStringList) {
        let udid = self.udid.clone();
        let qt_t = self.qt_thread();
        let keys_owned = keys.clone();
        let diag = self.device.as_ref().unwrap().clone().diag;
        RUNTIME.spawn(async move {
            let mut map = QVariantMap::default();

            let keys_vec: Vec<String> = keys_owned
                .into_iter()
                .map(|qstr| qstr.to_string())
                .collect();
            let qt_thread = qt_t.clone();
            let result = diag.lock().await.mobilegestalt(Some(keys_vec)).await;

            match result {
                Ok(opt_dict) => {
                    if let Some(mut root_dict) = opt_dict {
                         let mobilegestalt_value = root_dict.remove("MobileGestalt");

                         let inner_mobilegestalt_dict = match mobilegestalt_value {
                            Some(Value::Dictionary(dict)) => dict,
                            _ => {
                                eprintln!(
                                    "query_mobilegestalt: MobileGestalt key not found or not a dictionary for device {udid}."
                                );
                                let _ = qt_thread.queue(move |t| {
                                    t.mobilegestaltInfoRetrieved(map);
                                });
                                return;
                            }
                        };

                        for (k, v) in inner_mobilegestalt_dict.into_iter() {
                            let v_str = format!("{v:?}");
                            map.insert(QString::from(k), QVariant::from(&QString::from(v_str)));
                        }
                    }
                    let _ = qt_thread.queue(move |t| {
                        t.mobilegestaltInfoRetrieved(map);
                    });
                }
                Err(e) => {
                    eprintln!(
                        "query_mobilegestalt: error querying MobileGestalt for device {udid}: {e}"
                    );
                    let _ = qt_thread.queue(move |t| {
                        t.mobilegestaltInfoRetrieved(map);
                    });
                }
            }
        });
    }

    fn get_cable_info(&self) {
        let udid = self.udid.clone();
        let qt_t = self.qt_thread();

        let diag = self.device.as_ref().unwrap().clone().diag;
        RUNTIME.spawn(async move {
            let qt_thread = qt_t.clone();
            let res = utils::get_cable_info(&mut *diag.lock().await).await;

            match res {
                Some(dict) => {
                    //FIXME: return a qvariantmap instead
                    let val = Value::Dictionary(dict);
                    let res = serde_json::to_string_pretty(&val);
                    if res.is_err() {
                        eprintln!(
                            "get_cable_info: Failed to serialize ioregistry values to XML for device {udid}."
                        );
                        let _ = qt_thread.queue(|t| {
                            t.cableInfoRetrieved(QString::from(""));
                        });
                        return;
                    }

                    let _ = qt_thread.queue(move |t| {
                        t.cableInfoRetrieved(QString::from(res.unwrap()));
                    });

                }
                None => {
                    eprintln!("get_cable_info: Failed to get ioregistry for device {udid}");
                    let _ = qt_thread.queue(|t| {
                        t.cableInfoRetrieved(QString::from(""));
                    });
                }
            }
        });
    }
    fn mount_dev_image(&self, version: QString, image_path: QString, sig: QString) {
        let udid = self.udid.clone();
        let qt_t = self.qt_thread();
        let image = image_path.to_string();
        let signature = sig.to_string();

        let provider_guard = self.device.as_ref().unwrap().clone().provider;
        RUNTIME.spawn(async move {
            let qt_thread = qt_t.clone();
            let provider = provider_guard.lock().await;

            let mut mounter = match {
                let provider_ref: &dyn IdeviceProvider = provider.as_ref();
                ImageMounter::connect(provider_ref).await
            } {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("mount_dev_image: Failed to connect to ImageMounter for device {udid}: {e}");
                    let _ = qt_thread.queue(|t| {
                        t.devImageMounted(version, false, false);
                    });
                    return;
                }
            };

            let mut file = match std::fs::File::open(&image) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("mount_dev_image: Failed to open image file {image} for device {udid}: {e}");
                    let _ = qt_thread.queue(|t| {
                        t.devImageMounted(version, false, false);
                    });
                    return;
                }
            };
            let mut buf = Vec::new();
            if let Err(e) = file.read_to_end(&mut buf) {
                eprintln!("mount_dev_image: Failed to read image file {image} for device {udid}: {e}");
                let _ = qt_thread.queue(|t| {
                    t.devImageMounted(version, false, false);
                });
                return;
            }

            let mut sig_file = match std::fs::File::open(&signature) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("mount_dev_image: Failed to open signature file {signature} for device {udid}: {e}");
                    let _ = qt_thread.queue(|t| {
                        t.devImageMounted(version,false,false);
                    });
                    return;
                }
            };

            let mut sig_buf: Vec<u8> = Vec::new();
            if let Err(e) = sig_file.read_to_end(&mut sig_buf) {
                eprintln!("mount_dev_image: Failed to read signature file {signature} for device {udid}: {e}");
                let _ = qt_thread.queue(|t| {
                    t.devImageMounted(version, false,false);
                });
                return;
            }

            match mounter.mount_developer(&buf, sig_buf).await {
                Ok(_) => {
                    let _ = qt_thread.queue(|t| {
                        t.devImageMounted(version, true ,false);
                    });
                }
                Err(idevice::IdeviceError::DeviceLocked) => {
                    eprintln!("mount_dev_image: Failed to mount developer image for device {udid}: device locked");
                    qt_thread.queue(|t| {
                        t.devImageMounted(version, false, true);
                    });
                }
                Err(e) => {
                    eprintln!("mount_dev_image: Failed to mount developer image for device {udid}: {e}");
                    qt_thread.queue(|t| {
                        t.devImageMounted(version, false, false);
                    });
                }
            };

        });
    }

    fn check_developer_image(&self) {
        let udid = self.udid.clone();
        let device = self.device.as_ref().unwrap().clone();
        let major = device.ios_version.major;
        let minor = device.ios_version.minor;
        let qt_thread = self.qt_thread();
        RUNTIME.spawn(async move {
            let result = crate::developer_images::mounted(&device, &udid, major, minor).await;
            let (success, mounted, error) = match result {
                Ok(mounted) => (true, mounted, QString::default()),
                Err(error) => (false, false, QString::from(format!("{error:#}"))),
            };
            qt_thread.queue(move |manager| manager.developerImageStatus(success, mounted, error));
        });
    }

    fn prepare_developer_image(&self, selection: QString, directory: QString, revision: QString) {
        let udid = self.udid.clone();
        let qt_thread = self.qt_thread();
        RUNTIME.spawn(async move {
            let progress_thread = qt_thread.clone();
            let result = crate::developer_images::prepare(
                &udid,
                &selection.to_string(),
                std::path::Path::new(&directory.to_string()),
                &revision.to_string(),
                &move |stage, progress| {
                    let stage = QString::from(stage);
                    progress_thread
                        .queue(move |manager| manager.developerImageProgress(stage, progress));
                },
            )
            .await;
            let (success, error) = match result {
                Ok(()) => (true, QString::default()),
                Err(error) => {
                    log::error!("Developer image preparation failed: {error:#}");
                    (false, QString::from(format!("{error:#}")))
                }
            };
            qt_thread.queue(move |manager| manager.developerImagePrepared(success, error));
        });
    }

    fn get_mounted_image(&self) {
        let udid = self.udid.clone();
        let qt_t = self.qt_thread();

        let provider_guard = self.device.as_ref().unwrap().clone().provider;

        RUNTIME.spawn(async move {
            let qt_thread = qt_t.clone();

            let mut mounter = match {
                let provider = provider_guard.lock().await;

                let provider_ref: &dyn IdeviceProvider = provider.as_ref();
                ImageMounter::connect(provider_ref).await
            } {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("get_mounted_image: Failed to connect to ImageMounter for device {udid}: {e}");
                    qt_thread.queue(|t| {
                        t.mountedImageRetrieved(false,false,QByteArray::default(), 0);
                    });
                    return;
                }
            };

            match mounter.lookup_image("Developer").await {
                Ok(res) => {
                    qt_thread.queue(move|t| {
                        t.mountedImageRetrieved(true,false,QByteArray::from(&res[..]), res.len() as u64);
                    });
                }
                Err(idevice::IdeviceError::DeviceLocked) => {
                    eprintln!("get_mounted_image: Failed to lookup mounted developer image for device {udid}: device locked");
                    qt_thread.queue(|t| {
                        t.mountedImageRetrieved(false,true,QByteArray::default(), 0);
                    });
                }
                Err(idevice::IdeviceError::NotFound) => {
                    eprintln!("get_mounted_image: No mounted developer image found for device {udid}");
                    let _ = qt_thread.queue(|t| {
                        t.mountedImageRetrieved(true,false,QByteArray::default(), 0);
                    });
                }
                Err(e) => {
                    eprintln!("get_mounted_image: Failed to lookup mounted developer image for device {udid}: {e}");
                    let _ = qt_thread.queue(|t| {
                        t.mountedImageRetrieved(false,false,QByteArray::default(), 0);
                    });
                }
            };

        });
    }

    fn reveal_developer_mode_option_in_ui(&self) {
        let udid = self.udid.clone();
        let qt_t = self.qt_thread();

        let provider_guard = self.device.as_ref().unwrap().clone().provider;

        RUNTIME.spawn(async move {
            let qt_thread = qt_t.clone();

            let mut amfi_client = match {
                let provider_guard = provider_guard.lock().await;
                let provider_ref: &dyn IdeviceProvider = provider_guard.as_ref();
                amfi::AmfiClient::connect(provider_ref).await
            } {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("reveal_developer_mode_option_in_ui: Failed to connect to AMFI service for device {udid}: {e}");
                    let _ = qt_thread.queue(|t| {
                        t.developerModeOptionRevealed(false);
                    });
                    return;
                }
            };


            match amfi_client.reveal_developer_mode_option_in_ui().await {
                Ok(_) => {
                    let _ = qt_thread.queue(|t| {
                        t.developerModeOptionRevealed(true);
                    });
                }
                Err(e) => {
                    eprintln!("reveal_developer_mode_option_in_ui: Failed to reveal developer mode option in UI for device {udid}: {e}");
                    let _ = qt_thread.queue(|t| {
                        t.developerModeOptionRevealed(false);
                    });
            }

            }
        });
    }

    fn fetch_installed_apps(&self) {
        let udid = self.udid.clone();
        let qt_t = self.qt_thread();

        let provider_guard = self.device.as_ref().unwrap().clone().provider;

        RUNTIME.spawn(async move {
            let qt_thread = qt_t.clone();

            let mut all_apps = QVariantMap::default();

            let mut ins_client = match {
                let provider_guard = provider_guard.lock().await;
                let provider_ref: &dyn IdeviceProvider = provider_guard.as_ref();
                InstallationProxyClient::connect(provider_ref).await
            } {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("fetch_installed_apps: Failed to connect to InstallationProxy service for device {udid}: {e}");
                    qt_thread.queue( move |t| {
                        t.installedAppsRetrieved(false, all_apps);
                    });
                    return;
                }
            };

            // Get both User and System apps
            for app_type in ["User", "System"] {
                let client_options = plist!({
                    "ApplicationType": app_type,
                    "ReturnAttributes": [
                        "CFBundleIdentifier",
                        "CFBundleDisplayName",
                        "CFBundleShortVersionString",
                        "CFBundleVersion",
                        "UIFileSharingEnabled"
                    ]
                });

                let apps = match ins_client.browse(Some(client_options)).await {
                    Ok(apps) => apps,
                    Err(e) => {
                        eprintln!("fetch_installed_apps: Failed to browse installed apps for device {udid} and app type {app_type}: {e}");
                        continue;
                    }
                };

                for app_info in apps {
                    if let plist::Value::Dictionary(app_dict) = app_info {
                        let bundle_id = app_dict
                            .get("CFBundleIdentifier")
                            .and_then(|v| v.as_string())
                            .unwrap_or_default();

                        if bundle_id.is_empty() {
                            continue;
                        }

                        let display = app_dict
                            .get("CFBundleDisplayName")
                            .and_then(|v| v.as_string())
                            .unwrap_or_default();
                        let version = app_dict
                            .get("CFBundleShortVersionString")
                            .and_then(|v| v.as_string())
                            .unwrap_or_default();
                        let fs_enabled = app_dict
                            .get("UIFileSharingEnabled")
                            .and_then(|v| v.as_boolean())
                            .unwrap_or(false);

                        let app_json = format!(
                            "{{\"bundle_id\":{},\"CFBundleDisplayName\":{},\"CFBundleShortVersionString\":{},\"UIFileSharingEnabled\":{},\"app_type\":{}}}",
                            serde_json::to_string(&bundle_id).unwrap_or_else(|_| format!("\"{}\"", bundle_id)),
                            serde_json::to_string(&display).unwrap_or_else(|_| format!("\"{}\"", display)),
                            serde_json::to_string(&version).unwrap_or_else(|_| format!("\"{}\"", version)),
                            fs_enabled,
                            serde_json::to_string(&app_type).unwrap_or_else(|_| format!("\"{}\"", app_type)),
                        );

                        all_apps.insert(
                            QString::from(bundle_id),
                            QVariant::from(&QString::from(app_json)),
                        );
                    }
                }
            }

            qt_thread.queue(move |t| {
                t.installedAppsRetrieved(true, all_apps);
            });
        });
    }

    fn set_location(&self, latitude: QString, longitude: QString) {
        let udid = self.udid.clone();
        let ios_version = self.ios_version;
        let qt_t = self.qt_thread();

        let provider_guard = self.device.as_ref().unwrap().clone().provider;
        RUNTIME.spawn(async move {
            let qt_thread = qt_t.clone();
            let action = QString::from("set");
            let latitude_str = latitude.to_string();
            let longitude_str = longitude.to_string();
            let result = tokio::time::timeout(
                Duration::from_secs(10),
                set_device_location_for_version(
                    provider_guard,
                    ios_version,
                    udid.clone(),
                    latitude_str,
                    longitude_str,
                ),
            )
            .await;

            let code = match result {
                Ok(Ok(_)) => 0,
                Ok(Err(e)) => {
                    eprintln!(
                        "set_location: failed to set virtual location for device {udid}: {e:?}"
                    );
                    e.code()
                }
                Err(_) => {
                    eprintln!("set_location: timed out");
                    idevice::IdeviceError::Timeout.code()
                }
            };

            qt_thread.queue(move |t| {
                t.locationSimulationCompleted(code == 0, code, action);
            });
        });
    }

    fn clear_location(&self) {
        let udid = self.udid.clone();
        let ios_version = self.ios_version;
        let qt_t = self.qt_thread();
        let provider_guard = self.device.as_ref().unwrap().clone().provider;

        RUNTIME.spawn(async move {
            let qt_thread = qt_t.clone();
            let action = QString::from("clear");
            let result = tokio::time::timeout(
                Duration::from_secs(10),
                clear_device_location_for_version(provider_guard, ios_version, udid.clone()),
            )
            .await;

            let code = match result {
                Ok(Ok(_)) => 0,
                Ok(Err(e)) => {
                    eprintln!(
                        "clear_location: failed to clear virtual location for device {udid}: {e:?}"
                    );
                    e.code()
                }
                Err(_) => {
                    eprintln!("clear_location: timed out");
                    idevice::IdeviceError::Timeout.code()
                }
            };

            qt_thread.queue(move |t| {
                t.locationSimulationCompleted(code == 0, code, action);
            });
        });
    }

    fn fetch_apps_disk_usage(&self) {
        let udid = self.udid.clone();
        let qt_t = self.qt_thread();

        let provider_guard = self.device.as_ref().unwrap().clone().provider;

        RUNTIME.spawn(async move {
            let qt_thread = qt_t.clone();

            let mut instproxy = {
                let provider_guard = provider_guard.lock().await;

                match InstallationProxyClient::connect(provider_guard.as_ref()).await {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("fetch_apps_disk_usage: Failed to connect to InstallationProxy service for device {udid}: {e}");
                        qt_thread.queue(move |t| {
                            t.appsDiskUsageRetrieved(false, 0);
                        });
                        return;
                    }
                }
            };

            match utils::calculate_apps_usage(&mut instproxy).await {
                Ok(apps_usage) => {
                    qt_thread.queue(move |t| {
                        t.appsDiskUsageRetrieved(true, apps_usage);
                    });
                }
                Err(e) => {
                    eprintln!("fetch_apps_disk_usage: Failed to calculate apps disk usage for device {udid}: {e}");
                    qt_thread.queue(move |t| {
                        t.appsDiskUsageRetrieved(false, 0);
                    });
                }
            };


        });
    }

    fn restart(&self) -> bool {
        let udid = self.udid.clone();

        let diag_guard = self.device.as_ref().unwrap().clone().diag;

        run_sync(async move {
            let mut diag = diag_guard.lock().await;

            if let Err(e) = diag.restart().await {
                eprintln!("restart: Failed to restart device {udid}: {e}");
                return false;
            }
            return true;
        })
    }

    fn shutdown(&self) -> bool {
        let udid = self.udid.clone();

        let diag_guard = self.device.as_ref().unwrap().clone().diag;
        run_sync(async move {
            let mut diag = diag_guard.lock().await;

            if let Err(e) = diag.shutdown().await {
                eprintln!("shutdown: Failed to shutdown device {udid}: {e}");
                return false;
            }
            return true;
        })
    }
    fn enter_recovery_mode(&self) -> bool {
        let udid = self.udid.clone();

        let lc_guard = self.device.as_ref().unwrap().clone().lockdown;
        run_sync(async move {
            let mut lc = lc_guard.lock().await;

            if let Err(e) = lc.enter_recovery().await {
                eprintln!(
                    "enter_recovery_mode: Failed to enter recovery mode for device {udid}: {e}"
                );
                return false;
            }
            return true;
        })
    }

    fn unpair(&self) {
        let udid = self.udid.clone();
        let device = self.device.as_ref().unwrap().clone();
        let q_thread = self.qt_thread();

        RUNTIME.spawn(async move {
            let result: anyhow::Result<()> = async {
                let pairing_file = {
                    let provider = device.provider.lock().await;
                    provider.get_pairing_file().await?
                };

                {
                    let mut lockdown = device.lockdown.lock().await;
                    lockdown.unpair(pairing_file.host_id).await?;
                }

                // The device-side trust relationship is authoritative. Removing the
                // usbmuxd record is best-effort because custom wireless providers may
                // not have a corresponding record in the local usbmuxd instance.
                match idevice::usbmuxd::UsbmuxdConnection::default().await {
                    Ok(mut usbmuxd) => {
                        if let Err(error) = usbmuxd.delete_pair_record(&udid).await {
                            log::warn!(
                                "unpair: device {udid} was unpaired, but its host pairing record could not be deleted: {error}"
                            );
                        }
                    }
                    Err(error) => {
                        log::warn!(
                            "unpair: device {udid} was unpaired, but usbmuxd was unavailable for host-record cleanup: {error}"
                        );
                    }
                }

                log::info!("Unpaired device {udid}");
                Ok(())
            }
            .await;

            match result {
                Ok(()) => q_thread.queue(|t| {
                    t.unpairCompleted(true, QString::default());
                }),
                Err(error) => {
                    log::error!("unpair: Failed to unpair device {udid}: {error}");
                    let error = QString::from(error.to_string());
                    q_thread.queue(move |t| {
                        t.unpairCompleted(false, error);
                    });
                }
            }
        });
    }

    fn install_ipa(&self, local_ipa_path: QString) {
        let udid = self.udid.clone();
        let qt_t = self.qt_thread();
        let local_ipa_path_owned = local_ipa_path.clone().to_string();
        // FIXME: this is a bit hacky
        let ipa_path_on_device = format!(
            "/PublicStaging/{}",
            local_ipa_path
                .to_string()
                .split('/')
                .last()
                .unwrap_or("app.ipa")
        );
        let cloned = self.device.as_ref().unwrap().clone();
        let afc_guard = cloned.afc;
        let provider_guard = cloned.provider;

        RUNTIME.spawn(async move {
            let qt_thread = qt_t.clone();

            let mut ins_client = match {

                let mut afc = afc_guard.lock().await;

                // Create the staging directory
                match utils::ensure_public_staging(&mut afc).await {
                    Ok(_) => (),
                    Err(e) => {
                        eprintln!("install_ipa: Failed to ensure /PublicStaging directory exists on device {udid}: {e}");
                        qt_thread.queue(move |t| {
                            t.installIpaInit(false, QString::from("Failed to prepare device for IPA upload"));
                        });
                        return;
                    }
                };

                match std::fs::exists(&local_ipa_path_owned)  {
                    Ok(true) => (),
                    Ok(false) => {
                        eprintln!("install_ipa: IPA file not found at path {local_ipa_path_owned}");
                        qt_thread.queue(move |t| {
                            t.installIpaInit(false, QString::from("IPA file not found"));
                        });
                        return;
                    }
                    Err(e) => {
                        eprintln!("install_ipa: Failed to check if IPA file exists at path {local_ipa_path_owned}: {e}");
                        qt_thread.queue(move |t| {
                            t.installIpaInit(false, QString::from("Failed to access IPA file"));
                        });
                        return;
                    }
                }



                match afc.open(&ipa_path_on_device, AfcFopenMode::WrOnly).await {
                    Ok(mut fd) => {
                        let mut local_file = match std::fs::File::open(&local_ipa_path_owned) {
                            Ok(f) => f,
                            Err(e) => {
                                eprintln!("install_ipa: Failed to open local IPA file for device {udid}: {e}");
                                qt_thread.queue(move |t| {
                                    t.installIpaInit(false, QString::from("Failed to open local IPA file"));
                                });
                                return;
                            }
                        };

                        let mut file_btytes = Vec::new();
                        match local_file.read_to_end(&mut file_btytes) {
                            Ok(bytes) => bytes,
                            Err(e) => {
                                eprintln!("install_ipa: Failed to read local IPA file for device {udid}: {e}");
                                qt_thread.queue(move |t| {
                                    t.installIpaInit(false, QString::from("Failed to read local IPA file"));
                                });
                                return;
                            }
                        };

                        if let Err(e) = fd.write_entire(&file_btytes).await {
                            eprintln!("install_ipa: Failed to upload IPA to device {udid}: {e}");
                            qt_thread.queue(move |t| {
                                t.installIpaInit(false, QString::from("Failed to upload IPA to device"));
                            });
                            return;
                        }
                    }
                    Err(e) => {
                        eprintln!("install_ipa: Failed to create file on device {udid} for IPA upload: {e}");
                        qt_thread.queue(move |t| {
                            t.installIpaInit(false, QString::from("Failed to create file on device for IPA upload"));
                        });
                        return;
                    }
                }


                let provider_guard = provider_guard.lock().await;
                let provider_ref: &dyn IdeviceProvider = provider_guard.as_ref();
                InstallationProxyClient::connect(provider_ref).await
            } {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("install_ipa: Failed to connect to InstallationProxy service for device {udid}: {e}");
                    qt_thread.queue(move |t| {
                        t.installIpaInit(false, QString::from("Failed to connect to Installation Proxy"));
                    });
                    return;
                }
            };

            qt_thread.queue(move |t| {
                t.installIpaInit(true, QString::from("Connected to Installation Proxy"));
            });

            let state = String::from("Installing IPA");

            let res = ins_client
                .install_with_callback(
                    ipa_path_on_device,
                    None,
                    move |(percent, state)| {
                        let qt_thread = qt_thread.clone();
                        async move {
                            let progress = percent as f64 / 100.0;

                            qt_thread
                                .queue(move |t| {
                                    t.installIpaProgress(
                                        progress,
                                        QString::from(state),
                                    );
                                });

                            println!(
                                "Installation progress: {percent}%"
                            );
                        }
                    },
                    state,
                )
                .await;

            if let Err(e) = res {
                eprintln!("install_ipa: Failed to install IPA on device {udid}: {e}");
            } else {
                println!("install_ipa: Successfully initiated installation on device {udid}");
            }
        });
    }

    fn enable_wifi_connections(&self) {
        let qt_t = self.qt_thread();
        let udid = self.udid.clone();

        let lc_guard = self.device.as_ref().unwrap().clone().lockdown;
        RUNTIME.spawn(async move {
            let qt_thread = qt_t.clone();

            let mut lc = lc_guard.lock().await;

            let value = Value::Boolean(true);
            match lc
                .set_value(
                    "EnableWifiConnections",
                    value,
                    Some("com.apple.mobile.wireless_lockdown"),
                )
                .await
            {
                Ok(_) => {
                    let _ = qt_thread.queue(|t| {
                        t.enableWifiConnectionsResult(true);
                    });
                }
                Err(e) => {
                    eprintln!("wireless: LockdownClient::set_value failed: {e:?} udid: {udid}");
                    let _ = qt_thread.queue(|t| {
                        t.enableWifiConnectionsResult(false);
                    });
                }
            }
        });
    }
    // for iOS 17+
    fn check_developer_mode_status(&self) {
        let lc_guard = self.device.as_ref().unwrap().clone().lockdown;
        let udid = self.udid.clone();
        let qt_t = self.qt_thread();

        RUNTIME.spawn(async move {
            let qt_thread = qt_t.clone();
            let mut lc = lc_guard.lock().await;
            let developer_mode_status = match lc
                .get_value(
                    Some("DeveloperModeStatus"),
                    Some("com.apple.security.mac.amfi"),
                )
                .await
            {
                Ok(Value::Boolean(b)) => b,
                other => {
                    eprintln!(
                        "check_developer_mode_status: failed to read DeveloperModeStatus for device {udid}: {other:?}"
                    );
                    false
                }
            };
            qt_thread.queue(move |t| {
                t.developerModeStatusChecked(developer_mode_status);
            });
        });
    }

    fn get_battery_info(&self, raw_product_type: QString, ios_major: u32) {
        let udid = self.udid.clone();
        let qt_t = self.qt_thread();

        let diag_guard = self.device.as_ref().unwrap().clone().diag;

        RUNTIME.spawn(async move {
            let qt_thread = qt_t.clone();
            let mut diag = diag_guard.lock().await;
            let mut info = QVariantMap::default();
            match crate::core::insert_battery_info(
                &mut diag,
                &mut info,
                raw_product_type.to_string(),
                ios_major,
            )
            .await
            {
                Ok(_) => {
                    qt_thread.queue(move |t| {
                        t.batteryInfoUpdated(info);
                    });
                }
                Err(e) => {
                    error!("get_battery_info: Failed to get battery info for device {udid}: {e}");
                    let error = QString::from(e.to_string());
                    qt_thread.queue(move |t| {
                        t.batteryInfoUpdateFailed(error);
                    });
                }
            }
        });
    }
}

async fn set_device_location_lockdown(
    provider: &dyn IdeviceProvider,
    latitude: &str,
    longitude: &str,
) -> Result<(), idevice::IdeviceError> {
    let mut client = LocationSimulationService::connect(provider).await?;
    client.set(latitude, longitude).await
}

async fn clear_device_location_lockdown(
    provider: &dyn IdeviceProvider,
) -> Result<(), idevice::IdeviceError> {
    let mut client = LocationSimulationService::connect(provider).await?;
    client.clear().await
}

async fn set_device_location_for_version(
    provider_guard: Arc<Mutex<Box<dyn IdeviceProvider>>>,
    ios_version: u32,
    udid: String,
    latitude: String,
    longitude: String,
) -> Result<(), idevice::IdeviceError> {
    let provider_guard = provider_guard.lock().await;

    if ios_version < 17 {
        return set_device_location_lockdown(
            provider_guard.as_ref(),
            latitude.as_str(),
            longitude.as_str(),
        )
        .await;
    }

    println!("Using RSD path for setting location on device {udid} with iOS version {ios_version}");
    let proxy = CoreDeviceProxy::connect(provider_guard.as_ref()).await?;
    set_device_location_rsd(
        proxy,
        latitude.parse::<f64>().unwrap_or(0.0),
        longitude.parse::<f64>().unwrap_or(0.0),
    )
    .await
}

async fn clear_device_location_for_version(
    provider_guard: Arc<Mutex<Box<dyn IdeviceProvider>>>,
    ios_version: u32,
    udid: String,
) -> Result<(), idevice::IdeviceError> {
    let provider_guard = provider_guard.lock().await;

    if ios_version < 17 {
        return clear_device_location_lockdown(provider_guard.as_ref()).await;
    }

    println!(
        "Using RSD path for clearing location on device {udid} with iOS version {ios_version}"
    );
    let proxy = CoreDeviceProxy::connect(provider_guard.as_ref()).await?;
    clear_device_location_rsd(proxy).await
}

// iOS 17+:
async fn set_device_location_rsd(
    proxy: CoreDeviceProxy,
    latitude: f64,
    longitude: f64,
) -> Result<(), idevice::IdeviceError> {
    let rsd_port = proxy.tunnel_info().server_rsd_port;
    let adapter = proxy.create_software_tunnel()?;
    let mut adapter = adapter.to_async_handle();
    let stream = adapter.connect(rsd_port).await?;

    let mut handshake = RsdHandshake::new(stream).await?;

    let mut remote_server = RemoteServerClient::connect_rsd(&mut adapter, &mut handshake).await?;
    remote_server.read_message(0).await?;

    let mut location_client = LocationSimulationClient::new(&mut remote_server).await?;
    location_client.set(latitude, longitude).await
}

// iOS 17+:
async fn clear_device_location_rsd(proxy: CoreDeviceProxy) -> Result<(), idevice::IdeviceError> {
    let rsd_port = proxy.tunnel_info().server_rsd_port;
    let adapter = proxy.create_software_tunnel()?;
    let mut adapter = adapter.to_async_handle();
    let stream = adapter.connect(rsd_port).await?;

    let mut handshake = RsdHandshake::new(stream).await?;

    let mut remote_server = RemoteServerClient::connect_rsd(&mut adapter, &mut handshake).await?;
    remote_server.read_message(0).await?;

    let mut location_client = LocationSimulationClient::new(&mut remote_server).await?;
    location_client.clear().await
}

impl Drop for ServiceManager {
    fn drop(&mut self) {
        println!("Service manager dropped")
    }
}
