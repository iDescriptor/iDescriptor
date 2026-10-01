// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

use anyhow::Context;
use futures::StreamExt;
use idevice::{
    IdeviceError, IdeviceService,
    afc::AfcClient,
    diagnostics_relay::DiagnosticsRelayClient,
    heartbeat,
    lockdown::LockdownClient,
    pairing_file::PairingFile,
    provider::TcpProvider,
    usbmuxd::{Connection, UsbmuxdAddr, UsbmuxdConnection, UsbmuxdListenEvent},
};
use qmetaobject::{qt_base_class, qt_method};
use qttypes::QVariantMap;

use ::log::{debug, error, info, trace, warn};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;
use std::{any::type_name, sync::Arc};
use std::{collections::HashMap, net::IpAddr};
use tokio::runtime::Builder;
use tokio::sync::Mutex;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::device_ctx::{
    DeviceServices, InsertDeviceResult, clean_device_from_app_state_if_current, iOSVersion,
    insert_device,
};
use crate::{
    APP_LABEL, EV_CONNECTED, EV_DISCONNECTED, EV_FAIL, EV_PAIRING_PENDING, RUNTIME,
    qt_threading::{QtThread, QtThreading},
    utils,
};
use crate::{device_db, qvariantmap_insert, run_sync};
use macros::QtThreading;
use plist::{Dictionary, Value};
use qmetaobject::prelude::*;

const WIRELESS_INIT_TIMEOUT: Duration = Duration::from_secs(20);
static NEXT_CONNECTION_ID: AtomicU64 = AtomicU64::new(1);

fn next_connection_id() -> u64 {
    NEXT_CONNECTION_ID.fetch_add(1, Ordering::Relaxed)
}

fn connection_event_info(connection_id: u64) -> QVariantMap {
    let mut info = QVariantMap::default();
    qvariantmap_insert!(info, "connection_id", connection_id);
    info
}

struct InitializedDevice {
    udid: String,
    info: QVariantMap,
    heartbeat_start: Option<oneshot::Sender<()>>,
}

#[derive(Debug)]
enum WirelessInitError {
    InvalidMac { mac: String },
    NoPairingRecords,
    NoMatchingPairingRecord,
    Transport(anyhow::Error),
}

