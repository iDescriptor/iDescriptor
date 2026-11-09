// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    RUNTIME,
    list_model::ListModel,
    qt_threading::{QtThread, QtThreading},
    qvariantmap_insert,
};
use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, NaiveDate, Utc};
use futures::StreamExt;
use log::{debug, error, info, warn};
use macros::QtThreading;
use qmetaobject::{SimpleListItem, prelude::*};
use qttypes::{QString, QStringList, QVariantList, QVariantMap};

use super::local;
use reqwest::{Client, StatusCode, header};
use serde_json::Value;
use sha1::{Digest as _, Sha1};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};
use tokio::{io::AsyncWriteExt, task::JoinHandle};
use tokio_util::sync::CancellationToken;

const API_BASE: &str = "https://api.ipsw.me/v4";

#[derive(Debug, Default, Clone)]
pub(super) struct FirmwareInfo {
    pub(super) identifier: String,
    pub(super) version: String,
    pub(super) build_id: String,
    pub(super) release_date: String,
    pub(super) file_size: u64,
    pub(super) url: String,
    pub(super) signed: bool,
    pub(super) sha1: String,
    pub(super) sha256: String,
}

impl FirmwareInfo {
    fn key(&self) -> String {
        firmware_key(&self.identifier, &self.build_id)
    }

    pub(super) fn file_name(&self) -> String {
        format!(
            "{}_{}_{}_Restore.ipsw",
            safe_file_segment(&self.identifier),
            safe_file_segment(&self.version),
            safe_file_segment(&self.build_id)
        )
    }

    pub(super) fn expected_digest(&self) -> Option<(&'static str, &str)> {
        if !self.sha256.is_empty() {
            Some(("sha256", &self.sha256))
        } else if !self.sha1.is_empty() {
            Some(("sha1", &self.sha1))
        } else {
            None
        }
    }
}

#[derive(SimpleListItem, Default, Clone)]
struct FirmwareItem {
    pub identifier: QString,
    pub supported_identifiers: QStringList,
    pub flash_identifiers: QStringList,
    pub file_name: QString,
    pub error: QString,
    pub version: QString,
    pub build_id: QString,
    pub release_date: QString,
    pub file_size: f64,
    pub signed: bool,
    pub local_path: QString,
    pub is_downloaded: bool,
    pub is_downloading: bool,
    pub is_verifying: bool,
    pub has_partial: bool,
    pub progress: f64,
    pub downloaded_bytes: f64,
    pub download_speed: f64,
    pub status: QString,
}

#[allow(non_snake_case)]
#[derive(QObject, Default, QtThreading)]
pub struct FirmwareManager {
    base: qt_base_class!(trait QObject),
    devices: qt_property!(QVariantList; NOTIFY devicesChanged),
    firmware_model: qt_property!(RefCell<ListModel<FirmwareItem>>; NOTIFY firmwareModelChanged),
    local_model: qt_property!(RefCell<ListModel<FirmwareItem>>; NOTIFY localModelChanged),
    local_loading: qt_property!(bool; NOTIFY localLoadingChanged),
    local_error: qt_property!(QString; NOTIFY localErrorChanged),
    local_active: qt_property!(bool; NOTIFY localActiveChanged),
    local_directory: PathBuf,
    local_revision: u64,
    local_files: HashMap<PathBuf, local::LocalFile>,
    local_jobs: HashSet<PathBuf>,
    local_failures: HashMap<PathBuf, String>,
    active_downloads: Mutex<HashMap<String, JoinHandle<()>>>,
    catalogs: Mutex<HashMap<String, Vec<FirmwareInfo>>>,
    stopping_downloads: HashSet<String>,
    download_cancellations: HashMap<String, CancellationToken>,
    download_paths: HashMap<String, PathBuf>,
    device_catalog_revision: u64,
    catalog_revision: u64,

    load_devices: qt_method!(fn(&mut self, refresh: bool)),
    fetch_firmwares: qt_method!(fn(&mut self, identifier: QString, dir: QString)),
    refresh_firmwares: qt_method!(fn(&mut self, identifier: QString, dir: QString)),
    download_firmware: qt_method!(
        fn(&mut self, identifier: QString, build_id: QString, dir: QString, force: bool) -> bool
    ),
    pause_download: qt_method!(fn(&mut self, identifier: QString, build_id: QString) -> bool),
    cancel_download: qt_method!(fn(&mut self, identifier: QString, build_id: QString) -> bool),
    verify_firmware:
        qt_method!(fn(&mut self, identifier: QString, build_id: QString, dir: QString) -> bool),
    scan_local_files: qt_method!(fn(&mut self, dir: QString)),
    verify_local_file: qt_method!(fn(&mut self, path: QString) -> bool),
    resume_local_download: qt_method!(fn(&mut self, path: QString) -> bool),
    cancel_local_download: qt_method!(fn(&mut self, path: QString) -> bool),
    get_item: qt_method!(fn(&self, index: i32) -> QVariantMap),

    localModelChanged: qt_signal!(),
    localLoadingChanged: qt_signal!(),
    localErrorChanged: qt_signal!(),
    localActiveChanged: qt_signal!(),
    localOperationFinished: qt_signal!(operation: QString, path: QString, success: bool, error: QString),
    devicesChanged: qt_signal!(),
    firmwareModelChanged: qt_signal!(),
    deviceCatalogFinished: qt_signal!(refreshed: bool, success: bool, error: QString),
    firmwareListFinished: qt_signal!(identifier: QString, refreshed: bool, success: bool, error: QString),
    downloadProgress: qt_signal!(identifier: QString, build_id: QString, downloaded_bytes: f64, total_bytes: f64, progress: f64, download_speed: f64),
    downloadFinished: qt_signal!(identifier: QString, build_id: QString, success: bool, path: QString, error: QString),
    downloadPaused: qt_signal!(identifier: QString, build_id: QString),
    downloadStopFailed: qt_signal!(identifier: QString, build_id: QString, error: QString),
    downloadCancelled: qt_signal!(identifier: QString, build_id: QString),
    verificationFinished: qt_signal!(identifier: QString, build_id: QString, success: bool, error: QString),
}

