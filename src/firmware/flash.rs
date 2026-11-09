// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    RUNTIME,
    list_model::ListModel,
    qt_threading::{QtThread, QtThreading},
    qvariantmap_insert,
};
use futures::{FutureExt, StreamExt};
use idevice::{
    Idevice, IdeviceError, IdeviceService,
    lockdown::LockdownClient,
    provider::IdeviceProvider,
    restore::{
        FdrClient, FdrConnector, FilesystemImage, RestoreCancel, RestoreContext, RestoreError,
        RestoreOptions, RestoreProgressEvent, RestoredClient,
        fdr::{FDR_CTRL_PORT, run_fdr_listener},
        img4,
        ipsw::{Ipsw, component_path, components_loaded_by_iboot},
        progress_channel,
        recovery::{
            ControlSetup, DeviceInfo, Mode, RecoveryDevice, RecoveryFuture, RecoveryTransport,
        },
        run_restore,
        state_machine::DataPortConnector,
    },
    tss::{TSSRequest, extract_img4_ticket, select_build_identity},
    usbmuxd::{Connection, UsbmuxdAddr},
};
use log::{debug, error, info, warn};
use macros::QtThreading;
use nusb::transfer::{Buffer, Bulk, ControlIn, ControlOut, ControlType, Out, Recipient};
use plist::Value;
use qmetaobject::{SimpleListItem, prelude::*};
use qttypes::{QString, QVariantMap};
use std::{
    cell::RefCell,
    future::Future,
    num::NonZeroU8,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncWrite, BufReader},
    task::JoinHandle,
};

const MAX_LOG_ENTRIES: usize = 500;
const APPLE_VENDOR_ID: u16 = 0x05AC;
const STAGE_COUNT: i32 = 8;

type IpswFile = BufReader<tokio::fs::File>;

#[derive(SimpleListItem, Default, Clone)]
struct FlashLogItem {
    pub timestamp: QString,
    pub level: QString,
    pub message: QString,
}

#[allow(non_snake_case)]
#[derive(QObject, Default, QtThreading)]
pub struct FlashManager {
    base: qt_base_class!(trait QObject),
    state: qt_property!(QVariantMap; NOTIFY stateChanged),
    busy: qt_property!(bool; NOTIFY busyChanged),
    log_model: qt_property!(RefCell<ListModel<FlashLogItem>>; NOTIFY logModelChanged),
    cancel_handle: Mutex<Option<RestoreCancel>>,
    task: Mutex<Option<JoinHandle<()>>>,

    start_flash: qt_method!(
        fn(
            &mut self,
            udid: QString,
            ecid: QString,
            ipsw_path: QString,
            identifier: QString,
        ) -> bool
    ),
    cancel_flash: qt_method!(fn(&mut self) -> bool),
    reset: qt_method!(fn(&mut self)),
    logs_text: qt_method!(fn(&self) -> QString),

    stateChanged: qt_signal!(),
    busyChanged: qt_signal!(),
    logModelChanged: qt_signal!(),
    flashFinished: qt_signal!(success: bool, cancelled: bool, error: QString),
}

impl FlashManager {
    fn logs_text(&self) -> QString {
        let lines = self
            .log_model
            .borrow()
            .values
            .iter()
            .map(|entry| {
                format!(
                    "{} {} {}",
                    entry.timestamp,
                    entry.level.to_string().to_uppercase(),
                    entry.message
                )
            })
            .collect::<Vec<_>>();
        QString::from(lines.join("\n"))
    }

    pub fn new_with_state() -> Self {
        let mut manager = Self::default();
        manager.state = initial_state();
        manager
    }

    fn start_flash(
        &mut self,
        udid: QString,
        ecid: QString,
        ipsw_path: QString,
        identifier: QString,
    ) -> bool {
        if self.busy {
            return false;
        }
        let path = PathBuf::from(ipsw_path.to_string());
        if !path.is_file() || !verification_sidecar(&path).is_file() {
            return false;
        }

        self.log_model.borrow_mut().reset_data(Vec::new());
        self.state = running_state();
        self.stateChanged();
        self.busy = true;
        self.busyChanged();

        let cancel = RestoreCancel::new();
        if let Ok(mut handle) = self.cancel_handle.lock() {
            *handle = Some(cancel.clone());
        }

        let request = FlashRequest {
            udid: udid.to_string(),
            ecid: ecid.to_string(),
            ipsw_path: path,
            identifier: identifier.to_string(),
        };
        let qt_thread = self.qt_thread();
        let reporter = FlashReporter::new(qt_thread.clone());
        let task = RUNTIME.spawn(async move {
            let result = run_flash(request, reporter.clone(), cancel).await;
            match &result {
                Ok(()) => reporter.log("info", "The erase restore finished successfully"),
                Err(IdeviceError::Restore(RestoreError::Cancelled)) => {
                    reporter.log("warning", "The restore was cancelled")
                }
                Err(err) => reporter.log("error", format!("Restore failed: {err}")),
            }
            qt_thread.queue(move |manager| {
                if let Ok(mut task) = manager.task.lock() {
                    task.take();
                }
                if let Ok(mut handle) = manager.cancel_handle.lock() {
                    handle.take();
                }
                manager.busy = false;
                manager.busyChanged();

                match result {
                    Ok(()) => {
                        set_state_value(&mut manager.state, "phase", QString::from("completed"));
                        set_state_value(
                            &mut manager.state,
                            "stage",
                            QString::from("Restore complete"),
                        );
                        set_state_value(
                            &mut manager.state,
                            "detail",
                            QString::from("The device accepted the firmware and is rebooting."),
                        );
                        set_state_value(&mut manager.state, "stageIndex", STAGE_COUNT);
                        set_state_value(&mut manager.state, "progress", 1.0_f64);
                        set_state_value(&mut manager.state, "indeterminate", false);
                        set_state_value(&mut manager.state, "canCancel", false);
                        manager.stateChanged();
                        manager.flashFinished(true, false, QString::default());
                    }
                    Err(err) => {
                        let cancelled =
                            matches!(err, IdeviceError::Restore(RestoreError::Cancelled));
                        let message = err.to_string();
                        set_state_value(
                            &mut manager.state,
                            "phase",
                            QString::from(if cancelled { "cancelled" } else { "failed" }),
                        );
                        set_state_value(
                            &mut manager.state,
                            "stage",
                            QString::from(if cancelled {
                                "Restore cancelled"
                            } else {
                                "Restore failed"
                            }),
                        );
                        set_state_value(
                            &mut manager.state,
                            "detail",
                            QString::from(message.clone()),
                        );
                        set_state_value(&mut manager.state, "indeterminate", false);
                        set_state_value(&mut manager.state, "canCancel", false);
                        manager.stateChanged();
                        manager.flashFinished(false, cancelled, QString::from(message));
                    }
                }
            });
        });

        match self.task.lock() {
            Ok(mut current) => {
                *current = Some(task);
                true
            }
            Err(err) => {
                task.abort();
                error!("Flash task state is unavailable: {err}");
                self.busy = false;
                self.busyChanged();
                false
            }
        }
    }