impl std::fmt::Display for WirelessInitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMac { mac } => write!(f, "invalid wireless device MAC address: {mac}"),
            Self::NoPairingRecords => write!(f, "no pairing records found"),
            Self::NoMatchingPairingRecord => {
                write!(
                    f,
                    "no pairing record authenticated the requested wireless device"
                )
            }
            Self::Transport(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for WirelessInitError {}

fn emit_initialized_device(qt_thread: QtThread<Core>, initialized: InitializedDevice) {
    let InitializedDevice {
        udid,
        info,
        heartbeat_start,
    } = initialized;
    qt_thread.queue(move |core_qobj| {
        core_qobj.deviceEvent(EV_CONNECTED, QString::from(udid), info);
    });
    if let Some(start) = heartbeat_start {
        let _ = start.send(());
    }
}

#[allow(non_snake_case)]
#[derive(Default, QObject, QtThreading)]
pub struct Core {
    base: qt_base_class!(trait QObject),
    init: qt_method!(fn(&mut self)),
    init_wireless_device: qt_method!(fn(&mut self, ip: QString, mac_address: QString)),
    init_wireless_device_custom: qt_method!(fn(&mut self, ip: QString, pairing_file: QString)),
    exit_recovery_mode: qt_method!(fn(&mut self, ecid: QString) -> bool),
    // mac address will only be available if the pairing file was read successfully, otherwise it will be empty
    customInitFailed: qt_signal!(ip: QString, mac_address: QString, error: QString),
    remove_device: qt_method!(fn(&mut self, udid: QString, connection_id: u64)),
    deviceEvent: qt_signal!(eventType : u32, udid : QString , info : QVariantMap),
    recoveryDeviceEvent: qt_signal!(eventType : u32, id : QString , info : QVariantMap),
    initFailed: qt_signal!(mac_address : QString),
    noPairingFile: qt_signal!(mac_address : QString),
    sleepyTimeDetected: qt_signal!(),
    is_init: bool,
}

impl Core {
    fn init(&mut self) {
        if self.is_init {
            debug!("Core is already initialized");
            return;
        }
        self.is_init = true;
        self.listen();
    }

    fn listen(&mut self) {
        let qt_t = self.qt_thread();
        let qt_t_recovery = qt_t.clone();

        thread::spawn(move || {
            let rt = Builder::new_current_thread().enable_all().build().unwrap();
            rt.block_on(async move {
                let mut device_map: HashMap<u32, (String, u64, JoinHandle<()>)> =
                    HashMap::new();

                loop {
                    match UsbmuxdConnection::default().await {
                        Ok(mut uc) => match uc.listen().await {
                            Ok(mut stream) => {
                                while let Some(evt) = stream.next().await {
                                    match evt {
                                        Ok(UsbmuxdListenEvent::Connected(d)) => {
                                            // ignore non-USB connections
                                            if d.connection_type != Connection::Usb {
                                                continue;
                                            }

                                            let udid = d.udid.clone();
                                            let device_id = d.device_id;
                                            let connection_id = next_connection_id();

                                            let qt_thread = qt_t.clone();
                                            let init_task = RUNTIME.spawn(async move {
                                                let pair_record_exists = {
                                                    let mut u2 =
                                                        match UsbmuxdConnection::default().await {
                                                            Ok(u) => u,
                                                            Err(_) => return,
                                                        };

                                                    match u2.get_pair_record(&udid).await {
                                                        Ok(_) => true,
                                                        Err(_) => false,
                                                    }
                                                };


                                                // we may not even need to check if pair record exists
                                                if pair_record_exists {
                                                    emit_connected(
                                                        qt_thread.clone(),
                                                        udid,
                                                        connection_id,
                                                    )
                                                    .await;
                                                    return;
                                                }


                                                match handle_pairing(qt_thread.clone(), udid.clone()).await {
                                                    Ok(_) => {
                                                        emit_connected(
                                                            qt_thread.clone(),
                                                            udid,
                                                            connection_id,
                                                        )
                                                        .await;
                                                    }
                                                    Err(e) => {
                                                        error!("Pairing failed for device {}: {e:?}", udid);
                                                        emit_pairing_failed(qt_thread.clone(), udid, "Failed to pair device");
                                                    }
                                                }

                                            });
                                            if let Some((_, _, old_init_task)) = device_map.insert(
                                                device_id,
                                                (d.udid, connection_id, init_task),
                                            ) {
                                                old_init_task.abort();
                                            }
                                        }
                                        /* DISCONNECTED */
                                        Ok(UsbmuxdListenEvent::Disconnected(device_id)) => {
                                            if let Some((udid, connection_id, init_task)) =
                                                device_map.remove(&device_id)
                                            {
                                                init_task.abort();
                                                if clean_device_from_app_state_if_current(
                                                    &udid,
                                                    connection_id,
                                                    true,
                                                )
                                                .await
                                                {
                                                    let qt_thread = qt_t.clone();
                                                    qt_thread.queue(move |core_qobj| {
                                                        core_qobj.deviceEvent(
                                                            EV_DISCONNECTED,
                                                            QString::from(udid),
                                                            connection_event_info(connection_id),
                                                        );
                                                    });
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            error!("usbmuxd listen error: {e:?}");
                                            break;
                                        }
                                    }
                                }
                            }
                            Err(e) => error!("Failed to start usbmuxd listen: {e:?}"),
                        },
                        Err(_) => {}
                    }

                    tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
                }
            });
        });

        RUNTIME.spawn(async move {
            let resolver = device_db::IRecoveryMetadataResolver;
            let mut events = match irecovery::watch_recovery_devices_with_metadata(&resolver).await {
                Ok(events) => Box::pin(events),
                Err(err) => {
                    error!("failed to watch recovery devices: {err}");
                    return;
                }
            };

            while let Some(event) = events.as_mut().next().await {
                match event {
                    Ok(irecovery::RecoveryEvent::Connected(device)) => {
                        let key = recovery_device_key(&device.id);
                        let info = collect_recovery_device_info(&device);
                        debug!(
                            "recovery device connected: id={:?} mode={:?} ecid={:?} model={} name={}",
                            device.id,
                            device.mode,
                            device.ecid,
                            device.hardware_model().unwrap_or("unknown"),
                            device.display_name(),
                        );
                        qt_t_recovery.queue(move |core_qobj| {
                            core_qobj.recoveryDeviceEvent(EV_CONNECTED, QString::from(key), info);
                        });
                    }
                    Ok(irecovery::RecoveryEvent::Disconnected(id)) => {
                        debug!("recovery device disconnected: id={id:?}");
                        qt_t_recovery.queue(move |core_qobj| {
                            core_qobj.recoveryDeviceEvent(
                                EV_DISCONNECTED,
                                QString::from(recovery_device_key(&id)),
                                QVariantMap::default(),
                            );
                        });
                    }
                    Err(err) => {
                        error!("recovery device watch error: {err}");
                    }
                }
            }
        });
    }

    fn exit_recovery_mode(&mut self, ecid: QString) -> bool {
        let ecid = ecid.to_string();
        run_sync(async move {
            let Some(ecid) = parse_recovery_ecid(&ecid) else {
                debug!("invalid recovery ECID: {ecid}");
                return false;
            };

            match irecovery::set_auto_boot_and_reboot(ecid, 3).await {
                Ok(()) => {
                    debug!("sent exit recovery command to ECID {ecid:#x}");
                    true
                }
                Err(err) => {
                    debug!("failed to exit recovery mode for ECID {ecid:#x}: {err}");
                    false
                }
            }
        })
    }

    fn init_wireless_device(&mut self, ip: QString, mac_address: QString) {
        let qt_thread = self.qt_thread();
        let ip_owned = ip.to_string();
        let mac_address_owned = mac_address.to_string();
        RUNTIME.spawn(async move {
            let addr = match ip_owned.parse::<IpAddr>() {
                Ok(addr) => addr,
                Err(e) => {
                    warn!("Invalid wireless device IP address {ip_owned}: {e}");
                    qt_thread.queue(move |core_qobj| {
                        core_qobj.initFailed(QString::from(mac_address_owned));
                    });
                    return;
                }
            };

            let result = tokio::time::timeout(
                WIRELESS_INIT_TIMEOUT,
                init_wireless_device_from_candidates(addr, &mac_address_owned, qt_thread.clone()),
            )
            .await;

            match result {
                Ok(Ok(initialized)) => {
                    info!(
                        "Successfully initialized wireless device mac={} ip={} udid={}",
                        mac_address_owned, ip_owned, initialized.udid
                    );
                    // emit event with info
                    emit_initialized_device(qt_thread, initialized);
                }
                Ok(Err(error)) => {
                    warn!(
                        "Failed to initialize wireless device mac={} ip={}: {}",
                        mac_address_owned, ip_owned, error
                    );

                    qt_thread.queue(move |core_qobj| match error {
                        WirelessInitError::NoPairingRecords
                        | WirelessInitError::NoMatchingPairingRecord => {
                            core_qobj.noPairingFile(QString::from(mac_address_owned));
                        }
                        WirelessInitError::InvalidMac { .. } | WirelessInitError::Transport(_) => {
                            core_qobj.initFailed(QString::from(mac_address_owned));
                        }
                    });
                }
                Err(_) => {
                    warn!(
                        "Timed out initializing wireless device mac={} ip={}",
                        mac_address_owned, ip_owned
                    );

                    qt_thread.queue(move |core_qobj| {
                        core_qobj.initFailed(QString::from(mac_address_owned));
                    });
                }
            }
        });
    }

    fn init_wireless_device_custom(&mut self, ip: QString, pairing_file: QString) {
        info!(
            "init_wireless_device_custom: IP: {} Pairing File: {}",
            ip, pairing_file
        );
        let qt_thread = self.qt_thread();
        let ip_owned = ip.to_string();
        let pairing_path = pairing_file.to_string();
        RUNTIME.spawn(async move {
            let pairing_file = match PairingFile::read_from_file(&pairing_path) {
                Ok(pf) => pf,
                Err(e) => {
                    error!("Failed to read pairing file: {e}");
                    qt_thread.queue(move |core_qobj| {
                        core_qobj.customInitFailed(
                            QString::from(ip_owned),
                            QString::from(""),
                            QString::from(e.to_string()),
                        );
                    });
                    return;
                }
            };

            let mac_address_owned = pairing_file.wifi_mac_address.clone();

            let addr = match ip_owned.parse::<IpAddr>() {
                Ok(addr) => addr,
                Err(e) => {
                    qt_thread.queue(move |core_qobj| {
                        core_qobj.customInitFailed(
                            QString::from(ip_owned),
                            QString::from(pairing_file.wifi_mac_address),
                            QString::from(e.to_string()),
                        );
                    });
                    return;
                }
            };

            let t = TcpProvider {
                addr,
                pairing_file,
                label: APP_LABEL.to_string(),
                scope_id: None,
            };

            let qt_t_clone = qt_thread.clone();
            let connection_id = next_connection_id();
            let result = tokio::select! {
                res = init_idescriptor_device(t, qt_t_clone, connection_id) => res,
                /* timeout */
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(20)) => {
                    error!("Timeout collecting device info for wireless device ip: {ip_owned}");
                    Err(IdeviceError::Timeout)
                }
            };

            match result {
                Ok(initialized) => {
                    // emit event with info
                    emit_initialized_device(qt_thread, initialized);
                }
                //pairing file doesn't belong to the device
                Err(IdeviceError::InvalidHostID) => {
                    error!("Invalid pairing file for wireless device ip: {ip_owned}");

                    qt_thread.queue(move |core_qobj| {
                        core_qobj.customInitFailed(
                            QString::from(ip_owned),
                            QString::from(mac_address_owned),
                            QString::from("invalid pairing file for this device"),
                        );
                    });
                }
                Err(e) => {
                    error!("Failed to initialize wireless device ip: {ip_owned} {e:?}");

                    qt_thread.queue(move |core_qobj| {
                        core_qobj.customInitFailed(
                            QString::from(ip_owned),
                            QString::from(mac_address_owned),
                            QString::from(e.to_string()),
                        );
                    });
                }
            }
        });
    }

    fn remove_device(&mut self, udid: QString, connection_id: u64) {
        let udid_str = udid.to_string();
        RUNTIME.spawn(async move {
            clean_device_from_app_state_if_current(&udid_str, connection_id, true).await;
        });
    }
}

// TODO: nusb provides a DeviceId with bus and addr like so
// recovery:DeviceId(DeviceId { bus: 3, addr: 47 })
// but should we use ECID anyway?
fn recovery_device_key(id: &irecovery::DeviceId) -> String {
    // device
    //     .ecid
    //     .map(|ecid| format!("recovery:{ecid:x}"))
    //     .unwrap_or_else(|| format!("recovery:{:?}", device.id))
    format!("recovery:{:?}", id)
}

fn recovery_mode_name(mode: irecovery::RecoveryMode) -> QString {
    let name = match mode {
        irecovery::RecoveryMode::Wtf => "WTF",
        irecovery::RecoveryMode::Dfu => "DFU",
        irecovery::RecoveryMode::Recovery => "Recovery",
        irecovery::RecoveryMode::Restore => "Restore",
        irecovery::RecoveryMode::Kis => "KIS",
        irecovery::RecoveryMode::Unknown(_) => "Unknown",
    };

    QString::from(name)
}

fn collect_recovery_device_info(device: &irecovery::RecoveryDevice) -> QVariantMap {
    let mut info = QVariantMap::default();

    qvariantmap_insert!(info, "display_name", QString::from(device.display_name()));
    qvariantmap_insert!(
        info,
        "hardware_model",
        QString::from(device.hardware_model().unwrap_or("unknown"))
    );
    qvariantmap_insert!(info, "mode", recovery_mode_name(device.mode));
    qvariantmap_insert!(info, "vendor_id", u32::from(device.vendor_id));
    qvariantmap_insert!(info, "product_id", u32::from(device.product_id));
    debug!("Recovery device ECID: {:?}", device.ecid);
    qvariantmap_insert!(
        info,
        "ecid",
        QString::from(
            device
                .ecid
                .map(|ecid| format!("{ecid:x}"))
                .unwrap_or_default()
        )
    );
    qvariantmap_insert!(
        info,
        "ecid_decimal",
        QString::from(device.ecid.map(|ecid| ecid.to_string()).unwrap_or_default())
    );
    qvariantmap_insert!(
        info,
        "usb_serial_number",
        QString::from(device.usb_serial_number.clone().unwrap_or_default())
    );
    qvariantmap_insert!(
        info,
        "placeholder_path",
        QString::from(utils::device_placeholder_path(
            device
                .metadata
                .as_ref()
                .map(|metadata| metadata.model_identifier)
                .unwrap_or_default()
        ))
    );

    if let Some(metadata) = &device.metadata {
        qvariantmap_insert!(
            info,
            "model_identifier",
            QString::from(metadata.model_identifier)
        );
        qvariantmap_insert!(info, "board", QString::from(metadata.board));
        qvariantmap_insert!(
            info,
            "marketing_name",
            QString::from(metadata.marketing_name)
        );
    }

    if let Some(device_info) = &device.device_info {
        qvariantmap_insert!(
            info,
            "serial_string",
            QString::from(device_info.serial_string.clone())
        );
        qvariantmap_insert!(
            info,
            "cpid",
            QString::from(
                device_info
                    .cpid
                    .map(|value| format!("{value:x}"))
                    .unwrap_or_default()
            )
        );
        qvariantmap_insert!(
            info,
            "bdid",
            QString::from(
                device_info
                    .bdid
                    .map(|value| format!("{value:x}"))
                    .unwrap_or_default()
            )
        );
        qvariantmap_insert!(
            info,
            "srtg",
            QString::from(device_info.srtg.clone().unwrap_or_default())
        );
        qvariantmap_insert!(
            info,
            "srnm",
            QString::from(device_info.srnm.clone().unwrap_or_default())
        );
        qvariantmap_insert!(
            info,
            "imei",
            QString::from(device_info.imei.clone().unwrap_or_default())
        );
    }

    info
}

//TODO: find a better way to do this
fn parse_recovery_ecid(ecid: &str) -> Option<u64> {
    let trimmed = ecid.trim();
    if trimmed.is_empty() {
        return None;
    }

    let without_prefix = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);

    u64::from_str_radix(without_prefix, 16)
        .ok()
        .or_else(|| trimmed.parse::<u64>().ok())
}

fn is_pairing_related_error(e: &IdeviceError) -> bool {
    matches!(
        e,
        IdeviceError::InvalidHostID
            | IdeviceError::PairingDialogResponsePending
            | IdeviceError::PasswordProtected
            | IdeviceError::UserDeniedPairing
            | IdeviceError::CanceledByUser
    )
}

async fn handle_pairing(qt_thread: QtThread<Core>, udid: String) -> Result<(), IdeviceError> {
    let udid_for_event = udid.clone();
    qt_thread.queue(move |core_qobj| {
        core_qobj.deviceEvent(
            EV_PAIRING_PENDING,
            QString::from(udid_for_event),
            QVariantMap::default(),
        );
    });

    let mut uc2 = UsbmuxdConnection::default().await?;

    let dev = uc2.get_device(&udid).await?;

    let provider = dev.to_provider(UsbmuxdAddr::default(), APP_LABEL);

    let mut lc = LockdownClient::connect(&provider).await?;

    let buid = uc2.get_buid().await?;

    let host_id = uuid::Uuid::new_v4().to_string().to_uppercase();

    info!(
        "Pairing with device {}, host_id: {}, buid: {}",
        udid, host_id, buid
    );
    let mut pf = loop {
        match lc.pair(host_id.clone(), buid.clone(), None).await {
            Ok(p) => {
                info!(
                    "Pairing successful with device {}, host_id: {}, buid: {}",
                    udid, host_id, buid
                );
                break p;
            }
            Err(IdeviceError::PairingDialogResponsePending) => {
                /* wait */
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                continue;
            }
            Err(IdeviceError::PasswordProtected) => {
                /* wait */
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                continue;
            }
            Err(IdeviceError::InvalidHostID) => {
                /* wait */
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                continue;
            }
            // TODO: we can also check for CanceledByUser or UserDeniedPairing
            Err(e) => {
                return Err(e);
            }
        }
    };
    info!("Paired with device {}, pairing file obtained", udid);
    pf.udid = Some(udid.clone());

    let bytes = pf.serialize()?;
    uc2.save_pair_record(&udid, bytes).await?;

    info!("Pairing record saved to usbmuxd for device {}.", udid);
    Ok(())
}

fn emit_pairing_failed(
    qt_thread: QtThread<Core>,
    udid: String,
    // FIXME: we may want to use this in the future
    _reason: &str,
) {
    qt_thread.queue(move |core_qobj| {
        core_qobj.deviceEvent(EV_FAIL, QString::from(udid), QVariantMap::default());
    });
}

async fn emit_connected(qt_thread: QtThread<Core>, udid: String, connection_id: u64) {
    // one init retry after successful pairing
    let mut retried_after_pair = false;

    loop {
        let mut uc = match UsbmuxdConnection::default().await {
            Ok(u) => u,
            Err(_) => return,
        };

        let dev = match uc.get_device(&udid).await {
            Ok(d) => d,
            Err(_) => return,
        };

        let provider = dev.to_provider(UsbmuxdAddr::default(), APP_LABEL);

        let qt_t_clone = qt_thread.clone();

        match init_idescriptor_device(provider, qt_t_clone, connection_id).await {
            Ok(initialized) => {
                info!("Emitting connected");
                emit_initialized_device(qt_thread, initialized);
                return;
            }
            Err(e) if is_pairing_related_error(&e) && !retried_after_pair => {
                match handle_pairing(qt_thread.clone(), udid.clone()).await {
                    Ok(_) => {
                        // retry init once
                        retried_after_pair = true;
                        info!(
                            "Pairing succeeded for device {}, retrying initialization.",
                            udid
                        );
                        continue;
                    }
                    Err(e) => {
                        error!("Pairing failed for device {}: {e:?}", udid);
                        emit_pairing_failed(
                            qt_thread.clone(),
                            udid.clone(),
                            "Failed to pair device",
                        );
                        return;
                    }
                }
            }
            Err(e) => {
                error!(
                    "Unhandled error while initializing device for udid {}: {e:?}",
                    udid
                );
                return;
            }
        }
    }
}

#[derive(Debug)]
struct PairingCandidate {
    pairing_file: PairingFile,
    path: String,
}

fn normalize_mac_address(mac: &str) -> String {
    mac.chars()
        .filter(|character| character.is_ascii_hexdigit())
        .flat_map(char::to_lowercase)
        .collect()
}

fn prioritize_by_mac<T, F>(candidates: &mut [T], requested_mac: &str, candidate_mac: F)
where
    F: Fn(&T) -> &str,
{
    let requested_mac = normalize_mac_address(requested_mac);
    candidates
        .sort_by_key(|candidate| normalize_mac_address(candidate_mac(candidate)) != requested_mac);
}

fn prioritize_pairing_candidates(candidates: &mut [PairingCandidate], requested_mac: &str) {
    prioritize_by_mac(candidates, requested_mac, |candidate| {
        &candidate.pairing_file.wifi_mac_address
    });
}

#[cfg(not(target_os = "macos"))]
async fn discover_pairing_candidates(
    _requested_mac: &str,
) -> anyhow::Result<Vec<PairingCandidate>> {
    tokio::task::spawn_blocking(|| {
        let lockdown_path = utils::get_lockdown_path();
        let entries = std::fs::read_dir(&lockdown_path).with_context(|| {
            format!(
                "failed to read lockdown directory {}",
                lockdown_path.display()
            )
        })?;

        let mut candidates = Vec::new();
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    debug!("Skipping unreadable lockdown directory entry: {error}");
                    continue;
                }
            };
            let path = entry.path();
            if !path.is_file()
                || path.to_str().map_or(true, |s| {
                    !s.ends_with(".plist") || s.ends_with("SystemConfiguration.plist")
                })
            {
                continue;
            }

            match PairingFile::read_from_file(&path) {
                Ok(pairing_file) => candidates.push(PairingCandidate {
                    pairing_file,
                    path: path.to_string_lossy().into_owned(),
                }),
                Err(error) => {
                    debug!(
                        "Skipping unusable pairing record path={}: {error}",
                        path.display()
                    );
                }
            }
        }

        Ok(candidates)
    })
    .await
    .context("pairing-record discovery worker failed")?
}