impl FirmwareManager {
    fn refresh_local_files(&mut self) {
        if !self.local_directory.as_os_str().is_empty() {
            self.scan_local_files(QString::from(
                self.local_directory.to_string_lossy().into_owned(),
            ));
        }
    }

    fn scan_local_files(&mut self, dir: QString) {
        let directory = PathBuf::from(dir.to_string());
        if directory != self.local_directory {
            self.local_model.borrow_mut().reset_data(Vec::new());
            self.local_files.clear();
        }
        self.local_directory = directory.clone();
        self.local_revision = self.local_revision.wrapping_add(1);
        let revision = self.local_revision;
        self.local_loading = true;
        self.localLoadingChanged();
        self.local_error = QString::default();
        self.localErrorChanged();
        let cached = self.local_files.clone();
        let qt_thread = self.qt_thread();
        RUNTIME.spawn(async move {
            let result =
                tokio::task::spawn_blocking(move || local::scan(&directory, &cached)).await;
            let result = result
                .map_err(anyhow::Error::from)
                .and_then(|result| result);
            qt_thread.queue(move |manager| {
                if manager.local_revision != revision {
                    return;
                }
                manager.local_loading = false;
                manager.localLoadingChanged();
                manager.local_active = manager
                    .download_paths
                    .values()
                    .any(|path| path.parent() == Some(manager.local_directory.as_path()))
                    || manager
                        .local_jobs
                        .iter()
                        .any(|path| path.parent() == Some(manager.local_directory.as_path()));
                manager.localActiveChanged();
                match result {
                    Ok(files) => {
                        let catalogs = manager
                            .catalogs
                            .lock()
                            .ok()
                            .map(|catalogs| catalogs.clone())
                            .unwrap_or_default();
                        let items = files
                            .iter()
                            .map(|file| {
                                let mut item = local_firmware_item(file, &catalogs);
                                let key = firmware_key(
                                    &item.identifier.to_string(),
                                    &item.build_id.to_string(),
                                );
                                let active_path = manager.download_paths.get(&key);
                                let active_here = active_path.is_some_and(|path| {
                                    file.path == *path
                                        || file.path == path.with_extension("ipsw.part")
                                });
                                let stopping =
                                    active_here && manager.stopping_downloads.contains(&key);
                                item.is_downloading = active_here && !stopping;
                                item.is_verifying =
                                    stopping || manager.local_jobs.contains(&file.path);
                                if stopping {
                                    item.status = QString::from("stopping");
                                } else if item.is_downloading {
                                    item.status = QString::from("downloading");
                                } else if item.is_verifying {
                                    item.status = QString::from("verifying");
                                }
                                if let Some(error) = manager.local_failures.get(&file.path) {
                                    item.error = QString::from(error.clone());
                                    if !item.is_verifying {
                                        item.status = QString::from("failed");
                                    }
                                }
                                item
                            })
                            .collect::<Vec<_>>();
                        manager.local_files = files
                            .into_iter()
                            .map(|file| (file.path.clone(), file))
                            .collect();
                        let same_paths = {
                            let model = manager.local_model.borrow();
                            model.values.len() == items.len()
                                && model
                                    .values
                                    .iter()
                                    .zip(&items)
                                    .all(|(a, b)| a.local_path == b.local_path)
                        };
                        let mut model = manager.local_model.borrow_mut();
                        if same_paths {
                            for (index, item) in items.into_iter().enumerate() {
                                model.change_line(index, item);
                            }
                        } else {
                            model.reset_data(items);
                        }
                    }
                    Err(error) => {
                        error!("Failed to scan local IPSWs: {error:#}");
                        manager.local_error = QString::from(format!("{error:#}"));
                        manager.localErrorChanged();
                    }
                }
            });
        });
    }

    fn verify_local_file(&mut self, path: QString) -> bool {
        self.prepare_local_file(path, false)
    }

    fn resume_local_download(&mut self, path: QString) -> bool {
        self.prepare_local_file(path, true)
    }

    fn prepare_local_file(&mut self, path: QString, resume: bool) -> bool {
        let path = PathBuf::from(path.to_string());
        let Some(file) = self.local_files.get(&path).cloned() else {
            return false;
        };
        if file.partial != resume
            || file.identifiers.is_empty()
            || !file.error.is_empty()
            || self.local_jobs.contains(&path)
            || self
                .download_paths
                .values()
                .any(|active| *active == path || active.with_extension("ipsw.part") == path)
        {
            return false;
        }
        self.local_jobs.insert(path.clone());
        self.local_failures.remove(&path);
        self.refresh_local_files();
        let qt_thread = self.qt_thread();
        RUNTIME.spawn(async move {
            let mut verification_attempted = false;
            let result: Result<_> = async {
                let catalogs = futures::stream::iter(file.identifiers.clone())
                    .map(|identifier| async move {
                        let catalog = tokio::time::timeout(
                            Duration::from_secs(30),
                            fetch_catalog(&identifier),
                        )
                        .await
                        .context("firmware catalog request timed out")??;
                        Ok::<_, anyhow::Error>((identifier, catalog))
                    })
                    .buffer_unordered(4)
                    .collect::<Vec<_>>()
                    .await;
                let mut available = Vec::new();
                let mut failures = Vec::new();
                for catalog in catalogs {
                    match catalog {
                        Ok(catalog) => available.push(catalog),
                        Err(error) => failures.push(format!("{error:#}")),
                    }
                }
                let firmware = available
                    .iter()
                    .find_map(|(_, catalog)| local::matching_firmware(&file, catalog).ok())
                    .cloned()
                    .with_context(|| {
                        if available.is_empty() {
                            failures.join("; ")
                        } else {
                            "file does not match a firmware build and size in the catalog".into()
                        }
                    })?;
                if !resume {
                    // Read again to catch replacement of a file after the directory scan.
                    let read_path = path.clone();
                    let (version, build, identifiers) =
                        tokio::task::spawn_blocking(move || local::read_manifest(&read_path))
                            .await??;
                    if version != file.version
                        || build != file.build
                        || identifiers != file.identifiers
                    {
                        bail!("the IPSW changed since the directory scan; refresh and try again");
                    }
                    let before = tokio::fs::metadata(&path).await?.modified()?;
                    verification_attempted = true;
                    verify_file(&path, &firmware).await?;
                    if tokio::fs::metadata(&path).await?.modified()? != before {
                        bail!("the IPSW changed during verification; refresh and try again");
                    }
                    write_verification_sidecar(&path, &firmware).await?;
                }
                Ok((available, firmware))
            }
            .await;
            if verification_attempted && result.is_err() {
                local::remove_verification(&path);
            }
            qt_thread.queue(move |manager| {
                manager.local_jobs.remove(&path);
                let mut error = None;
                match result {
                    Ok((catalogs, firmware)) => {
                        if let Ok(mut cache) = manager.catalogs.lock() {
                            for (identifier, catalog) in catalogs {
                                cache.insert(identifier, catalog);
                            }
                        }
                        if resume {
                            let directory = path
                                .parent()
                                .unwrap_or(Path::new("."))
                                .to_string_lossy()
                                .into_owned();
                            if !manager.download_firmware(
                                QString::from(firmware.identifier),
                                QString::from(firmware.build_id),
                                QString::from(directory),
                                false,
                            ) {
                                error =
                                    Some("the firmware download could not be resumed".to_owned());
                            }
                        }
                    }
                    Err(failure) => error = Some(format!("{failure:#}")),
                }
                if let Some(error) = &error {
                    warn!("Local IPSW action failed for {}: {error}", path.display());
                    manager.local_failures.insert(path.clone(), error.clone());
                }
                manager.refresh_local_files();
                manager.localOperationFinished(
                    QString::from(if resume { "resume" } else { "verify" }),
                    QString::from(path.to_string_lossy().into_owned()),
                    error.is_none(),
                    QString::from(error.unwrap_or_default()),
                );
            });
        });
        true
    }

