// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    RUNTIME, device_ctx,
    qt_threading::{QtThread, QtThreading},
    utils,
};
use chrono::{Datelike, NaiveDate};
use idevice::IdeviceService;
use idevice::afc::{AfcClient, FileInfo, opcode::AfcFopenMode};
use log::{debug, error, info, warn};
use macros::QtThreading;
use nom_exif::{
    EntryValue, Exif, ExifDateTime, ExifTag, MediaKind, MediaParser, MediaSource, TrackInfoTag,
};
use qmetaobject::prelude::*;
use qttypes::QStringList;
use std::{
    collections::{HashMap, HashSet},
    io,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
};

pub static DEFAULT_CHUNK_SIZE: usize = 1024 * 1024;

#[derive(QObject, Default, QtThreading)]
#[allow(non_snake_case)]
pub struct IOManager {
    base: qt_base_class!(trait QObject),
    jobs: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    start_gallery_export: qt_method!(
        fn(
            &self,
            udid: QString,
            job_id: QString,
            device_paths: QStringList,
            destination_dir: QString,
        )
    ),
    start_export: qt_method!(
        fn(
            &self,
            udid: QString,
            job_id: QString,
            device_paths: QStringList,
            destination_dir: QString,
            allow_directories: bool,
        )
    ),
    start_export_with_afc2: qt_method!(
        fn(
            &self,
            udid: QString,
            job_id: QString,
            device_paths: QStringList,
            destination_dir: QString,
            allow_directories: bool,
        )
    ),
    start_export_with_hause_arrest_afc: qt_method!(
        fn(
            &self,
            udid: QString,
            job_id: QString,
            device_paths: QStringList,
            destination_dir: QString,
            hause_arrest_afc: QString,
            allow_directories: bool,
        )
    ),
    start_import: qt_method!(
        fn(
            &self,
            udid: QString,
            job_id: QString,
            local_paths: QStringList,
            destination_dir: QString,
        )
    ),
    start_import_with_afc2: qt_method!(
        fn(
            &self,
            udid: QString,
            job_id: QString,
            local_paths: QStringList,
            destination_dir: QString,
        )
    ),
    start_import_with_hause_arrest_afc: qt_method!(
        fn(
            &self,
            udid: QString,
            job_id: QString,
            local_paths: QStringList,
            destination_dir: QString,
            hause_arrest_afc: QString,
        )
    ),
    has_active_tasks: qt_method!(fn(&self) -> bool),
    cancel_job: qt_method!(fn(&self, job_id: QString)),
    cancel_all_jobs: qt_method!(fn(&self)),
    fileTransferProgress: qt_signal!(
        job_id: QString,
        file_name: QString,
        bytes_transferred: i64,
        total_bytes: i64
    ),
    exportItemFinished: qt_signal!(
        job_id: QString,
        file_name: QString,
        destination_path: QString,
        success: bool,
        bytes_transferred: i64,
        error_message: QString
    ),
    exportJobFinished: qt_signal!(
        job_id: QString,
        cancelled: bool,
        successful_items: i32,
        failed_items: i32,
        total_bytes: i64
    ),
    importItemFinished: qt_signal!(
        job_id: QString,
        file_name: QString,
        destination_path: QString,
        success: bool,
        bytes_transferred: i64,
        error_message: QString
    ),
    importJobFinished: qt_signal!(
        job_id: QString,
        cancelled: bool,
        successful_items: i32,
        failed_items: i32,
        total_bytes: i64
    ),
}

struct TransferItemResult {
    success: bool,
    bytes_transferred: i64,
    destination_path: String,
    error_message: Option<String>,
}

struct RemoteExportFile {
    remote_path: String,
    relative_path: PathBuf,
    info: FileInfo,
}

struct DirectoryExportManifest {
    directories: Vec<PathBuf>,
    files: Vec<RemoteExportFile>,
    total_bytes: i64,
}

enum AfcKind {
    Standard,
    Afc2,
    HouseArrest(String),
}

impl AfcKind {
    fn description(&self) -> String {
        match self {
            AfcKind::Standard => "standard".to_string(),
            AfcKind::Afc2 => "afc2".to_string(),
            AfcKind::HouseArrest(bundle_id) => format!("house-arrest:{bundle_id}"),
        }
    }
}

impl IOManager {
    fn start_gallery_export(
        &self,
        udid: QString,
        job_id: QString,
        device_paths: QStringList,
        destination_dir: QString,
    ) {
        self.spawn_export(
            udid,
            job_id,
            device_paths,
            destination_dir,
            AfcKind::Standard,
            false,
            true,
        );
    }

    fn has_active_tasks(&self) -> bool {
        !self
            .jobs
            .lock()
            .expect("IOManager jobs map mutex poisoned")
            .is_empty()
    }

    fn start_export(
        &self,
        udid: QString,
        job_id: QString,
        device_paths: QStringList,
        destination_dir: QString,
        allow_directories: bool,
    ) {
        self.spawn_export(
            udid,
            job_id,
            device_paths,
            destination_dir,
            AfcKind::Standard,
            allow_directories,
            false,
        );
    }

    fn start_export_with_afc2(
        &self,
        udid: QString,
        job_id: QString,
        device_paths: QStringList,
        destination_dir: QString,
        allow_directories: bool,
    ) {
        self.spawn_export(
            udid,
            job_id,
            device_paths,
            destination_dir,
            AfcKind::Afc2,
            allow_directories,
            false,
        );
    }

    fn start_export_with_hause_arrest_afc(
        &self,
        udid: QString,
        job_id: QString,
        device_paths: QStringList,
        destination_dir: QString,
        hause_arrest_afc: QString,
        allow_directories: bool,
    ) {
        self.spawn_export(
            udid,
            job_id,
            device_paths,
            destination_dir,
            AfcKind::HouseArrest(hause_arrest_afc.to_string()),
            allow_directories,
            false,
        );
    }

    fn start_import(
        &self,
        udid: QString,
        job_id: QString,
        local_paths: QStringList,
        destination_dir: QString,
    ) {
        self.spawn_import(
            udid,
            job_id,
            local_paths,
            destination_dir,
            AfcKind::Standard,
        );
    }

    fn start_import_with_afc2(
        &self,
        udid: QString,
        job_id: QString,
        local_paths: QStringList,
        destination_dir: QString,
    ) {
        self.spawn_import(udid, job_id, local_paths, destination_dir, AfcKind::Afc2);
    }

    fn start_import_with_hause_arrest_afc(
        &self,
        udid: QString,
        job_id: QString,
        local_paths: QStringList,
        destination_dir: QString,
        hause_arrest_afc: QString,
    ) {
        self.spawn_import(
            udid,
            job_id,
            local_paths,
            destination_dir,
            AfcKind::HouseArrest(hause_arrest_afc.to_string()),
        );
    }

    fn cancel_job(&self, job_id: QString) {
        let job_id_str = job_id.to_string();
        let guard = self.jobs.lock().expect("IOManager jobs map mutex poisoned");
        if let Some(flag) = guard.get(&job_id_str) {
            info!("IOManager cancel requested: job_id={job_id_str}");
            flag.store(true, Ordering::Relaxed);
        } else {
            warn!("IOManager cancel requested for unknown job: job_id={job_id_str}");
        }
    }