    fn cancel_flash(&mut self) -> bool {
        if !self.busy {
            return false;
        }
        let Ok(handle) = self.cancel_handle.lock() else {
            return false;
        };
        let Some(handle) = handle.as_ref() else {
            return false;
        };
        handle.cancel();
        set_state_value(&mut self.state, "cancellationRequested", true);
        set_state_value(
            &mut self.state,
            "detail",
            QString::from("Cancellation requested. Waiting for a safe stopping point…"),
        );
        set_state_value(&mut self.state, "canCancel", false);
        self.stateChanged();
        true
    }

    fn reset(&mut self) {
        if self.busy {
            return;
        }
        self.state = initial_state();
        self.log_model.borrow_mut().reset_data(Vec::new());
        self.stateChanged();
    }
}

fn initial_state() -> QVariantMap {
    let mut state = QVariantMap::default();
    qvariantmap_insert!(state, "phase", QString::from("ready"));
    qvariantmap_insert!(state, "stage", QString::from("Ready to restore"));
    qvariantmap_insert!(state, "detail", QString::default());
    qvariantmap_insert!(state, "stageIndex", 0_i32);
    qvariantmap_insert!(state, "stageCount", STAGE_COUNT);
    qvariantmap_insert!(state, "progress", 0.0_f64);
    qvariantmap_insert!(state, "indeterminate", false);
    qvariantmap_insert!(state, "component", QString::default());
    qvariantmap_insert!(state, "bytesSent", 0.0_f64);
    qvariantmap_insert!(state, "bytesTotal", 0.0_f64);
    qvariantmap_insert!(state, "speed", 0.0_f64);
    qvariantmap_insert!(state, "canCancel", false);
    qvariantmap_insert!(state, "cancellationRequested", false);
    qvariantmap_insert!(state, "targetEcid", QString::default());
    state
}

fn running_state() -> QVariantMap {
    let mut state = initial_state();
    set_state_value(&mut state, "phase", QString::from("running"));
    set_state_value(&mut state, "stage", QString::from("Validating firmware"));
    set_state_value(
        &mut state,
        "detail",
        QString::from("Opening the IPSW and checking its manifest…"),
    );
    set_state_value(&mut state, "stageIndex", 1_i32);
    set_state_value(&mut state, "indeterminate", true);
    set_state_value(&mut state, "canCancel", true);
    state
}

fn set_state_value<T: Into<QVariant>>(state: &mut QVariantMap, key: &str, value: T) {
    state[QString::from(key)] = value.into();
}

#[derive(Clone)]
struct FlashReporter {
    qt_thread: QtThread<FlashManager>,
    started: Instant,
    transfer: Arc<Mutex<TransferSample>>,
}

struct TransferSample {
    component: String,
    bytes: u64,
    at: Instant,
}

impl FlashReporter {
    fn new(qt_thread: QtThread<FlashManager>) -> Self {
        Self {
            qt_thread,
            started: Instant::now(),
            transfer: Arc::new(Mutex::new(TransferSample {
                component: String::new(),
                bytes: 0,
                at: Instant::now(),
            })),
        }
    }

    fn log(&self, level: &str, message: impl Into<String>) {
        let level = level.to_string();
        let message = message.into();
        match level.as_str() {
            "error" => error!("Flash: {message}"),
            "warning" => warn!("Flash: {message}"),
            "debug" => debug!("Flash: {message}"),
            _ => info!("Flash: {message}"),
        }
        let elapsed = self.started.elapsed().as_secs();
        let timestamp = format!("+{:02}:{:02}", elapsed / 60, elapsed % 60);
        self.qt_thread.queue(move |manager| {
            let mut model = manager.log_model.borrow_mut();
            if model.values.len() >= MAX_LOG_ENTRIES {
                model.remove(0);
            }
            model.push(FlashLogItem {
                timestamp: QString::from(timestamp),
                level: QString::from(level),
                message: QString::from(message),
            });
        });
    }

    fn stage(&self, index: i32, stage: &str, detail: &str, indeterminate: bool) {
        self.log("info", format!("{stage}: {detail}"));
        let stage = QString::from(stage);
        let detail = QString::from(detail);
        self.qt_thread.queue(move |manager| {
            set_state_value(&mut manager.state, "stageIndex", index);
            set_state_value(&mut manager.state, "stage", stage);
            set_state_value(&mut manager.state, "detail", detail);
            set_state_value(&mut manager.state, "progress", 0.0_f64);
            set_state_value(&mut manager.state, "indeterminate", indeterminate);
            set_state_value(&mut manager.state, "component", QString::default());
            set_state_value(&mut manager.state, "bytesSent", 0.0_f64);
            set_state_value(&mut manager.state, "bytesTotal", 0.0_f64);
            set_state_value(&mut manager.state, "speed", 0.0_f64);
            manager.stateChanged();
        });
    }

    fn target_ecid(&self, ecid: u64) {
        self.qt_thread.queue(move |manager| {
            set_state_value(
                &mut manager.state,
                "targetEcid",
                QString::from(ecid.to_string()),
            );
            manager.stateChanged();
        });
    }

