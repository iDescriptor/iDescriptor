// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{RUNTIME, device_ctx::get_device, qt_threading::QtThreading, qvariantmap_insert};
use anyhow::Context;
use idevice::utils::installation::install_package_with_callback;
use ipatool::error::IpaToolError;
use ipatool::{CredentialBackend, DownloadArgs, IpaTool};
use log::{debug, error, info};
use macros::QtThreading;
use qmetaobject::prelude::*;
use qttypes::QVariantMap;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        mpsc::{self, SyncSender},
    },
};
use tokio::task::JoinHandle;
use zeroize::Zeroize;

enum AuthCodeResponse {
    Code(String),
    Cancelled,
}

#[cfg(target_os = "windows")]
const KEYRING_PASSPHRASE_SERVICE: &str = "iDescriptor:ipatool:keyring-passphrase:v1";
#[cfg(target_os = "windows")]
const KEYRING_PASSPHRASE_ACCOUNT: &str = "account";

#[cfg(target_os = "windows")]
fn load_remembered_keyring_passphrase() -> Option<String> {
    let entry = keyring::Entry::new(KEYRING_PASSPHRASE_SERVICE, KEYRING_PASSPHRASE_ACCOUNT).ok()?;
    match entry.get_password() {
        Ok(passphrase) => Some(passphrase),
        Err(keyring::Error::NoEntry) => None,
        Err(err) => {
            debug!("Could not read the remembered ipatool keyring passphrase: {err}");
            None
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn load_remembered_keyring_passphrase() -> Option<String> {
    None
}

#[cfg(target_os = "windows")]
fn remember_keyring_passphrase(passphrase: &str) -> anyhow::Result<()> {
    let entry = keyring::Entry::new(KEYRING_PASSPHRASE_SERVICE, KEYRING_PASSPHRASE_ACCOUNT)?;
    entry.set_password(passphrase)?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn remember_keyring_passphrase(_passphrase: &str) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(target_os = "windows")]
fn forget_keyring_passphrase() -> anyhow::Result<()> {
    let entry = keyring::Entry::new(KEYRING_PASSPHRASE_SERVICE, KEYRING_PASSPHRASE_ACCOUNT)?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(err.into()),
    }
}

#[cfg(not(target_os = "windows"))]
fn forget_keyring_passphrase() -> anyhow::Result<()> {
    Ok(())
}

struct TaskDirectory {
    path: PathBuf,
}

impl TaskDirectory {
    async fn create(parent: &Path, task_id: &str) -> anyhow::Result<Self> {
        let path = parent.join(format!(".idescriptor-{task_id}"));
        tokio::fs::create_dir_all(&path)
            .await
            .with_context(|| format!("Failed to create temporary directory {}", path.display()))?;
        Ok(Self { path })
    }
}

impl Drop for TaskDirectory {
    fn drop(&mut self) {
        if let Err(err) = std::fs::remove_dir_all(&self.path)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            debug!(
                "Failed to remove temporary IPA directory {}: {err}",
                self.path.display()
            );
        }
    }
}
#[allow(non_snake_case)]
#[derive(QObject, Default, QtThreading)]
pub struct Apps {
    base: qt_base_class!(trait QObject),
    state: qt_property!(QVariantMap; NOTIFY  state_changed),
    state_changed: qt_signal!(),
    ipa_tool: Option<Arc<IpaTool>>,
    sign_in_busy: bool,
    uses_encrypted_account_file: bool,
    pending_auth_code: Option<(String, SyncSender<AuthCodeResponse>)>,
    tasks: HashMap<String, JoinHandle<()>>,
    init: qt_method!(fn(&mut self, load_saved_account: bool)),
    sign_in: qt_method!(
        fn(
            &mut self,
            email: QString,
            password: QString,
            keyring_passphrase: QString,
            remember_passphrase: bool,
        )
    ),
    unlock_keyring: qt_method!(fn(&mut self, passphrase: QString, remember_passphrase: bool)),
    keyring_passphrase_cancelled: qt_method!(fn(&mut self)),
    generate_keyring_passphrase: qt_method!(fn(&self) -> QString),
    auth_code_received: qt_method!(fn(&mut self, request_id: QString, code: QString)),
    auth_code_cancelled: qt_method!(fn(&mut self, request_id: QString)),
    sign_out: qt_method!(fn(&mut self)),
    search: qt_method!(fn(&mut self, term: QString)),
    download_ipa: qt_method!(fn(&mut self, bundle_id: QString, output_path: QString) -> QString),
    install_app: qt_method!(fn(&mut self, bundle_id: QString, udid: QString) -> QString),
    cancel_task: qt_method!(fn(&mut self, task_id: QString)),
    authCodeRequested: qt_signal!(request_id: QString, email: QString),
    signInFinished: qt_signal!(success: bool, error: QString),
    keyringPassphraseRequested: qt_signal!(),
    keyringPassphraseFinished: qt_signal!(success: bool, error: QString),
    search_ready: qt_signal!(search_term : QString, success: bool, res: QString),
    downloadIpaProgress: qt_signal!(task_id: QString, progress: f64),
    downloadIpaFinished: qt_signal!(task_id: QString, success: bool, path: QString, error: QString),
    installAppProgress: qt_signal!(task_id: QString, progress: f64, phase: QString),
    installAppFinished: qt_signal!(task_id: QString, success: bool, error: QString),
}

impl Drop for Apps {
    fn drop(&mut self) {
        for (_, task) in self.tasks.drain() {
            task.abort();
        }
    }
}

impl Apps {
    pub fn new_with_state() -> Self {
        let mut state = QVariantMap::default();
        qvariantmap_insert!(state, "init", false);
        qvariantmap_insert!(state, "error", QString::default());
        qvariantmap_insert!(state, "email", QString::default());
        qvariantmap_insert!(state, "signInBusy", false);
        qvariantmap_insert!(state, "authCodePending", false);
        qvariantmap_insert!(state, "usesEncryptedAccountFile", false);

        let mut def = Self::default();
        def.state = state;
        def
    }

    fn set_state(&mut self, init: bool, email: QString, error: QString) {
        let mut state = QVariantMap::default();
        qvariantmap_insert!(state, "init", init);
        qvariantmap_insert!(state, "error", error);
        qvariantmap_insert!(state, "email", email);
        qvariantmap_insert!(state, "signInBusy", self.sign_in_busy);
        qvariantmap_insert!(state, "authCodePending", self.pending_auth_code.is_some());
        qvariantmap_insert!(
            state,
            "usesEncryptedAccountFile",
            self.uses_encrypted_account_file
        );
        self.state = state;
        self.state_changed();
    }

    fn init(&mut self, load_saved_account: bool) {
        let q_thread = self.qt_thread();
        RUNTIME.spawn(async move {
            let remembered = load_remembered_keyring_passphrase();
            let result = IpaTool::new_default_with_passphrase(remembered).map(|tool| {
                let uses_file = tool.credential_backend() == CredentialBackend::EncryptedFile;
                let requires_passphrase = load_saved_account && tool.requires_keyring_passphrase();
                let account = if load_saved_account && !requires_passphrase {
                    tool.account_info()
                } else {
                    Ok(None)
                };
                (Arc::new(tool), uses_file, requires_passphrase, account)
            });

            q_thread.queue(move |apps| match result {
                Ok((tool, uses_file, requires_passphrase, account)) => {
                    apps.ipa_tool = Some(tool);
                    apps.uses_encrypted_account_file = uses_file;
                    match account {
                        Ok(account) => {
                            let email = account.map(|account| account.email).unwrap_or_default();
                            apps.set_state(true, QString::from(email), QString::default());
                            if requires_passphrase {
                                apps.keyringPassphraseRequested();
                            }
                        }
                        Err(err) => {
                            error!("Could not load the saved ipatool account: {err}");
                            apps.set_state(
                                true,
                                QString::default(),
                                QString::from(err.to_string()),
                            );
                        }
                    }
                }
                Err(err) => {
                    error!("Could not initialize ipatool: {err}");
                    apps.set_state(true, QString::default(), QString::from(err.to_string()));
                }
            });
        });
    }

    fn sign_in(
        &mut self,
        email: QString,
        password: QString,
        keyring_passphrase: QString,
        remember_passphrase: bool,
    ) {
        if self.sign_in_busy {
            self.signInFinished(false, QString::from("Sign in is already in progress."));
            return;
        }

        self.sign_in_busy = true;
        self.set_state(true, QString::default(), QString::default());

        let email_string = email.to_string();
        let mut password_string = password.to_string();
        let mut keyring_passphrase = keyring_passphrase.to_string();
        let Some(tool) = self.ipa_tool.clone() else {
            self.sign_in_busy = false;
            self.set_state(
                true,
                QString::default(),
                QString::from("IpaTool not initialized"),
            );
            self.signInFinished(false, QString::from("IpaTool not initialized"));
            return;
        };

        let q_thread = self.qt_thread();

        RUNTIME.spawn(async move {
            let q_thread_for_cb = q_thread.clone();
            let email_for_cb = email_string.clone();
            let auth_code_cb: Box<dyn Fn() -> ipatool::Result<String> + Send + Sync> =
                Box::new(move || {
                    let request_id = uuid::Uuid::new_v4().to_string();
                    let (tx, rx) = mpsc::sync_channel::<AuthCodeResponse>(1);
                    let qt = q_thread_for_cb.clone();
                    let request_id_for_qt = request_id.clone();
                    let email_for_qt = email_for_cb.clone();

                    qt.queue(move |apps| {
                        if let Some((_old_id, old_tx)) = apps.pending_auth_code.take() {
                            let _ = old_tx.send(AuthCodeResponse::Cancelled);
                        }

                        apps.pending_auth_code = Some((request_id_for_qt.clone(), tx));
                        apps.set_state(true, QString::default(), QString::default());
                        apps.authCodeRequested(
                            QString::from(request_id_for_qt),
                            QString::from(email_for_qt),
                        );
                    });

                    match rx.recv() {
                        Ok(AuthCodeResponse::Code(code)) if !code.is_empty() => Ok(code),
                        Ok(AuthCodeResponse::Code(_))
                        | Ok(AuthCodeResponse::Cancelled)
                        | Err(_) => Err(IpaToolError::AuthCodeRequired),
                    }
                });

            let result: ipatool::Result<(Arc<IpaTool>, String)> = async {
                if tool.credential_backend() == CredentialBackend::EncryptedFile {
                    tool.set_keyring_passphrase(keyring_passphrase.clone())?;
                }
                tool.login(&email_string, &password_string, Some(auth_code_cb), None)
                    .await?;

                let account_email = tool
                    .account_info()?
                    .map(|acc| acc.email)
                    .unwrap_or_else(|| email_string.clone());

                Ok((tool, account_email))
            }
            .await;

            if result.is_ok() && remember_passphrase && !keyring_passphrase.is_empty() {
                if let Err(err) = remember_keyring_passphrase(&keyring_passphrase) {
                    error!("Could not remember the ipatool keyring passphrase: {err}");
                }
            }
            password_string.zeroize();
            keyring_passphrase.zeroize();

            q_thread.queue(move |apps| {
                apps.sign_in_busy = false;
                if let Some((_id, tx)) = apps.pending_auth_code.take() {
                    let _ = tx.send(AuthCodeResponse::Cancelled);
                }

                match result {
                    Ok((tool, account_email)) => {
                        apps.ipa_tool = Some(tool);
                        apps.set_state(true, QString::from(account_email), QString::default());
                        apps.signInFinished(true, QString::default());
                    }
                    Err(err) => {
                        let error = QString::from(format!("{}", err));
                        debug!("Sign in failed: {}", error.to_string());
                        apps.set_state(true, QString::default(), error.clone());
                        apps.signInFinished(false, error);
                    }
                }
            });
        });
    }

    fn unlock_keyring(&mut self, passphrase: QString, remember_passphrase: bool) {
        let Some(tool) = self.ipa_tool.clone() else {
            self.keyringPassphraseFinished(false, QString::from("IpaTool not initialized"));
            return;
        };
        let mut passphrase = passphrase.to_string();
        let q_thread = self.qt_thread();
        RUNTIME.spawn(async move {
            let result = tool
                .set_keyring_passphrase(passphrase.clone())
                .and_then(|()| tool.account_info());
            if result.is_ok() && remember_passphrase {
                if let Err(err) = remember_keyring_passphrase(&passphrase) {
                    error!("Could not remember the ipatool keyring passphrase: {err}");
                }
            }
            passphrase.zeroize();
            q_thread.queue(move |apps| match result {
                Ok(account) => {
                    let email = account.map(|account| account.email).unwrap_or_default();
                    apps.set_state(true, QString::from(email), QString::default());
                    apps.keyringPassphraseFinished(true, QString::default());
                }
                Err(err) => {
                    let message = QString::from(err.to_string());
                    apps.keyringPassphraseFinished(false, message);
                }
            });
        });
    }

    fn keyring_passphrase_cancelled(&mut self) {
        self.set_state(true, QString::default(), QString::default());
    }

    fn generate_keyring_passphrase(&self) -> QString {
        const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
        let mut random = [0_u8; 24];
        if let Err(err) = getrandom::fill(&mut random) {
            error!("Could not generate a keyring passphrase: {err}");
            return QString::default();
        }
        let mut passphrase = String::with_capacity(29);
        for (index, byte) in random.into_iter().enumerate() {
            if index > 0 && index % 4 == 0 {
                passphrase.push('-');
            }
            passphrase.push(ALPHABET[usize::from(byte) % ALPHABET.len()] as char);
        }
        random.zeroize();
        let generated = QString::from(passphrase.clone());
        passphrase.zeroize();
        generated
    }

    fn auth_code_received(&mut self, request_id: QString, code: QString) {
        let request_id = request_id.to_string();
        let code = code.to_string().trim().to_string();

        let Some((pending_id, tx)) = self.pending_auth_code.take() else {
            return;
        };

        if pending_id != request_id {
            self.pending_auth_code = Some((pending_id, tx));
            return;
        }

        let _ = tx.send(AuthCodeResponse::Code(code));
        //self.set_state(true, QString::default(), QString::default());
    }

    fn auth_code_cancelled(&mut self, request_id: QString) {
        let request_id = request_id.to_string();

        let Some((pending_id, tx)) = self.pending_auth_code.take() else {
            return;
        };

        if pending_id != request_id {
            self.pending_auth_code = Some((pending_id, tx));
            return;
        }

        let _ = tx.send(AuthCodeResponse::Cancelled);
        // self.set_state(true, QString::default(), QString::default());
    }

    // DOES NOTHING IF CALLED BEFORE CALLING `init`
    fn search(&mut self, term: QString) {
        let Some(tool) = &self.ipa_tool else {
            eprintln!("IpaTool not initialized");
            return;
        };
        let tool = tool.clone();
        let q_thread = self.qt_thread();

        RUNTIME.spawn(async move {
            let serialized: anyhow::Result<String> = async {
                let apps = tool.search(&term.to_string(), 20).await?;
                Ok(serde_json::to_string(&apps)?)
            }
            .await;

            match serialized {
                Ok(serialized) => {
                    debug!(
                        "Search successful for term '{}', result: {}",
                        term, serialized
                    );
                    q_thread.queue(|s| s.search_ready(term, true, QString::from(serialized)));
                }
                // FIXME: also return the error
                Err(err) => {
                    error!("Error in ipatool search {}", err.to_string());
                    q_thread.queue(|s| s.search_ready(term, false, QString::default()));
                }
            };
        });
    }

    fn download_ipa(&mut self, bundle_id: QString, output_path: QString) -> QString {
        let Some(tool) = self.ipa_tool.clone() else {
            return QString::default();
        };

        let bundle_id = bundle_id.to_string();
        if bundle_id.trim().is_empty() {
            return QString::default();
        }

        let output_path = PathBuf::from(output_path.to_string());
        let task_id = uuid::Uuid::new_v4().to_string();
        let task_id_for_task = task_id.clone();
        let q_thread = self.qt_thread();

        let task = RUNTIME.spawn(async move {
            let result: anyhow::Result<String> = async {
                tokio::fs::create_dir_all(&output_path)
                    .await
                    .with_context(|| {
                        format!(
                            "Failed to create IPA download directory {}",
                            output_path.display()
                        )
                    })?;

                let task_dir = TaskDirectory::create(&output_path, &task_id_for_task).await?;
                let temporary_ipa_path = task_dir.path.join(format!("{bundle_id}.ipa"));
                let progress_qt = q_thread.clone();
                let progress_task_id = task_id_for_task.clone();
                let downloaded_path = tool
                    .download_with_progress(
                        DownloadArgs {
                            bundle_id,
                            output_path: Some(temporary_ipa_path.to_string_lossy().into_owned()),
                            external_version_id: None,
                            acquire_license: true,
                        },
                        move |downloaded, total| {
                            let progress = total
                                .filter(|total| *total > 0)
                                .map(|total| downloaded as f64 / total as f64)
                                .unwrap_or(-1.0);
                            let id = progress_task_id.clone();
                            progress_qt.queue(move |apps| {
                                apps.downloadIpaProgress(QString::from(id), progress);
                            });
                        },
                    )
                    .await?;

                let downloaded_path = PathBuf::from(downloaded_path);
                let file_name = downloaded_path
                    .file_name()
                    .context("The downloaded IPA path has no file name")?;
                let destination = output_path.join(file_name);
                if destination.exists() {
                    std::fs::remove_file(&destination).with_context(|| {
                        format!("Failed to replace IPA at {}", destination.display())
                    })?;
                }
                std::fs::rename(&downloaded_path, &destination)
                    .with_context(|| format!("Failed to save IPA to {}", destination.display()))?;

                Ok(destination.to_string_lossy().into_owned())
            }
            .await;

            let finished_task_id = task_id_for_task.clone();
            q_thread.queue(move |apps| {
                if apps.tasks.remove(&finished_task_id).is_none() {
                    return;
                }

                match result {
                    Ok(path) => {
                        info!("IPA download completed: {path}");
                        apps.downloadIpaFinished(
                            QString::from(finished_task_id),
                            true,
                            QString::from(path),
                            QString::default(),
                        );
                    }
                    Err(err) => {
                        error!("IPA download failed: {err:#}");
                        apps.downloadIpaFinished(
                            QString::from(finished_task_id),
                            false,
                            QString::default(),
                            QString::from(format!("{err:#}")),
                        );
                    }
                }
            });
        });

        self.tasks.insert(task_id.clone(), task);
        QString::from(task_id)
    }

    fn install_app(&mut self, bundle_id: QString, udid: QString) -> QString {
        let Some(tool) = self.ipa_tool.clone() else {
            return QString::default();
        };

        let bundle_id = bundle_id.to_string();
        let udid = udid.to_string();
        if bundle_id.trim().is_empty() || udid.trim().is_empty() {
            return QString::default();
        }

        let task_id = uuid::Uuid::new_v4().to_string();
        let task_id_for_task = task_id.clone();
        let q_thread = self.qt_thread();

        let task = RUNTIME.spawn(async move {
            let result: anyhow::Result<()> = async {
                let device = get_device(&udid).await?;
                let task_dir =
                    TaskDirectory::create(&std::env::temp_dir(), &task_id_for_task).await?;
                let temporary_ipa_path = task_dir.path.join(format!("{bundle_id}.ipa"));
                let progress_qt = q_thread.clone();
                let progress_task_id = task_id_for_task.clone();
                let ipa_path = tool
                    .download_with_progress(
                        DownloadArgs {
                            bundle_id,
                            output_path: Some(temporary_ipa_path.to_string_lossy().into_owned()),
                            external_version_id: None,
                            acquire_license: true,
                        },
                        move |downloaded, total| {
                            let progress = total
                                .filter(|total| *total > 0)
                                .map(|total| (downloaded as f64 / total as f64) * 0.5)
                                .unwrap_or(-1.0);
                            let id = progress_task_id.clone();
                            progress_qt.queue(move |apps| {
                                apps.installAppProgress(
                                    QString::from(id),
                                    progress,
                                    QString::from("download"),
                                );
                            });
                        },
                    )
                    .await
                    .context("Failed to download the IPA")?;

                let installing_qt = q_thread.clone();
                let installing_task_id = task_id_for_task.clone();
                installing_qt.queue(move |apps| {
                    apps.installAppProgress(
                        QString::from(installing_task_id),
                        0.5,
                        QString::from("install"),
                    );
                });

                let provider = device.provider.lock().await;
                let callback_qt = q_thread.clone();
                let callback_task_id = task_id_for_task.clone();
                install_package_with_callback(
                    provider.as_ref(),
                    &ipa_path,
                    None,
                    move |(percentage, ())| {
                        let qt = callback_qt.clone();
                        let id = callback_task_id.clone();
                        async move {
                            let progress = 0.5 + (percentage.min(100) as f64 / 200.0);
                            qt.queue(move |apps| {
                                apps.installAppProgress(
                                    QString::from(id),
                                    progress,
                                    QString::from("install"),
                                );
                            });
                        }
                    },
                    (),
                )
                .await
                .context("Failed to install the IPA")?;

                Ok(())
            }
            .await;

            let finished_task_id = task_id_for_task.clone();
            q_thread.queue(move |apps| {
                if apps.tasks.remove(&finished_task_id).is_none() {
                    return;
                }

                match result {
                    Ok(()) => {
                        info!("App installation completed for device {udid}");
                        apps.installAppFinished(
                            QString::from(finished_task_id),
                            true,
                            QString::default(),
                        );
                    }
                    Err(err) => {
                        error!("App installation failed for device {udid}: {err:#}");
                        apps.installAppFinished(
                            QString::from(finished_task_id),
                            false,
                            QString::from(format!("{err:#}")),
                        );
                    }
                }
            });
        });

        self.tasks.insert(task_id.clone(), task);
        QString::from(task_id)
    }

    fn cancel_task(&mut self, task_id: QString) {
        let task_id = task_id.to_string();
        if let Some(task) = self.tasks.remove(&task_id) {
            task.abort();
            info!("Cancelled app-store task {task_id}");
        }
    }

    fn sign_out(&mut self) {
        let Some(tool) = &self.ipa_tool else {
            eprintln!("IpaTool not initialized");
            return;
        };

        match tool.revoke() {
            Ok(()) => {
                if let Err(err) = forget_keyring_passphrase() {
                    error!("Could not forget the ipatool keyring passphrase: {err}");
                }
                self.set_state(true, QString::default(), QString::default())
            }
            Err(err) => self.set_state(true, QString::default(), QString::from(format!("{}", err))),
        }
    }
}