    fn cancel_local_download(&mut self, path: QString) -> bool {
        let path = PathBuf::from(path.to_string());
        let Some(file) = self
            .local_files
            .get(&path)
            .cloned()
            .filter(|file| file.partial)
        else {
            return false;
        };
        if self.local_jobs.contains(&path) {
            return false;
        }
        if let Some(identifier) = file.identifiers.first() {
            let key = firmware_key(identifier, &file.build);
            if self
                .download_paths
                .get(&key)
                .is_some_and(|active| active.with_extension("ipsw.part") == path)
            {
                return self
                    .cancel_download(QString::from(identifier.clone()), QString::from(file.build));
            }
        }
        self.local_jobs.insert(path.clone());
        self.refresh_local_files();
        let qt_thread = self.qt_thread();
        RUNTIME.spawn(async move {
            let result = tokio::fs::remove_file(&path).await;
            qt_thread.queue(move |manager| {
                manager.local_jobs.remove(&path);
                let error = result
                    .err()
                    .filter(|error| error.kind() != std::io::ErrorKind::NotFound)
                    .map(|error| error.to_string());
                manager.refresh_local_files();
                manager.localOperationFinished(
                    QString::from("cancel"),
                    QString::from(path.to_string_lossy().into_owned()),
                    error.is_none(),
                    QString::from(error.unwrap_or_default()),
                );
            });
        });
        true
    }

    fn load_devices(&mut self, refresh: bool) {
        if !refresh && !self.devices.is_empty() {
            self.deviceCatalogFinished(false, true, QString::default());
            return;
        }

        self.device_catalog_revision = self.device_catalog_revision.wrapping_add(1);
        let revision = self.device_catalog_revision;
        let qt_thread = self.qt_thread();
        RUNTIME.spawn(async move {
            let result = fetch_devices().await;
            qt_thread.queue(move |manager| {
                if manager.device_catalog_revision != revision {
                    debug!("Ignoring stale IPSW device catalog result");
                    return;
                }

                match result {
                    Ok(devices) => {
                        manager.devices = devices_to_variant_list(devices);
                        manager.devicesChanged();
                        manager.deviceCatalogFinished(refresh, true, QString::default());
                    }
                    Err(err) => {
                        let message = err.to_string();
                        error!("Failed to fetch IPSW device catalog: {message}");
                        manager.deviceCatalogFinished(refresh, false, QString::from(message));
                    }
                }
            });
        });
    }

    fn fetch_firmwares(&mut self, identifier: QString, dir: QString) {
        self.load_firmwares(identifier, dir, false);
    }

    fn refresh_firmwares(&mut self, identifier: QString, dir: QString) {
        self.load_firmwares(identifier, dir, true);
    }

    fn load_firmwares(&mut self, identifier: QString, dir: QString, refresh: bool) {
        let identifier_string = identifier.to_string();
        if identifier_string.trim().is_empty() {
            self.firmware_model.borrow_mut().reset_data(Vec::new());
            self.firmwareListFinished(
                identifier,
                refresh,
                false,
                QString::from("No device identifier was provided"),
            );
            return;
        }

        self.catalog_revision = self.catalog_revision.wrapping_add(1);
        let revision = self.catalog_revision;
        let cached = if refresh {
            None
        } else {
            self.catalogs
                .lock()
                .ok()
                .and_then(|catalogs| catalogs.get(&identifier_string).cloned())
        };
        let qt_thread = self.qt_thread();
        let directory = PathBuf::from(dir.to_string());

        RUNTIME.spawn(async move {
            let result = match cached {
                Some(firmwares) => Ok(firmwares),
                None => fetch_catalog(&identifier_string).await,
            };

            qt_thread.queue(move |manager| {
                if manager.catalog_revision != revision {
                    debug!("Ignoring stale IPSW catalog result for {identifier_string}");
                    return;
                }

                match result {
                    Ok(firmwares) => {
                        if let Ok(mut catalogs) = manager.catalogs.lock() {
                            catalogs.insert(identifier_string.clone(), firmwares.clone());
                        }
                        let active = manager.active_downloads.lock().ok();
                        let items = firmwares
                            .iter()
                            .map(|firmware| firmware_item(firmware, &directory, active.as_deref()))
                            .collect();
                        manager.firmware_model.borrow_mut().reset_data(items);
                        manager.firmwareListFinished(
                            QString::from(identifier_string),
                            refresh,
                            true,
                            QString::default(),
                        );
                    }
                    Err(err) => {
                        let message = err.to_string();
                        error!("Failed to fetch IPSW catalog for {identifier_string}: {message}");
                        manager.firmwareListFinished(
                            QString::from(identifier_string),
                            refresh,
                            false,
                            QString::from(message),
                        );
                    }
                }
            });
        });
    }

