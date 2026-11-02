// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    RUNTIME,
    list_model::ListModel,
    qt_threading::{QtThread, QtThreading},
    qvariantmap_insert,
};
use idevice::{
    IdeviceError, IdeviceService,
    afc::{AfcClient, errors::AfcError, file::OwnedFileDescriptor, opcode::AfcFopenMode},
    installation_proxy::InstallationProxyClient,
    lockdown::LockdownClient,
    mobilebackup2::{
        BackupDelegate, BackupOptions, DirEntryInfo, FsBackupDelegate, MobileBackup2Client,
        RestoreOptions,
    },
    notification_proxy::{
        MOBILEBACKUP2_NOTIFICATIONS, MobileBackup2Notification, NotificationProxyClient,
        SYNC_DID_FINISH, SYNC_DID_START, SYNC_LOCK_REQUEST, SYNC_WILL_START,
    },
    provider::IdeviceProvider,
    springboardservices::SpringBoardServicesClient,
};
use log::{debug, error, info, warn};
use macros::QtThreading;
use plist_macro::plist;
use qmetaobject::{SimpleListItem, prelude::*};
use qttypes::{QString, QVariantMap};
use std::{
    cell::RefCell,
    collections::HashMap,
    error::Error as StdError,
    fmt,
    future::Future,
    io::{Read, Write},
    path::{Path, PathBuf},
    pin::Pin,
    time::{Duration, SystemTime},
};
use tokio::task::JoinHandle;

#[derive(SimpleListItem, Default, Clone)]
struct BackupListItem {
    pub udid: QString,
    pub source: QString,
    pub device_name: QString,
    pub display_name: QString,
    pub last_backup: QString,
    pub version: QString,
    pub encrypted: bool,
    pub path: QString,
    pub status: QString,
}

// #[derive(Debug, Clone)]
// struct BackupInfo {
//     udid: String,
//     source: String,
//     device_name: String,
//     display_name: String,
//     last_backup: String,
//     version: String,
//     encrypted: bool,
//     path: String,
//     status: String,
// }

#[allow(non_snake_case)]
#[derive(QObject, Default, QtThreading)]
pub struct BackupManager {
    base: qt_base_class!(trait QObject),
    state: qt_property!(QVariantMap; NOTIFY state_changed),
    state_changed: qt_signal!(),
    busy: qt_property!(bool; NOTIFY busy_changed),
    busy_changed: qt_signal!(),
    backup_model: qt_property!(RefCell<ListModel<BackupListItem>>; NOTIFY backup_model_changed),
    backup_model_changed: qt_signal!(),
    progressUpdate: qt_signal!(udid: QString, progress: f64),
    operationFinished: qt_signal!(
        operation: QString,
        udid: QString,
        success: bool,
        errorCode: i32,
        errorString: QString
    ),
    backupInfoReady: qt_signal!(udid: QString, success: bool, res: QString),
    backupInfoReadyWithoutDevice: qt_signal!(udid: QString, success: bool, res: QString),
    fileReceived: qt_signal!(udid: QString, path: QString),
    backupPasscodeChanged: qt_signal!(udid: QString, requested: bool),
    backupCancellationRequested: qt_signal!(udid: QString),
    backupEncryptionStatusReady: qt_signal!(
        udid: QString,
        success: bool,
        enabled: bool,
        errorString: QString
    ),
    backupEncryptionFinished: qt_signal!(
        udid: QString,
        success: bool,
        enabled: bool,
        errorString: QString
    ),
    init: qt_method!(fn(&mut self, backup_root_path: QString)),
    // refresh :
    does_backup_exist_for_udid: qt_method!(fn(&self, udid: QString) -> bool),
    get_backup_info: qt_method!(fn(&self, udid: QString, root: QString)),
    // refresh_backups: qt_method!(fn(&mut self, root: QString)),
    start_backup: qt_method!(fn(&mut self, root: QString, udid: QString, force_full: bool)),
    // udid is the connected device to restore onto, source_udid is the backup
    // folder to restore from (may belong to a different device)
    start_restore: qt_method!(
        fn(
            &mut self,
            root: QString,
            udid: QString,
            source_udid: QString,
            password: QString,
            reboot: bool,
            copy: bool,
            preserve_settings: bool,
            system_files: bool,
            remove_items_not_restored: bool,
        )
    ),
    erase_device: qt_method!(fn(&mut self, root: QString, udid: QString)),
    get_backup_info_without_device: qt_method!(fn(&self, udid: QString, root: QString)),
    cancel_operation: qt_method!(fn(&mut self, udid: String) -> bool),
    has_active_tasks: qt_method!(fn(&self) -> bool),
    cancel_all_operations: qt_method!(fn(&mut self) -> bool),
    get_backup_metadata: qt_method!(fn(&self, udid: QString, root: QString) -> QVariantMap),
    get_backup_encryption_status: qt_method!(fn(&self, udid: QString)),
    change_backup_password: qt_method!(
        fn(&mut self, root: QString, udid: QString, old_password: QString, new_password: QString)
    ),
    tasks: HashMap<String, JoinHandle<()>>,
}

impl BackupManager {
    pub fn new_with_state() -> Self {
        let mut state = QVariantMap::default();
        qvariantmap_insert!(state, "success", false);
        qvariantmap_insert!(state, "loading", true);

        let mut manager = Self::default();
        manager.state = state;
        manager
    }

    // A very bad to verify but it'll do it for the most part
    fn is_backup_dir(path: &PathBuf) -> bool {
        let manifest_db = path.join("Manifest.db");
        let manifest_plist = path.join("Manifest.plist");
        let status_plist = path.join("Status.plist");
        path.is_dir() && manifest_db.exists() && manifest_plist.exists() && status_plist.exists()
    }

    fn set_loading(&mut self) {
        let mut state = QVariantMap::default();
        qvariantmap_insert!(state, "success", false);
        qvariantmap_insert!(state, "loading", true);
        self.state = state;
        self.state_changed();
    }

    // fn update_row(){}

    fn init(&mut self, backup_root_path: QString) {
        self.set_loading();
        let mut state = QVariantMap::default();
        let path = PathBuf::from(backup_root_path);

        let existing_backups: Vec<BackupListItem> = match std::fs::read_dir(&path) {
            Ok(entries) => {
                let entries = entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| Self::is_backup_dir(p))
                    .map(|p| {
                        let mut item = BackupListItem::default();
                        // file_name is actually dir name here
                        item.udid = QString::from(p.file_name().unwrap().display().to_string());
                        if let Some((device_name, version, encrypted)) =
                            read_backup_list_details(&p)
                        {
                            item.device_name = QString::from(device_name);
                            item.version = QString::from(version);
                            item.encrypted = encrypted;
                        }
                        item
                    })
                    .collect();

                qvariantmap_insert!(state, "success", true);
                qvariantmap_insert!(state, "loading", false);

                entries
            }
            Err(_) => {
                qvariantmap_insert!(state, "success", false);
                qvariantmap_insert!(state, "loading", false);

                Vec::new()
            }
        };