#[cfg(target_os = "macos")]
async fn discover_pairing_candidates(requested_mac: &str) -> anyhow::Result<Vec<PairingCandidate>> {
    let mut candidates = Vec::new();

    // macOS denies directory enumeration for /var/db/lockdown but permits reading a
    // pairing record when its full path is known. Here we try the path cached during an earlier
    // usbmuxd enumeration before relying on the device still being visible to usbmuxd.
    if let Some(path) = crate::settings_manager::idevice_default_pairing_file(requested_mac) {
        match PairingFile::read_from_file(&path) {
            Ok(pairing_file) => {
                info!(
                    "Loaded saved macOS pairing record requested_mac={} path={}",
                    requested_mac, path
                );
                candidates.push(PairingCandidate { pairing_file, path });
            }
            Err(error) => {
                warn!(
                    "Failed to read saved macOS pairing record requested_mac={} path={} error={}",
                    requested_mac, path, error
                );
            }
        }
    }

    let mut usbmuxd = match UsbmuxdConnection::default().await {
        Ok(usbmuxd) => usbmuxd,
        Err(error) if !candidates.is_empty() => {
            debug!("Using saved macOS pairing record because usbmuxd is unavailable: {error}");
            return Ok(candidates);
        }
        Err(error) => {
            return Err(error).context("failed to connect to usbmuxd for pairing records");
        }
    };
    let devices = match usbmuxd.get_devices().await {
        Ok(devices) => devices,
        Err(error) if !candidates.is_empty() => {
            debug!(
                "Using saved macOS pairing record because usbmuxd device enumeration failed: {error}"
            );
            return Ok(candidates);
        }
        Err(error) => {
            return Err(error).context("failed to list usbmuxd devices for pairing records");
        }
    };

    for device in devices {
        use std::path::PathBuf;

        match usbmuxd.get_pair_record(&device.udid).await {
            Ok(pairing_file) => {
                let path = PathBuf::new()
                    .join(utils::get_lockdown_path())
                    .join(format!("{}.plist", device.udid))
                    .to_string_lossy()
                    .into_owned();
                info!("Caching {}", path);
                crate::settings_manager::cache_idevice_default_pairing_file(
                    &pairing_file.wifi_mac_address,
                    &path,
                );

                let already_loaded = candidates.iter().any(|candidate| {
                    candidate.pairing_file.udid == pairing_file.udid || candidate.path == path
                });
                if !already_loaded {
                    candidates.push(PairingCandidate { pairing_file, path });
                }
            }
            Err(error) => {
                debug!(
                    "Skipping unavailable usbmuxd pairing record udid={}: {error}",
                    device.udid
                );
            }
        }
    }

    Ok(candidates)
}