    fn get_item(&self, index: i32) -> QVariantMap {
        let mut map = QVariantMap::default();
        if index < 0 {
            return map;
        }
        let model = self.firmware_model.borrow();
        let Some(item) = model.values.get(index as usize) else {
            return map;
        };
        qvariantmap_insert!(map, "identifier", item.identifier.clone());
        qvariantmap_insert!(map, "version", item.version.clone());
        qvariantmap_insert!(map, "build_id", item.build_id.clone());
        qvariantmap_insert!(map, "release_date", item.release_date.clone());
        qvariantmap_insert!(map, "file_size", item.file_size);
        qvariantmap_insert!(map, "signed", item.signed);
        qvariantmap_insert!(map, "local_path", item.local_path.clone());
        qvariantmap_insert!(map, "is_downloaded", item.is_downloaded);
        qvariantmap_insert!(map, "is_downloading", item.is_downloading);
        qvariantmap_insert!(map, "is_verifying", item.is_verifying);
        qvariantmap_insert!(map, "has_partial", item.has_partial);
        qvariantmap_insert!(map, "progress", item.progress);
        qvariantmap_insert!(map, "downloaded_bytes", item.downloaded_bytes);
        qvariantmap_insert!(map, "download_speed", item.download_speed);
        qvariantmap_insert!(map, "status", item.status.clone());
        map
    }

    fn download_firmware(
        &mut self,
        identifier: QString,
        build_id: QString,
        dir: QString,
        force: bool,
    ) -> bool {
        let identifier_string = identifier.to_string();
        let build_string = build_id.to_string();
        let key = firmware_key(&identifier_string, &build_string);
        let firmware = self
            .catalogs
            .lock()
            .ok()
            .and_then(|catalogs| catalogs.get(&identifier_string).cloned())
            .and_then(|firmwares| {
                firmwares
                    .into_iter()
                    .find(|firmware| firmware.build_id == build_string)
            });
        let Some(firmware) = firmware else {
            error!("No IPSW metadata for {identifier_string} build {build_string}");
            return false;
        };

        if self.stopping_downloads.contains(&key) {
            return false;
        }
        let target_dir = PathBuf::from(dir.to_string());
        if let Err(err) = fs::create_dir_all(&target_dir) {
            error!(
                "Failed to create IPSW directory {}: {err}",
                target_dir.display()
            );
            return false;
        }

        match self.active_downloads.lock() {
            Ok(downloads) if downloads.contains_key(&key) => return false,
            Ok(_) => {}
            Err(err) => {
                error!("IPSW download state is unavailable: {err}");
                return false;
            }
        }

        let local_path = target_dir.join(firmware.file_name());
        if self.local_jobs.contains(&local_path) {
            return false;
        }
        self.download_paths.insert(key.clone(), local_path.clone());
        mutate_item(self, &key, |item| {
            item.local_path = QString::from(local_path.to_string_lossy().into_owned());
            item.is_downloading = true;
            item.is_verifying = false;
            item.download_speed = 0.0;
            item.status = QString::from("downloading");
        });

        let qt_thread = self.qt_thread();
        let task_key = key.clone();
        let cancellation = CancellationToken::new();
        self.download_cancellations
            .insert(key.clone(), cancellation.clone());
        let handle = RUNTIME.spawn(async move {
            let result = download_ipsw(
                &firmware,
                &target_dir,
                force,
                qt_thread.clone(),
                cancellation,
            )
            .await;
            let path = target_dir.join(firmware.file_name());
            let identifier = firmware.identifier.clone();
            let build_id = firmware.build_id.clone();

            qt_thread.queue(move |manager| {
                if manager.stopping_downloads.contains(&task_key) {
                    return;
                }
                manager.download_cancellations.remove(&task_key);
                manager.download_paths.remove(&task_key);
                if let Ok(mut active) = manager.active_downloads.lock() {
                    active.remove(&task_key);
                }
                match result {
                    Ok(()) => {
                        mutate_item(manager, &task_key, |item| {
                            item.is_downloading = false;
                            item.is_verifying = false;
                            item.is_downloaded = true;
                            item.has_partial = false;
                            item.progress = 100.0;
                            item.downloaded_bytes = item.file_size;
                            item.download_speed = 0.0;
                            item.status = QString::from("downloaded");
                            item.local_path = QString::from(path.to_string_lossy().to_string());
                        });
                        manager.downloadFinished(
                            QString::from(identifier),
                            QString::from(build_id),
                            true,
                            QString::from(path.to_string_lossy().to_string()),
                            QString::default(),
                        );
                    }
                    Err(err) => {
                        let message = err.to_string();
                        mutate_item(manager, &task_key, |item| {
                            item.is_downloading = false;
                            item.is_verifying = false;
                            item.download_speed = 0.0;
                            item.has_partial = path.with_extension("ipsw.part").exists();
                            item.status = QString::from("failed");
                        });
                        manager.downloadFinished(
                            QString::from(identifier),
                            QString::from(build_id),
                            false,
                            QString::default(),
                            QString::from(message),
                        );
                    }
                }
            });
        });
        let started = match self.active_downloads.lock() {
            Ok(mut downloads) => {
                downloads.insert(key, handle);
                true
            }
            Err(err) => {
                handle.abort();
                error!("IPSW download state is unavailable: {err}");
                false
            }
        };
        if started {
            self.refresh_local_files();
        }
        started
    }

    fn pause_download(&mut self, identifier: QString, build_id: QString) -> bool {
        self.stop_download(identifier, build_id, false)
    }

    fn cancel_download(&mut self, identifier: QString, build_id: QString) -> bool {
        self.stop_download(identifier, build_id, true)
    }