    fn cancel_all_jobs(&self) {
        let guard = self.jobs.lock().expect("IOManager jobs map mutex poisoned");
        info!("IOManager cancel all requested: jobs={}", guard.len());
        for flag in guard.values() {
            flag.store(true, Ordering::Relaxed);
        }
    }

    fn spawn_export(
        &self,
        udid: QString,
        job_id: QString,
        device_paths: QStringList,
        destination_dir: QString,
        afc_kind: AfcKind,
        allow_directories: bool,
        gallery_export: bool,
    ) {
        let udid = udid.to_string();
        let job_id = job_id.to_string();
        let destination_dir = destination_dir.to_string();
        let items = qstring_list_to_vec(device_paths);
        let item_count = items.len();
        let afc_kind_description = afc_kind.description();
        info!(
            "IOManager export requested: job_id={job_id} udid={udid} items={item_count} destination_dir={destination_dir} afc={afc_kind_description}"
        );
        if item_count == 0 {
            warn!("IOManager export requested with no items: job_id={job_id}");
        }
        let cancel_flag = self.register_job(&job_id);
        let jobs = self.jobs.clone();
        let qt_thread = self.qt_thread();

        RUNTIME.spawn(async move {
            debug!(
                "IOManager export creating AFC client: job_id={job_id} udid={udid} afc={afc_kind_description}"
            );
            let mut afc = match create_afc_client(&udid, afc_kind).await {
                Ok(afc) => {
                    debug!("IOManager export AFC client ready: job_id={job_id}");
                    afc
                }
                Err(err) => {
                    error!(
                        "IOManager export failed to create AFC client: job_id={job_id} udid={udid} afc={afc_kind_description}: {err}"
                    );
                    finish_export_job(&qt_thread, job_id.clone(), true, 0, 0, 0);
                    unregister_job(&jobs, &job_id);
                    return;
                }
            };

            handle_start_export(
                &mut afc,
                job_id.clone(),
                items,
                destination_dir,
                qt_thread,
                jobs,
                cancel_flag,
                allow_directories,
                gallery_export,
            )
            .await;
        });
    }

    fn spawn_import(
        &self,
        udid: QString,
        job_id: QString,
        local_paths: QStringList,
        destination_dir: QString,
        afc_kind: AfcKind,
    ) {
        let udid = udid.to_string();
        let job_id = job_id.to_string();
        let destination_dir = destination_dir.to_string();
        let items = qstring_list_to_vec(local_paths);
        let item_count = items.len();
        let afc_kind_description = afc_kind.description();
        info!(
            "IOManager import requested: job_id={job_id} udid={udid} items={item_count} destination_dir={destination_dir} afc={afc_kind_description}"
        );
        if item_count == 0 {
            warn!("IOManager import requested with no items: job_id={job_id}");
        }
        let cancel_flag = self.register_job(&job_id);
        let jobs = self.jobs.clone();
        let qt_thread = self.qt_thread();

        RUNTIME.spawn(async move {
            debug!(
                "IOManager import creating AFC client: job_id={job_id} udid={udid} afc={afc_kind_description}"
            );
            let mut afc = match create_afc_client(&udid, afc_kind).await {
                Ok(afc) => {
                    debug!("IOManager import AFC client ready: job_id={job_id}");
                    afc
                }
                Err(err) => {
                    error!(
                        "IOManager import failed to create AFC client: job_id={job_id} udid={udid} afc={afc_kind_description}: {err}"
                    );
                    finish_import_job(&qt_thread, job_id.clone(), true, 0, 0, 0);
                    unregister_job(&jobs, &job_id);
                    return;
                }
            };

            handle_start_import(
                &mut afc,
                job_id.clone(),
                items,
                destination_dir,
                qt_thread,
                jobs,
                cancel_flag,
            )
            .await;
        });
    }

    fn register_job(&self, job_id: &str) -> Arc<AtomicBool> {
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let mut guard = self.jobs.lock().expect("IOManager jobs map mutex poisoned");
        if guard
            .insert(job_id.to_string(), cancel_flag.clone())
            .is_some()
        {
            warn!("IOManager replaced existing job registration: job_id={job_id}");
        } else {
            debug!("IOManager registered job: job_id={job_id}");
        }
        cancel_flag
    }
}

fn qstring_list_to_vec(paths: QStringList) -> Vec<String> {
    paths.into_iter().map(|path| path.to_string()).collect()
}

async fn create_afc_client(udid: &str, afc_kind: AfcKind) -> anyhow::Result<AfcClient> {
    let afc_kind_description = afc_kind.description();
    debug!("IOManager resolving device for AFC client: udid={udid} afc={afc_kind_description}");
    let device = device_ctx::get_device_opt(udid)
        .await
        .ok_or_else(|| anyhow::anyhow!("device {udid} not found"))?;

    match afc_kind {
        AfcKind::Standard => Ok(AfcClient::connect(device.provider.lock().await.as_ref()).await?),
        AfcKind::Afc2 => Ok(AfcClient::new_afc2(device.provider.lock().await.as_ref()).await?),
        AfcKind::HouseArrest(bundle_id) => {
            let provider = device.provider.lock().await;
            Ok(utils::vend_app_documents(provider.as_ref(), &bundle_id).await?)
        }
    }
}

async fn handle_start_export(
    afc: &mut AfcClient,
    job_id: String,
    device_paths: Vec<String>,
    destination_dir: String,
    qt_thread: QtThread<IOManager>,
    jobs: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    cancel_flag: Arc<AtomicBool>,
    allow_directories: bool,
    gallery_export: bool,
) {
    let mut successful = 0_i32;
    let mut failed = 0_i32;
    let mut total_bytes = 0_i64;
    let mut cancelled = false;

    debug!(
        "IOManager export job started: job_id={job_id} items={}",
        device_paths.len()
    );

    for device_path in device_paths {
        if cancel_flag.load(Ordering::Relaxed) {
            cancelled = true;
            info!("IOManager export job cancellation observed: job_id={job_id}");
            break;
        }

        debug!("IOManager export item requested: job_id={job_id} device_path={device_path}");
        match export_single_item(
            afc,
            &device_path,
            &destination_dir,
            &job_id,
            &qt_thread,
            &cancel_flag,
            allow_directories,
            gallery_export,
        )
        .await
        {
            Ok(result) if result.success => {
                successful += 1;
                total_bytes += result.bytes_transferred;
                debug!(
                    "IOManager export item finished: job_id={job_id} device_path={device_path} bytes={}",
                    result.bytes_transferred
                );
                emit_export_item_finished(&qt_thread, &job_id, &device_path, result, true);
            }
            Ok(result) => {
                failed += 1;
                warn!(
                    "IOManager export item did not complete successfully: job_id={job_id} device_path={device_path} bytes={} error={}",
                    result.bytes_transferred,
                    result.error_message.as_deref().unwrap_or("cancelled")
                );
                emit_export_item_finished(&qt_thread, &job_id, &device_path, result, false);
            }
            Err(err) => {
                failed += 1;
                error!(
                    "IOManager export item failed: job_id={job_id} device_path={device_path}: {err}"
                );
                let file_name = file_name_for_path(&device_path);
                let job_id_signal = job_id.clone();
                qt_thread.queue(move |mgr| {
                    mgr.exportItemFinished(
                        QString::from(job_id_signal),
                        QString::from(file_name),
                        QString::default(),
                        false,
                        0,
                        QString::from(err),
                    );
                });
            }
        }
    }

    cancelled |= cancel_flag.load(Ordering::Relaxed);
    finish_export_job(
        &qt_thread,
        job_id.clone(),
        cancelled,
        successful,
        failed,
        total_bytes,
    );
    info!(
        "IOManager export job finished: job_id={job_id} cancelled={cancelled} successful={successful} failed={failed} total_bytes={total_bytes}"
    );
    unregister_job(&jobs, &job_id);
}