    fn operation(&self, operation: u64, progress: u64) {
        let progress = progress.min(100) as f64 / 100.0;
        self.qt_thread.queue(move |manager| {
            set_state_value(&mut manager.state, "stageIndex", 7_i32);
            set_state_value(
                &mut manager.state,
                "stage",
                QString::from("Installing firmware"),
            );
            set_state_value(
                &mut manager.state,
                "detail",
                QString::from(format!("Device operation {operation}")),
            );
            set_state_value(&mut manager.state, "progress", progress);
            set_state_value(&mut manager.state, "indeterminate", false);
            manager.stateChanged();
        });
    }

    fn step(&self, step: String) {
        self.log("info", format!("RestoreOS requested {step}"));
        let friendly = friendly_step(&step);
        self.qt_thread.queue(move |manager| {
            set_state_value(&mut manager.state, "stageIndex", 7_i32);
            set_state_value(
                &mut manager.state,
                "stage",
                QString::from("Installing firmware"),
            );
            set_state_value(&mut manager.state, "detail", QString::from(friendly));
            set_state_value(&mut manager.state, "indeterminate", true);
            manager.stateChanged();
        });
    }

    fn transfer(&self, component: String, sent: u64, total: Option<u64>) {
        let now = Instant::now();
        let speed = self
            .transfer
            .lock()
            .ok()
            .map(|mut sample| {
                let speed = if sample.component == component {
                    let elapsed = now.duration_since(sample.at).as_secs_f64();
                    if elapsed > 0.0 {
                        sent.saturating_sub(sample.bytes) as f64 / elapsed
                    } else {
                        0.0
                    }
                } else {
                    0.0
                };
                sample.component = component.clone();
                sample.bytes = sent;
                sample.at = now;
                speed
            })
            .unwrap_or(0.0);
        let total = total.unwrap_or(0);
        let progress = if total > 0 {
            sent as f64 / total as f64
        } else {
            0.0
        };
        self.qt_thread.queue(move |manager| {
            set_state_value(&mut manager.state, "component", QString::from(component));
            set_state_value(&mut manager.state, "bytesSent", sent as f64);
            set_state_value(&mut manager.state, "bytesTotal", total as f64);
            set_state_value(&mut manager.state, "speed", speed);
            set_state_value(&mut manager.state, "progress", progress);
            set_state_value(&mut manager.state, "indeterminate", total == 0);
            manager.stateChanged();
        });
    }
}

fn friendly_step(step: &str) -> String {
    match step {
        "RootTicket" => "Sending the personalized restore ticket…",
        "SystemImageData" | "RecoveryOSASRImage" => "Sending the operating-system image…",
        "BuildIdentityDict" | "BuildIdentityDictV2" => "Sending device-specific build information…",
        "NORData" => "Installing boot firmware…",
        "BasebandData" | "BasebandBootData" => "Installing cellular firmware…",
        "FirmwareUpdaterData" | "FirmwareUpdaterDataV2" | "FirmwareUpdaterDataV3" => {
            "Updating device firmware…"
        }
        "FDRTrustData" => "Establishing the restore trust channel…",
        other => return format!("Processing {other}…"),
    }
    .to_string()
}

struct FlashRequest {
    udid: String,
    ecid: String,
    ipsw_path: PathBuf,
    identifier: String,
}

struct AcquiredDevice {
    recovery: RecoveryDevice,
    ap_nonce: Option<Vec<u8>>,
    sep_nonce: Option<Vec<u8>>,
}