        self.backup_model.borrow_mut().reset_data(existing_backups);
        self.state = state;
        self.state_changed();
        self.backup_model_changed();
    }

    // fn refresh(&self) {

    // }
    // TODO: find an O(1) solution
    fn does_backup_exist_for_udid(&self, udid: QString) -> bool {
        let mut found = false;
        for item in self.backup_model.borrow().iter() {
            if item.udid == udid {
                found = true;
                break;
            }
        }
        found
    }

    fn get_backup_info(&self, udid: QString, root: QString) {
        let q_thread = self.qt_thread().clone();
        RUNTIME.spawn(async move {
            let udid = udid.to_string();
            let res = get_info_for_backup(q_thread.clone(), udid.clone(), root.to_string()).await;
            q_thread.queue(move |manager| match res {
                Ok(json) => {
                    manager.backupInfoReady(QString::from(udid), true, QString::from(json));
                }
                Err(err) => {
                    error!("failed to get backup info for {udid}: {err}");
                    manager.backupInfoReady(
                        QString::from(udid),
                        false,
                        QString::from(err.to_string()),
                    );
                }
            });
        });
    }

    // this one doesn't require device to be connected
    fn get_backup_info_without_device(&self, udid: QString, root: QString) {
        let q_thread = self.qt_thread().clone();
        RUNTIME.spawn(async move {
            get_info_for_backup_without_device(q_thread, udid.to_string(), root.to_string()).await;
        });
    }

    fn get_backup_encryption_status(&self, udid: QString) {
        let q_thread = self.qt_thread();
        let udid = udid.to_string();
        RUNTIME.spawn(async move {
            let result = query_backup_encryption(&udid).await;
            q_thread.queue(move |manager| match result {
                Ok(enabled) => manager.backupEncryptionStatusReady(
                    QString::from(udid),
                    true,
                    enabled,
                    QString::default(),
                ),
                Err(err) => {
                    warn!("failed to query backup encryption for {udid}: {err}");
                    manager.backupEncryptionStatusReady(
                        QString::from(udid),
                        false,
                        false,
                        QString::from(err.to_string()),
                    );
                }
            });
        });
    }

    fn change_backup_password(
        &mut self,
        root: QString,
        udid: QString,
        old_password: QString,
        new_password: QString,
    ) {
        let udid = udid.to_string();
        if udid.is_empty() || new_password.is_empty() {
            self.backupEncryptionFinished(
                QString::from(udid),
                false,
                false,
                QString::from("A new backup password is required."),
            );
            return;
        }
        if self.tasks.contains_key(&udid) {
            self.backupEncryptionFinished(
                QString::from(udid),
                false,
                false,
                QString::from("Another backup operation is already running for this device."),
            );
            return;
        }

        let root = root.to_string();
        let old_password = old_password.to_string();
        let new_password = new_password.to_string();
        let task_udid = udid.clone();
        let q_thread = self.qt_thread();
        let task = RUNTIME.spawn(async move {
            let result = run_change_backup_password(
                q_thread.clone(),
                &udid,
                &root,
                &old_password,
                &new_password,
            )
            .await;

            q_thread.queue(move |manager| {
                manager.tasks.remove(&udid);
                manager.update_busy_from_tasks();
                manager.backupPasscodeChanged(QString::from(udid.clone()), false);
                match result {
                    Ok(enabled) => manager.backupEncryptionFinished(
                        QString::from(udid),
                        true,
                        enabled,
                        QString::default(),
                    ),
                    Err(err) => {
                        error!("failed to change backup encryption for {udid}: {err}");
                        manager.backupEncryptionFinished(
                            QString::from(udid),
                            false,
                            false,
                            QString::from(err.to_string()),
                        );
                    }
                }
            });
        });
        self.tasks.insert(task_udid, task);
        self.busy = true;
        self.busy_changed();
    }

    fn start_backup(&mut self, root: QString, udid: QString, force_full: bool) {
        let udid = udid.to_string();
        let root = root.to_string();
        let q_thread = self.qt_thread();
        let udid_for_task = udid.clone();
        let task = RUNTIME.spawn(async move {
            let finish_udid = udid.clone();
            let result = run_backup(q_thread.clone(), udid, root.clone(), force_full).await;
            let (success, error_code, error_string) = match result {
                Ok(()) => (true, 0, String::new()),
                Err(err) => {
                    error!("backup failed for {finish_udid}: {err}");
                    let (code, message) = operation_error_details(&err);
                    (false, code, message)
                }
            };
            q_thread.queue(move |manager| {
                manager.tasks.remove(&finish_udid);
                manager.update_busy_from_tasks();
                manager.operationFinished(
                    QString::from("backup"),
                    QString::from(finish_udid),
                    success,
                    error_code,
                    QString::from(error_string),
                );
            });
        });
        if let Some(prev_task) = self.tasks.insert(udid_for_task, task) {
            // cancel if there was any prev task
            // running for the same udid
            prev_task.abort();
        }
        self.busy = true;
        self.busy_changed();
    }

    fn cancel_operation(&mut self, udid: String) -> bool {
        if let Some(task) = self.tasks.remove(&udid) {
            task.abort();
            self.update_busy_from_tasks();
            true
        } else {
            false
        }
    }

    fn has_active_tasks(&self) -> bool {
        let val = self.tasks.values().any(|task| !task.is_finished());
        println!("Active task {}", val);
        val
    }

    fn cancel_all_operations(&mut self) -> bool {
        let task_count = self.tasks.len();
        for (_, task) in self.tasks.drain() {
            task.abort();
        }
        self.update_busy_from_tasks();

        if task_count > 0 {
            info!("cancelled {task_count} backup manager task(s)");
        }
        task_count > 0
    }

    fn update_busy_from_tasks(&mut self) {
        let busy = self.has_active_tasks();
        if self.busy == busy {
            return;
        }

        self.busy = busy;
        self.busy_changed();
    }

    fn start_restore(
        &mut self,
        root: QString,
        udid: QString,
        source_udid: QString,
        password: QString,
        reboot: bool,
        copy: bool,
        preserve_settings: bool,
        system_files: bool,
        remove_items_not_restored: bool,
    ) {
        let udid = udid.to_string();
        let udid_for_task = udid.clone();
        let source_udid = if source_udid.is_empty() {
            udid.clone()
        } else {
            source_udid.to_string()
        };
        let root = root.to_string();
        let password = password.to_string();
        let q_thread = self.qt_thread();
        debug!(
            "Starting restore of backup {source_udid} onto device {udid} with password: {}",
            if password.is_empty() {
                "(none)"
            } else {
                "(provided)"
            }
        );
        let task = RUNTIME.spawn(async move {
            let finish_udid = udid.clone();
            let result = run_restore(
                q_thread.clone(),
                udid,
                source_udid,
                root.clone(),
                password,
                reboot,
                copy,
                preserve_settings,
                system_files,
                remove_items_not_restored,
            )
            .await;

            let (success, error_code, error_string) = match result {
                Ok(()) => (true, 0, String::new()),
                Err(err) => {
                    error!("restore failed for {finish_udid}: {err}");
                    let (code, message) = operation_error_details(&err);
                    (false, code, message)
                }
            };
            q_thread.queue(move |manager| {
                manager.tasks.remove(&finish_udid);
                manager.update_busy_from_tasks();
                manager.operationFinished(
                    QString::from("restore"),
                    QString::from(finish_udid),
                    success,
                    error_code,
                    QString::from(error_string),
                );
            });
        });

        if let Some(prev_task) = self.tasks.insert(udid_for_task, task) {
            prev_task.abort();
        }
        self.busy = true;
        self.busy_changed();
    }

    fn get_backup_metadata(&self, udid: QString, root: QString) -> QVariantMap {
        let mut result_metadata = QVariantMap::default();

        match read_backup_metadata(udid, root) {
            Ok((was_passcode_set, is_encrypted)) => {
                qvariantmap_insert!(result_metadata, "WasPasscodeSet", was_passcode_set);
                qvariantmap_insert!(result_metadata, "IsEncrypted", is_encrypted);
                qvariantmap_insert!(result_metadata, "success", true);
            }
            Err(err) => {
                error!("failed to get backup metadata: {err}");
                qvariantmap_insert!(result_metadata, "success", false);
            }
        }
        result_metadata
    }

    fn erase_device(&mut self, root: QString, udid: QString) {
        let udid = udid.to_string();
        let udid_for_task = udid.clone();
        let root = root.to_string();
        let q_thread = self.qt_thread();

        let task = RUNTIME.spawn(async move {
            let finish_udid = udid.clone();
            let result = run_erase(udid, root).await;
            let real_success = result.is_ok();
            /*
                device immediately disconnects after erase command
                we will just assume success for now
                23:50:51 [ERROR] failed to erase device c1daf1e6d74de3862141f00c0a*****: device socket io failed
            */
            let success = true;
            debug!("Assuming erase success for {finish_udid}: {real_success}");

            q_thread.queue(move |manager| {
                manager.tasks.remove(&finish_udid);
                manager.update_busy_from_tasks();
                manager.operationFinished(
                    QString::from("erase"),
                    QString::from(finish_udid),
                    success,
                    0,
                    QString::default(),
                );
            });
        });

        if let Some(previous_task) = self.tasks.insert(udid_for_task, task) {
            previous_task.abort();
        }
        self.busy = true;
        self.busy_changed();
    }
}