async fn handle_start_import(
    afc: &mut AfcClient,
    job_id: String,
    local_paths: Vec<String>,
    destination_dir: String,
    qt_thread: QtThread<IOManager>,
    jobs: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    cancel_flag: Arc<AtomicBool>,
) {
    let mut successful = 0_i32;
    let mut failed = 0_i32;
    let mut total_bytes = 0_i64;
    let mut cancelled = false;

    debug!(
        "IOManager import job started: job_id={job_id} items={}",
        local_paths.len()
    );

    for local_path in local_paths {
        if cancel_flag.load(Ordering::Relaxed) {
            cancelled = true;
            info!("IOManager import job cancellation observed: job_id={job_id}");
            break;
        }

        debug!("IOManager import item requested: job_id={job_id} local_path={local_path}");
        match import_single_item(
            afc,
            &local_path,
            &destination_dir,
            &job_id,
            &qt_thread,
            &cancel_flag,
        )
        .await
        {
            Ok(result) if result.success => {
                successful += 1;
                total_bytes += result.bytes_transferred;
                debug!(
                    "IOManager import item finished: job_id={job_id} local_path={local_path} destination_path={} bytes={}",
                    result.destination_path, result.bytes_transferred
                );
                emit_import_item_finished(&qt_thread, &job_id, &local_path, result, true);
            }
            Ok(result) => {
                failed += 1;
                warn!(
                    "IOManager import item did not complete successfully: job_id={job_id} local_path={local_path} destination_path={} bytes={} error={}",
                    result.destination_path,
                    result.bytes_transferred,
                    result.error_message.as_deref().unwrap_or("cancelled")
                );
                emit_import_item_finished(&qt_thread, &job_id, &local_path, result, false);
            }
            Err(err) => {
                failed += 1;
                error!(
                    "IOManager import item failed: job_id={job_id} local_path={local_path}: {err}"
                );
                let file_name = file_name_for_path(&local_path);
                let job_id_signal = job_id.clone();
                qt_thread.queue(move |mgr| {
                    mgr.importItemFinished(
                        QString::from(job_id_signal),
                        QString::from(file_name),
                        QString::default(),
                        false,
                        0,
                        QString::from(err),
                    );
                });
            }
        }
    }

    finish_import_job(
        &qt_thread,
        job_id.clone(),
        cancelled,
        successful,
        failed,
        total_bytes,
    );
    info!(
        "IOManager import job finished: job_id={job_id} cancelled={cancelled} successful={successful} failed={failed} total_bytes={total_bytes}"
    );
    unregister_job(&jobs, &job_id);
}

async fn export_single_item(
    afc: &mut AfcClient,
    device_path: &str,
    destination_dir: &str,
    job_id: &str,
    qt_thread: &QtThread<IOManager>,
    cancel_flag: &Arc<AtomicBool>,
    allow_directories: bool,
    gallery_export: bool,
) -> Result<TransferItemResult, String> {
    fs::create_dir_all(destination_dir)
        .await
        .map_err(|e| format!("Failed to create destination directory {destination_dir}: {e}"))?;

    let resolved = afc
        .get_file_info_resolved(device_path.to_string())
        .await
        .map_err(|e| format!("Failed to resolve device path {device_path}: {e}"))?;
    let resolved_path = resolved.resolved_path;
    let info = resolved.info;

    let file_name = file_name_for_path(device_path);
    let (staged_file, local_file) = if gallery_export && info.st_ifmt != "S_IFDIR" {
        let (staged, file) = StagedFile::create_in(Path::new(destination_dir))
            .await
            .map_err(|error| format!("Failed to create gallery temporary file: {error}"))?;
        (Some(staged), Some(file))
    } else {
        (None, None)
    };
    let output_path = if let Some(staged) = staged_file.as_ref() {
        staged.path().to_path_buf()
    } else {
        unique_output_path(&Path::new(destination_dir).join(&file_name)).await
    };
    let mut output_path_str = output_path.to_string_lossy().to_string();
    debug!(
        "IOManager export item preparing: job_id={job_id} device_path={device_path} output_path={output_path_str}"
    );

    if info.st_ifmt == "S_IFDIR" {
        if !allow_directories {
            return Err(format!("Directory export is not enabled for {device_path}"));
        }

        return export_directory(
            afc,
            &resolved_path,
            &output_path,
            &file_name,
            job_id,
            qt_thread,
            cancel_flag,
        )
        .await;
    }

    let transferred = export_remote_file(
        afc,
        &resolved_path,
        &output_path,
        &info,
        job_id,
        qt_thread,
        &file_name,
        0,
        info.size as i64,
        cancel_flag,
        local_file,
    )
    .await?;

    if staged_file.is_some() {
        if cancel_flag.load(Ordering::Relaxed) {
            return Ok(TransferItemResult {
                success: false,
                bytes_transferred: transferred,
                destination_path: String::new(),
                error_message: Some("Export cancelled".to_string()),
            });
        }
        if transferred as u64 != info.size as u64 {
            return Err(format!(
                "Incomplete gallery download: expected {} bytes, received {transferred}",
                info.size
            ));
        }
        let finalized = gallery_export_finalize(
            &output_path,
            Path::new(destination_dir),
            &file_name,
            info.creation.date(),
            cancel_flag,
        )
        .await
        .map_err(|error| format!("Failed to finalize gallery export: {error}"))?;
        // Only aware embedded times identify an exact instant. Do not assign
        // the exporting computer's timezone to naive EXIF timestamps.
        let timestamp = gallery_export_timestamp(finalized.capture_time, &info);
        set_export_times(&finalized.path, timestamp);
        output_path_str = finalized.path.to_string_lossy().to_string();
    }

    Ok(TransferItemResult {
        success: !cancel_flag.load(Ordering::Relaxed),
        bytes_transferred: transferred,
        destination_path: output_path_str,
        error_message: cancel_flag
            .load(Ordering::Relaxed)
            .then(|| "Export cancelled".to_string()),
    })
}

