// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::device_ctx;
use crate::media_decoder::decode_heic_image;
use crate::media_streamer::MediaStreamSession;
use crate::qt_threading::QtThreading;
use crate::utils::{image_to_b64, is_heic_file};
use crate::{RUNTIME, qvariantmap_insert, run_sync};
use base64::{Engine as _, engine::general_purpose};
use idevice::afc::opcode::AfcFopenMode;
use idevice::afc::{AfcClient, FileInfo};
use log::{debug, error, info, warn};
use macros::QtThreading;
use qmetaobject::prelude::*;
use qttypes::{QStringList, QVariantMap};
use std::{collections::HashSet, path::Component, sync::Arc};
use tokio::sync::Mutex;

#[allow(non_snake_case)]
#[derive(QObject, Default, QtThreading)]
pub struct AfcServices {
    base: qt_base_class!(trait QObject),
    afc: Option<Arc<Mutex<AfcClient>>>,
    udid: String,
    // file_to_buffer: qt_method!(fn(self, file_path: QString) -> QByteArray),
    // get_file_size: qt_method!(fn(self, path: QString) -> i64),
    check_is_dir_and_list: qt_method!(fn(&self, path: QString)),

    check_is_dir_and_list_finished: qt_signal!(
        success: bool,
        entries: QVariantMap
    ),
    fileToBase64ImgReady: qt_signal!(file_path: QString, source: QString),
    fileToBase64ImgFailed: qt_signal!(file_path: QString, error: QString),
    file_to_base64_img: qt_method!(fn(&self, file_path: QString)),
    fileInfoReady: qt_signal!(file_path: QString, info: QVariantMap),
    fileInfoFailed: qt_signal!(file_path: QString, error: QString),
    get_file_info: qt_method!(fn(&self, file_path: QString)),
    deletePathsFinished: qt_signal!(
        request_id: QString,
        successful_items: i32,
        failed_items: i32,
        first_error: QString
    ),
    delete_paths: qt_method!(
        fn(&self, request_id: QString, paths: QStringList, recursive_directories_confirmed: bool)
    ),
    start_video_stream: qt_method!(fn(&self, file_path: QString) -> QString),
    release_video_stream: qt_method!(fn(&self, url: QString)),
    delete_path: qt_method!(fn(&self, path: QString) -> bool),
    //only required for hause_arrest afc
    bundle_id: qt_property!(QString),
}

impl AfcServices {
    pub fn from_afc_client(
        afc_client: Arc<Mutex<AfcClient>>,
        /* udid is for debugging purposes */
        udid: String,
        //only required for hause_arrest afc
        bundle_id: Option<String>,
    ) -> Self {
        let mut service = Self::default();
        service.afc = Some(afc_client);
        service.udid = udid;
        service.bundle_id = bundle_id.map_or_else(QString::default, QString::from);
        service
    }

    fn afc_client(&self, operation: &str) -> Option<Arc<Mutex<AfcClient>>> {
        let Some(afc) = self.afc.as_ref() else {
            let udid = if self.udid.is_empty() {
                "unknown"
            } else {
                &self.udid
            };
            warn!("AfcServices cannot {operation}: AFC client is unavailable for udid={udid}");
            return None;
        };

        Some(afc.clone())
    }

    fn file_to_base64_img(&self, file_path: QString) {
        let Some(afc) = self.afc_client("load a preview image") else {
            return;
        };
        let path = file_path.to_string();
        let qt_thread = self.qt_thread();

        RUNTIME.spawn(async move {
            let result: anyhow::Result<QString> = async {
                let mut afc = afc.lock().await;
                let mut file = afc.open(&path, AfcFopenMode::RdOnly).await?;
                let read_result = file.read_entire().await;
                file.close().await.ok();
                let data = read_result?;
                drop(afc);

                if data.is_empty() {
                    anyhow::bail!("Image file is empty");
                }

                if is_heic_file(&path) {
                    let image = decode_heic_image(&data)?;
                    return Ok(image_to_b64(image));
                }

                let encoded = general_purpose::STANDARD.encode(data);
                Ok(QString::from(format!(
                    "data:{};base64,{encoded}",
                    image_mime_type(&path)
                )))
            }
            .await;

            let signal_path = QString::from(path.clone());
            qt_thread.queue(move |service| match result {
                Ok(source) => service.fileToBase64ImgReady(signal_path, source),
                Err(err) => {
                    warn!("Failed to load preview image {path}: {err}");
                    service.fileToBase64ImgFailed(signal_path, QString::from(err.to_string()));
                }
            });
        });
    }