// impl Drop for BackupManager {
//     fn drop(&mut self) {
//         if let Some(task) = self.active_task.take() {
//             task.abort();
//         }
//     }
// }

// impl From<BackupInfo> for BackupListItem {
//     fn from(info: BackupInfo) -> Self {
//         Self {
//             udid: QString::from(info.udid),
//             source: QString::from(info.source),
//             device_name: QString::from(info.device_name),
//             display_name: QString::from(info.display_name),
//             last_backup: QString::from(info.last_backup),
//             version: QString::from(info.version),
//             encrypted: info.encrypted,
//             path: QString::from(info.path),
//             status: QString::from(info.status),
//         }
//     }
// }

// (device name, iOS version, encrypted) from Manifest.plist, used to label backups
fn read_backup_list_details(backup_dir: &Path) -> Option<(String, String, bool)> {
    let manifest = plist::Value::from_file(backup_dir.join("Manifest.plist")).ok()?;
    let manifest = manifest.as_dictionary()?;
    let lockdown = manifest
        .get("Lockdown")
        .and_then(plist::Value::as_dictionary);
    let lockdown_string = |key: &str| {
        lockdown
            .and_then(|dict| dict.get(key))
            .and_then(plist::Value::as_string)
            .unwrap_or_default()
            .to_owned()
    };
    let encrypted = manifest
        .get("IsEncrypted")
        .and_then(plist::Value::as_boolean)
        .unwrap_or(false);
    Some((
        lockdown_string("DeviceName"),
        lockdown_string("ProductVersion"),
        encrypted,
    ))
}

fn read_backup_metadata(udid: QString, root: QString) -> anyhow::Result<(bool, bool)> {
    let path = PathBuf::from(root.to_string())
        .join(udid.to_string())
        .join("Manifest.plist");

    let metadata = plist::Value::from_file(&path)?;
    let metadata = metadata
        .as_dictionary()
        .ok_or_else(|| anyhow::anyhow!("Manifest.plist is not a dictionary"))?;

    let was_passcode_set = metadata
        .get("WasPasscodeSet")
        .and_then(|v| v.as_boolean())
        .unwrap_or(false);
    let is_encrypted = metadata
        .get("IsEncrypted")
        .and_then(|v| v.as_boolean())
        .ok_or_else(|| anyhow::anyhow!("IsEncrypted is not a boolean"))?;

    Ok((was_passcode_set, is_encrypted))
}

#[allow(non_camel_case_types)]
#[derive(Clone)]
struct iDescriptorBackupDelegate {
    fs: FsBackupDelegate,
    qt_thread: QtThread<BackupManager>,
    #[allow(dead_code)]
    operation: &'static str,
    #[allow(dead_code)]
    root: String,
    udid: QString,
    // start_time: Arc<Mutex<Option<Instant>>>,
}

impl iDescriptorBackupDelegate {
    fn new(
        qt_thread: QtThread<BackupManager>,
        operation: &'static str,
        root: String,
        udid: QString,
    ) -> Self {
        Self {
            fs: FsBackupDelegate,
            qt_thread,
            operation,
            root,
            udid,
            // start_time: Arc::new(Mutex::new(None)),
        }
    }
}

impl BackupDelegate for iDescriptorBackupDelegate {
    fn get_free_disk_space(&self, path: &Path) -> u64 {
        self.fs.get_free_disk_space(path)
    }