    fn stop_download(&mut self, identifier: QString, build_id: QString, delete: bool) -> bool {
        let key = firmware_key(&identifier.to_string(), &build_id.to_string());
        if self.stopping_downloads.contains(&key) {
            return false;
        }
        let path = if let Some(path) = self.download_paths.get(&key) {
            path.clone()
        } else {
            let model = self.firmware_model.borrow();
            let Some(item) = model.values.iter().find(|item| {
                firmware_key(&item.identifier.to_string(), &item.build_id.to_string()) == key
            }) else {
                return false;
            };
            if item.is_verifying || (!item.is_downloading && (!delete || !item.has_partial)) {
                return false;
            }
            let path = item.local_path.to_string();
            if path.is_empty() {
                return false;
            }
            PathBuf::from(path)
        };
        let handle = match self.active_downloads.lock() {
            Ok(mut downloads) => downloads.remove(&key),
            Err(err) => {
                error!("IPSW download state is unavailable: {err}");
                return false;
            }
        };
        if !delete && handle.is_none() {
            return false;
        }
        self.stopping_downloads.insert(key.clone());
        mutate_item(self, &key, |item| {
            item.is_downloading = false;
            item.is_verifying = true;
            item.download_speed = 0.0;
            item.status = QString::from(if delete { "cancelling" } else { "pausing" });
        });
        if let Some(cancellation) = self.download_cancellations.remove(&key) {
            cancellation.cancel();
        }
        let qt_thread = self.qt_thread();
        RUNTIME.spawn(async move {
            let result = finish_stopped_download(handle, &path, delete).await;
            let partial = path.with_extension("ipsw.part");
            let partial_bytes = tokio::fs::metadata(&partial).await.ok().map(|m| m.len());
            let completed = !delete && path.is_file() && verification_sidecar(&path).is_file();
            qt_thread.queue(move |manager| {
                manager.stopping_downloads.remove(&key);
                manager.download_paths.remove(&key);
                mutate_item(manager, &key, |item| {
                    item.is_downloading = false;
                    item.is_verifying = false;
                    item.download_speed = 0.0;
                    item.has_partial = partial_bytes.is_some();
                    item.is_downloaded = completed;
                    item.downloaded_bytes = if completed {
                        item.file_size
                    } else {
                        partial_bytes.unwrap_or(0) as f64
                    };
                    item.progress = if item.file_size > 0.0 {
                        item.downloaded_bytes * 100.0 / item.file_size
                    } else {
                        0.0
                    };
                    item.status = QString::from(if result.is_err() {
                        "failed"
                    } else if completed {
                        "downloaded"
                    } else if item.has_partial {
                        "partial"
                    } else {
                        "available"
                    });
                });
                match result {
                    Err(err) => {
                        error!("Failed to stop IPSW download: {err:#}");
                        manager.downloadStopFailed(
                            identifier,
                            build_id,
                            QString::from(format!("{err:#}")),
                        );
                    }
                    Ok(()) if delete => manager.downloadCancelled(identifier, build_id),
                    Ok(()) => manager.downloadPaused(identifier, build_id),
                }
            });
        });
        true
    }

    fn verify_firmware(&mut self, identifier: QString, build_id: QString, dir: QString) -> bool {
        if self.stopping_downloads.contains(&firmware_key(
            &identifier.to_string(),
            &build_id.to_string(),
        )) {
            return false;
        }
        let identifier_string = identifier.to_string();
        let build_string = build_id.to_string();
        let key = firmware_key(&identifier_string, &build_string);
        let firmware = self
            .catalogs
            .lock()
            .ok()
            .and_then(|catalogs| catalogs.get(&identifier_string).cloned())
            .and_then(|firmwares| {
                firmwares
                    .into_iter()
                    .find(|firmware| firmware.build_id == build_string)
            });
        let Some(firmware) = firmware else {
            return false;
        };
        match self.active_downloads.lock() {
            Ok(jobs) if jobs.contains_key(&key) => return false,
            Ok(_) => {}
            Err(_) => return false,
        }
        let path = PathBuf::from(dir.to_string()).join(firmware.file_name());
        if self.local_jobs.contains(&path) {
            return false;
        }
        mutate_item(self, &key, |item| {
            item.is_verifying = true;
            item.status = QString::from("verifying");
        });
        let qt_thread = self.qt_thread();
        let task_key = key.clone();
        let handle = RUNTIME.spawn(async move {
            let result = async {
                verify_file(&path, &firmware).await?;
                write_verification_sidecar(&path, &firmware).await
            }
            .await;
            let identifier = firmware.identifier.clone();
            let build_id = firmware.build_id.clone();
            qt_thread.queue(move |manager| {
                if let Ok(mut active) = manager.active_downloads.lock() {
                    active.remove(&task_key);
                }
                let success = result.is_ok();
                let message = result.err().map(|err| err.to_string()).unwrap_or_default();
                mutate_item(manager, &task_key, |item| {
                    item.is_verifying = false;
                    item.is_downloaded = success;
                    item.status = QString::from(if success { "downloaded" } else { "invalid" });
                });
                manager.verificationFinished(
                    QString::from(identifier),
                    QString::from(build_id),
                    success,
                    QString::from(message),
                );
            });
        });
        match self.active_downloads.lock() {
            Ok(mut jobs) => {
                jobs.insert(key, handle);
                true
            }
            Err(_) => {
                handle.abort();
                false
            }
        }
    }
}

fn firmware_key(identifier: &str, build_id: &str) -> String {
    format!("{identifier}:{build_id}")
}

fn safe_file_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn firmware_item(
    firmware: &FirmwareInfo,
    directory: &Path,
    active: Option<&HashMap<String, JoinHandle<()>>>,
) -> FirmwareItem {
    let path = directory.join(firmware.file_name());
    let partial = path.with_extension("ipsw.part");
    let is_downloaded = local::catalog_approves(&path, firmware);
    let is_downloading = active.is_some_and(|active| active.contains_key(&firmware.key()));
    let partial_bytes = fs::metadata(&partial)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let progress = if is_downloaded {
        100.0
    } else if firmware.file_size > 0 {
        partial_bytes as f64 * 100.0 / firmware.file_size as f64
    } else {
        0.0
    };
    let status = if is_downloading {
        "downloading"
    } else if is_downloaded {
        "downloaded"
    } else if path.exists() {
        "unverified"
    } else if partial.exists() {
        "partial"
    } else {
        "available"
    };

    FirmwareItem {
        identifier: QString::from(firmware.identifier.clone()),
        version: QString::from(firmware.version.clone()),
        build_id: QString::from(firmware.build_id.clone()),
        supported_identifiers: vec![QString::from(firmware.identifier.clone())].into(),
        flash_identifiers: if firmware.signed {
            vec![QString::from(firmware.identifier.clone())].into()
        } else {
            QStringList::default()
        },
        file_name: QString::from(firmware.file_name()),
        error: QString::default(),
        release_date: format_release_date(&firmware.release_date),
        file_size: firmware.file_size as f64,
        signed: firmware.signed,
        local_path: QString::from(path.to_string_lossy().to_string()),
        is_downloaded,
        is_downloading,
        is_verifying: false,
        has_partial: partial.exists(),
        progress,
        downloaded_bytes: partial_bytes as f64,
        download_speed: 0.0,
        status: QString::from(status),
    }
}