async fn run_flash(
    request: FlashRequest,
    reporter: FlashReporter,
    cancel: RestoreCancel,
) -> Result<(), IdeviceError> {
    reporter.log(
        "info",
        format!(
            "Starting erase restore for {} using {}",
            request.identifier,
            request.ipsw_path.display()
        ),
    );
    ensure_not_cancelled(&cancel)?;

    reporter.stage(
        1,
        "Validating firmware",
        "Reading BuildManifest.plist and checking restore support…",
        true,
    );
    let mut ipsw = open_ipsw(&request.ipsw_path).await?;
    let build_manifest = ipsw.build_manifest().await?;
    if !manifest_supports_img4(&build_manifest) {
        return Err(IdeviceError::Restore(RestoreError::Unsupported(
            "IPSWs without IMG4 firmware components".into(),
        )));
    }
    reporter.log("info", "BuildManifest contains IMG4 restore components");

    reporter.stage(
        2,
        "Preparing the device",
        "Reading device identifiers and personalization nonces…",
        true,
    );
    let acquired = acquire_recovery_device(&request, &reporter, &cancel).await?;
    let recovery = acquired.recovery;
    let info = recovery.info().clone();
    let board_id = required_identifier(info.bdid, "BDID")?;
    let chip_id = required_identifier(info.cpid, "CPID")?;
    let ecid = required_identifier(info.ecid, "ECID")?;
    let ap_nonce = acquired.ap_nonce.or_else(|| info.ap_nonce.clone());
    let sep_nonce = acquired.sep_nonce.or_else(|| info.sep_nonce.clone());
    reporter.log(
        "info",
        format!("Matched recovery device ECID {ecid:#x}, CPID {chip_id:#x}, BDID {board_id:#x}"),
    );
    reporter.log(
        "info",
        format!(
            "Personalization data: AP nonce {} bytes, SEP nonce {} bytes",
            ap_nonce.as_ref().map_or(0, Vec::len),
            sep_nonce.as_ref().map_or(0, Vec::len)
        ),
    );
    let ap_nonce = ap_nonce.ok_or_else(|| {
        IdeviceError::Restore(RestoreError::Other(
            "the recovery device did not expose an AP nonce; reconnect it and enter recovery mode again"
                .into(),
        ))
    })?;

    let build_identity =
        select_build_identity(&build_manifest, board_id, chip_id, Some("Erase"))?.clone();
    reporter.log("info", "Selected the matching erase build identity");
    ensure_not_cancelled(&cancel)?;

    reporter.stage(
        3,
        "Preparing the filesystem",
        "Extracting the operating-system image to temporary storage…",
        false,
    );
    let os_path = component_path(&build_identity, "OS")?;
    let filesystem_size = ipsw.file_size(&os_path).unwrap_or(0);
    let filesystem_path = restore_filesystem_path(&request.ipsw_path, ecid);
    let _temp_guard = TemporaryFile::new(filesystem_path.clone());
    let output = tokio::fs::File::create(&filesystem_path).await?;
    let mut output = ProgressWriter::new(
        output,
        reporter.clone(),
        cancel.clone(),
        "filesystem extraction",
        filesystem_size,
    );
    if let Err(err) = ipsw.extract_to_writer(&os_path, &mut output).await {
        if cancel.is_cancelled() {
            return Err(IdeviceError::Restore(RestoreError::Cancelled));
        }
        return Err(err);
    }
    output.finish();
    drop(output);
    let mut filesystem = tokio::fs::File::open(&filesystem_path).await?;
    ensure_not_cancelled(&cancel)?;

    reporter.stage(
        4,
        "Requesting Apple signing",
        "Requesting a personalized restore ticket from Apple…",
        true,
    );
    let parameters = plist_macro::plist!({
        "ApProductionMode": true,
        "ApSecurityMode": true,
        "ApSupportsImg4": true
    });
    let parameters = parameters.as_dictionary().unwrap().clone();
    let mut tss = TSSRequest::new();
    tss.set_ap_img4_ticket(true);
    tss.add_common_tags(board_id, chip_id, ecid, Some(ap_nonce), sep_nonce);
    tss.add_ap_tags(&build_identity);
    tss.add_ap_manifest_tags(&build_identity, &parameters)?;
    let response = match tss.send().await? {
        Value::Dictionary(dictionary) => dictionary,
        _ => {
            return Err(IdeviceError::Restore(RestoreError::TssResponse(
                "response is not a dictionary".into(),
            )));
        }
    };
    let ticket = extract_img4_ticket(&response)?;
    reporter.log(
        "info",
        format!("Apple returned a {} byte personalized ticket", ticket.len()),
    );
    ensure_not_cancelled(&cancel)?;

    reporter.stage(
        5,
        "Booting RestoreOS",
        "Uploading the personalized restore boot chain over USB…",
        true,
    );
    boot_to_restore(
        recovery,
        &mut ipsw,
        &build_identity,
        &ticket,
        ecid,
        &reporter,
        &cancel,
    )
    .await?;

    reporter.stage(
        6,
        "Connecting to RestoreOS",
        "Waiting for the device to expose the restored service…",
        true,
    );
    let addr = UsbmuxdAddr::default();
    let mut restored = RestoredClient::connect_by_ecid(
        &addr,
        ecid,
        "idescriptor-restore",
        Duration::from_secs(90),
    )
    .await?;
    let device_id = restored.device_id;
    reporter.log("info", "Connected to com.apple.mobile.restored");

    let fdr_connector = Arc::new(UsbmuxFdrConnector {
        addr: addr.clone(),
        device_id,
    });
    match start_fdr(fdr_connector.clone()).await {
        Ok(()) => reporter.log("info", "FDR trust channel started"),
        Err(err) => reporter.log("warning", format!("FDR did not start; continuing: {err}")),
    }

    reporter.stage(
        7,
        "Installing firmware",
        "RestoreOS is requesting firmware components…",
        true,
    );
    let mut data_ports = UsbmuxDataPorts { addr, device_id };
    let (progress_tx, mut progress_rx) = progress_channel();
    let progress_reporter = reporter.clone();
    let progress_task = tokio::spawn(async move {
        while let Some(event) = progress_rx.recv().await {
            match event {
                RestoreProgressEvent::Operation {
                    operation,
                    progress,
                } => progress_reporter.operation(operation, progress),
                RestoreProgressEvent::Step(step) => progress_reporter.step(step),
                RestoreProgressEvent::Transfer {
                    component,
                    sent,
                    total,
                } => progress_reporter.transfer(component, sent, total),
            }
        }
    });

    let context = RestoreContext {
        restored: &mut restored,
        build_identity: &build_identity,
        board_id,
        chip_id,
        ecid,
        tss_ticket: &ticket,
        components: &mut ipsw,
        filesystem: Some(&mut filesystem as &mut dyn FilesystemImage),
        data_ports: &mut data_ports,
        progress: Some(progress_tx),
        cancel: Some(cancel),
    };
    let result = run_restore(context, RestoreOptions::new().build()).await;
    let _ = progress_task.await;
    result?;

    reporter.stage(
        8,
        "Finalizing",
        "RestoreOS reported success. Waiting for the device to reboot…",
        true,
    );
    reporter.log("info", "Restore completed successfully");
    Ok(())
}

fn ensure_not_cancelled(cancel: &RestoreCancel) -> Result<(), IdeviceError> {
    if cancel.is_cancelled() {
        Err(IdeviceError::Restore(RestoreError::Cancelled))
    } else {
        Ok(())
    }
}

fn required_identifier(value: Option<u64>, name: &str) -> Result<u64, IdeviceError> {
    value.ok_or_else(|| {
        IdeviceError::Restore(RestoreError::Other(format!(
            "recovery device did not report {name}"
        )))
    })
}

async fn open_ipsw(path: &Path) -> Result<Ipsw<IpswFile>, IdeviceError> {
    let file = tokio::fs::File::open(path).await.map_err(|err| {
        IdeviceError::Restore(RestoreError::Ipsw(format!(
            "open {}: {err}",
            path.display()
        )))
    })?;
    Ipsw::new(BufReader::new(file)).await
}