    fn open_file_read<'a>(
        &'a self,
        path: &'a Path,
    ) -> Pin<Box<dyn Future<Output = Result<Box<dyn Read + Send>, IdeviceError>> + Send + 'a>> {
        self.fs.open_file_read(path)
    }

    fn create_file_write<'a>(
        &'a self,
        path: &'a Path,
    ) -> Pin<Box<dyn Future<Output = Result<Box<dyn Write + Send>, IdeviceError>> + Send + 'a>>
    {
        self.fs.create_file_write(path)
    }

    fn create_dir_all<'a>(
        &'a self,
        path: &'a Path,
    ) -> Pin<Box<dyn Future<Output = Result<(), IdeviceError>> + Send + 'a>> {
        self.fs.create_dir_all(path)
    }

    fn remove<'a>(
        &'a self,
        path: &'a Path,
    ) -> Pin<Box<dyn Future<Output = Result<(), IdeviceError>> + Send + 'a>> {
        self.fs.remove(path)
    }

    fn rename<'a>(
        &'a self,
        from: &'a Path,
        to: &'a Path,
    ) -> Pin<Box<dyn Future<Output = Result<(), IdeviceError>> + Send + 'a>> {
        self.fs.rename(from, to)
    }

    fn copy<'a>(
        &'a self,
        src: &'a Path,
        dst: &'a Path,
    ) -> Pin<Box<dyn Future<Output = Result<(), IdeviceError>> + Send + 'a>> {
        self.fs.copy(src, dst)
    }

    fn exists<'a>(&'a self, path: &'a Path) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>> {
        self.fs.exists(path)
    }

    fn is_dir<'a>(&'a self, path: &'a Path) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>> {
        self.fs.is_dir(path)
    }

    fn list_dir<'a>(
        &'a self,
        path: &'a Path,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<DirEntryInfo>, IdeviceError>> + Send + 'a>> {
        self.fs.list_dir(path)
    }

    fn on_file_received(&self, path: &str, file_count: u32) {
        debug!("backup manager received file {file_count}: {path}");

        let udid = self.udid.clone();
        let path = QString::from(path.to_string());
        self.qt_thread.queue(move |manager| {
            manager.fileReceived(udid, path);
        });
    }

    fn on_progress(&self, bytes_done: u64, bytes_total: u64, overall_progress: f64) {
        // let _elapsed = {
        //     let mut start = self.start_time.lock().unwrap();
        //     let start = start.get_or_insert_with(Instant::now);
        //     start.elapsed()
        // };

        let progress = if bytes_total > 0 {
            bytes_done as f64 / bytes_total as f64
        } else if overall_progress >= 0.0 {
            overall_progress / 100.0
        } else {
            0.0
        }
        .clamp(0.0, 1.0);

        // FIXME: It's way too inaccurate
        // let time_remaining = if progress > 0.0 && progress < 1.0 {
        //     let total = Duration::from_secs_f64(elapsed.as_secs_f64() / progress);
        //     Self::translated_time_remaining(total.saturating_sub(elapsed).as_secs())
        // } else {
        //     QString::default()
        // };

        let udid = self.udid.clone();
        self.qt_thread.queue(move |manager| {
            debug!("Progress: {}", progress);
            manager.progressUpdate(udid, progress);
        });
    }
}

async fn get_info_for_backup(
    qt_thread: QtThread<BackupManager>,
    udid: String,
    root: String,
) -> anyhow::Result<String> {
    println!("udid:{}, root:{}", udid, root);
    let device = crate::device_ctx::get_device(udid.clone()).await?;
    let provider_guard = device.provider.lock().await;
    let provider_ref: &dyn IdeviceProvider = provider_guard.as_ref();
    let mut client = MobileBackup2Client::connect(provider_ref).await?;
    drop(provider_guard);

    let delegate = iDescriptorBackupDelegate::new(qt_thread, "backup", root.clone(), udid.into());
    let dict = client
        .info_from_path(&PathBuf::from(root), None, &delegate)
        .await?;
    let value_dict = plist::Value::from(dict);
    let json = serde_json::to_string(&value_dict)?;

    Ok(json)
}

async fn get_info_for_backup_without_device(
    qt_thread: QtThread<BackupManager>,
    udid: String,
    root: String,
) {
    let signal_udid = udid.clone();
    let res: anyhow::Result<String> = async {
        let backup_path = PathBuf::from(root).join(&udid);
        let manifest_path = backup_path.join("Manifest.plist");
        let size_path = backup_path.clone();

        let bytes = tokio::fs::read(manifest_path).await?;
        let plist: plist::Value = plist::from_bytes(&bytes)?;
        let (folder_size, folder_files) = calculate_folder_size(size_path).await?;
        let mut value = serde_json::to_value(&plist)?;

        if let serde_json::Value::Object(ref mut obj) = value {
            obj.insert("FolderSize".into(), serde_json::json!(folder_size));
            obj.insert("FolderFiles".into(), serde_json::json!(folder_files));
        }

        Ok(serde_json::to_string(&value)?)
    }
    .await;

    qt_thread.queue(move |manager| match res {
        Ok(json) => {
            manager.backupInfoReadyWithoutDevice(
                QString::from(signal_udid),
                true,
                QString::from(json),
            );
        }
        Err(err) => {
            error!("failed to get backup info without device for {signal_udid}: {err}");
            manager.backupInfoReadyWithoutDevice(
                QString::from(signal_udid),
                false,
                QString::from(err.to_string()),
            );
        }
    });
}

async fn calculate_folder_size(path: PathBuf) -> anyhow::Result<(u64, u64)> {
    tokio::task::spawn_blocking(move || {
        let mut total_size = 0_u64;
        let mut total_files = 0_u64;
        let mut pending = vec![path];

        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                let metadata = entry.metadata()?;

                if metadata.is_dir() {
                    pending.push(entry.path());
                } else if metadata.is_file() {
                    total_size = total_size.saturating_add(metadata.len());
                    total_files = total_files.saturating_add(1);
                }
            }
        }

        Ok((total_size, total_files))
    })
    .await?
}

async fn query_backup_encryption(udid: &str) -> anyhow::Result<bool> {
    let device = crate::device_ctx::get_device(udid.to_owned()).await?;
    let mut lockdown = device.lockdown.lock().await;
    let result = lockdown
        .get_value(Some("WillEncrypt"), Some("com.apple.mobile.backup"))
        .await;
    backup_encryption_from_lockdown_result(result)
}

fn backup_encryption_from_lockdown_result(
    result: Result<plist::Value, IdeviceError>,
) -> anyhow::Result<bool> {
    match result {
        Ok(plist::Value::Boolean(enabled)) => Ok(enabled),
        Err(IdeviceError::MissingValue) => Ok(false),
        Ok(value) => anyhow::bail!("WillEncrypt returned an unexpected value: {value:?}"),
        Err(err) => Err(err.into()),
    }
}