async fn init_wireless_device_from_candidates(
    addr: IpAddr,
    requested_mac: &str,
    qt_thread: QtThread<Core>,
) -> Result<InitializedDevice, WirelessInitError> {
    let normalized_requested_mac = normalize_mac_address(requested_mac);
    if normalized_requested_mac.len() != 12 {
        return Err(WirelessInitError::InvalidMac {
            mac: requested_mac.to_string(),
        });
    }

    let mut candidates = discover_pairing_candidates(requested_mac)
        .await
        .map_err(WirelessInitError::Transport)?;
    if candidates.is_empty() {
        return Err(WirelessInitError::NoPairingRecords);
    }
    prioritize_pairing_candidates(&mut candidates, requested_mac);

    let candidate_count = candidates.len();
    for (index, candidate) in candidates.into_iter().enumerate() {
        info!(
            "Trying wireless pairing record {}/{} target_ip={} requested_mac={} candidate_mac={} path={}",
            index + 1,
            candidate_count,
            addr,
            requested_mac,
            candidate.pairing_file.wifi_mac_address,
            candidate.path,
        );

        let candidate_mac_normalized =
            normalize_mac_address(&candidate.pairing_file.wifi_mac_address);

        let provider = TcpProvider {
            addr,
            pairing_file: candidate.pairing_file,
            label: APP_LABEL.to_string(),
            scope_id: None,
        };

        let connection_id = next_connection_id();
        match init_idescriptor_device(provider, qt_thread.clone(), connection_id).await {
            Ok(initialized) => {
                info!(
                    "Successfully initialized wireless device target_ip={} requested_mac={} candidate_mac={} candidate_udid={} path={}",
                    addr, requested_mac, candidate_mac_normalized, initialized.udid, candidate.path,
                );
                return Ok(initialized);
            }
            Err(IdeviceError::InvalidHostID) => {
                warn!(
                    "Pairing record did not authenticate wireless device target_ip={} requested_mac={} candidate_mac={} path={}",
                    addr, requested_mac, candidate_mac_normalized, candidate.path,
                );
                continue;
            }
            Err(e) => {
                warn!(
                    "Failed to initialize wireless device target_ip={} requested_mac={} candidate_mac={} path={} error={:?}",
                    addr, requested_mac, candidate_mac_normalized, candidate.path, e,
                );
                continue;
            }
        }
    }

    warn!(
        "Exhausted {} pairing records for wireless device target_ip={} requested_mac={}",
        candidate_count, addr, requested_mac
    );
    Err(WirelessInitError::NoMatchingPairingRecord)
}