fn format_release_date(value: &str) -> QString {
    DateTime::parse_from_rfc3339(value)
        .map(|date| date.with_timezone(&Utc).date_naive())
        .or_else(|_| NaiveDate::parse_from_str(value, "%Y-%m-%d"))
        .map(|date| QString::from(date.format("%-d %b %Y").to_string()))
        .unwrap_or_default()
}

fn local_firmware_item(
    file: &local::LocalFile,
    catalogs: &HashMap<String, Vec<FirmwareInfo>>,
) -> FirmwareItem {
    let metadata = file
        .identifiers
        .iter()
        .filter_map(|identifier| catalogs.get(identifier))
        .flat_map(|catalog| catalog.iter())
        .filter(|firmware| firmware.build_id == file.build && firmware.version == file.version)
        .collect::<Vec<_>>();
    let verified = !file.partial
        && file.error.is_empty()
        && if metadata.is_empty() {
            // Preserve offline approval when no catalog for this model has been loaded.
            file.verified
        } else {
            metadata
                .iter()
                .any(|firmware| local::catalog_approves(&file.path, firmware))
        };
    let flash_identifiers = metadata
        .iter()
        .filter(|firmware| {
            firmware.signed && (firmware.file_size == 0 || firmware.file_size == file.bytes)
        })
        .map(|firmware| QString::from(firmware.identifier.clone()))
        .collect::<Vec<_>>();
    let total = if file.partial {
        metadata.first().map_or(0, |firmware| firmware.file_size)
    } else {
        file.bytes
    };
    FirmwareItem {
        identifier: QString::from(file.identifiers.first().cloned().unwrap_or_default()),
        supported_identifiers: file
            .identifiers
            .iter()
            .map(|id| QString::from(id.clone()))
            .collect::<Vec<_>>()
            .into(),
        signed: !flash_identifiers.is_empty(),
        flash_identifiers: flash_identifiers.into(),
        file_name: QString::from(
            file.path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
        ),
        local_path: QString::from(file.path.to_string_lossy().into_owned()),
        version: QString::from(file.version.clone()),
        build_id: QString::from(file.build.clone()),
        file_size: total as f64,
        downloaded_bytes: file.bytes as f64,
        progress: if total > 0 {
            file.bytes as f64 * 100.0 / total as f64
        } else {
            0.0
        },
        is_downloaded: verified,
        has_partial: file.partial,
        error: QString::from(file.error.clone()),
        status: QString::from(if !file.error.is_empty() {
            "invalid"
        } else if file.partial {
            "partial"
        } else if verified {
            "downloaded"
        } else {
            "unverified"
        }),
        ..Default::default()
    }
}

fn mutate_item(manager: &mut FirmwareManager, key: &str, mutate: impl FnOnce(&mut FirmwareItem)) {
    let index = {
        let model = manager.firmware_model.borrow();
        model.values.iter().position(|item| {
            firmware_key(&item.identifier.to_string(), &item.build_id.to_string()) == key
        })
    };
    if let Some(index) = index {
        manager.firmware_model.borrow_mut().mutate(index, mutate);
    }
}

async fn fetch_catalog(identifier: &str) -> Result<Vec<FirmwareInfo>> {
    let url = format!("{API_BASE}/ipsw/device/{}", urlencoding::encode(identifier));
    let response = Client::new()
        .get(&url)
        .send()
        .await
        .with_context(|| format!("failed to request firmware catalog for {identifier}"))?
        .error_for_status()
        .with_context(|| format!("firmware catalog rejected the request for {identifier}"))?;
    let body: Value = response
        .json()
        .await
        .context("invalid firmware catalog response")?;
    let returned_identifier = body
        .get("identifier")
        .and_then(Value::as_str)
        .unwrap_or(identifier);
    let entries = body
        .get("firmwares")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("firmware catalog response has no firmwares array"))?;

    let mut firmwares: Vec<FirmwareInfo> = entries
        .iter()
        .filter_map(|entry| {
            let version = entry.get("version")?.as_str()?.to_string();
            let build_id = entry.get("buildid")?.as_str()?.to_string();
            let url = entry.get("url")?.as_str()?.to_string();
            Some(FirmwareInfo {
                identifier: entry
                    .get("identifier")
                    .and_then(Value::as_str)
                    .unwrap_or(returned_identifier)
                    .to_string(),
                version,
                build_id,
                release_date: entry
                    .get("releasedate")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                file_size: entry.get("filesize").and_then(Value::as_u64).unwrap_or(0),
                url,
                signed: entry
                    .get("signed")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                sha1: entry
                    .get("sha1sum")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                sha256: entry
                    .get("sha256sum")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            })
        })
        .collect();
    firmwares.sort_by(|a, b| {
        b.signed
            .cmp(&a.signed)
            .then_with(|| b.release_date.cmp(&a.release_date))
            .then_with(|| b.build_id.cmp(&a.build_id))
    });
    Ok(firmwares)
}

#[derive(Debug)]
struct IpswDevice {
    name: String,
    identifier: String,
}