async fn run_change_backup_password(
    qt_thread: QtThread<BackupManager>,
    udid: &str,
    root: &str,
    old_password: &str,
    new_password: &str,
) -> anyhow::Result<bool> {
    let was_encrypted = query_backup_encryption(udid).await?;
    if was_encrypted && old_password.is_empty() {
        anyhow::bail!("The current backup password is required.");
    }

    tokio::fs::create_dir_all(root).await?;
    let device = crate::device_ctx::get_device(udid.to_owned()).await?;
    let provider_guard = device.provider.lock().await;
    let provider_ref: &dyn IdeviceProvider = provider_guard.as_ref();
    let mut client = MobileBackup2Client::connect(provider_ref).await?;
    let mut notification_client = match NotificationProxyClient::connect(provider_ref).await {
        Ok(mut notifications) => match notifications
            .observe_notifications(MOBILEBACKUP2_NOTIFICATIONS)
            .await
        {
            Ok(()) => Some(notifications),
            Err(err) => {
                warn!("unable to observe backup encryption notifications for {udid}: {err}");
                None
            }
        },
        Err(err) => {
            warn!("unable to connect to notification proxy for {udid}: {err}");
            None
        }
    };
    drop(provider_guard);

    let delegate = iDescriptorBackupDelegate::new(
        qt_thread.clone(),
        "change-password",
        root.to_owned(),
        QString::from(udid),
    );
    let old_password = was_encrypted.then_some(old_password);
    info!(
        "{} encrypted backups for {udid}",
        if was_encrypted {
            "changing password for"
        } else {
            "enabling"
        }
    );

    let response = if let Some(notifications) = notification_client.as_mut() {
        let operation = client.change_password_from_path(
            Path::new(root),
            old_password,
            Some(new_password),
            &delegate,
        );
        tokio::pin!(operation);
        let mut monitor_notifications = true;

        loop {
            tokio::select! {
                result = &mut operation => break result?,
                notification = notifications.receive_notification(), if monitor_notifications => {
                    match notification {
                        Ok(name) => match MobileBackup2Notification::from_name(&name) {
                            Some(MobileBackup2Notification::PasscodeRequested) => {
                                let signal_udid = QString::from(udid);
                                qt_thread.queue(move |manager| {
                                    manager.backupPasscodeChanged(signal_udid, true);
                                });
                            }
                            Some(MobileBackup2Notification::PasscodeRequestDismissed) => {
                                let signal_udid = QString::from(udid);
                                qt_thread.queue(move |manager| {
                                    manager.backupPasscodeChanged(signal_udid, false);
                                });
                            }
                            Some(MobileBackup2Notification::BackupDomainChanged) => {
                                debug!("backup encryption domain changed for {udid}");
                            }
                            Some(MobileBackup2Notification::CancelRequested) => {
                                return Err(BackupCancelledError.into());
                            }
                            None => {}
                        },
                        Err(err) => {
                            warn!("backup encryption notification monitoring stopped for {udid}: {err}");
                            monitor_notifications = false;
                        }
                    }
                }
            }
        }
    } else {
        client
            .change_password_from_path(Path::new(root), old_password, Some(new_password), &delegate)
            .await?
    };

    check_response(response)?;
    let _ = client.disconnect().await;
    for attempt in 0..5 {
        if query_backup_encryption(udid).await? {
            info!("encrypted backups are enabled for {udid}");
            return Ok(true);
        }
        if attempt < 4 {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    }
    anyhow::bail!("The device did not enable encrypted backups.");
}

async fn verify_backup_snapshot_finished(root: &str, udid: &str) -> anyhow::Result<()> {
    let status_path = PathBuf::from(root).join(udid).join("Status.plist");
    tokio::task::spawn_blocking(move || {
        let status = plist::Value::from_file(&status_path)?;
        let snapshot_state = status
            .as_dictionary()
            .and_then(|dict| dict.get("SnapshotState"))
            .and_then(plist::Value::as_string)
            .ok_or_else(|| anyhow::anyhow!("Status.plist is missing SnapshotState"))?;
        if snapshot_state != "finished" {
            anyhow::bail!("backup snapshot state is {snapshot_state:?}, expected \"finished\"");
        }
        Ok(())
    })
    .await?
}

const ITUNES_SYNC_LOCK_PATH: &str = "/com.apple.itunes.lock_sync";
const RESTORE_APPLICATIONS_DIR: &str = "/iTunesRestore";
const RESTORE_APPLICATIONS_PATH: &str = "/iTunesRestore/RestoreApplications.plist";

struct BackupSyncSession {
    notifications: NotificationProxyClient,
    lock_file: OwnedFileDescriptor,
}

impl BackupSyncSession {
    async fn begin(provider: &dyn IdeviceProvider) -> anyhow::Result<Self> {
        let mut notifications = NotificationProxyClient::connect(provider).await?;
        notifications.post_notification(SYNC_WILL_START).await?;

        let afc = AfcClient::connect(provider).await?;
        let mut lock_file = afc
            .open_owned(ITUNES_SYNC_LOCK_PATH, AfcFopenMode::Rw)
            .await?;
        notifications.post_notification(SYNC_LOCK_REQUEST).await?;

        for attempt in 0..50 {
            match lock_file.lock_exclusive().await {
                Ok(()) => {
                    notifications.post_notification(SYNC_DID_START).await?;
                    return Ok(Self {
                        notifications,
                        lock_file,
                    });
                }
                Err(IdeviceError::Afc(AfcError::OpWouldBlock)) if attempt < 49 => {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
                Err(err) => return Err(err.into()),
            }
        }

        anyhow::bail!("Timed out waiting for the iTunes synchronization lock")
    }

    async fn finish(mut self) -> anyhow::Result<()> {
        let unlock_result = self.lock_file.unlock().await;
        let close_result = self.lock_file.close().await;
        let notification_result = self.notifications.post_notification(SYNC_DID_FINISH).await;

        unlock_result?;
        close_result?;
        notification_result?;
        Ok(())
    }
}

async fn read_afc_file(afc: &mut AfcClient, path: &str) -> anyhow::Result<Vec<u8>> {
    let mut file = afc.open(path, AfcFopenMode::RdOnly).await?;
    let contents = file.read_entire().await;
    let close_result = file.close().await;
    let contents = contents?;
    close_result?;
    Ok(contents)
}

fn copy_plist_field(
    destination: &mut plist::Dictionary,
    source: &plist::Dictionary,
    source_key: &str,
    destination_key: &str,
) {
    if let Some(value) = source.get(source_key) {
        destination.insert(destination_key.to_owned(), value.clone());
    }
}

async fn create_backup_info_plist(
    provider: &dyn IdeviceProvider,
    udid: &str,
) -> anyhow::Result<plist::Value> {
    let pairing_file = provider.get_pairing_file().await?;
    let mut lockdown = LockdownClient::connect(provider).await?;
    lockdown.start_session(&pairing_file).await?;
    let device_values = lockdown
        .get_value(None, None)
        .await?
        .into_dictionary()
        .ok_or_else(|| anyhow::anyhow!("Lockdown device values were not a dictionary"))?;
    let itunes_settings = lockdown
        .get_value(None, Some("com.apple.iTunes"))
        .await
        .unwrap_or_else(|err| {
            warn!("unable to read iTunes settings for backup metadata: {err}");
            plist::Value::Dictionary(plist::Dictionary::new())
        });
    let minimum_itunes_version = lockdown
        .get_value(Some("MinITunesVersion"), Some("com.apple.mobile.iTunes"))
        .await
        .ok();

    let mut installed_applications = Vec::new();
    let mut applications = plist::Dictionary::new();
    let browse_options = plist!({
        "ApplicationType": "User",
        "ReturnAttributes": [
            "CFBundleIdentifier",
            "ApplicationSINF",
            "iTunesMetadata"
        ]
    });
    let app_entries = match InstallationProxyClient::connect(provider).await {
        Ok(mut installation_proxy) => installation_proxy
            .browse(Some(browse_options))
            .await
            .unwrap_or_else(|err| {
                warn!("unable to enumerate installed applications for Info.plist: {err}");
                Vec::new()
            }),
        Err(err) => {
            warn!("unable to connect to installation proxy for Info.plist: {err}");
            Vec::new()
        }
    };
    let mut springboard = match SpringBoardServicesClient::connect(provider).await {
        Ok(client) => Some(client),
        Err(err) => {
            warn!("unable to connect to SpringBoard services for backup icons: {err}");
            None
        }
    };

    for entry in app_entries {
        let Some(entry) = entry.into_dictionary() else {
            continue;
        };
        let Some(bundle_id) = entry
            .get("CFBundleIdentifier")
            .and_then(plist::Value::as_string)
            .map(str::to_owned)
        else {
            continue;
        };
        installed_applications.push(plist::Value::String(bundle_id.clone()));

        let (Some(sinf), Some(metadata)) =
            (entry.get("ApplicationSINF"), entry.get("iTunesMetadata"))
        else {
            continue;
        };
        let mut app = plist::Dictionary::new();
        app.insert("ApplicationSINF".into(), sinf.clone());
        app.insert("iTunesMetadata".into(), metadata.clone());
        if let Some(springboard) = springboard.as_mut()
            && let Ok(icon) = springboard.get_icon_pngdata(bundle_id.clone()).await
        {
            app.insert("PlaceholderIcon".into(), plist::Value::Data(icon));
        }
        applications.insert(bundle_id, plist::Value::Dictionary(app));
    }

    let mut info = plist::Dictionary::new();
    info.insert(
        "Applications".into(),
        plist::Value::Dictionary(applications),
    );
    copy_plist_field(&mut info, &device_values, "BuildVersion", "Build Version");
    copy_plist_field(&mut info, &device_values, "DeviceName", "Device Name");
    copy_plist_field(&mut info, &device_values, "DeviceName", "Display Name");
    copy_plist_field(
        &mut info,
        &device_values,
        "IntegratedCircuitCardIdentity",
        "ICCID",
    );
    copy_plist_field(
        &mut info,
        &device_values,
        "InternationalMobileEquipmentIdentity",
        "IMEI",
    );
    copy_plist_field(
        &mut info,
        &device_values,
        "MobileEquipmentIdentifier",
        "MEID",
    );
    copy_plist_field(&mut info, &device_values, "PhoneNumber", "Phone Number");
    copy_plist_field(&mut info, &device_values, "ProductType", "Product Type");
    copy_plist_field(
        &mut info,
        &device_values,
        "ProductVersion",
        "Product Version",
    );
    copy_plist_field(&mut info, &device_values, "SerialNumber", "Serial Number");
    info.insert(
        "GUID".into(),
        plist::Value::String(uuid::Uuid::new_v4().simple().to_string().to_uppercase()),
    );
    info.insert(
        "Installed Applications".into(),
        plist::Value::Array(installed_applications),
    );
    info.insert(
        "Last Backup Date".into(),
        plist::Value::Date(SystemTime::now().into()),
    );
    info.insert(
        "Target Identifier".into(),
        plist::Value::String(udid.to_owned()),
    );
    info.insert("Target Type".into(), plist::Value::String("Device".into()));
    info.insert(
        "Unique Identifier".into(),
        plist::Value::String(udid.to_uppercase()),
    );

    let mut afc = AfcClient::connect(provider).await?;
    if let Ok(contents) = read_afc_file(&mut afc, "/Books/iBooksData2.plist").await {
        info.insert("iBooks Data 2".into(), plist::Value::Data(contents));
    }

    let mut itunes_files = plist::Dictionary::new();
    for name in [
        "ApertureAlbumPrefs",
        "IC-Info.sidb",
        "IC-Info.sidv",
        "PhotosFolderAlbums",
        "PhotosFolderName",
        "PhotosFolderPrefs",
        "VoiceMemos.plist",
        "iPhotoAlbumPrefs",
        "iTunesApplicationIDs",
        "iTunesPrefs",
        "iTunesPrefs.plist",
    ] {
        let path = format!("/iTunes_Control/iTunes/{name}");
        if let Ok(contents) = read_afc_file(&mut afc, &path).await {
            itunes_files.insert(name.into(), plist::Value::Data(contents));
        }
    }
    info.insert(
        "iTunes Files".into(),
        plist::Value::Dictionary(itunes_files),
    );
    info.insert("iTunes Settings".into(), itunes_settings);
    info.insert(
        "iTunes Version".into(),
        minimum_itunes_version.unwrap_or_else(|| plist::Value::String("10.0.1".into())),
    );

    Ok(plist::Value::Dictionary(info))
}

async fn write_backup_info_plist(
    provider: &dyn IdeviceProvider,
    root: &str,
    udid: &str,
) -> anyhow::Result<()> {
    info!("generating iTunes-compatible Info.plist for backup {udid}");
    let info = create_backup_info_plist(provider, udid).await?;
    let mut bytes = Vec::new();
    plist::to_writer_xml(&mut bytes, &info)?;
    let backup_dir = PathBuf::from(root).join(udid);
    tokio::fs::create_dir_all(&backup_dir).await?;
    let temporary_path = backup_dir.join("Info.plist.tmp");
    let final_path = backup_dir.join("Info.plist");
    tokio::fs::write(&temporary_path, bytes).await?;
    if let Err(err) = tokio::fs::remove_file(&final_path).await
        && err.kind() != std::io::ErrorKind::NotFound
    {
        return Err(err.into());
    }
    tokio::fs::rename(&temporary_path, &final_path).await?;
    Ok(())
}

async fn validate_restore_backup(
    root: &str,
    udid: &str,
    password: &str,
) -> anyhow::Result<plist::Value> {
    info!("validating backup metadata before restoring {udid}");
    let backup_dir = PathBuf::from(root).join(udid);
    let info_path = backup_dir.join("Info.plist");
    let manifest_path = backup_dir.join("Manifest.plist");
    let status_path = backup_dir.join("Status.plist");
    let password_supplied = !password.is_empty();

    tokio::task::spawn_blocking(move || {
        let info = plist::Value::from_file(&info_path)
            .map_err(|err| anyhow::anyhow!("Invalid or missing Info.plist: {err}"))?;
        let manifest = plist::Value::from_file(&manifest_path)
            .map_err(|err| anyhow::anyhow!("Invalid or missing Manifest.plist: {err}"))?;
        let status = plist::Value::from_file(&status_path)
            .map_err(|err| anyhow::anyhow!("Invalid or missing Status.plist: {err}"))?;

        let snapshot_state = status
            .as_dictionary()
            .and_then(|dict| dict.get("SnapshotState"))
            .and_then(plist::Value::as_string);
        if snapshot_state != Some("finished") {
            anyhow::bail!("The selected backup does not contain a finished snapshot");
        }
        let encrypted = manifest
            .as_dictionary()
            .and_then(|dict| dict.get("IsEncrypted"))
            .and_then(plist::Value::as_boolean)
            .unwrap_or(false);
        if encrypted && !password_supplied {
            anyhow::bail!("A password is required to restore this encrypted backup");
        }
        Ok(info)
    })
    .await?
}

fn is_version_newer(
    candidate: &crate::device_ctx::iOSVersion,
    reference: &crate::device_ctx::iOSVersion,
) -> bool {
    (candidate.major, candidate.minor, candidate.patch)
        > (reference.major, reference.minor, reference.patch)
}

// iOS refuses to restore a backup made on a newer iOS version than the one
// installed on the target device, fail early with a clear message instead
fn ensure_backup_compatible_with_device(
    device: &crate::device_ctx::DeviceServices,
    info: &plist::Value,
) -> anyhow::Result<()> {
    let Some(backup_version) = info
        .as_dictionary()
        .and_then(|dict| dict.get("Product Version"))
        .and_then(plist::Value::as_string)
    else {
        warn!("backup Info.plist has no Product Version; skipping compatibility check");
        return Ok(());
    };

    let backup_version = crate::device_ctx::iOSVersion::from_str(backup_version);
    if is_version_newer(&backup_version, &device.ios_version) {
        anyhow::bail!(
            "This backup was made on iOS {} but the device is running iOS {}. Update the device to iOS {} or later before restoring this backup.",
            backup_version.raw_version_str,
            device.ios_version.raw_version_str,
            backup_version.raw_version_str
        );
    }
    Ok(())
}

async fn write_restore_applications(
    provider: &dyn IdeviceProvider,
    info: &plist::Value,
) -> anyhow::Result<()> {
    let Some(applications) = info
        .as_dictionary()
        .and_then(|dict| dict.get("Applications"))
    else {
        info!("Info.plist has no Applications entry; skipping app restore preparation");
        return Ok(());
    };

    let mut bytes = Vec::new();
    plist::to_writer_xml(&mut bytes, applications)?;
    info!("preparing RestoreApplications.plist");
    let mut afc = AfcClient::connect(provider).await?;
    if let Err(err) = afc.mk_dir(RESTORE_APPLICATIONS_DIR).await
        && !matches!(err, IdeviceError::Afc(AfcError::ObjectExists))
    {
        return Err(err.into());
    }
    let mut file = afc
        .open(RESTORE_APPLICATIONS_PATH, AfcFopenMode::WrOnly)
        .await?;
    let write_result = file.write_entire(&bytes).await;
    let close_result = file.close().await;
    write_result?;
    close_result?;
    Ok(())
}

async fn remove_restore_applications(provider: &dyn IdeviceProvider) {
    let Ok(mut afc) = AfcClient::connect(provider).await else {
        return;
    };
    let _ = afc.remove(RESTORE_APPLICATIONS_PATH).await;
    let _ = afc.remove(RESTORE_APPLICATIONS_DIR).await;
}

async fn run_backup(
    qt_thread: QtThread<BackupManager>,
    udid: String,
    root: String,
    force_full: bool,
) -> anyhow::Result<()> {
    let device = crate::device_ctx::get_device(udid.clone()).await?;
    let provider_guard = device.provider.lock().await;
    let provider_ref: &dyn IdeviceProvider = provider_guard.as_ref();
    let mut client = MobileBackup2Client::connect(provider_ref).await?;

    let mut notification_client = match NotificationProxyClient::connect(provider_ref).await {
        Ok(mut notification_client) => {
            match notification_client
                .observe_notifications(MOBILEBACKUP2_NOTIFICATIONS)
                .await
            {
                Ok(()) => Some(notification_client),
                Err(err) => {
                    warn!("unable to observe backup notifications for {udid}: {err}");
                    None
                }
            }
        }
        Err(err) => {
            warn!("unable to connect to notification proxy for backup {udid}: {err}");
            None
        }
    };
    write_backup_info_plist(provider_ref, &root, &udid).await?;
    let sync_session = BackupSyncSession::begin(provider_ref).await?;
    drop(provider_guard);

    let delegate = iDescriptorBackupDelegate::new(
        qt_thread.clone(),
        "backup",
        root.clone(),
        QString::from(udid.clone()),
    );
    let options = BackupOptions::new().with_force_full_backup(force_full);
    info!(
        "starting {} backup for {udid}",
        if force_full { "full" } else { "incremental" }
    );

    enum BackupWaitResult<T> {
        Finished(T),
        DeviceCancelled,
    }

    let backup_result = if let Some(notifications) = notification_client.as_mut() {
        let backup =
            client.backup_from_path_with_options(Path::new(&root), None, options, &delegate);
        tokio::pin!(backup);
        let mut monitor_notifications = true;

        loop {
            tokio::select! {
                result = &mut backup => break BackupWaitResult::Finished(result),
                notification = notifications.receive_notification(), if monitor_notifications => {
                    match notification {
                        Ok(name) => match MobileBackup2Notification::from_name(&name) {
                            Some(MobileBackup2Notification::PasscodeRequested) => {
                                let signal_udid = QString::from(udid.clone());
                                qt_thread.queue(move |manager| {
                                    manager.backupPasscodeChanged(signal_udid, true);
                                });
                            }
                            Some(MobileBackup2Notification::PasscodeRequestDismissed) => {
                                let signal_udid = QString::from(udid.clone());
                                qt_thread.queue(move |manager| {
                                    manager.backupPasscodeChanged(signal_udid, false);
                                });
                            }
                            Some(MobileBackup2Notification::CancelRequested) => {
                                let signal_udid = QString::from(udid.clone());
                                qt_thread.queue(move |manager| {
                                    manager.backupCancellationRequested(signal_udid);
                                });
                                break BackupWaitResult::DeviceCancelled;
                            }
                            Some(MobileBackup2Notification::BackupDomainChanged) => {
                                debug!("backup encryption domain changed for {udid}");
                            }
                            None => {}
                        },
                        Err(err) => {
                            warn!("backup notification monitoring stopped for {udid}: {err}");
                            monitor_notifications = false;
                        }
                    }
                }
            }
        }
    } else {
        BackupWaitResult::Finished(
            client
                .backup_from_path_with_options(Path::new(&root), None, options, &delegate)
                .await,
        )
    };

    let signal_udid = QString::from(udid.clone());
    qt_thread.queue(move |manager| {
        manager.backupPasscodeChanged(signal_udid, false);
    });

    let operation_result: anyhow::Result<()> = async {
        let response = match backup_result {
            BackupWaitResult::Finished(result) => result?,
            BackupWaitResult::DeviceCancelled => {
                return Err(BackupCancelledError.into());
            }
        };
        check_response(response)?;
        verify_backup_snapshot_finished(&root, &udid).await
    }
    .await;
    let _ = client.disconnect().await;
    let sync_result = sync_session.finish().await;
    operation_result?;
    sync_result?;
    info!("backup completed");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_restore(
    qt_thread: QtThread<BackupManager>,
    udid: String,
    source_udid: String,
    root: String,
    password: String,
    reboot: bool,
    copy: bool,
    preserve_settings: bool,
    system_files: bool,
    remove_items_not_restored: bool,
) -> anyhow::Result<()> {
    let info_plist = validate_restore_backup(&root, &source_udid, &password).await?;
    let device = crate::device_ctx::get_device(udid.clone()).await?;
    if source_udid != udid {
        info!("restoring backup from another device ({source_udid}) onto {udid}");
        ensure_backup_compatible_with_device(&device, &info_plist)?;
    }
    let provider_guard = device.provider.lock().await;
    let provider_ref: &dyn IdeviceProvider = provider_guard.as_ref();
    let mut client = MobileBackup2Client::connect(provider_ref).await?;
    let sync_session = BackupSyncSession::begin(provider_ref).await?;
    if let Err(err) = write_restore_applications(provider_ref, &info_plist).await {
        remove_restore_applications(provider_ref).await;
        let _ = sync_session.finish().await;
        return Err(err);
    }
    drop(provider_guard);

    let mut options = RestoreOptions::new()
        .with_reboot(reboot)
        .with_copy(copy)
        .with_preserve_settings(preserve_settings)
        .with_system_files(system_files)
        .with_remove_items_not_restored(remove_items_not_restored);
    if !password.is_empty() {
        options = options.with_password(password);
    }

    let delegate = iDescriptorBackupDelegate::new(
        qt_thread,
        "restore",
        root.clone(),
        QString::from(udid.clone()),
    );
    let operation_result: anyhow::Result<()> = async {
        let response = client
            .restore_from_path(
                Path::new(&root),
                Some(source_udid.as_str()),
                Some(options),
                &delegate,
            )
            .await?;
        check_response(response)
    }
    .await;
    let _ = client.disconnect().await;
    if operation_result.is_err() {
        let provider_guard = device.provider.lock().await;
        remove_restore_applications(provider_guard.as_ref()).await;
    }
    let sync_result = sync_session.finish().await;
    operation_result?;
    sync_result?;
    info!("restore completed");
    Ok(())
}

async fn run_erase(udid: String, root: String) -> anyhow::Result<()> {
    let device = crate::device_ctx::get_device(udid.clone()).await?;
    let provider_guard = device.provider.lock().await;
    let provider_ref: &dyn IdeviceProvider = provider_guard.as_ref();
    let mut client = MobileBackup2Client::connect(provider_ref).await?;
    drop(provider_guard);

    let delegate = FsBackupDelegate;
    client
        .erase_device_from_path(Path::new(&root), &delegate)
        .await?;
    let _ = client.disconnect().await;
    info!("erase command sent");
    Ok(())
}

#[derive(Debug)]
struct MobileBackupResponseError {
    code: i32,
    description: String,
}

impl fmt::Display for MobileBackupResponseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "ErrorCode {}: {}", self.code, self.description)
    }
}

