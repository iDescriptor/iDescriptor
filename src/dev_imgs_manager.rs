// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

use idevice::IdeviceService;
use idevice::mobile_image_mounter::ImageMounter;
use idevice::provider::IdeviceProvider;
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::list_model::ListModel;
use crate::{
    RUNTIME,
    qt_threading::{QtThread, QtThreading},
    qvariantmap_insert,
};
use futures::StreamExt;
use macros::QtThreading;
use qmetaobject::SimpleListItem;
use qmetaobject::prelude::*;
use qttypes::{QString, QVariantMap};
use reqwest::Client;
use std::cell::RefCell;
use tokio::io::AsyncWriteExt;
use tokio::task::JoinHandle;

#[derive(SimpleListItem, Default, Clone)]
struct DiskImageItem {
    pub version: QString,
    pub compatibility: QString,
    pub is_mounted: bool,
    pub is_downloaded: bool,
    pub progress: f64,
    pub is_downloading: bool,
}

#[allow(non_snake_case)]
#[derive(QObject, Default, QtThreading)]
pub struct DevImgsManager {
    base: qt_base_class!(trait QObject),
    image_list: qt_property!(QVariantMap; NOTIFY imageListChanged),
    mounted_image_info: qt_property!(QVariantMap; NOTIFY mountedImageInfoChanged),
    activate_downloads: std::sync::Mutex<HashMap<String, JoinHandle<()>>>,
    image_list_revision: u64,
    mounted_check_revision: u64,
    download_image: qt_method!(fn(version: QString, index: u32, dir: QString) -> bool),
    state: qt_property!(QString; NOTIFY stateChanged),
    fetch_image_list: qt_method!(fn(udid: QString, dir: QString)),
    refresh_image_list: qt_method!(fn(udid: QString, dir: QString)),
    handle_download_finished: qt_method!(fn(version: QString)),
    cancel_download: qt_method!(fn(version: QString) -> bool),
    get_locations_for_version: qt_method!(fn(version: QString, dir: QString) -> QVariantMap),
    get_best_compatible_version: qt_method!(fn(ios_major: u32, dir: QString) -> QVariantMap),
    get_item: qt_method!(fn(index: i32) -> QVariantMap),
    check_mounted_image: qt_method!(fn(udid: QString)),
    image_model: qt_property!(RefCell<ListModel<DiskImageItem>>; NOTIFY imageModelChanged),

    stateChanged: qt_signal!(),
    imageModelChanged: qt_signal!(),
    imageListChanged: qt_signal!(),
    mountedImageInfoChanged: qt_signal!(),

    downloadProgressForIndex: qt_signal!(index: u32, version: QString, progress: f64),
    imageDownloadFinished: qt_signal!(version: QString, index: u32, success: bool, error: QString),
    imageDownloadCancelled: qt_signal!(version: QString),
    imageListRefreshFinished: qt_signal!(udid: QString, refreshed: bool, success: bool, error: QString),
    mountedImageCheckFinished: qt_signal!(udid: QString, success: bool, isMounted: bool, isLocked: bool, error: QString),
}