async fn acquire_recovery_device(
    request: &FlashRequest,
    reporter: &FlashReporter,
    cancel: &RestoreCancel,
) -> Result<AcquiredDevice, IdeviceError> {
    ensure_not_cancelled(cancel)?;
    // A retry may start after the normal-mode UDID has disappeared. Prefer a
    // currently connected normal device, otherwise use its previously saved ECID.
    let normal_device = if request.udid.trim().is_empty() {
        None
    } else {
        crate::device_ctx::get_device_opt(request.udid.clone()).await
    };
    let Some(device) = normal_device else {
        let ecid = request.ecid.parse::<u64>().map_err(|_| {
            IdeviceError::Restore(RestoreError::Other(
                "connect the selected device over USB in normal, recovery, or DFU mode".into(),
            ))
        })?;
        reporter.target_ecid(ecid);
        let events = nusb::watch_devices().map_err(usb_error)?;
        reporter.stage(
            2,
            "Waiting for recovery device",
            "Waiting for the selected ECID to remain connected for five seconds…",
            true,
        );
        let transport =
            wait_for_stable_recovery(ecid, Duration::from_secs(90), cancel, events).await?;
        let recovery = RecoveryDevice::new(transport).await?;
        return Ok(AcquiredDevice {
            recovery,
            ap_nonce: None,
            sep_nonce: None,
        });
    };
    if device.is_wireless {
        return Err(IdeviceError::Restore(RestoreError::Other(
            "firmware restore requires a wired USB connection".into(),
        )));
    }

    let mut lockdown = device.lockdown.lock().await;

    let ecid = lockdown
        .get_value(Some("UniqueChipID"), None)
        .await?
        .as_unsigned_integer()
        .ok_or_else(|| {
            IdeviceError::Restore(RestoreError::Other("UniqueChipID is unavailable".into()))
        })?;
    if !request.ecid.is_empty() && request.ecid.parse::<u64>().ok() != Some(ecid) {
        return Err(IdeviceError::Restore(RestoreError::Other(
            "the normal-mode device does not match the selected ECID".into(),
        )));
    }
    reporter.target_ecid(ecid);
    let ap_nonce = read_nonce(&mut lockdown, "ApNonce").await;
    let sep_nonce = read_nonce(&mut lockdown, "SEPNonce").await;
    reporter.log(
        "info",
        format!("Read personalization data for ECID {ecid:#x}"),
    );
    ensure_not_cancelled(cancel)?;
    // Subscribe before requesting recovery so even a fast reconnect is observed.
    let events = nusb::watch_devices().map_err(usb_error)?;
    reporter.stage(
        2,
        "Entering recovery mode",
        "Requesting recovery mode for the selected device…",
        true,
    );
    lockdown.enter_recovery().await?;
    drop(lockdown);

    reporter.stage(
        2,
        "Waiting for recovery device",
        "Waiting for the same ECID to remain connected for five seconds…",
        true,
    );
    let transport = wait_for_stable_recovery(ecid, Duration::from_secs(90), cancel, events).await?;
    let recovery = RecoveryDevice::new(transport).await?;
    Ok(AcquiredDevice {
        recovery,
        ap_nonce,
        sep_nonce,
    })
}

async fn read_nonce(lockdown: &mut LockdownClient, key: &str) -> Option<Vec<u8>> {
    lockdown
        .get_value(Some(key), None)
        .await
        .ok()
        .and_then(|value| value.as_data().map(ToOwned::to_owned))
}

async fn send_component(
    recovery: &mut RecoveryDevice,
    ipsw: &mut Ipsw<IpswFile>,
    build_identity: &plist::Dictionary,
    ticket: &[u8],
    name: &str,
    reporter: &FlashReporter,
) -> Result<(), IdeviceError> {
    let raw = ipsw.read_component(build_identity, name).await?;
    let personalized =
        img4::stitch_component(&raw, ticket, img4::restore_fourcc_override(name), &[])?;
    reporter.log(
        "info",
        format!("Uploading {name} ({} bytes)", personalized.len()),
    );
    recovery.send_buffer(&personalized).await
}

async fn boot_to_restore(
    mut recovery: RecoveryDevice,
    ipsw: &mut Ipsw<IpswFile>,
    build_identity: &plist::Dictionary,
    ticket: &[u8],
    ecid: u64,
    reporter: &FlashReporter,
    cancel: &RestoreCancel,
) -> Result<(), IdeviceError> {
    if recovery.mode().is_recovery() {
        send_component(
            &mut recovery,
            ipsw,
            build_identity,
            ticket,
            "iBEC",
            reporter,
        )
        .await?;
        let _ = recovery.send_command_with_request("go", 1).await;
        let _ = recovery.finish_transfer().await;
        drop(recovery);
        tokio::time::sleep(Duration::from_secs(3)).await;
        let transport = find_recovery_transport(Some(ecid), Duration::from_secs(45)).await?;
        recovery = RecoveryDevice::new(transport).await?;
    }
    ensure_not_cancelled(cancel)?;

    for _ in 0..30 {
        if recovery
            .getenv("build-version")
            .await
            .is_ok_and(|value| !value.is_empty())
        {
            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
        ensure_not_cancelled(cancel)?;
    }
    recovery.set_autoboot(false).await?;

    if send_component(
        &mut recovery,
        ipsw,
        build_identity,
        ticket,
        "RestoreLogo",
        reporter,
    )
    .await
    .is_ok()
    {
        let _ = recovery.send_command("setpicture 4").await;
        let _ = recovery.send_command("bgcolor 0 0 0").await;
    }
    for name in components_loaded_by_iboot(build_identity) {
        ensure_not_cancelled(cancel)?;
        send_component(&mut recovery, ipsw, build_identity, ticket, &name, reporter).await?;
        recovery.send_command("firmware").await?;
    }
    send_component(
        &mut recovery,
        ipsw,
        build_identity,
        ticket,
        "RestoreRamDisk",
        reporter,
    )
    .await?;
    recovery.send_command("ramdisk").await?;
    tokio::time::sleep(Duration::from_secs(2)).await;
    send_component(
        &mut recovery,
        ipsw,
        build_identity,
        ticket,
        "RestoreDeviceTree",
        reporter,
    )
    .await?;
    recovery.send_command("devicetree").await?;
    if has_component(build_identity, "RestoreSEP")
        && send_component(
            &mut recovery,
            ipsw,
            build_identity,
            ticket,
            "RestoreSEP",
            reporter,
        )
        .await
        .is_ok()
    {
        let _ = recovery.send_command("rsepfirmware").await;
    }
    send_component(
        &mut recovery,
        ipsw,
        build_identity,
        ticket,
        "RestoreKernelCache",
        reporter,
    )
    .await?;
    let _ = recovery.finish_transfer().await;
    recovery
        .send_command("setenv boot-args rd=md0 nand-enable-reformat=1 -progress")
        .await?;
    let _ = recovery.send_command_with_request("bootx", 1).await;
    Ok(())
}

fn manifest_supports_img4(manifest: &plist::Dictionary) -> bool {
    manifest
        .get("BuildIdentities")
        .and_then(Value::as_array)
        .is_some_and(|identities| {
            identities
                .iter()
                .filter_map(Value::as_dictionary)
                .any(build_identity_supports_img4)
        })
}

fn build_identity_supports_img4(identity: &plist::Dictionary) -> bool {
    if identity.get("Ap,SupportsImg4").and_then(Value::as_boolean) == Some(true) {
        return true;
    }

    // Apple does not include `Ap,SupportsImg4` in every BuildManifest. Modern
    // manifests still identify the format through their `.im4p` components.
    identity
        .get("Manifest")
        .and_then(Value::as_dictionary)
        .is_some_and(|components| {
            components.values().any(|component| {
                component
                    .as_dictionary()
                    .and_then(|component| component.get("Info"))
                    .and_then(Value::as_dictionary)
                    .and_then(|info| info.get("Path"))
                    .and_then(Value::as_string)
                    .is_some_and(|path| path.to_ascii_lowercase().ends_with(".im4p"))
            })
        })
}

fn has_component(identity: &plist::Dictionary, name: &str) -> bool {
    identity
        .get("Manifest")
        .and_then(Value::as_dictionary)
        .is_some_and(|manifest| manifest.contains_key(name))
}

fn restore_filesystem_path(ipsw: &Path, ecid: u64) -> PathBuf {
    let name = ipsw
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("firmware.ipsw");
    ipsw.with_file_name(format!(".{name}.{ecid:x}.restore.dmg"))
}

fn verification_sidecar(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.verified", path.to_string_lossy()))
}

struct TemporaryFile(PathBuf);
impl TemporaryFile {
    fn new(path: PathBuf) -> Self {
        Self(path)
    }
}
impl Drop for TemporaryFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

struct ProgressWriter {
    file: tokio::fs::File,
    reporter: FlashReporter,
    cancel: RestoreCancel,
    component: String,
    total: u64,
    sent: u64,
    last_report: Instant,
}

impl ProgressWriter {
    fn new(
        file: tokio::fs::File,
        reporter: FlashReporter,
        cancel: RestoreCancel,
        component: &str,
        total: u64,
    ) -> Self {
        Self {
            file,
            reporter,
            cancel,
            component: component.into(),
            total,
            sent: 0,
            last_report: Instant::now() - Duration::from_secs(1),
        }
    }
    fn finish(&self) {
        self.reporter
            .transfer(self.component.clone(), self.sent, Some(self.total));
    }
}

impl AsyncWrite for ProgressWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        if self.cancel.is_cancelled() {
            return Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "restore cancelled",
            )));
        }
        match Pin::new(&mut self.file).poll_write(cx, buf) {
            Poll::Ready(Ok(written)) => {
                self.sent = self.sent.saturating_add(written as u64);
                if self.last_report.elapsed() >= Duration::from_millis(250) {
                    self.reporter
                        .transfer(self.component.clone(), self.sent, Some(self.total));
                    self.last_report = Instant::now();
                }
                Poll::Ready(Ok(written))
            }
            other => other,
        }
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.file).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.file).poll_shutdown(cx)
    }
}