async fn init_idescriptor_device<
    T: idevice::provider::IdeviceProvider + Send + Sync + Clone + 'static,
>(
    provider: T,
    qt_thread: QtThread<Core>,
    connection_id: u64,
) -> Result<InitializedDevice, IdeviceError> {
    let provider_name = type_name::<T>();
    let is_wireless = provider_name == "idevice::provider::TcpProvider";

    let pf = idevice::provider::IdeviceProvider::get_pairing_file(&provider).await?;
    let mut lc = LockdownClient::connect(&provider).await?;
    lc.start_session(&pf).await?;

    let mut def_vals = match lc.get_value(None, None).await {
        Ok(v) => v,
        Err(e) => {
            error!("get_value(None, None) failed: {e:?}");
            return Err(e);
        }
    };
    debug!("init_idescriptor_device: Default values obtained.");

    // FIXME: we may need our own error types here
    // but InternalError should be fine for now
    // maybe use anyhow?
    let def_vals_dict = def_vals.as_dictionary_mut().ok_or_else(|| {
        IdeviceError::InternalError(
            "lc.get_value(None, None).await is not a dictionary".to_string(),
        )
    })?;

    let udid = def_vals_dict
        .get("UniqueDeviceID")
        .and_then(|v| v.as_string())
        .ok_or_else(|| {
            IdeviceError::InternalError("Missing UniqueDeviceID in Lockdown response".to_string())
        })?
        .to_string();

    let ios_version = def_vals_dict
        .get("ProductVersion")
        .and_then(|v| v.as_string())
        .ok_or_else(|| {
            IdeviceError::InternalError("Missing ProductVersion in Lockdown response".to_string())
        })?
        .to_string();

    if udid.is_empty() {
        error!("init_idescriptor_device: UDID is empty.");
        return Err(IdeviceError::InvalidHostID);
    }

    let mut hb = None;

    if is_wireless {
        debug!("init_idescriptor_device: Attempting to connect to HeartbeatClient.");
        hb = match heartbeat::HeartbeatClient::connect(&provider).await {
            Ok(h) => Some(h),
            Err(e) => {
                error!("heartbeat: connect failed: {e:?}");
                return Err(e);
            }
        };
        debug!("init_idescriptor_device: Connected to HeartbeatClient.");
    }

    debug!("init_idescriptor_device: Attempting to connect to AFC client.");
    let mut afc_client = AfcClient::connect(&provider).await?;
    // useful for time taking stuff for example video thumbnail generation
    // images get loaded with the main client
    // and video thumbnails are handled by the secondary client
    let afc_client_secondary = AfcClient::connect(&provider).await?;

    debug!("init_idescriptor_device: Connected to AfcClient.");

    debug!("init_idescriptor_device: Attempting to connect to DiagnosticsRelayClient.");
    let mut diag_relay = DiagnosticsRelayClient::connect(&provider).await?;

    debug!("init_idescriptor_device: Connected to DiagnosticsRelayClient.");

    let afc2 = match AfcClient::new_afc2(&provider).await {
        Ok(c) => Some(Arc::new(Mutex::new(c))),
        Err(e) => {
            warn!("AfcClient::new_afc2 failed: {e:?}");
            None
        }
    };
    let afc2_secondary = if afc2.is_some() {
        match AfcClient::new_afc2(&provider).await {
            Ok(c) => Some(Arc::new(Mutex::new(c))),
            Err(e) => {
                warn!("Secondary AfcClient::new_afc2 failed: {e:?}");
                None
            }
        }
    } else {
        None
    };

    let mut info = collect_info(
        def_vals_dict,
        &mut afc_client,
        &mut lc,
        &mut diag_relay,
        is_wireless,
    )
    .await?;

    debug!("init_idescriptor_device: Storing device services.");
    let (heartbeat_task, heartbeat_start) = if is_wireless {
        let Some(hb_client) = hb else {
            error!(
                "init_idescriptor_device: Heartbeat client is None, cannot spawn heartbeat task."
            );
            return Err(IdeviceError::Heartbeat(idevice::HeartbeatError::Unknown));
        };

        debug!("init_idescriptor_device: Spawning paused heartbeat task.");
        let (start_tx, start_rx) = oneshot::channel();
        let task = spawn_heartbeat_task(
            hb_client,
            qt_thread.clone(),
            udid.clone(),
            connection_id,
            start_rx,
        )
        .await
        .map_err(|()| IdeviceError::Heartbeat(idevice::HeartbeatError::Unknown))?;
        (Some(task), Some(start_tx))
    } else {
        (None, None)
    };

    let device_services = DeviceServices {
        connection_id,
        is_wireless,
        afc: Arc::new(Mutex::new(afc_client)),
        afc_secondary: Arc::new(Mutex::new(afc_client_secondary)),
        afc2,
        afc2_secondary,
        diag: Arc::new(Mutex::new(diag_relay)),
        heartbeat_task,
        video_streams: Arc::new(Mutex::new(HashMap::new())),
        provider: Arc::new(Mutex::new(Box::new(provider))),
        lockdown: Arc::new(Mutex::new(lc)),
        ios_version: iOSVersion::from_str(&ios_version),
    };

    match insert_device(udid.clone(), device_services).await {
        InsertDeviceResult::Inserted => {}
        InsertDeviceResult::Replaced(replaced_connection_id) => {
            let replaced_udid = udid.clone();
            qt_thread.queue(move |core_qobj| {
                core_qobj.deviceEvent(
                    EV_DISCONNECTED,
                    QString::from(replaced_udid),
                    connection_event_info(replaced_connection_id),
                );
            });
        }
        InsertDeviceResult::Rejected => {
            return Err(IdeviceError::InternalError(format!(
                "stale connection initialization rejected for {udid} connection {connection_id}"
            )));
        }
    }

    qvariantmap_insert!(
        info,
        "connection_id",
        QString::from(connection_id.to_string())
    );
    info!("init_idescriptor_device: Device has been initialized.");

    Ok(InitializedDevice {
        udid,
        info,
        heartbeat_start,
    })
}