impl DevImgsManager {
    pub fn parse_disk_dir(dir: String) -> HashMap<String, bool> {
        let mut res = HashMap::new();
        let path = Path::new(&dir);
        if !path.exists() {
            return res;
        }

        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if let Some(version) = path.file_name().and_then(|s| s.to_str()) {
                        let dmg_path = path.join("DeveloperDiskImage.dmg");
                        let sig_path = path.join("DeveloperDiskImage.dmg.signature");
                        // maybe assign false if one of the files is missing,
                        // but for now just ignore it, as redownloading is possible
                        if dmg_path.exists() && sig_path.exists() {
                            res.insert(version.to_string(), true);
                        }
                    }
                }
            }
        }

        res
    }

    // udid is need, so we can check if the device has
    // a developer disk image mounted
    pub fn fetch_image_list(&mut self, udid: QString, dir: QString) {
        self.load_image_list(udid, dir, false);
    }

    fn refresh_image_list(&mut self, udid: QString, dir: QString) {
        self.load_image_list(udid, dir, true);
    }

    fn load_image_list(&mut self, udid: QString, dir: QString, refresh_manifest: bool) {
        self.image_list_revision = self.image_list_revision.wrapping_add(1);
        let request_revision = self.image_list_revision;
        let qt_thread = self.qt_thread();
        let get_mounted_image = Self::get_mounted_image(udid.clone());

        RUNTIME.spawn(async move {
            let result: anyhow::Result<Vec<DiskImageItem>> = async {
                let data = if refresh_manifest {
                    crate::dev_imgs::refresh().await?
                } else {
                    crate::dev_imgs::get().await?
                };
                let parsed_disk_dir = DevImgsManager::parse_disk_dir(dir.to_string());
                debug!(
                    "Parsed developer disk image directory: {:?}",
                    parsed_disk_dir
                );
                debug!(
                    "Loading developer disk images for device {} in directory {}",
                    udid.to_string(),
                    dir.to_string()
                );

                let mounted_image = get_mounted_image.await;
                let dev = crate::device_ctx::get_device(udid.clone()).await;

                let (should_compare_signatures, mounted_sig) = match mounted_image {
                    Ok((success, _is_locked, data)) => (success && !data.is_empty(), data),
                    Err(e) => {
                        warn!(
                            "Error fetching mounted image info for device {}: {}",
                            udid.to_string(),
                            e
                        );
                        (false, Vec::new())
                    }
                };

                // (u32, u32) = (major, minor)
                let device_version: Option<(u32, u32)> = match dev {
                    Ok(dev) => Some((dev.ios_version.major, dev.ios_version.minor)),
                    Err(e) => {
                        warn!(
                            "Error fetching device info for device {}: {}",
                            udid.to_string(),
                            e
                        );
                        None
                    }
                };

                // compatibility priority: 0 = exact, 1 = same major, 2 = incompatible, 3 = no device
                let compat_priority = |item: &DiskImageItem| -> u8 {
                    match device_version {
                        None => 3,
                        Some((maj, min)) => {
                            let v = item.version.to_string();
                            let mut parts = v.splitn(2, '.');
                            let item_maj = parts
                                .next()
                                .and_then(|s| s.parse::<u32>().ok())
                                .unwrap_or(0);
                            let item_min = parts
                                .next()
                                .and_then(|s| s.parse::<u32>().ok())
                                .unwrap_or(0);

                            if item_maj == maj && item_min == min {
                                0
                            } else if item_maj == maj {
                                1
                            } else {
                                2
                            }
                        }
                    }
                };

                let compat_label = |priority: u8| -> &'static str {
                    match priority {
                        0 => "Compatible",
                        1 => "MaybeCompatible",
                        2 => "Incompatible",
                        _ => "No device",
                    }
                };

                let mut items: Vec<DiskImageItem> = data
                    .iter()
                    .map(|(version, _)| {
                        let is_mounted = if should_compare_signatures {
                            crate::utils::compare_signatures(version, &mounted_sig, dir.to_string())
                        } else {
                            false
                        };
                        DiskImageItem {
                            version: version.to_string().into(),
                            is_downloaded: parsed_disk_dir.contains_key(version),
                            is_mounted,
                            ..Default::default()
                        }
                    })
                    .collect();

                // sort: compatibility first, then version descending within each group
                items.sort_by(|a, b| {
                    let pa = compat_priority(a);
                    let pb = compat_priority(b);
                    pa.cmp(&pb).then_with(|| {
                        // descending version sort: compare b vs a
                        let parse = |s: &str| -> (u32, u32) {
                            let mut p = s.splitn(2, '.');
                            let maj = p.next().and_then(|x| x.parse().ok()).unwrap_or(0);
                            let min = p.next().and_then(|x| x.parse().ok()).unwrap_or(0);
                            (maj, min)
                        };
                        let va = parse(&a.version.to_string());
                        let vb = parse(&b.version.to_string());
                        vb.cmp(&va) // reversed for descending
                    })
                });

                // stamp compatibility label now that order is resolved
                for item in &mut items {
                    let p = compat_priority(item);
                    item.compatibility = compat_label(p).into();
                }

                Ok(items)
            }
            .await;

            qt_thread.queue(move |q| {
                if q.image_list_revision != request_revision {
                    debug!(
                        "Ignoring stale developer disk image result for device {}",
                        udid.to_string()
                    );
                    return;
                }

                match result {
                    Ok(mut items) => {
                        let active_downloads = q.activate_downloads.lock().unwrap();
                        for item in &mut items {
                            item.is_downloading =
                                active_downloads.contains_key(&item.version.to_string());
                        }
                        drop(active_downloads);
                        q.image_model.borrow_mut().reset_data(items);
                        q.imageListRefreshFinished(
                            udid,
                            refresh_manifest,
                            true,
                            QString::default(),
                        );
                    }
                    Err(err) => {
                        let message = err.to_string();
                        error!("Failed to load developer disk image list: {}", message);
                        q.imageListRefreshFinished(
                            udid,
                            refresh_manifest,
                            false,
                            QString::from(message),
                        );
                    }
                }
            });
        });
    }

    fn check_mounted_image(&mut self, udid: QString) {
        self.mounted_check_revision = self.mounted_check_revision.wrapping_add(1);
        let request_revision = self.mounted_check_revision;
        let qt_thread = self.qt_thread();
        RUNTIME.spawn(async move {
            let result = Self::get_mounted_image(udid.clone()).await;
            match result {
                Ok((is_mounted, is_locked, data)) => {
                    let mut map = QVariantMap::default();
                    qvariantmap_insert!(map, "is_mounted", is_mounted);
                    qvariantmap_insert!(map, "is_locked", is_locked);
                    qvariantmap_insert!(map, "signature", QString::from(hex::encode(data)));
                    qt_thread.queue(move |q| {
                        if q.mounted_check_revision != request_revision {
                            return;
                        }
                        q.mounted_image_info = map;
                        q.mountedImageInfoChanged();
                        q.mountedImageCheckFinished(
                            udid,
                            true,
                            is_mounted,
                            is_locked,
                            QString::default(),
                        );
                    });
                }
                Err(e) => {
                    let message = e.to_string();
                    error!(
                        "check_mounted_image: Failed to check mounted image for device {}: {}",
                        udid.to_string(),
                        message
                    );
                    qt_thread.queue(move |q| {
                        if q.mounted_check_revision != request_revision {
                            return;
                        }
                        let mut map = QVariantMap::default();
                        qvariantmap_insert!(map, "is_mounted", false);
                        qvariantmap_insert!(map, "is_locked", false);
                        qvariantmap_insert!(map, "signature", QString::default());
                        q.mounted_image_info = map;
                        q.mountedImageInfoChanged();
                        q.mountedImageCheckFinished(
                            udid,
                            false,
                            false,
                            false,
                            QString::from(message),
                        );
                    });
                }
            }
        });
    }

    fn get_item(&self, index: i32) -> QVariantMap {
        let mut map = QVariantMap::default();
        if index < 0 {
            return map;
        }

        let model = self.image_model.borrow();
        let Some(item) = model.values.get(index as usize) else {
            return map;
        };

        //TODO: we propably can clone the model item
        //instead of returning a new map
        qvariantmap_insert!(map, "version", item.version.clone());
        qvariantmap_insert!(map, "compatibility", item.compatibility.clone());
        qvariantmap_insert!(map, "is_mounted", item.is_mounted);
        qvariantmap_insert!(map, "is_downloaded", item.is_downloaded);
        qvariantmap_insert!(map, "progress", item.progress);
        qvariantmap_insert!(map, "is_downloading", item.is_downloading);
        map
    }

    async fn get_mounted_image(udid: QString) -> anyhow::Result<(bool, bool, Vec<u8>)> {
        let dev = crate::device_ctx::get_device(udid.clone()).await?;
        // TODO: we need to move dev_imgs_manager.rs(which was only iOS 16 and below)
        // into crate::developer_images (17 and up)
        if dev.ios_version.major >= 17 {
            return match crate::developer_images::mounted(
                &dev,
                &udid.to_string(),
                dev.ios_version.major,
                dev.ios_version.minor,
            )
            .await
            {
                Ok(mounted) => Ok((mounted, false, Vec::new())),
                Err(error)
                    if matches!(
                        error.downcast_ref::<idevice::IdeviceError>(),
                        Some(idevice::IdeviceError::DeviceLocked)
                    ) =>
                {
                    Ok((false, true, Vec::new()))
                }
                Err(error) => Err(error),
            };
        }
        let provider_guard = dev.clone().provider;

        let mut mounter = async {
            let provider = provider_guard.lock().await;
            let provider_ref: &dyn IdeviceProvider = provider.as_ref();
            ImageMounter::connect(provider_ref).await
        }
        .await?;

        match mounter.lookup_image("Developer").await {
            Ok(res) => {
                return Ok((true, false, res));
            }
            Err(idevice::IdeviceError::DeviceLocked) => {
                eprintln!(
                    "get_mounted_image: Failed to lookup mounted developer image for device {udid}: device locked"
                );
                return Ok((false, true, Vec::new()));
            }
            Err(idevice::IdeviceError::NotFound) => {
                eprintln!("get_mounted_image: No mounted developer image found for device {udid}");
                return Ok((false, false, Vec::new()));
            }
            Err(e) => {
                error!(
                    "get_mounted_image: Failed to lookup mounted developer image for device {udid}: {e}"
                );
                Err(e.into())
            }
        }
    }

    pub fn cancel_download(&mut self, version: QString) -> bool {
        let version_str = version.to_string();
        let mut downloads = self.activate_downloads.lock().unwrap();
        if let Some(handle) = downloads.remove(&version_str) {
            handle.abort();
            info!("Download for version {} has been cancelled.", version_str);
            drop(downloads);

            let index = {
                let model = self.image_model.borrow();
                model
                    .values
                    .iter()
                    .position(|item| item.version.to_string() == version_str)
            };
            if let Some(index) = index {
                self.image_model.borrow_mut().mutate(index, |item| {
                    item.is_downloading = false;
                    item.progress = 0.0;
                });
            }
            self.imageDownloadCancelled(version);
            true
        } else {
            warn!(
                "No active download found for version {} to cancel.",
                version_str
            );
            false
        }
    }

    // TODO: maybe we can do better here
    // Will be called from QML side when a download is finished
    // this avoids using Arc on activate_downloads
    fn handle_download_finished(&mut self, version: QString) {
        self.activate_downloads
            .lock()
            .unwrap()
            .remove(&version.to_string());
    }

    fn get_locations_for_version(&self, version: QString, dir: QString) -> QVariantMap {
        let mut map = QVariantMap::default();
        let version_str = version.to_string();
        let base_path = PathBuf::from(&dir.to_string());
        let version_path = base_path.join(&version_str);
        let dmg_path = version_path.join("DeveloperDiskImage.dmg");
        let sig_path = version_path.join("DeveloperDiskImage.dmg.signature");

        qvariantmap_insert!(
            map,
            "dmg",
            QString::from(dmg_path.to_str().unwrap_or_default())
        );
        qvariantmap_insert!(
            map,
            "sig",
            QString::from(sig_path.to_str().unwrap_or_default())
        );
        qvariantmap_insert!(map, "exists", dmg_path.exists() && sig_path.exists());

        map
    }

    fn get_best_compatible_version(&self, ios_major: u32, dir: QString) -> QVariantMap {
        let mut map = QVariantMap::default();
        qvariantmap_insert!(map, "version", QString::default());
        qvariantmap_insert!(map, "exists", false);
        qvariantmap_insert!(map, "found", false);

        let images = match crate::run_sync(async { crate::dev_imgs::get().await }) {
            Ok(images) => images,
            Err(e) => {
                warn!(
                    "get_best_compatible_version: failed to fetch image list for iOS {}: {}",
                    ios_major, e
                );
                return map;
            }
        };

        let mut versions: Vec<(u32, u32, String)> = images
            .keys()
            .filter_map(|version| {
                let mut parts = version.splitn(2, '.');
                let major = parts.next()?.parse::<u32>().ok()?;
                let minor = parts
                    .next()
                    .and_then(|part| part.parse::<u32>().ok())
                    .unwrap_or(0);

                if major == ios_major {
                    Some((major, minor, version.clone()))
                } else {
                    None
                }
            })
            .collect();

        versions.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.2.cmp(&a.2)));

        let Some((_, _, version)) = versions.into_iter().next() else {
            return map;
        };

        let version_path = PathBuf::from(&dir.to_string()).join(&version);
        let exists = version_path.join("DeveloperDiskImage.dmg").exists()
            && version_path
                .join("DeveloperDiskImage.dmg.signature")
                .exists();

        qvariantmap_insert!(map, "version", QString::from(version));
        qvariantmap_insert!(map, "exists", exists);
        qvariantmap_insert!(map, "found", true);

        map
    }

    fn download_image(&mut self, version: QString, index: u32, dir: QString) -> bool {
        debug!(
            "download_image called with version: {}",
            version.to_string()
        );
        let version_str = version.to_string();
        let mut downloads = self.activate_downloads.lock().unwrap();

        if downloads.contains_key(&version_str) {
            return false; // already in progress
        }

        // FIXME: dont block if possible
        let images = match crate::run_sync(async { crate::dev_imgs::get().await }) {
            Ok(images) => images,
            Err(err) => {
                error!(
                    "Failed to get the developer disk image manifest for {}: {}",
                    version_str, err
                );
                return false;
            }
        };
        let Some(zip_url) = images
            .get(&version_str)
            .and_then(|urls| urls.first())
            .cloned()
        else {
            error!(
                "No download URL is available for developer image {}",
                version_str
            );
            return false;
        };

        let target_dir = PathBuf::from(dir.to_string());
        if let Err(err) = fs::create_dir_all(&target_dir) {
            error!(
                "Failed to create developer disk image directory {}: {}",
                target_dir.display(),
                err
            );
            return false;
        }

        let model_index = {
            let model = self.image_model.borrow();
            model
                .values
                .iter()
                .position(|item| item.version.to_string() == version_str)
        };
        if let Some(model_index) = model_index {
            self.image_model.borrow_mut().mutate(model_index, |item| {
                item.is_downloading = true;
                item.progress = 0.0;
            });
        }

        let qt_thread = self.qt_thread();
        let url = zip_url.to_string();

        let handle = RUNTIME.spawn(async move {
            let result =
                download_and_extract(&version_str, &url, &target_dir, qt_thread.clone(), index)
                    .await;

            let (success, message) = match result {
                Ok(()) => (true, String::new()),
                Err(e) => (false, e.to_string()),
            };

            qt_thread.queue(move |q| {
                let model_index = {
                    let model = q.image_model.borrow();
                    model
                        .values
                        .iter()
                        .position(|item| item.version.to_string() == version_str)
                };
                if let Some(model_index) = model_index {
                    q.image_model.borrow_mut().mutate(model_index, |item| {
                        item.is_downloading = false;
                        if success {
                            item.progress = 100.0;
                        }
                    });
                }
                q.imageDownloadFinished(
                    QString::from(version_str),
                    index,
                    success,
                    QString::from(message),
                );
            });
        });

        downloads.insert(version.to_string(), handle);
        true
    }
}