async fn start_fdr(connector: Arc<UsbmuxFdrConnector>) -> Result<(), IdeviceError> {
    let mut last_error = None;
    for attempt in 0..5 {
        match connector.connect_device_port(FDR_CTRL_PORT).await {
            Ok(connection) => {
                let mut fdr = FdrClient::new(connection);
                match fdr.ctrl_handshake().await {
                    Ok(port) => {
                        tokio::spawn(run_fdr_listener(fdr, connector, port));
                        return Ok(());
                    }
                    Err(err) => last_error = Some(err),
                }
            }
            Err(err) => last_error = Some(err),
        }
        if attempt < 4 {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
    Err(last_error.unwrap_or_else(|| {
        IdeviceError::Restore(RestoreError::Other("FDR failed to start".into()))
    }))
}

async fn connect_usb_device_port(
    addr: &UsbmuxdAddr,
    device_id: Option<u32>,
    port: u16,
    label: &str,
) -> Result<Idevice, IdeviceError> {
    let device_id = match device_id {
        Some(id) => id,
        None => {
            let mut mux = addr.connect(1).await?;
            mux.get_devices()
                .await?
                .into_iter()
                .find(|device| device.connection_type == Connection::Usb)
                .ok_or(IdeviceError::DeviceNotFound)?
                .device_id
        }
    };
    addr.connect(1)
        .await?
        .connect_to_device(device_id, port, label)
        .await
}

struct UsbmuxDataPorts {
    addr: UsbmuxdAddr,
    device_id: Option<u32>,
}
impl DataPortConnector for UsbmuxDataPorts {
    fn connect(
        &self,
        port: u16,
    ) -> Pin<Box<dyn Future<Output = Result<Idevice, IdeviceError>> + Send>> {
        let addr = self.addr.clone();
        let device_id = self.device_id;
        Box::pin(async move {
            connect_usb_device_port(&addr, device_id, port, "idescriptor-restore-data").await
        })
    }
}

struct UsbmuxFdrConnector {
    addr: UsbmuxdAddr,
    device_id: Option<u32>,
}
impl FdrConnector for UsbmuxFdrConnector {
    fn connect_device_port(
        &self,
        port: u16,
    ) -> Pin<Box<dyn Future<Output = Result<Idevice, IdeviceError>> + Send>> {
        let addr = self.addr.clone();
        let device_id = self.device_id;
        Box::pin(async move {
            connect_usb_device_port(&addr, device_id, port, "idescriptor-restore-fdr").await
        })
    }
}

fn usb_error(error: impl std::fmt::Display) -> IdeviceError {
    IdeviceError::Restore(RestoreError::Recovery(format!("USB: {error}")))
}

struct NusbRecoveryTransport {
    device: nusb::Device,
    interface: Option<nusb::Interface>,
    product_id: u16,
    serial: String,
}
impl std::fmt::Debug for NusbRecoveryTransport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NusbRecoveryTransport")
            .field("product_id", &format_args!("{:#06x}", self.product_id))
            .finish_non_exhaustive()
    }
}
impl RecoveryTransport for NusbRecoveryTransport {
    fn control_out<'a>(
        &'a mut self,
        setup: ControlSetup,
        data: &'a [u8],
        timeout_ms: u32,
    ) -> RecoveryFuture<'a, usize> {
        let device = self.device.clone();
        let data = data.to_vec();
        Box::pin(async move {
            let control_type = match (setup.request_type >> 5) & 0b11 {
                0 => ControlType::Standard,
                1 => ControlType::Class,
                _ => ControlType::Vendor,
            };
            let recipient = match setup.request_type & 0x1F {
                1 => Recipient::Interface,
                2 => Recipient::Endpoint,
                _ => Recipient::Device,
            };
            device
                .control_out(
                    ControlOut {
                        control_type,
                        recipient,
                        request: setup.request,
                        value: setup.value,
                        index: setup.index,
                        data: &data,
                    },
                    Duration::from_millis(timeout_ms as u64),
                )
                .await
                .map_err(usb_error)?;
            Ok(data.len())
        })
    }
    fn control_in<'a>(
        &'a mut self,
        setup: ControlSetup,
        length: u16,
        timeout_ms: u32,
    ) -> RecoveryFuture<'a, Vec<u8>> {
        let device = self.device.clone();
        Box::pin(async move {
            let control_type = match (setup.request_type >> 5) & 0b11 {
                0 => ControlType::Standard,
                1 => ControlType::Class,
                _ => ControlType::Vendor,
            };
            let recipient = match setup.request_type & 0x1F {
                1 => Recipient::Interface,
                2 => Recipient::Endpoint,
                _ => Recipient::Device,
            };
            device
                .control_in(
                    ControlIn {
                        control_type,
                        recipient,
                        request: setup.request,
                        value: setup.value,
                        index: setup.index,
                        length,
                    },
                    Duration::from_millis(timeout_ms as u64),
                )
                .await
                .map_err(usb_error)
        })
    }
    fn bulk_out<'a>(
        &'a mut self,
        endpoint: u8,
        data: &'a [u8],
        _timeout_ms: u32,
    ) -> RecoveryFuture<'a, usize> {
        let interface = self.interface.clone();
        let data = data.to_vec();
        Box::pin(async move {
            let interface =
                interface.ok_or_else(|| usb_error("no claimed interface for bulk transfer"))?;
            let mut endpoint = interface
                .endpoint::<Bulk, Out>(endpoint)
                .map_err(usb_error)?;
            let length = data.len();
            endpoint.submit(Buffer::from(data));
            endpoint.next_complete().await.status.map_err(usb_error)?;
            Ok(length)
        })
    }
    fn serial_number(&mut self) -> RecoveryFuture<'_, String> {
        let serial = self.serial.clone();
        Box::pin(async move { Ok(serial) })
    }
    fn nonce_descriptor(&mut self) -> RecoveryFuture<'_, String> {
        let device = self.device.clone();
        Box::pin(async move {
            let index = NonZeroU8::new(1).expect("USB descriptor index is nonzero");
            let timeout = Duration::from_secs(10);
            let language = match device
                .get_string_descriptor_supported_languages(timeout)
                .await
            {
                Ok(mut languages) => languages
                    .next()
                    .unwrap_or(nusb::descriptors::language_id::US_ENGLISH),
                Err(err) => {
                    debug!("Unable to read recovery USB descriptor languages: {err}");
                    nusb::descriptors::language_id::US_ENGLISH
                }
            };
            match device.get_string_descriptor(index, language, timeout).await {
                Ok(value) => Ok(value),
                Err(err) => {
                    debug!("Unable to read recovery nonce descriptor: {err}");
                    Ok(String::new())
                }
            }
        })
    }
    fn product_id(&self) -> u16 {
        self.product_id
    }
    fn set_configuration(&mut self, configuration: u8) -> RecoveryFuture<'_, ()> {
        let device = self.device.clone();
        Box::pin(async move {
            device
                .set_configuration(configuration)
                .await
                .map_err(usb_error)
        })
    }
    fn claim_interface(&mut self, interface: u8, alt_setting: u8) -> RecoveryFuture<'_, ()> {
        Box::pin(async move {
            let claimed = self
                .device
                .claim_interface(interface)
                .await
                .map_err(usb_error)?;
            if alt_setting != 0 {
                claimed
                    .set_alt_setting(alt_setting)
                    .await
                    .map_err(usb_error)?;
            }
            self.interface = Some(claimed);
            Ok(())
        })
    }
    fn reset(&mut self) -> RecoveryFuture<'_, ()> {
        let device = self.device.clone();
        Box::pin(async move { device.reset().await.map_err(usb_error) })
    }
}

