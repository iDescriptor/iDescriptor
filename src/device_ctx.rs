// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

use idevice::{
    afc::AfcClient, diagnostics_relay::DiagnosticsRelayClient, lockdown::LockdownClient,
};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::{image_cache, image_loader, media_streamer::MediaStreamSession};

#[derive(Clone)]
#[allow(non_camel_case_types)]
pub struct iOSVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub raw_version_str: String,
}

impl iOSVersion {
    pub fn new(major: u32, minor: u32, patch: u32, raw_version_str: String) -> Self {
        Self {
            major,
            minor,
            patch,
            raw_version_str,
        }
    }

    pub fn from_str_opt(version_str: &str) -> Option<Self> {
        let parts: Vec<&str> = version_str.split('.').collect();
        if parts.len() != 3 {
            return None;
        }

        let major = parts[0].parse::<u32>().ok()?;
        let minor = parts[1].parse::<u32>().ok()?;
        let patch = parts[2].parse::<u32>().ok()?;

        Some(Self::new(major, minor, patch, version_str.to_string()))
    }

    pub fn from_str(version_str: &str) -> Self {
        let parts: Vec<&str> = version_str.split('.').collect();

        let major = parts
            .get(0)
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);
        let minor = parts
            .get(1)
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);
        let patch = parts
            .get(2)
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);

        Self::new(major, minor, patch, version_str.to_string())
    }
}

#[derive(Clone)]
pub struct DeviceServices {
    pub connection_id: u64,
    pub is_wireless: bool,
    pub afc: Arc<Mutex<AfcClient>>,
    pub afc_secondary: Arc<Mutex<AfcClient>>,
    pub afc2: Option<Arc<Mutex<AfcClient>>>,
    pub afc2_secondary: Option<Arc<Mutex<AfcClient>>>,
    pub diag: Arc<Mutex<DiagnosticsRelayClient>>,
    pub heartbeat_task: Option<Arc<JoinHandle<()>>>,
    pub video_streams: Arc<Mutex<HashMap<String, MediaStreamSession>>>,
    pub provider: Arc<Mutex<Box<dyn idevice::provider::IdeviceProvider>>>,
    pub lockdown: Arc<Mutex<LockdownClient>>,
    pub ios_version: iOSVersion,
}

static APP_DEVICE_STATE: Lazy<Mutex<HashMap<String, DeviceServices>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub async fn get_device(udid: impl Into<String>) -> anyhow::Result<DeviceServices> {
    let udid_str = udid.into();
    let maybe_device = APP_DEVICE_STATE.lock().await.get(&udid_str).cloned();

    match maybe_device {
        Some(d) => Ok(d),
        None => anyhow::bail!(format!("Device with udid {} does not exist", udid_str)),
    }
}

pub async fn get_device_opt(udid: impl Into<String>) -> Option<DeviceServices> {
    let udid_str = udid.into();
    APP_DEVICE_STATE.lock().await.get(&udid_str).cloned()
}

pub async fn get_device_for_connection_opt(
    udid: impl Into<String>,
    connection_id: u64,
) -> Option<DeviceServices> {
    let udid = udid.into();
    APP_DEVICE_STATE
        .lock()
        .await
        .get(&udid)
        .filter(|device| device.connection_id == connection_id)
        .cloned()
}

async fn clean_removed_device(udid: &str, svc: DeviceServices, abort_heartbeat: bool) {
    image_loader::cancel_for_udid(udid);

    if abort_heartbeat {
        if let Some(task) = &svc.heartbeat_task {
            task.abort();
        }
    }

    let sessions = {
        let mut streams = svc.video_streams.lock().await;
        streams
            .drain()
            .map(|(_, session)| session)
            .collect::<Vec<_>>()
    };
    for mut session in sessions {
        session.shutdown().await;
    }

    image_cache::clear_for_udid(udid);
}

pub async fn clean_device_from_app_state_if_current(
    udid: &str,
    connection_id: u64,
    abort_heartbeat: bool,
) -> bool {
    let svc = {
        let mut state = APP_DEVICE_STATE.lock().await;
        match state.get(udid) {
            Some(device) if device.connection_id == connection_id => state.remove(udid),
            _ => None,
        }
    };

    if let Some(svc) = svc {
        clean_removed_device(udid, svc, abort_heartbeat).await;
        println!(
            "Removed device with UDID {} connection {}",
            udid, connection_id
        );
        true
    } else {
        log::debug!(
            "Ignored stale removal for UDID {} connection {}",
            udid,
            connection_id
        );
        false
    }
}

pub enum InsertDeviceResult {
    Inserted,
    Replaced(u64),
    Rejected,
}

/// Inserts a complete device connection. Older initializations are rejected so they
/// cannot replace a newer connection that finished first.
pub async fn insert_device(
    udid: impl Into<String>,
    services: DeviceServices,
) -> InsertDeviceResult {
    let udid = udid.into();
    let mut state = APP_DEVICE_STATE.lock().await;
    if let Some(current) = state.get(&udid) {
        if current.connection_id > services.connection_id {
            return InsertDeviceResult::Rejected;
        }
        if !current.is_wireless && services.is_wireless {
            log::info!(
                "Rejecting wireless connection {} for udid {} because USB connection {} is active",
                services.connection_id,
                udid,
                current.connection_id
            );
            return InsertDeviceResult::Rejected;
        }
    }

    if let Some(mut old) = state.insert(udid.clone(), services) {
        let old_connection_id = old.connection_id;
        if let Some(task) = old.heartbeat_task.take() {
            task.abort();
        }
        eprintln!(
            "Replaced existing device connection - UDID {} connection {}",
            udid, old_connection_id
        );
        return InsertDeviceResult::Replaced(old_connection_id);
    }
    InsertDeviceResult::Inserted
}