async fn download_and_extract(
    version: &str,
    url: &str,
    target_dir: &Path,
    qt_thread: QtThread<DevImgsManager>,
    index: u32,
) -> anyhow::Result<()> {
    // download with progress
    let client = Client::new();
    let response = client.get(url).send().await?;
    let total = response.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;

    let tmp_zip = target_dir.join(format!("img_{}.zip.tmp", version));
    let mut file = tokio::fs::File::create(&tmp_zip).await?;
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;

        if total > 0 {
            let progress = (downloaded as f64 / total as f64) * 100.0;
            let version_str = version.to_string();
            qt_thread.queue(move |q| {
                let model_index = {
                    let model = q.image_model.borrow();
                    model
                        .values
                        .iter()
                        .position(|item| item.version.to_string() == version_str)
                };
                if let Some(model_index) = model_index {
                    q.image_model.borrow_mut().mutate(model_index, |item| {
                        item.progress = progress;
                    });
                }
                q.downloadProgressForIndex(index, QString::from(version_str), progress);
            });
        }
    }
    file.flush().await?;
    drop(file);

    let tmp_zip_std = tmp_zip.to_owned();
    let target_dir_owned = target_dir.to_owned();
    let version_owned = version.to_owned();
    println!("Extracting zip to {:?}", target_dir_owned);

    // zip extraction
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let file = std::fs::File::open(&tmp_zip_std)?;
        let mut archive = zip::ZipArchive::new(file)?;
        archive.extract(&target_dir_owned)?;
        std::fs::remove_file(&tmp_zip_std)?;

        let extracted_path = target_dir_owned.join(version_owned);
        debug!("Zip extracted to {:?}", extracted_path);
        // validate expected files exist
        let dmg = extracted_path.join("DeveloperDiskImage.dmg");
        let sig = extracted_path.join("DeveloperDiskImage.dmg.signature");
        if !dmg.exists() || !sig.exists() {
            anyhow::bail!("Zip extracted but required files missing (.dmg or .dmg.signature)");
        }
        Ok(())
    })
    .await??;

    Ok(())
}