    fn get_file_info(&self, file_path: QString) {
        let Some(afc) = self.afc_client("get file information") else {
            return;
        };
        let path = file_path.to_string();
        let qt_thread = self.qt_thread();

        RUNTIME.spawn(async move {
            let result = {
                let mut afc = afc.lock().await;
                afc.get_file_info(&path).await
            };

            let signal_path = QString::from(path.clone());
            qt_thread.queue(move |service| match result {
                Ok(info) => service.fileInfoReady(signal_path, file_info_to_qvariant_map(info)),
                Err(err) => {
                    warn!("Failed to get AFC file info for {path}: {err}");
                    service.fileInfoFailed(signal_path, QString::from(err.to_string()));
                }
            });
        });
    }

    fn delete_paths(
        &self,
        request_id: QString,
        paths: QStringList,
        recursive_directories_confirmed: bool,
    ) {
        let Some(afc) = self.afc_client("delete paths") else {
            return;
        };
        let request_id = request_id.to_string();
        let paths: Vec<String> = paths.into_iter().map(|path| path.to_string()).collect();
        let qt_thread = self.qt_thread();

        RUNTIME.spawn(async move {
            let mut successful_items = 0_i32;
            let mut failed_items = 0_i32;
            let mut first_error = None;
            let mut seen_paths = HashSet::new();
            let mut afc = afc.lock().await;

            for path in paths {
                if !seen_paths.insert(path.clone()) {
                    continue;
                }

                let result: Result<(), String> = async {
                    validate_deletion_path(&path)?;
                    let info = afc
                        .get_file_info(&path)
                        .await
                        .map_err(|err| format!("Failed to inspect {path}: {err}"))?;

                    if info.st_ifmt == "S_IFDIR" {
                        if !recursive_directories_confirmed {
                            return Err(format!(
                                "Refusing to recursively delete unconfirmed directory {path}"
                            ));
                        }

                        afc.remove_all(&path)
                            .await
                            .map_err(|err| format!("Failed to delete directory {path}: {err}"))
                    } else {
                        afc.remove(&path)
                            .await
                            .map_err(|err| format!("Failed to delete {path}: {err}"))
                    }
                }
                .await;

                match result {
                    Ok(()) => successful_items += 1,
                    Err(err) => {
                        warn!("AFC batch deletion failed: {err}");
                        failed_items += 1;
                        if first_error.is_none() {
                            first_error = Some(err);
                        }
                    }
                }
            }

            let request_id = QString::from(request_id);
            let first_error = QString::from(first_error.unwrap_or_default());
            qt_thread.queue(move |service| {
                service.deletePathsFinished(
                    request_id,
                    successful_items,
                    failed_items,
                    first_error,
                );
            });
        });
    }

    fn check_is_dir_and_list(&self, path: QString) {
        let Some(afc_arc) = self.afc_client("list a directory") else {
            return;
        };
        let path_str = path.to_string();
        let qt_thread = self.qt_thread();
        RUNTIME.spawn(async move {
            let mut map = QVariantMap::default();
            let mut afc = afc_arc.lock().await;
            let success = match afc.list_dir(&path_str).await {
                Ok(list) => {
                    for name in list {
                        // ui already has up/down buttons maybe unnecessary
                        if name == "." || name == ".." {
                            continue;
                        }
                        let full_path = format!("{}/{}", path_str, name);
                        let is_dir = match afc.get_file_info(&full_path).await {
                            Ok(info) if info.st_ifmt == "S_IFLNK" => {
                                match afc.get_file_info_resolved(&full_path).await {
                                    Ok(resolved) => resolved.info.st_ifmt == "S_IFDIR",
                                    Err(e) => {
                                        warn!(
                                            "Failed to resolve AFC symbolic link {full_path}: {e}"
                                        );
                                        false
                                    }
                                }
                            }
                            Ok(info) => info.st_ifmt == "S_IFDIR",
                            Err(e) => {
                                warn!("Failed to get AFC file info for {full_path}: {e}");
                                false
                            }
                        };
                        map.insert(QString::from(name), QVariant::from(&is_dir));
                    }
                    true
                }
                Err(e) => {
                    eprintln!("Failed to read directory {path_str}: {e}");
                    false
                }
            };

            qt_thread.queue(move |q| {
                q.check_is_dir_and_list_finished(success, map);
            });
        });
    }