async fn fetch_devices() -> Result<Vec<IpswDevice>> {
    let url = format!("{API_BASE}/devices");
    let response = Client::new()
        .get(&url)
        .send()
        .await
        .context("failed to request the IPSW device catalog")?
        .error_for_status()
        .context("the IPSW device catalog request was rejected")?;
    let entries: Vec<Value> = response
        .json()
        .await
        .context("invalid IPSW device catalog response")?;
    let mut identifiers = HashSet::new();
    let mut devices: Vec<IpswDevice> = entries
        .into_iter()
        .filter_map(|entry| {
            let identifier = entry.get("identifier")?.as_str()?.trim().to_string();
            if identifier.is_empty() || !identifiers.insert(identifier.clone()) {
                return None;
            }
            let name = entry
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(&identifier)
                .trim()
                .to_string();
            Some(IpswDevice { name, identifier })
        })
        .collect();
    devices.sort_by(|a, b| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
            .then_with(|| a.identifier.cmp(&b.identifier))
    });
    if devices.is_empty() {
        bail!("the IPSW device catalog is empty");
    }
    Ok(devices)
}

fn devices_to_variant_list(devices: Vec<IpswDevice>) -> QVariantList {
    let mut list = QVariantList::default();
    for device in devices {
        let mut map = QVariantMap::default();
        qvariantmap_insert!(map, "name", QString::from(device.name));
        qvariantmap_insert!(map, "identifier", QString::from(device.identifier));
        list.push(QVariant::from(map));
    }
    list
}

async fn download_ipsw(
    firmware: &FirmwareInfo,
    directory: &Path,
    force: bool,
    qt_thread: QtThread<FirmwareManager>,
    cancellation: CancellationToken,
) -> Result<()> {
    let final_path = directory.join(firmware.file_name());
    let partial_path = final_path.with_extension("ipsw.part");
    let sidecar_path = verification_sidecar(&final_path);

    if force {
        let _ = tokio::fs::remove_file(&final_path).await;
        let _ = tokio::fs::remove_file(&partial_path).await;
        let _ = tokio::fs::remove_file(&sidecar_path).await;
    } else if final_path.exists() {
        match verify_file(&final_path, firmware).await {
            Ok(()) => {
                write_verification_sidecar(&final_path, firmware).await?;
                return Ok(());
            }
            Err(err) => {
                warn!(
                    "Existing IPSW {} failed verification and will be replaced: {err}",
                    final_path.display()
                );
                tokio::fs::remove_file(&final_path).await?;
                let _ = tokio::fs::remove_file(&sidecar_path).await;
            }
        }
    }

    let client = Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .build()
        .context("failed to create firmware download client")?;
    let mut offset = tokio::fs::metadata(&partial_path)
        .await
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    if firmware.file_size > 0 && offset > firmware.file_size {
        tokio::fs::remove_file(&partial_path).await?;
        offset = 0;
    }
    if firmware.file_size > 0 && offset == firmware.file_size {
        // A pause during final verification can leave a complete partial file.
        // Verify it locally instead of asking the server for a range past EOF.
        verify_file(&partial_path, firmware).await?;
        if cancellation.is_cancelled() {
            bail!("download stopped");
        }
        tokio::fs::rename(&partial_path, &final_path).await?;
        write_verification_sidecar(&final_path, firmware).await?;
        return Ok(());
    }

    let mut request = client.get(&firmware.url);
    if offset > 0 {
        request = request.header(header::RANGE, format!("bytes={offset}-"));
    }
    let mut response = tokio::select! {
        _ = cancellation.cancelled() => bail!("download stopped"),
        response = request.send() => response.context("firmware download request failed")?,
    };
    if offset > 0 && response.status() != StatusCode::PARTIAL_CONTENT {
        warn!(
            "Firmware server did not accept resume for {} {}; restarting",
            firmware.identifier, firmware.build_id
        );
        offset = 0;
        response = tokio::select! {
            _ = cancellation.cancelled() => bail!("download stopped"),
            response = client.get(&firmware.url).send() => response.context("firmware restart request failed")?,
        };
    }
    response = response
        .error_for_status()
        .context("firmware download was rejected")?;

    let mut options = tokio::fs::OpenOptions::new();
    options.create(true).write(true);
    if offset > 0 {
        options.append(true);
    } else {
        options.truncate(true);
    }
    let mut file = options.open(&partial_path).await?;
    let response_length = response.content_length().unwrap_or(0);
    let total = if firmware.file_size > 0 {
        firmware.file_size
    } else {
        offset.saturating_add(response_length)
    };
    let mut downloaded = offset;
    let mut stream = response.bytes_stream();
    let mut last_update = Instant::now();
    let mut last_update_bytes = offset;

    emit_progress(&qt_thread, firmware, downloaded, total, 0.0);

    loop {
        let chunk = tokio::select! {
            _ = cancellation.cancelled() => {
                file.flush().await?;
                bail!("download stopped");
            }
            chunk = stream.next() => chunk,
        };
        let Some(chunk) = chunk else { break };
        let chunk = chunk.context("failed while reading firmware download")?;
        file.write_all(&chunk).await?;
        downloaded = downloaded.saturating_add(chunk.len() as u64);
        if last_update.elapsed() >= Duration::from_millis(200) {
            let elapsed = last_update.elapsed().as_secs_f64();
            let speed = downloaded.saturating_sub(last_update_bytes) as f64 / elapsed;
            emit_progress(&qt_thread, firmware, downloaded, total, speed);
            last_update = Instant::now();
            last_update_bytes = downloaded;
        }
    }
    file.flush().await?;
    drop(file);
    if cancellation.is_cancelled() {
        bail!("download stopped");
    }
    let elapsed = last_update.elapsed().as_secs_f64();
    let speed = if elapsed > 0.0 {
        downloaded.saturating_sub(last_update_bytes) as f64 / elapsed
    } else {
        0.0
    };
    emit_progress(&qt_thread, firmware, downloaded, total, speed);

    if firmware.file_size > 0 && downloaded != firmware.file_size {
        bail!(
            "downloaded file size is {downloaded} bytes; expected {} bytes",
            firmware.file_size
        );
    }

    let key = firmware.key();
    qt_thread.queue(move |manager| {
        if manager.stopping_downloads.contains(&key) {
            return;
        }
        mutate_item(manager, &key, |item| {
            item.is_downloading = false;
            item.is_verifying = true;
            item.download_speed = 0.0;
            item.progress = 100.0;
            item.status = QString::from("verifying");
        });
    });
    verify_file(&partial_path, firmware).await?;
    if cancellation.is_cancelled() {
        bail!("download stopped");
    }
    if final_path.exists() {
        tokio::fs::remove_file(&final_path).await?;
    }
    tokio::fs::rename(&partial_path, &final_path).await?;
    write_verification_sidecar(&final_path, firmware).await?;
    info!(
        "Downloaded verified IPSW {} {} to {}",
        firmware.identifier,
        firmware.build_id,
        final_path.display()
    );
    Ok(())
}