async fn export_directory(
    afc: &mut AfcClient,
    device_path: &str,
    output_path: &Path,
    progress_name: &str,
    job_id: &str,
    qt_thread: &QtThread<IOManager>,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<TransferItemResult, String> {
    let manifest = build_directory_export_manifest(afc, device_path, cancel_flag).await?;
    let output_path_str = output_path.to_string_lossy().to_string();

    for relative_directory in &manifest.directories {
        if cancel_flag.load(Ordering::Relaxed) {
            return Ok(TransferItemResult {
                success: false,
                bytes_transferred: 0,
                destination_path: output_path_str,
                error_message: Some("Export cancelled".to_string()),
            });
        }

        let local_directory = output_path.join(relative_directory);
        fs::create_dir_all(&local_directory).await.map_err(|err| {
            format!(
                "Failed to create exported directory {}: {err}",
                local_directory.display()
            )
        })?;
    }

    let mut transferred = 0_i64;
    for file in manifest.files {
        if cancel_flag.load(Ordering::Relaxed) {
            break;
        }

        let local_path = output_path.join(&file.relative_path);
        if let Some(parent) = local_path.parent() {
            fs::create_dir_all(parent).await.map_err(|err| {
                format!(
                    "Failed to create exported directory {}: {err}",
                    parent.display()
                )
            })?;
        }

        transferred += export_remote_file(
            afc,
            &file.remote_path,
            &local_path,
            &file.info,
            job_id,
            qt_thread,
            progress_name,
            transferred,
            manifest.total_bytes,
            cancel_flag,
            None,
        )
        .await?;
    }

    Ok(TransferItemResult {
        success: !cancel_flag.load(Ordering::Relaxed),
        bytes_transferred: transferred,
        destination_path: output_path_str,
        error_message: cancel_flag
            .load(Ordering::Relaxed)
            .then(|| "Export cancelled".to_string()),
    })
}

async fn build_directory_export_manifest(
    afc: &mut AfcClient,
    device_path: &str,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<DirectoryExportManifest, String> {
    let mut manifest = DirectoryExportManifest {
        directories: vec![PathBuf::new()],
        files: Vec::new(),
        total_bytes: 0,
    };
    let mut root_ancestors = HashSet::new();
    root_ancestors.insert(device_path.to_string());
    let mut pending_directories = vec![(device_path.to_string(), PathBuf::new(), root_ancestors)];

    while let Some((remote_directory, relative_directory, ancestors)) = pending_directories.pop() {
        if cancel_flag.load(Ordering::Relaxed) {
            return Err("Export cancelled while enumerating directory".to_string());
        }

        let entries = afc
            .list_dir(&remote_directory)
            .await
            .map_err(|err| format!("Failed to list directory {remote_directory}: {err}"))?;

        for name in entries {
            if name == "." || name == ".." {
                continue;
            }
            validate_remote_child_name(&name)?;

            let remote_path = remote_child_path(&remote_directory, &name);
            let relative_path = relative_directory.join(&name);
            let resolved = afc
                .get_file_info_resolved(&remote_path)
                .await
                .map_err(|err| format!("Failed to resolve {remote_path}: {err}"))?;
            let resolved_path = resolved.resolved_path;
            let info = resolved.info;

            if info.st_ifmt == "S_IFDIR" {
                if ancestors.contains(&resolved_path) {
                    return Err(format!(
                        "Refusing symbolic-link directory cycle at {remote_path} -> {resolved_path}"
                    ));
                }
                // only current ancestors are checked,
                // so separate aliases may export the same target
                // this should be ok
                let mut child_ancestors = ancestors.clone();
                child_ancestors.insert(resolved_path.clone());
                manifest.directories.push(relative_path.clone());
                pending_directories.push((resolved_path, relative_path, child_ancestors));
            } else {
                manifest.total_bytes = manifest.total_bytes.saturating_add(info.size as i64);
                manifest.files.push(RemoteExportFile {
                    remote_path: resolved_path,
                    relative_path,
                    info,
                });
            }
        }
    }

    Ok(manifest)
}

#[allow(clippy::too_many_arguments)]
async fn export_remote_file(
    afc: &mut AfcClient,
    device_path: &str,
    output_path: &Path,
    info: &FileInfo,
    job_id: &str,
    qt_thread: &QtThread<IOManager>,
    progress_name: &str,
    progress_offset: i64,
    progress_total: i64,
    cancel_flag: &Arc<AtomicBool>,
    local_file: Option<fs::File>,
) -> Result<i64, String> {
    use tokio::io::AsyncReadExt;

    let output_path_str = output_path.to_string_lossy().to_string();
    let mut remote = afc
        .open(device_path, AfcFopenMode::RdOnly)
        .await
        .map_err(|err| format!("Failed to open device file {device_path}: {err}"))?;
    // A gallery temporary file is already reserved with create_new. Write through
    // that handle instead of reopening its path and potentially truncating a replacement.
    let mut local = match local_file {
        Some(file) => file,
        None => fs::File::create(output_path)
            .await
            .map_err(|err| format!("Failed to create local file {output_path_str}: {err}"))?,
    };
    let mut chunk = vec![0_u8; DEFAULT_CHUNK_SIZE];
    let mut transferred = 0_i64;

    loop {
        if cancel_flag.load(Ordering::Relaxed) {
            break;
        }

        let read = remote
            .read(&mut chunk)
            .await
            .map_err(|err| format!("Failed to read from device file {device_path}: {err}"))?;
        if read == 0 {
            break;
        }

        local
            .write_all(&chunk[..read])
            .await
            .map_err(|err| format!("Failed to write to local file {output_path_str}: {err}"))?;
        transferred += read as i64;
        emit_progress(
            qt_thread,
            job_id,
            progress_name,
            progress_offset.saturating_add(transferred),
            progress_total,
        );
    }

    let _ = remote.close().await;
    local
        .flush()
        .await
        .map_err(|error| format!("Failed to flush local file {output_path_str}: {error}"))?;
    drop(local);

    if transferred > 0 {
        preserve_export_times(output_path, info);
    }

    Ok(transferred)
}

fn remote_child_path(parent: &str, name: &str) -> String {
    if parent.ends_with('/') {
        format!("{parent}{name}")
    } else {
        format!("{parent}/{name}")
    }
}

fn validate_remote_child_name(name: &str) -> Result<(), String> {
    let mut components = Path::new(name).components();
    let valid = matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none();
    if valid {
        Ok(())
    } else {
        Err(format!("Refusing unsafe remote directory entry {name:?}"))
    }
}

async fn import_single_item(
    afc: &mut AfcClient,
    local_path: &str,
    destination_dir: &str,
    job_id: &str,
    qt_thread: &QtThread<IOManager>,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<TransferItemResult, String> {
    use tokio::io::AsyncReadExt;

    let file_name = file_name_for_path(local_path);
    let device_path = if destination_dir.ends_with('/') {
        format!("{destination_dir}{file_name}")
    } else {
        format!("{destination_dir}/{file_name}")
    };
    debug!(
        "IOManager import item preparing: job_id={job_id} local_path={local_path} device_path={device_path}"
    );

    let mut local = fs::File::open(local_path)
        .await
        .map_err(|e| format!("Failed to open local file {local_path}: {e}"))?;
    let metadata = local
        .metadata()
        .await
        .map_err(|e| format!("Failed to stat local file {local_path}: {e}"))?;
    let file_size = metadata.len() as i64;

    let mut remote = afc
        .open(&device_path, AfcFopenMode::WrOnly)
        .await
        .map_err(|e| format!("Failed to open device file {device_path} for writing: {e}"))?;

    let mut chunk = vec![0u8; DEFAULT_CHUNK_SIZE];
    let mut transferred = 0_i64;

    loop {
        if cancel_flag.load(Ordering::Relaxed) {
            debug!(
                "IOManager import item cancellation observed: job_id={job_id} local_path={local_path} bytes={transferred}"
            );
            break;
        }

        let read = local
            .read(&mut chunk)
            .await
            .map_err(|e| format!("Failed to read from local file {local_path}: {e}"))?;
        if read == 0 {
            break;
        }

        remote
            .write_all(&chunk[..read])
            .await
            .map_err(|e| format!("Failed to write to device file {device_path}: {e}"))?;
        transferred += read as i64;

        emit_progress(qt_thread, job_id, &file_name, transferred, file_size);
    }

    let _ = remote.close().await;

    Ok(TransferItemResult {
        success: !cancel_flag.load(Ordering::Relaxed),
        bytes_transferred: transferred,
        destination_path: device_path,
        error_message: None,
    })
}

fn preserve_export_times(output_path: &Path, info: &FileInfo) {
    set_export_times(output_path, info.modified);
}

fn gallery_export_timestamp(
    capture_time: Option<nom_exif::ExifDateTime>,
    info: &FileInfo,
) -> chrono::NaiveDateTime {
    match capture_time {
        Some(nom_exif::ExifDateTime::Aware(time)) => time.naive_utc(),
        Some(nom_exif::ExifDateTime::Naive(_)) => info.modified,
        None => info.creation,
    }
}

fn set_export_times(output_path: &Path, timestamp: chrono::NaiveDateTime) {
    let modified_utc = timestamp.and_utc();
    let mtime = filetime::FileTime::from_unix_time(
        modified_utc.timestamp(),
        modified_utc.timestamp_subsec_nanos(),
    );
    if let Err(error) = filetime::set_file_times(output_path, mtime, mtime) {
        warn!(
            "Failed to preserve file time for {}: {error}",
            output_path.display()
        );
    }
}

async fn unique_output_path(base_path: &Path) -> PathBuf {
    if fs::try_exists(base_path).await.unwrap_or(false) {
        let stem = base_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("file");
        let ext = base_path.extension().and_then(|ext| ext.to_str());
        let parent = base_path.parent().unwrap_or_else(|| Path::new("."));

        for index in 1..1000 {
            let file_name = match ext {
                Some(ext) if !ext.is_empty() => format!("{stem} ({index}).{ext}"),
                _ => format!("{stem} ({index})"),
            };
            let candidate = parent.join(file_name);
            if !fs::try_exists(&candidate).await.unwrap_or(false) {
                return candidate;
            }
        }
    }

    base_path.to_path_buf()
}

fn file_name_for_path(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_string()
}

fn emit_progress(
    qt_thread: &QtThread<IOManager>,
    job_id: &str,
    file_name: &str,
    transferred: i64,
    total: i64,
) {
    let job_id = job_id.to_string();
    let file_name = file_name.to_string();
    qt_thread.queue(move |mgr| {
        mgr.fileTransferProgress(
            QString::from(job_id),
            QString::from(file_name),
            transferred,
            total,
        );
    });
}

fn emit_export_item_finished(
    qt_thread: &QtThread<IOManager>,
    job_id: &str,
    source_path: &str,
    result: TransferItemResult,
    success: bool,
) {
    let job_id = job_id.to_string();
    let file_name = file_name_for_path(source_path);
    let destination_path = result.destination_path;
    let error_message = result.error_message.unwrap_or_default();
    qt_thread.queue(move |mgr| {
        mgr.exportItemFinished(
            QString::from(job_id),
            QString::from(file_name),
            QString::from(destination_path),
            success,
            result.bytes_transferred,
            QString::from(error_message),
        );
    });
}

fn emit_import_item_finished(
    qt_thread: &QtThread<IOManager>,
    job_id: &str,
    source_path: &str,
    result: TransferItemResult,
    success: bool,
) {
    let job_id = job_id.to_string();
    let file_name = file_name_for_path(source_path);
    let destination_path = result.destination_path;
    let error_message = result.error_message.unwrap_or_default();
    qt_thread.queue(move |mgr| {
        mgr.importItemFinished(
            QString::from(job_id),
            QString::from(file_name),
            QString::from(destination_path),
            success,
            result.bytes_transferred,
            QString::from(error_message),
        );
    });
}

fn finish_export_job(
    qt_thread: &QtThread<IOManager>,
    job_id: String,
    cancelled: bool,
    successful: i32,
    failed: i32,
    total_bytes: i64,
) {
    qt_thread.queue(move |mgr| {
        mgr.exportJobFinished(
            QString::from(job_id),
            cancelled,
            successful,
            failed,
            total_bytes,
        );
    });
}

fn finish_import_job(
    qt_thread: &QtThread<IOManager>,
    job_id: String,
    cancelled: bool,
    successful: i32,
    failed: i32,
    total_bytes: i64,
) {
    qt_thread.queue(move |mgr| {
        mgr.importJobFinished(
            QString::from(job_id),
            cancelled,
            successful,
            failed,
            total_bytes,
        );
    });
}

fn unregister_job(jobs: &Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>, job_id: &str) {
    let mut guard = jobs.lock().expect("IOManager jobs map mutex poisoned");
    if guard.remove(job_id).is_some() {
        debug!("IOManager unregistered job: job_id={job_id}");
    } else {
        warn!("IOManager tried to unregister unknown job: job_id={job_id}");
    }
}

pub struct StagedFile {
    path: PathBuf,
}

impl StagedFile {
    pub async fn create_in(destination: &Path) -> io::Result<(Self, fs::File)> {
        for _ in 0..8 {
            let path = destination.join(format!(
                ".idescriptor-gallery-{}.part",
                uuid::Uuid::new_v4()
            ));
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            options.mode(0o600);
            match options.open(&path).await {
                Ok(file) => return Ok((Self { path }, file)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Could not reserve a unique gallery temporary file",
        ))
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for StagedFile {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_file(&self.path)
            && error.kind() != io::ErrorKind::NotFound
        {
            log::warn!(
                "Could not remove gallery temporary file {}: {error}",
                self.path.display()
            );
        }
    }
}

#[derive(Debug)]
pub struct FinalizedExport {
    pub path: PathBuf,
    pub capture_time: Option<ExifDateTime>,
}

fn entry_capture_time(value: &EntryValue) -> Option<ExifDateTime> {
    // Preserve the recorded calendar day, including when a timezone is absent.
    // Do not reinterpret a naive EXIF time using the exporting computer's timezone.
    let time = value.as_datetime()?;
    (1..=9999)
        .contains(&time.into_naive().year())
        .then_some(time)
}

fn image_capture_time(exif: &Exif, tag: ExifTag) -> Option<ExifDateTime> {
    let naive = entry_capture_time(exif.get(tag)?)?.into_naive();
    // nom-exif can compose date tags with a shared offset. Pair offsets explicitly
    // so CreateDate cannot inherit DateTimeOriginal's unrelated timezone (or vice versa).
    let offset_tag = match tag {
        ExifTag::DateTimeOriginal => ExifTag::OffsetTimeOriginal,
        ExifTag::CreateDate => ExifTag::OffsetTimeDigitized,
        _ => return None,
    };
    let offset = exif
        .get(offset_tag)
        .and_then(EntryValue::as_str)
        .and_then(|text| text.trim().parse::<chrono::FixedOffset>().ok());
    match offset {
        Some(offset) => {
            use chrono::TimeZone;
            Some(ExifDateTime::Aware(
                offset.from_local_datetime(&naive).single()?,
            ))
        }
        None => Some(ExifDateTime::Naive(naive)),
    }
}

fn read_capture_time(path: &Path) -> anyhow::Result<Option<ExifDateTime>> {
    let source = MediaSource::open(path)?;
    let mut parser = MediaParser::new();
    let time = match source.kind() {
        MediaKind::Image => {
            let exif: Exif = parser.parse_exif(source)?.into();
            [ExifTag::DateTimeOriginal, ExifTag::CreateDate]
                .into_iter()
                .find_map(|tag| image_capture_time(&exif, tag))
        }
        MediaKind::Track => parser
            .parse_track(source)?
            .get(TrackInfoTag::CreateDate)
            .and_then(entry_capture_time)
            // A zero QuickTime timestamp is an unset date, not a capture in 1904.
            .filter(|time| {
                time.into_naive().date() != NaiveDate::from_ymd_opt(1904, 1, 1).unwrap()
            }),
    };
    Ok(time)
}

pub async fn gallery_export_finalize(
    staged_path: &Path,
    destination: &Path,
    original_name: &str,
    creation_date: NaiveDate,
    cancelled: &AtomicBool,
) -> io::Result<FinalizedExport> {
    let path = staged_path.to_path_buf();
    let capture_time = match tokio::task::spawn_blocking(move || read_capture_time(&path)).await {
        Ok(Ok(time)) => time,
        Ok(Err(error)) => {
            log::debug!("Embedded capture date unavailable; using AFC creation date: {error}");
            None
        }
        Err(error) => {
            log::warn!("Capture-date parser task failed; using AFC creation date: {error}");
            None
        }
    };
    let date = capture_time
        .map(|time| time.into_naive().date())
        .unwrap_or(creation_date);
    let filename = format!("{}_{original_name}", date.format("%Y-%m-%d"));
    let base = destination.join(filename);
    for index in 0..1000 {
        if cancelled.load(Ordering::Relaxed) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Export cancelled",
            ));
        }
        let candidate = collision_path(&base, index);
        match place_without_overwrite(staged_path, &candidate, cancelled).await {
            Ok(()) => {
                return Ok(FinalizedExport {
                    path: candidate,
                    capture_time,
                });
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "Too many gallery export filename collisions",
    ))
}

fn collision_path(base: &Path, index: usize) -> PathBuf {
    if index == 0 {
        return base.to_path_buf();
    }
    let stem = base.file_stem().unwrap_or_default().to_string_lossy();
    let name = match base.extension().filter(|ext| !ext.is_empty()) {
        Some(extension) => format!("{stem} ({index}).{}", extension.to_string_lossy()),
        None => format!("{stem} ({index})"),
    };
    base.with_file_name(name)
}

async fn place_without_overwrite(
    source: &Path,
    destination: &Path,
    cancelled: &AtomicBool,
) -> io::Result<()> {
    // TODO: hard_link is used here bc we dont want to overwrite an existing file
    // however maybe in the future we may ask to keep or replace the file
    match fs::hard_link(source, destination).await {
        Ok(()) => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Err(error),
        // FAT/exFAT and some network volumes do not support hard links. Reserve the
        // destination atomically before copying instead of using an overwriting rename.
        Err(_) => copy_without_overwrite(source, destination, cancelled).await,
    }
}

async fn copy_without_overwrite(
    source: &Path,
    destination: &Path,
    cancelled: &AtomicBool,
) -> io::Result<()> {
    let mut input = fs::File::open(source).await?;
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .await?;
    let result = async {
        // TODO: this is happening from pc -> pc not from pc -> device or device -> pc,
        // so we may need another var instead of DEFAULT_CHUNK_SIZE , but it's ok for now
        let mut buffer = vec![0_u8; DEFAULT_CHUNK_SIZE];
        loop {
            if cancelled.load(Ordering::Relaxed) {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Export cancelled",
                ));
            }
            let count = input.read(&mut buffer).await?;
            if count == 0 {
                break;
            }
            output.write_all(&buffer[..count]).await?;
        }
        output.flush().await
    }
    .await;
    drop(output);
    if result.is_err() {
        // This path was created exclusively by this attempt, not a pre-existing file.
        if let Err(error) = fs::remove_file(destination).await {
            log::warn!(
                "Could not remove incomplete gallery export {}: {error}",
                destination.display()
            );
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gallery_times_use_aware_capture_then_safe_afc_fallbacks() {
        let directory = std::env::temp_dir().join(format!(
            "idescriptor-gallery-times-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&directory).unwrap();
        let guard = crate::utils::TempDirGuard::new(directory);
        let path = guard.path().join("gallery.JPG");
        std::fs::write(&path, b"unchanged metadata bytes").unwrap();
        let modified = chrono::DateTime::from_timestamp(1_600_000_000, 123_456_789)
            .unwrap()
            .naive_utc();
        let info = FileInfo {
            size: 24,
            blocks: 1,
            creation: modified - chrono::Duration::days(1),
            modified,
            st_nlink: "1".into(),
            st_ifmt: "S_IFREG".into(),
            st_link_target: None,
        };
        let capture =
            chrono::DateTime::parse_from_rfc3339("2020-02-29T00:15:00.987654321+12:00").unwrap();
        let expected_utc = chrono::DateTime::parse_from_rfc3339("2020-02-28T12:15:00.987654321Z")
            .unwrap()
            .naive_utc();
        for (embedded, expected) in [
            (Some(nom_exif::ExifDateTime::Aware(capture)), expected_utc),
            (
                Some(nom_exif::ExifDateTime::Naive(capture.naive_local())),
                info.modified,
            ),
            (None, info.creation),
        ] {
            let timestamp = gallery_export_timestamp(embedded, &info);
            assert_eq!(timestamp, expected);
            set_export_times(&path, timestamp);
            let metadata = std::fs::metadata(&path).unwrap();
            assert_eq!(
                filetime::FileTime::from_last_modification_time(&metadata).unix_seconds(),
                expected.and_utc().timestamp()
            );
            assert_eq!(
                filetime::FileTime::from_last_access_time(&metadata).unix_seconds(),
                expected.and_utc().timestamp()
            );
        }
        assert_eq!(std::fs::read(&path).unwrap(), b"unchanged metadata bytes");
    }

    #[test]
    fn existing_timestamp_policy_preserves_remote_mtime_as_mtime_and_atime() {
        let directory =
            std::env::temp_dir().join(format!("idescriptor-times-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let guard = crate::utils::TempDirGuard::new(directory);
        let path = guard.path().join("exported.JPG");
        std::fs::write(&path, b"original bytes").unwrap();
        let modified = chrono::DateTime::from_timestamp(1_600_000_000, 123_456_789)
            .unwrap()
            .naive_utc();
        let info = FileInfo {
            size: 14,
            blocks: 1,
            creation: modified - chrono::Duration::days(1),
            modified,
            st_nlink: "1".into(),
            st_ifmt: "S_IFREG".into(),
            st_link_target: None,
        };
        preserve_export_times(&path, &info);
        let metadata = std::fs::metadata(&path).unwrap();
        // Filesystem timestamp precision varies; check whole seconds portably.
        assert_eq!(
            filetime::FileTime::from_last_modification_time(&metadata).unix_seconds(),
            1_600_000_000
        );
        assert_eq!(
            filetime::FileTime::from_last_access_time(&metadata).unix_seconds(),
            1_600_000_000
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"original bytes");
    }

    #[tokio::test]
    async fn dated_export_names_keep_existing_collision_handling() {
        let directory =
            std::env::temp_dir().join(format!("idescriptor-export-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).await.unwrap();
        let _guard = crate::utils::TempDirGuard::new(directory.clone());
        let path = directory.join("2026-10-22_IMG_0042.MOV");
        fs::write(&path, b"existing").await.unwrap();
        let next = unique_output_path(&path).await;
        assert_eq!(
            next.file_name().unwrap().to_str(),
            Some("2026-10-22_IMG_0042 (1).MOV")
        );
        assert_eq!(fs::read(&path).await.unwrap(), b"existing");
    }

    fn fixture(bytes: &[u8]) -> (crate::utils::TempDirGuard, PathBuf) {
        let directory = std::env::temp_dir().join(format!(
            "idescriptor-metadata-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&directory).unwrap();
        let guard = crate::utils::TempDirGuard::new(directory);
        let path = guard.path().join("media");
        std::fs::write(&path, bytes).unwrap();
        (guard, path)
    }

    fn jpeg(original: &[u8], created: &[u8]) -> Vec<u8> {
        jpeg_with_offsets(original, created, &[(0x9011, b"+12:00\0")])
    }

    fn jpeg_with_offsets(original: &[u8], created: &[u8], offsets: &[(u16, &[u8])]) -> Vec<u8> {
        // Minimal JPEG APP1/EXIF fixture: IFD0 points to date/offset ASCII tags.
        // No copyrighted photo or personal metadata is embedded.
        let mut tiff = b"II\x2a\0\x08\0\0\0".to_vec();
        tiff.extend_from_slice(&1_u16.to_le_bytes());
        tiff.extend_from_slice(&0x8769_u16.to_le_bytes());
        tiff.extend_from_slice(&4_u16.to_le_bytes());
        tiff.extend_from_slice(&1_u32.to_le_bytes());
        tiff.extend_from_slice(&26_u32.to_le_bytes());
        tiff.extend_from_slice(&0_u32.to_le_bytes());
        let mut fields = vec![(0x9003_u16, original), (0x9004_u16, created)];
        fields.extend_from_slice(offsets);
        tiff.extend_from_slice(&(fields.len() as u16).to_le_bytes());
        let mut offset = 26 + 2 + fields.len() as u32 * 12 + 4;
        for (tag, value) in &fields {
            tiff.extend_from_slice(&tag.to_le_bytes());
            tiff.extend_from_slice(&2_u16.to_le_bytes());
            tiff.extend_from_slice(&(value.len() as u32).to_le_bytes());
            tiff.extend_from_slice(&offset.to_le_bytes());
            offset += value.len() as u32;
        }
        tiff.extend_from_slice(&0_u32.to_le_bytes());
        for (_, value) in fields {
            tiff.extend_from_slice(value);
        }
        let mut result = b"\xff\xd8\xff\xe1".to_vec();
        result.extend_from_slice(&((tiff.len() + 8) as u16).to_be_bytes());
        result.extend_from_slice(b"Exif\0\0");
        result.extend_from_slice(&tiff);
        // Include enough bytes for the parser's 128-byte format-sniffing header.
        result.extend_from_slice(b"\xff\xfe\x00\x22");
        result.extend_from_slice(&[b' '; 32]);
        result.extend_from_slice(b"\xff\xd9");
        result
    }

    fn atom(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut result = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        result.extend_from_slice(kind);
        result.extend_from_slice(body);
        result
    }

    #[test]
    fn parses_jpeg_original_date_and_falls_back_to_create_date() {
        for (original, expected) in [
            (b"2020:02:29 00:15:00\0".as_slice(), "2020-02-29"),
            (b"0000:00:00 00:00:00\0".as_slice(), "2021-03-01"),
        ] {
            let (_guard, path) = fixture(&jpeg(original, b"2021:03:01 12:00:00\0"));
            assert_eq!(
                read_capture_time(&path)
                    .unwrap()
                    .unwrap()
                    .into_naive()
                    .date()
                    .to_string(),
                expected
            );
        }
    }

    #[test]
    fn jpeg_without_a_matching_timezone_keeps_the_full_naive_time() {
        // CreateDate uses OffsetTimeDigitized, not the OffsetTimeOriginal in this fixture.
        let (_guard, path) = fixture(&jpeg(b"0000:00:00 00:00:00\0", b"2021:03:01 12:34:56\0"));
        let expected = chrono::NaiveDate::from_ymd_opt(2021, 3, 1)
            .unwrap()
            .and_hms_opt(12, 34, 56)
            .unwrap();
        assert_eq!(
            read_capture_time(&path).unwrap(),
            Some(ExifDateTime::Naive(expected))
        );
    }

    #[test]
    fn jpeg_pairs_each_date_with_its_own_offset_and_keeps_missing_offsets_naive() {
        let original = b"2020:02:29 00:15:00\0";
        let created = b"2021:03:01 12:34:56\0";
        for (original, expected) in [
            (original.as_slice(), "2020-02-29T00:15:00+12:00"),
            (
                b"0000:00:00 00:00:00\0".as_slice(),
                "2021-03-01T12:34:56-07:00",
            ),
        ] {
            let bytes = jpeg_with_offsets(
                original,
                created,
                &[(0x9011, b"+12:00\0"), (0x9012, b"-07:00\0")],
            );
            let (_guard, path) = fixture(&bytes);
            assert_eq!(
                read_capture_time(&path).unwrap(),
                Some(ExifDateTime::Aware(
                    chrono::DateTime::parse_from_rfc3339(expected).unwrap()
                ))
            );
        }
        let (_guard, path) = fixture(&jpeg_with_offsets(original, created, &[]));
        assert!(matches!(
            read_capture_time(&path).unwrap(),
            Some(ExifDateTime::Naive(_))
        ));
    }

    #[test]
    fn parses_mov_and_mp4_container_creation_dates() {
        for brand in [b"qt  ", b"mp42"] {
            let mut ftyp = brand.to_vec();
            ftyp.extend_from_slice(&0_u32.to_be_bytes());
            ftyp.extend_from_slice(brand);
            let mut bytes = atom(b"ftyp", &ftyp);
            let created = chrono::DateTime::parse_from_rfc3339("2020-02-29T23:30:00Z")
                .unwrap()
                .timestamp()
                + 2_082_844_800;
            let mut mvhd = vec![0; 4]; // Full-box version 0 and flags.
            for value in [created as u32, created as u32, 1000, 1000] {
                mvhd.extend_from_slice(&value.to_be_bytes());
            }
            mvhd.extend_from_slice(&[0; 76]);
            mvhd.extend_from_slice(&1_u32.to_be_bytes());
            bytes.extend_from_slice(&atom(b"moov", &atom(b"mvhd", &mvhd)));
            let (_guard, path) = fixture(&bytes);
            let capture_time = read_capture_time(&path).unwrap().unwrap();
            let expected = chrono::DateTime::parse_from_rfc3339("2020-02-29T23:30:00Z").unwrap();
            assert_eq!(capture_time, ExifDateTime::Aware(expected));
            assert_eq!(capture_time.into_naive().date().to_string(), "2020-02-29");
        }
    }

    #[test]
    fn embedded_dates_keep_the_recorded_calendar_day() {
        let aware = chrono::DateTime::parse_from_rfc3339("2020-02-29T00:15:00+12:00").unwrap();
        assert_eq!(
            entry_capture_time(&EntryValue::DateTime(aware))
                .unwrap()
                .into_naive()
                .date()
                .to_string(),
            "2020-02-29"
        );
        assert_eq!(
            entry_capture_time(&EntryValue::NaiveDateTime(aware.naive_local()))
                .unwrap()
                .into_naive()
                .date()
                .to_string(),
            "2020-02-29"
        );
        assert_eq!(
            entry_capture_time(&EntryValue::Text("invalid".into())),
            None
        );
    }

    #[tokio::test]
    async fn staging_reserves_distinct_files_directly_in_destination_and_cleans_them() {
        let directory =
            std::env::temp_dir().join(format!("idescriptor-staging-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&directory).await.unwrap();
        let _directory_guard = crate::utils::TempDirGuard::new(directory.clone());
        let (first, mut writer) = StagedFile::create_in(&directory).await.unwrap();
        let (second, second_writer) = StagedFile::create_in(&directory).await.unwrap();
        assert_ne!(first.path(), second.path());
        for path in [first.path(), second.path()] {
            assert_eq!(path.parent(), Some(directory.as_path()));
            assert_eq!(
                path.extension().and_then(|extension| extension.to_str()),
                Some("part")
            );
            assert!(fs::metadata(path).await.unwrap().is_file());
        }
        let first_path = first.path().to_path_buf();
        writer.write_all(b"complete media bytes").await.unwrap();
        writer.flush().await.unwrap();
        drop(writer);
        let final_path = gallery_export_finalize(
            first.path(),
            &directory,
            "IMG_0042.MOV",
            NaiveDate::from_ymd_opt(2020, 2, 29).unwrap(),
            &AtomicBool::new(false),
        )
        .await
        .unwrap();
        drop(first);
        assert!(!first_path.exists());
        assert_eq!(
            fs::read(&final_path.path).await.unwrap(),
            b"complete media bytes"
        );
        let second_path = second.path().to_path_buf();
        drop(second_writer);
        drop(second);
        assert!(!second_path.exists());
        let mut entries = fs::read_dir(&directory).await.unwrap();
        assert_eq!(
            entries.next_entry().await.unwrap().unwrap().path(),
            final_path.path
        );
        assert!(entries.next_entry().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn staging_cleanup_runs_on_error_and_cancellation() {
        let directory = std::env::temp_dir().join(format!(
            "idescriptor-staging-error-test-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&directory).await.unwrap();
        let _directory_guard = crate::utils::TempDirGuard::new(directory.clone());
        let result: io::Result<()> = async {
            let (_staged, mut writer) = StagedFile::create_in(&directory).await?;
            writer.write_all(b"partial download").await?;
            writer.flush().await?;
            Err(io::Error::other("simulated transfer failure"))
        }
        .await;
        assert!(result.is_err());
        assert!(
            fs::read_dir(&directory)
                .await
                .unwrap()
                .next_entry()
                .await
                .unwrap()
                .is_none()
        );

        let (staged, writer) = StagedFile::create_in(&directory).await.unwrap();
        let path = staged.path().to_path_buf();
        drop(writer);
        let error = gallery_export_finalize(
            staged.path(),
            &directory,
            "cancelled.JPG",
            NaiveDate::from_ymd_opt(2020, 2, 29).unwrap(),
            &AtomicBool::new(true),
        )
        .await
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        drop(staged);
        assert!(!path.exists());
        assert!(
            fs::read_dir(&directory)
                .await
                .unwrap()
                .next_entry()
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn finalization_prefers_embedded_date_and_leaves_metadata_bytes_unchanged() {
        let bytes = jpeg(b"2020:02:29 00:15:00\0", b"2021:03:01 12:00:00\0");
        let (guard, staged) = fixture(&bytes);
        let path = gallery_export_finalize(
            &staged,
            guard.path(),
            "été photo.JPG",
            NaiveDate::from_ymd_opt(2026, 10, 22).unwrap(),
            &AtomicBool::new(false),
        )
        .await
        .unwrap();
        assert_eq!(
            path.path.file_name().unwrap().to_str(),
            Some("2020-02-29_été photo.JPG")
        );
        let expected = chrono::DateTime::parse_from_rfc3339("2020-02-29T00:15:00+12:00").unwrap();
        assert_eq!(path.capture_time, Some(ExifDateTime::Aware(expected)));
        assert_eq!(fs::read(&path.path).await.unwrap(), bytes);
    }

    #[tokio::test]
    async fn missing_metadata_uses_creation_date_and_never_overwrites() {
        let directory =
            std::env::temp_dir().join(format!("idescriptor-gallery-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&directory).await.unwrap();
        let _guard = crate::utils::TempDirGuard::new(directory.clone());
        let staged = directory.join("staged");
        fs::write(&staged, b"no embedded metadata").await.unwrap();
        let existing = directory.join("2020-02-29_IMG_0042.MOV");
        fs::write(&existing, b"existing").await.unwrap();
        let path = gallery_export_finalize(
            &staged,
            &directory,
            "IMG_0042.MOV",
            NaiveDate::from_ymd_opt(2020, 2, 29).unwrap(),
            &AtomicBool::new(false),
        )
        .await
        .unwrap();
        assert_eq!(
            path.path.file_name().unwrap().to_str(),
            Some("2020-02-29_IMG_0042 (1).MOV")
        );
        assert_eq!(fs::read(&existing).await.unwrap(), b"existing");
        assert_eq!(path.capture_time, None);
        assert_eq!(fs::read(&path.path).await.unwrap(), b"no embedded metadata");
        let cancelled = gallery_export_finalize(
            &staged,
            &directory,
            "cancelled.JPG",
            NaiveDate::from_ymd_opt(2020, 2, 29).unwrap(),
            &AtomicBool::new(true),
        )
        .await;
        assert_eq!(cancelled.unwrap_err().kind(), io::ErrorKind::Interrupted);
        assert!(!directory.join("2020-02-29_cancelled.JPG").exists());
    }

    #[tokio::test]
    async fn copy_fallback_preserves_bytes_and_existing_files() {
        let directory = std::env::temp_dir().join(format!(
            "idescriptor-gallery-copy-test-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&directory).await.unwrap();
        let _guard = crate::utils::TempDirGuard::new(directory.clone());
        let source = directory.join("source");
        let destination = directory.join("destination");
        fs::write(&source, b"source bytes").await.unwrap();
        copy_without_overwrite(&source, &destination, &AtomicBool::new(false))
            .await
            .unwrap();
        assert_eq!(fs::read(&destination).await.unwrap(), b"source bytes");
        let error = copy_without_overwrite(&source, &destination, &AtomicBool::new(false))
            .await
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&destination).await.unwrap(), b"source bytes");
        let cancelled = directory.join("cancelled");
        assert!(
            copy_without_overwrite(&source, &cancelled, &AtomicBool::new(true))
                .await
                .is_err()
        );
        assert!(!cancelled.exists());
    }
}