async fn collect_info(
    def_vals_dict: &mut Dictionary,
    mut afc: &mut AfcClient,
    lc: &mut LockdownClient,
    diag_relay: &mut DiagnosticsRelayClient,
    is_wireless: bool,
) -> Result<QVariantMap, IdeviceError> {
    let mut info = QVariantMap::default();

    debug!("init_idescriptor_device: Attempting to get default values from Lockdown.");

    let disk_vals = lc.get_value(None, Some("com.apple.disk_usage")).await?;

    let disk_vals_dict = disk_vals.as_dictionary().ok_or_else(|| {
        IdeviceError::InternalError(
            "lc.get_value(None, Some(\"com.apple.disk_usage\")).await is not a dictionary"
                .to_string(),
        )
    })?;

    debug!("init_idescriptor_device: Attempting to get AFC device info.");
    let afc_info = match afc.get_device_info().await {
        Ok(i) => i,
        Err(e) => {
            error!("get_device_info failed: {e:?}");
            return Err(e);
        }
    };
    debug!("init_idescriptor_device: AFC device info obtained.");

    let keys_to_insert_string = [
        "DeviceName",
        "DeviceClass",
        "DeviceColor",
        "ModelNumber",
        "CPUArchitecture",
        "BuildVersion",
        "HardwareModel",
        "HardwarePlatform",
        "EthernetAddress",
        "BluetoothAddress",
        "FirmwareVersion",
        "ProductVersion",
        "WiFiAddress",
        "UniqueDeviceID",
        "InternationalMobileEquipmentIdentity",
        "SerialNumber",
        "ProductType",
        "ActivationState",
    ];

    // product_type
    let product_type = def_vals_dict
        .get("ProductType")
        .and_then(|v| v.as_string())
        .unwrap_or_else(|| "");

    let db_info = device_db::find_by_identifier(product_type).unwrap_or(&device_db::UNKNOWN_DEVICE);

    info.insert(
        QString::from("product_type"),
        QVariant::from(&QString::from(db_info.display_name)),
    );

    info.insert(
        QString::from("marketing_name"),
        QVariant::from(&QString::from(db_info.marketing_name)),
    );

    info.insert(
        QString::from("icon_path"),
        QVariant::from(&QString::from(utils::device_icon_path(product_type))),
    );

    info.insert(
        QString::from("placeholder_path"),
        QVariant::from(&QString::from(utils::device_placeholder_path(product_type))),
    );

    // region
    let region_info = def_vals_dict
        .get("RegionInfo")
        .and_then(|v| v.as_string())
        .unwrap_or_else(|| "");

    info.insert(
        QString::from("region"),
        QVariant::from(&QString::from(device_db::parse_region_info(region_info))),
    );

    for key in keys_to_insert_string.iter() {
        info.insert(
            QString::from(key.to_string()),
            QVariant::from(&QString::from(
                def_vals_dict
                    .get(key)
                    .and_then(|v| v.as_string())
                    .unwrap_or_else(|| ""),
            )),
        );
    }

    let disk_info_keys = [
        "TotalDiskCapacity",
        "TotalDataCapacity",
        "TotalSystemCapacity",
        "TotalDataAvailable",
    ];

    for key in disk_info_keys.iter() {
        info.insert(
            QString::from(*key),
            QVariant::from(
                disk_vals_dict
                    .get(*key)
                    .and_then(|v| v.as_unsigned_integer())
                    .unwrap_or(0),
            ),
        );
    }

    info.insert(
        QString::from("Model"),
        QVariant::from(&QString::from(afc_info.model)),
    );
    info.insert(
        QString::from("TotalBytes"),
        QVariant::from(afc_info.total_bytes as u64),
    );
    info.insert(
        QString::from("FreeBytes"),
        QVariant::from(afc_info.free_bytes as u64),
    );
    info.insert(
        QString::from("BlockSize"),
        QVariant::from(afc_info.block_size as u64),
    );

    info.insert(
        QString::from("Jailbroken"),
        QVariant::from(utils::detect_jailbroken(&mut afc).await),
    );

    info.insert(
        QString::from("connection_type"),
        QVariant::from(if is_wireless {
            QString::from("Wireless")
        } else {
            QString::from("USB")
        }),
    );

    info.insert(
        QString::from("ProductionDevice"),
        QVariant::from(QString::from(
            def_vals_dict
                .get("ProductionSOC")
                .and_then(|value| value.as_boolean())
                .map(|production| if production { "Yes" } else { "No" })
                .unwrap_or("Unknown"),
        )),
    );

    info.insert(QString::from("is_wireless"), QVariant::from(is_wireless));

    // parse ios version
    let ios_version: Vec<&str> = def_vals_dict
        .get("ProductVersion")
        .and_then(|v| v.as_string())
        .unwrap_or_else(|| "")
        .split(".")
        .collect();

    let ios_major = ios_version
        .get(0)
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);
    let ios_minor = ios_version
        .get(1)
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);
    let ios_patch = ios_version
        .get(2)
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);

    info.insert(
        QString::from("ios_version_major"),
        QVariant::from(ios_major),
    );
    info.insert(
        QString::from("ios_version_minor"),
        QVariant::from(ios_minor),
    );
    info.insert(
        QString::from("ios_version_patch"),
        QVariant::from(ios_patch),
    );

    let developer_mode_status = match lc
        .get_value(
            Some("DeveloperModeStatus"),
            Some("com.apple.security.mac.amfi"),
        )
        .await
    {
        Ok(Value::Boolean(b)) => b,
        _ => false,
    };

    // not helpful on iOS 16 and below
    // as they need dev disk images
    info.insert(
        QString::from("developer_mode_enabled"),
        QVariant::from(developer_mode_status),
    );

    insert_battery_info(diag_relay, &mut info, product_type.into(), ios_major)
        .await
        .unwrap_or_else(|e| {
            debug!("Failed to insert battery info: {e:?}");
        });

    #[cfg(target_os = "macos")]
    if !is_wireless {
        if let (Some(mac_address), Some(udid)) = (
            def_vals_dict
                .get("WiFiAddress")
                .and_then(|value| value.as_string())
                .filter(|value| !value.is_empty()),
            def_vals_dict
                .get("UniqueDeviceID")
                .and_then(|value| value.as_string())
                .filter(|value| !value.is_empty()),
        ) {
            let path = crate::utils::join_with_lockdown_path(&format!("{udid}.plist"))
                .display()
                .to_string();
            crate::settings_manager::cache_idevice_default_pairing_file(&mac_address, &path);
            info!("Cached pairing file for macOS, udid: {udid}, path: {path}");
        }
    }

    Ok(info)
}