fn emit_progress(
    qt_thread: &QtThread<FirmwareManager>,
    firmware: &FirmwareInfo,
    downloaded: u64,
    total: u64,
    download_speed: f64,
) {
    let identifier = firmware.identifier.clone();
    let build_id = firmware.build_id.clone();
    let key = firmware.key();
    let progress = if total > 0 {
        downloaded as f64 * 100.0 / total as f64
    } else {
        0.0
    };
    qt_thread.queue(move |manager| {
        if manager.stopping_downloads.contains(&key) {
            return;
        }
        mutate_item(manager, &key, |item| {
            item.downloaded_bytes = downloaded as f64;
            item.progress = progress;
            item.download_speed = download_speed;
            item.has_partial = true;
        });
        manager.downloadProgress(
            QString::from(identifier),
            QString::from(build_id),
            downloaded as f64,
            total as f64,
            progress,
            download_speed,
        );
    });
}

async fn verify_file(path: &Path, firmware: &FirmwareInfo) -> Result<()> {
    let metadata = tokio::fs::metadata(path)
        .await
        .with_context(|| format!("firmware file does not exist: {}", path.display()))?;
    if firmware.file_size > 0 && metadata.len() != firmware.file_size {
        bail!(
            "firmware file has {} bytes; expected {} bytes",
            metadata.len(),
            firmware.file_size
        );
    }
    let Some((algorithm, expected)) = firmware.expected_digest() else {
        bail!("firmware catalog did not provide a checksum");
    };
    let path = path.to_owned();
    let expected = expected.to_ascii_lowercase();
    let actual = tokio::task::spawn_blocking(move || checksum_file(&path, algorithm)).await??;
    if actual != expected {
        bail!("{algorithm} checksum does not match the firmware catalog");
    }
    Ok(())
}

fn checksum_file(path: &Path, algorithm: &str) -> Result<String> {
    use std::io::Read;
    let mut file = fs::File::open(path)?;
    let mut buffer = vec![0u8; 1024 * 1024];
    match algorithm {
        "sha256" => {
            let mut digest = Sha256::new();
            loop {
                let read = file.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                digest.update(&buffer[..read]);
            }
            Ok(hex::encode(digest.finalize()))
        }
        "sha1" => {
            let mut digest = Sha1::new();
            loop {
                let read = file.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                digest.update(&buffer[..read]);
            }
            Ok(hex::encode(digest.finalize()))
        }
        _ => bail!("unsupported checksum algorithm {algorithm}"),
    }
}

async fn finish_stopped_download(
    handle: Option<JoinHandle<()>>,
    path: &Path,
    delete: bool,
) -> Result<()> {
    // Let pending filesystem writes finish before deleting files or allowing a resume.
    if let Some(handle) = handle {
        handle
            .await
            .context("download task failed while stopping")?;
    }
    if delete {
        // These files belong only to this firmware. The download directory is shared.
        for file in [
            path.with_extension("ipsw.part"),
            path.to_owned(),
            verification_sidecar(path),
        ] {
            match tokio::fs::remove_file(&file).await {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => {
                    return Err(err).with_context(|| {
                        format!("failed to delete cancelled download {}", file.display())
                    });
                }
            }
        }
        local::remove_verification(path);
    }
    Ok(())
}

pub(super) fn verification_sidecar(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.verified", path.to_string_lossy()))
}

async fn write_verification_sidecar(path: &Path, firmware: &FirmwareInfo) -> Result<()> {
    let Some((_, digest)) = firmware.expected_digest() else {
        return Ok(());
    };
    tokio::fs::write(verification_sidecar(path), digest.to_ascii_lowercase()).await?;
    let path = path.to_owned();
    let firmware = firmware.clone();
    tokio::task::spawn_blocking(move || local::write_record(&path, &firmware)).await??;
    Ok(())
}

#[cfg(test)]
mod download_stop_tests {
    use super::*;

    async fn check_stop(delete: bool) -> Result<()> {
        let directory = std::env::temp_dir().join(format!(
            "idescriptor-stop-{}-{}-{}",
            std::process::id(),
            delete,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        tokio::fs::create_dir_all(&directory).await?;
        let path = directory.join("firmware.ipsw");
        let partial = path.with_extension("ipsw.part");
        let other = directory.join("other-firmware.ipsw.part");
        tokio::fs::write(&other, b"keep other download").await?;
        let writer_path = path.clone();
        let writer = tokio::spawn(async move {
            // The stop operation must wait for a writer that has not yet finished.
            tokio::task::yield_now().await;
            tokio::fs::write(
                writer_path.with_extension("ipsw.part"),
                b"resume these bytes",
            )
            .await
            .unwrap();
            tokio::fs::write(verification_sidecar(&writer_path), b"checksum")
                .await
                .unwrap();
        });
        finish_stopped_download(Some(writer), &path, delete).await?;
        assert_eq!(tokio::fs::read(&other).await?, b"keep other download");
        if delete {
            assert!(!partial.exists());
            assert!(!verification_sidecar(&path).exists());
            // Cancelling again also succeeds when files are already absent.
            finish_stopped_download(None, &path, true).await?;
        } else {
            assert_eq!(tokio::fs::read(&partial).await?, b"resume these bytes");
        }
        tokio::fs::remove_dir_all(directory).await?;
        Ok(())
    }

    #[tokio::test]
    async fn pause_preserves_bytes_after_pending_writer_finishes() -> Result<()> {
        check_stop(false).await
    }

    #[tokio::test]
    async fn cancel_deletes_download_after_pending_writer_and_keeps_other_downloads() -> Result<()>
    {
        check_stop(true).await
    }
}