    fn start_video_stream(&self, file_path: QString) -> QString {
        let Some(afc) = self.afc_client("start a video stream") else {
            return QString::default();
        };
        let path_str = file_path.to_string();
        let udid = self.udid.clone();
        let stream_udid = udid.clone();

        info!("Starting media stream for udid={udid} path={path_str}");
        let result: anyhow::Result<String> = run_sync(async move {
            let device = device_ctx::get_device(&stream_udid).await?;
            let (url, session) = MediaStreamSession::start(afc, path_str).await?;
            device
                .video_streams
                .lock()
                .await
                .insert(url.clone(), session);
            Ok(url)
        });

        match result {
            Ok(url) => {
                info!("Serving media stream at {url} for udid={udid}");
                QString::from(url)
            }
            Err(err) => {
                error!("Failed to start media stream for udid={udid}: {err}");
                QString::default()
            }
        }
    }

    fn release_video_stream(&self, url: QString) {
        if self.afc_client("release a video stream").is_none() {
            return;
        }
        let udid = self.udid.clone();
        let url_str = url.to_string();

        if url_str.is_empty() {
            return;
        }

        RUNTIME.spawn(async move {
            let Some(device) = device_ctx::get_device_opt(&udid).await else {
                eprintln!("release_video_stream: device {udid} not found");
                return;
            };

            let session = {
                let mut video_streams = device.video_streams.lock().await;
                video_streams.remove(&url_str)
            };

            if let Some(mut session) = session {
                info!("Shutting down media stream {url_str}");
                session.shutdown().await;
            } else {
                warn!("No active media stream for {url_str}");
            }
        });
    }

    fn delete_path(&self, path: QString) -> bool {
        let Some(afc_arc) = self.afc_client("delete a path") else {
            return false;
        };
        let path_str = path.to_string();

        run_sync(async move {
            let mut afc = afc_arc.lock().await;

            match afc.remove(&path_str).await {
                Ok(_) => true,
                Err(e) => {
                    eprintln!("delete_path: delete({path_str}) failed: {e}");
                    false
                }
            }
        })
    }
}

fn image_mime_type(path: &str) -> &'static str {
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "webp" => "image/webp",
        "tif" | "tiff" => "image/tiff",
        _ => "application/octet-stream",
    }
}

fn validate_deletion_path(path: &str) -> Result<(), String> {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed == "/" || trimmed == "." || trimmed == ".." {
        return Err(format!("Refusing to delete unsafe path {trimmed:?}"));
    }

    if std::path::Path::new(trimmed)
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return Err(format!(
            "Refusing to delete path containing '..': {trimmed}"
        ));
    }

    Ok(())
}

fn file_info_to_qvariant_map(info: FileInfo) -> QVariantMap {
    let mut map = QVariantMap::default();
    qvariantmap_insert!(map, "size", info.size as i64);
    qvariantmap_insert!(map, "blocks", info.blocks as i64);
    qvariantmap_insert!(
        map,
        "creation",
        QString::from(info.creation.format("%Y-%m-%d %H:%M:%S%.3f").to_string())
    );
    qvariantmap_insert!(
        map,
        "modified",
        QString::from(info.modified.format("%Y-%m-%d %H:%M:%S%.3f").to_string())
    );
    qvariantmap_insert!(map, "hardLinks", QString::from(info.st_nlink));
    qvariantmap_insert!(map, "type", QString::from(info.st_ifmt));
    qvariantmap_insert!(
        map,
        "linkTarget",
        QString::from(info.st_link_target.unwrap_or_default())
    );
    map
}

impl Drop for AfcServices {
    fn drop(&mut self) {
        debug!("AfcServices dropped for udid: {}", self.udid);
    }
}