pub async fn insert_battery_info(
    diag_relay: &mut DiagnosticsRelayClient,
    info: &mut QVariantMap,
    raw_product_type: String,
    ios_major: u32,
) -> anyhow::Result<()> {
    let mut parsed = QVariantMap::default();

    let battery_info = match utils::query_battery_info(diag_relay).await {
        Some(info) => info,
        None => {
            debug!(
                "query_battery_info returned None for device {}",
                raw_product_type
            );
            anyhow::bail!("query_battery_info didn't return a dict")
        }
    };

    // old devices do not have "BatteryData"
    let is_old_device = battery_info.get("BatteryData").is_none();
    let battery_info = if is_old_device {
        utils::parse_diag_info_old(battery_info)
    } else {
        utils::parse_diag_info(battery_info, raw_product_type, ios_major)
    };

    qvariantmap_insert!(parsed, "cycle_count", battery_info.cycle_count);
    qvariantmap_insert!(
        parsed,
        "battery_serial_number",
        QString::from(battery_info.battery_serial_number)
    );
    qvariantmap_insert!(parsed, "design_capacity", battery_info.design_capacity);
    qvariantmap_insert!(parsed, "max_capacity", battery_info.max_capacity);
    qvariantmap_insert!(
        parsed,
        "battery_health",
        QString::from(battery_info.battery_health)
    );
    qvariantmap_insert!(parsed, "is_charging", battery_info.is_charging);
    qvariantmap_insert!(parsed, "fully_charged", battery_info.fully_charged);
    qvariantmap_insert!(
        parsed,
        "current_battery_level",
        battery_info.current_battery_level
    );
    qvariantmap_insert!(
        parsed,
        "usb_connection_type",
        QString::from(battery_info.usb_connection_type)
    );
    qvariantmap_insert!(parsed, "adapter_voltage", battery_info.adapter_voltage);
    qvariantmap_insert!(parsed, "adapter_watts", battery_info.adapter_watts);

    qvariantmap_insert!(*info, "DIAG_INFO", &parsed);
    Ok(())
}