// Track one USB connection, not just a model or ECID. Re-enumeration or a mode
// change restarts settling even if it is still the same physical device.
struct RecoveryPresence<T> {
    connection: Option<(T, Instant)>,
}

impl<T> Default for RecoveryPresence<T> {
    fn default() -> Self {
        Self { connection: None }
    }
}

impl<T: Copy + Eq> RecoveryPresence<T> {
    fn observe(&mut self, identity: T, now: Instant) {
        if self
            .connection
            .as_ref()
            .is_none_or(|(current, _)| *current != identity)
        {
            self.connection = Some((identity, now));
        }
    }

    fn disconnect(&mut self, identity: T) {
        if self
            .connection
            .as_ref()
            .is_some_and(|(current, _)| *current == identity)
        {
            self.connection = None;
        }
    }

    fn ready(&self, now: Instant, settling: Duration) -> bool {
        self.connection
            .as_ref()
            .is_some_and(|(_, since)| now.duration_since(*since) >= settling)
    }
}

fn matching_recovery_device(device: &nusb::DeviceInfo, ecid: u64) -> bool {
    device.vendor_id() == APPLE_VENDOR_ID
        && Mode::from_product_id(device.product_id()).is_some()
        && DeviceInfo::parse(device.serial_number().unwrap_or_default()).ecid == Some(ecid)
}