impl StdError for MobileBackupResponseError {}

#[derive(Debug)]
struct BackupCancelledError;

impl fmt::Display for BackupCancelledError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("The backup was cancelled from the device.")
    }
}

impl StdError for BackupCancelledError {}

fn operation_error_details(error: &anyhow::Error) -> (i32, String) {
    let code = error
        .downcast_ref::<MobileBackupResponseError>()
        .map(|error| error.code)
        .or_else(|| error.downcast_ref::<BackupCancelledError>().map(|_| -2))
        .unwrap_or(-1);
    (code, error.to_string())
}

fn check_response(response: Option<plist::Dictionary>) -> anyhow::Result<()> {
    let response = response
        .ok_or_else(|| anyhow::anyhow!("MobileBackup2 disconnected without a final response"))?;
    let code = response
        .get("ErrorCode")
        .and_then(|value| {
            value.as_signed_integer().or_else(|| {
                value
                    .as_unsigned_integer()
                    .and_then(|code| i64::try_from(code).ok())
            })
        })
        .ok_or_else(|| anyhow::anyhow!("MobileBackup2 response is missing ErrorCode"))?;

    if code != 0 {
        let desc = response
            .get("ErrorDescription")
            .and_then(|v| v.as_string())
            .unwrap_or("Unknown error");
        return Err(MobileBackupResponseError {
            code: i32::try_from(code).unwrap_or(-1),
            description: desc.to_owned(),
        }
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_successful_mobilebackup_response() {
        let mut response = plist::Dictionary::new();
        response.insert("ErrorCode".into(), plist::Value::Integer(0_u64.into()));
        assert!(check_response(Some(response)).is_ok());
    }

    #[test]
    fn preserves_signed_mobilebackup_error_code() {
        let mut response = plist::Dictionary::new();
        response.insert("ErrorCode".into(), plist::Value::Integer((-7_i64).into()));
        response.insert(
            "ErrorDescription".into(),
            plist::Value::String("Incorrect password".into()),
        );

        let error = check_response(Some(response)).expect_err("response should fail");
        let response_error = error
            .downcast_ref::<MobileBackupResponseError>()
            .expect("typed MobileBackup2 error");
        assert_eq!(response_error.code, -7);
        assert_eq!(response_error.description, "Incorrect password");
    }

    #[test]
    fn detects_backup_from_newer_ios() {
        use crate::device_ctx::iOSVersion;
        let device = iOSVersion::from_str("17.4.1");
        assert!(is_version_newer(&iOSVersion::from_str("18.0"), &device));
        assert!(is_version_newer(&iOSVersion::from_str("17.5"), &device));
        assert!(!is_version_newer(&iOSVersion::from_str("17.4.1"), &device));
        assert!(!is_version_newer(&iOSVersion::from_str("16.7.2"), &device));
    }

    #[test]
    fn rejects_disconnect_without_final_response() {
        assert!(check_response(None).is_err());
    }

    #[test]
    fn restore_options_match_system_settings_cli_flow() {
        let options = RestoreOptions::new()
            .with_reboot(true)
            .with_copy(false)
            .with_preserve_settings(false)
            .with_system_files(true)
            .with_remove_items_not_restored(false)
            .with_password("secret")
            .to_plist();

        assert_eq!(
            options.get("RestoreSystemFiles"),
            Some(&plist::Value::Boolean(true))
        );
        assert_eq!(
            options.get("RestorePreserveSettings"),
            Some(&plist::Value::Boolean(false))
        );
        assert_eq!(
            options.get("RestoreDontCopyBackup"),
            Some(&plist::Value::Boolean(true))
        );
        assert!(!options.contains_key("RestoreShouldReboot"));
    }
}