async fn spawn_heartbeat_task(
    mut hb_client: heartbeat::HeartbeatClient,
    qt_thread: QtThread<Core>,
    udid: String,
    connection_id: u64,
    start_rx: oneshot::Receiver<()>,
) -> Result<Arc<JoinHandle<()>>, ()> {
    let udid_for_hb = udid.clone();
    Ok(Arc::new(RUNTIME.spawn(async move {
        if start_rx.await.is_err() {
            debug!("heartbeat: start cancelled for {udid_for_hb} connection {connection_id}");
            return;
        }
        info!("heartbeat: starting heartbeat task");
        let mut interval = 15u64;
        let mut fails = 0;
        loop {
            trace!("heartbeat:  Getting marco (interval: {interval}s)");
            match hb_client.get_marco(interval).await {
                Ok(next) => {
                    interval = next;
                    fails = 0;
                }
                Err(e) => {
                    fails += 1;

                    match e {
                        IdeviceError::Heartbeat(idevice::HeartbeatError::SleepyTime) => {
                            info!("heartbeat: SleepyTime detected");
                            qt_thread.queue(move |core_qobj| {
                                core_qobj.sleepyTimeDetected();
                            });
                        }
                        _ => {}
                    };

                    if fails >= 3 {
                        error!("heartbeat: too many failures, giving up");
                        if clean_device_from_app_state_if_current(
                            &udid_for_hb,
                            connection_id,
                            false,
                        )
                        .await
                        {
                            let udid_for_event = udid_for_hb.clone();
                            let _ = qt_thread.queue(move |core_qobj| {
                                core_qobj.deviceEvent(
                                    EV_DISCONNECTED,
                                    QString::from(udid_for_event),
                                    connection_event_info(connection_id),
                                );
                            });
                        }
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(500 * fails)).await;
                    continue;
                }
            }

            trace!("heartbeat:  Sending polo.");
            if let Err(e) = hb_client.send_polo().await {
                fails += 1;
                warn!("heartbeat: send_polo failed (fail count: {fails}): {e:?}");
                match e {
                    IdeviceError::Heartbeat(idevice::HeartbeatError::SleepyTime) => {
                        info!("heartbeat: SleepyTime detected");
                        qt_thread.queue(move |core_qobj| {
                            core_qobj.sleepyTimeDetected();
                        });
                    }
                    _ => {}
                };
                if fails >= 3 {
                    error!("heartbeat: too many failures, giving up");
                    if clean_device_from_app_state_if_current(&udid_for_hb, connection_id, false)
                        .await
                    {
                        let udid_for_event = udid_for_hb.clone();
                        let _ = qt_thread.queue(move |core_qobj| {
                            core_qobj.deviceEvent(
                                EV_DISCONNECTED,
                                QString::from(udid_for_event),
                                connection_event_info(connection_id),
                            );
                        });
                    }
                    break;
                }

                tokio::time::sleep(Duration::from_millis(500 * fails)).await;
                continue;
            }
            interval += 5;
        }

        info!("heartbeat: heartbeat task ended.");
    })))
}