async fn wait_for_flash_cancellation(cancel: &RestoreCancel) {
    while !cancel.is_cancelled() {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn wait_for_stable_recovery(
    ecid: u64,
    timeout: Duration,
    cancel: &RestoreCancel,
    mut events: nusb::hotplug::HotplugWatch,
) -> Result<Box<dyn RecoveryTransport>, IdeviceError> {
    ensure_not_cancelled(cancel)?;
    let wait = async {
        let mut candidate = nusb::list_devices()
            .await
            .map_err(usb_error)?
            .find(|device| matching_recovery_device(device, ecid));
        let mut presence = RecoveryPresence::default();
        if let Some(device) = &candidate {
            presence.observe((device.id(), device.product_id()), Instant::now());
        }
        let mut tick = tokio::time::interval(Duration::from_millis(250));
        loop {
            tokio::select! {
                // Drain queued disconnects before deciding a connection has settled.
                biased;
                event = events.next() => match event {
                    Some(nusb::hotplug::HotplugEvent::Connected(device)) if matching_recovery_device(&device, ecid) => {
                        presence.observe((device.id(), device.product_id()), Instant::now());
                        candidate = Some(device);
                    }
                    Some(nusb::hotplug::HotplugEvent::Disconnected(id)) => {
                        if let Some(device) = &candidate {
                            if device.id() == id {
                                presence.disconnect((device.id(), device.product_id()));
                                candidate = None;
                            }
                        }
                    }
                    Some(_) => {}
                    None => return Err(IdeviceError::Restore(RestoreError::Other("USB device watcher stopped".into()))),
                },
                _ = tick.tick() => {
                    let Some(device_info) = &candidate else { continue };
                    if !presence.ready(Instant::now(), Duration::from_secs(5)) { continue; }
                    let still_present = nusb::list_devices().await.map_err(usb_error)?
                        .any(|device| device.id() == device_info.id() && device.product_id() == device_info.product_id()
                            && matching_recovery_device(&device, ecid));
                    if !still_present {
                        presence.disconnect((device_info.id(), device_info.product_id()));
                        candidate = None;
                        continue;
                    }
                    let device = match device_info.open().await {
                        Ok(device) => device,
                        Err(_) => {
                            // Retry access within the original deadline, with a fresh settling period.
                            presence.connection = None;
                            presence.observe((device_info.id(), device_info.product_id()), Instant::now());
                            continue;
                        }
                    };
                    // Opening may yield while a disconnect/reconnect is queued.
                    // Process those events before accepting this transport.
                    let mut disconnected = false;
                    while let Some(Some(event)) = events.next().now_or_never() {
                        if let nusb::hotplug::HotplugEvent::Disconnected(id) = event {
                            if id == device_info.id() { disconnected = true; }
                        }
                    }
                    if disconnected {
                        presence.connection = None;
                        candidate = nusb::list_devices().await.map_err(usb_error)?
                            .find(|device| matching_recovery_device(device, ecid));
                        if let Some(device) = &candidate {
                            presence.observe((device.id(), device.product_id()), Instant::now());
                        }
                        continue;
                    }
                    ensure_not_cancelled(cancel)?;
                    return Ok(Box::new(NusbRecoveryTransport {
                        device,
                        interface: None,
                        product_id: device_info.product_id(),
                        serial: device_info.serial_number().unwrap_or_default().to_string(),
                    }) as Box<dyn RecoveryTransport>);
                }
            }
        }
    };
    tokio::select! {
        biased;
        _ = wait_for_flash_cancellation(cancel) => Err(IdeviceError::Restore(RestoreError::Cancelled)),
        result = tokio::time::timeout(timeout, wait) => result.unwrap_or_else(|_| {
            Err(IdeviceError::Restore(RestoreError::Other(format!(
                "device ECID {ecid} did not remain available in recovery/DFU mode for five seconds within {} seconds; reconnect it over USB and retry",
                timeout.as_secs()
            ))))
        }),
    }
}

async fn find_recovery_transport(
    ecid: Option<u64>,
    timeout: Duration,
) -> Result<Box<dyn RecoveryTransport>, IdeviceError> {
    let deadline = Instant::now() + timeout;
    loop {
        for device_info in nusb::list_devices().await.map_err(usb_error)? {
            if device_info.vendor_id() != APPLE_VENDOR_ID
                || Mode::from_product_id(device_info.product_id()).is_none()
            {
                continue;
            }
            let serial = device_info.serial_number().unwrap_or_default().to_string();
            if ecid.is_some_and(|expected| DeviceInfo::parse(&serial).ecid != Some(expected)) {
                continue;
            }
            let device = match device_info.open().await {
                Ok(device) => device,
                Err(_) => continue,
            };
            return Ok(Box::new(NusbRecoveryTransport {
                device,
                interface: None,
                product_id: device_info.product_id(),
                serial,
            }));
        }
        if Instant::now() >= deadline {
            return Err(IdeviceError::DeviceNotFound);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

#[cfg(test)]
mod recovery_presence_tests {
    use super::*;

    #[test]
    fn requires_five_seconds_on_the_same_connection() {
        let now = Instant::now();
        let settling = Duration::from_secs(5);
        let mut presence = RecoveryPresence::default();
        presence.observe((1_u8, 0x1281_u16), now);
        presence.observe((1, 0x1281), now + Duration::from_secs(4));
        assert!(!presence.ready(now + Duration::from_secs(4), settling));
        assert!(presence.ready(now + settling, settling));
    }

    #[test]
    fn reconnect_and_mode_change_restart_settling() {
        let now = Instant::now();
        let settling = Duration::from_secs(5);
        let mut presence = RecoveryPresence::default();
        presence.observe((1_u8, 0x1281_u16), now);
        presence.disconnect((1, 0x1281));
        assert!(!presence.ready(now + Duration::from_secs(10), settling));
        presence.observe((2, 0x1281), now + Duration::from_secs(10));
        assert!(!presence.ready(now + Duration::from_secs(14), settling));
        presence.observe((2, 0x1227), now + Duration::from_secs(14));
        assert!(!presence.ready(now + Duration::from_secs(18), settling));
        assert!(presence.ready(now + Duration::from_secs(19), settling));
    }

    #[test]
    fn unrelated_disconnect_does_not_interrupt_target() {
        let now = Instant::now();
        let mut presence = RecoveryPresence::default();
        presence.observe(1_u8, now);
        presence.disconnect(2);
        assert!(presence.ready(now + Duration::from_secs(5), Duration::from_secs(5)));
    }

    #[tokio::test]
    async fn cancellation_is_observed_while_waiting() {
        let cancel = RestoreCancel::new();
        let worker_cancel = cancel.clone();
        let worker = tokio::spawn(async move {
            wait_for_flash_cancellation(&worker_cancel).await;
        });
        tokio::task::yield_now().await;
        cancel.cancel();
        tokio::time::timeout(Duration::from_secs(1), worker)
            .await
            .unwrap()
            .unwrap();
    }
}
