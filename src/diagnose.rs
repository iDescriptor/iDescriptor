// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{RUNTIME, qt_threading::QtThreading, qvariantmap_insert};
#[cfg(any(
    target_os = "linux",
    all(target_os = "windows", not(feature = "windows_store"))
))]
use anyhow::{Context, bail};
use anyhow::{Result, anyhow};
#[cfg(all(target_os = "linux", feature = "flatpak"))]
use cpp::cpp;
use log::{error, warn};
use macros::QtThreading;
use qmetaobject::prelude::*;
use qttypes::{QVariantList, QVariantMap};
#[cfg(target_os = "windows")]
use std::mem::MaybeUninit;
#[cfg(target_os = "windows")]
use windows_sys::Win32::{
    Foundation::{ERROR_SERVICE_DOES_NOT_EXIST, GetLastError},
    System::Services::{
        CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceStatusEx, SC_HANDLE,
        SC_MANAGER_CONNECT, SC_STATUS_PROCESS_INFO, SERVICE_QUERY_STATUS, SERVICE_RUNNING,
        SERVICE_STATUS_PROCESS,
    },
};

cpp::cpp! {{
    #ifdef IDESCRIPTOR_FLATPAK
    #include <QDBusConnection>
    #include <QDBusConnectionInterface>
    #include <QDBusReply>
    #include <QDebug>

    static bool idescriptor_is_avahi_available()
    {
        QDBusConnection bus = QDBusConnection::systemBus();

        if (!bus.isConnected()) {
            qWarning() << "Cannot connect to the D-Bus system bus";
            return false;
        }

        QDBusConnectionInterface *interface = bus.interface();
        QDBusReply<bool> reply =
            interface->isServiceRegistered(QStringLiteral("org.freedesktop.Avahi"));

        if (!reply.isValid()) {
            qWarning() << "Failed to query Avahi service registration:"
                       << reply.error().message();
            return false;
        }

        return reply.value();
    }
    #else
    // have to define this because
    // cpp_build scans cpp! blocks even when the Rust call is feature-gated.
    static bool idescriptor_is_avahi_available()
    {
        return false;
    }
    #endif
}}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Availability {
    Available,
    AvailableButNotRunning,
    Unavailable,
    UnableToCheck,
}

pub(crate) enum AirPlayRequirement {
    Ready,
    NeedsAction {
        dependency_id: &'static str,
        reason: &'static str,
    },
    UnableToCheck {
        detail: String,
    },
}

#[derive(Clone, Debug)]
struct DependencyStatus {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    optional: bool,
    availability: Availability,
    action_text: &'static str,
    action_mode: &'static str,
    documentation_url: &'static str,
}

#[derive(QObject, Default, QtThreading)]
pub struct Diagnose {
    base: qt_base_class!(trait QObject),
    state: qt_property!(QVariantMap; NOTIFY state_changed),
    state_changed: qt_signal!(),
    check: qt_method!(fn(&mut self)),
    install: qt_method!(fn(&mut self, dependency_id: QString)),
    clear_notice: qt_method!(fn(&mut self)),
}

impl Diagnose {
    pub fn new_with_state() -> Self {
        let mut def = Self::default();
        def.state = build_state(Vec::new(), true, QString::default(), false);
        def
    }

    fn check(&mut self) {
        self.state = build_state(Vec::new(), true, QString::default(), false);
        self.state_changed();

        let q_thread = self.qt_thread();
        RUNTIME.spawn(async move {
            let result = check_dependencies().await;
            q_thread.queue(move |t| match result {
                Ok(items) => {
                    t.state = build_state(items, false, QString::default(), false);
                    t.state_changed();
                }
                Err(err) => {
                    error!("diagnose check failed: {err:#}");
                    t.state = build_error_state(format!("{err:#}"));
                    t.state_changed();
                }
            });
        });
    }

    fn install(&mut self, dependency_id: QString) {
        let dependency_id = dependency_id.to_string();
        if dependency_id.is_empty() {
            return;
        }

        self.set_installing(&dependency_id);
        let q_thread = self.qt_thread();

        RUNTIME.spawn(async move {
            let result = install_dependency(&dependency_id).await;
            let check_result = check_dependencies().await;

            q_thread.queue(move |t| {
                let notice = match result {
                    Ok(message) => QString::from(message),
                    Err(err) => {
                        error!("dependency install action failed for {dependency_id}: {err:#}");
                        QString::from(format!("{err:#}"))
                    }
                };

                t.state = match check_result {
                    Ok(items) => build_state(items, false, notice, false),
                    Err(err) => build_error_state(format!("{err:#}")),
                };
                t.state_changed();
            });
        });
    }

    fn clear_notice(&mut self) {
        let mut state = self.state.clone();
        qvariantmap_insert!(state, "notice", QString::default());
        self.state = state;
        self.state_changed();
    }

    fn set_installing(&mut self, dependency_id: &str) {
        let mut state = self.state.clone();
        qvariantmap_insert!(state, "checking", false);
        qvariantmap_insert!(state, "installingId", QString::from(dependency_id));
        qvariantmap_insert!(state, "notice", QString::default());
        self.state = state;
        self.state_changed();
    }
}

fn build_error_state(error: String) -> QVariantMap {
    let mut state = QVariantMap::default();
    qvariantmap_insert!(state, "checking", false);
    qvariantmap_insert!(state, "error", QString::from(error));
    qvariantmap_insert!(
        state,
        "summary",
        QString::from("Unable to check system dependencies")
    );
    qvariantmap_insert!(state, "summaryKind", QString::from("error"));
    qvariantmap_insert!(state, "shouldExpand", true);
    qvariantmap_insert!(state, "requiredMissing", true);
    qvariantmap_insert!(state, "notice", QString::default());
    qvariantmap_insert!(state, "installingId", QString::default());
    qvariantmap_insert!(state, "items", QVariantList::default());
    state
}

fn build_state(
    items: Vec<DependencyStatus>,
    checking: bool,
    notice: QString,
    required_missing_override: bool,
) -> QVariantMap {
    let total = items.iter().filter(|item| !item.optional).count();
    let installed = items
        .iter()
        .filter(|item| !item.optional && item.availability == Availability::Available)
        .count();
    let required_missing = required_missing_override
        || items.iter().any(|item| {
            !item.optional
                && matches!(
                    item.availability,
                    Availability::Unavailable
                        | Availability::UnableToCheck
                        | Availability::AvailableButNotRunning
                )
        });
    let optional_available = items
        .iter()
        .filter(|item| item.optional && item.availability != Availability::Available)
        .count();

    let summary = if checking {
        "Checking system dependencies...".to_string()
    } else if required_missing {
        format!("Missing required dependencies ({installed}/{total} installed)")
    } else if optional_available > 0 {
        let noun = if optional_available == 1 {
            "capability"
        } else {
            "capabilities"
        };
        format!("{optional_available} optional {noun} available")
    } else if total == 0 {
        "No system dependencies are required on this platform".to_string()
    } else {
        "All required dependencies are installed".to_string()
    };

    let summary_kind = if checking {
        "loading"
    } else if required_missing {
        "error"
    } else if optional_available > 0 {
        "warning"
    } else {
        "ok"
    };

    let mut state = QVariantMap::default();
    qvariantmap_insert!(state, "checking", checking);
    qvariantmap_insert!(state, "error", QString::default());
    qvariantmap_insert!(state, "summary", QString::from(summary));
    qvariantmap_insert!(state, "summaryKind", QString::from(summary_kind));
    qvariantmap_insert!(state, "shouldExpand", required_missing);
    qvariantmap_insert!(state, "requiredMissing", required_missing);
    qvariantmap_insert!(state, "notice", notice);
    qvariantmap_insert!(state, "installingId", QString::default());
    qvariantmap_insert!(state, "items", items_to_variant_list(items));
    state
}

fn items_to_variant_list(items: Vec<DependencyStatus>) -> QVariantList {
    let mut list = QVariantList::default();
    for item in items {
        let mut map = QVariantMap::default();
        qvariantmap_insert!(map, "id", QString::from(item.id));
        qvariantmap_insert!(map, "name", QString::from(item.name));
        qvariantmap_insert!(map, "description", QString::from(item.description));
        qvariantmap_insert!(map, "optional", item.optional);
        qvariantmap_insert!(map, "availability", availability_code(item.availability));
        qvariantmap_insert!(
            map,
            "statusText",
            QString::from(status_text(item.availability))
        );
        qvariantmap_insert!(
            map,
            "statusKind",
            QString::from(status_kind(item.availability))
        );
        qvariantmap_insert!(map, "actionText", QString::from(item.action_text));
        qvariantmap_insert!(map, "actionMode", QString::from(item.action_mode));
        qvariantmap_insert!(
            map,
            "documentationUrl",
            QString::from(item.documentation_url)
        );
        let action_visible = item.availability != Availability::Available
            && !(cfg!(all(target_os = "linux", feature = "flatpak")) && item.id == "avahi");
        qvariantmap_insert!(map, "actionVisible", action_visible);
        list.push(QVariant::from(map));
    }
    list
}

fn availability_code(availability: Availability) -> i32 {
    match availability {
        Availability::Available => 0,
        Availability::AvailableButNotRunning => 1,
        Availability::Unavailable => 2,
        Availability::UnableToCheck => 3,
    }
}

fn status_text(availability: Availability) -> &'static str {
    match availability {
        Availability::Available => "Installed",
        Availability::AvailableButNotRunning => "Installed, not running",
        Availability::Unavailable => "Missing",
        Availability::UnableToCheck => "Unable to check",
    }
}

fn status_kind(availability: Availability) -> &'static str {
    match availability {
        Availability::Available => "ok",
        Availability::AvailableButNotRunning => "warning",
        Availability::Unavailable | Availability::UnableToCheck => "error",
    }
}

async fn check_dependencies() -> Result<Vec<DependencyStatus>> {
    #[cfg(target_os = "windows")]
    {
        return check_windows_dependencies().await;
    }

    #[cfg(target_os = "linux")]
    {
        return check_linux_dependencies().await;
    }

    #[allow(unreachable_code)]
    Ok(Vec::new())
}

pub(crate) async fn check_airplay_requirement() -> AirPlayRequirement {
    #[cfg(target_os = "linux")]
    let (dependency_id, availability) = ("avahi", check_avahi_status().await);

    #[cfg(target_os = "windows")]
    let (dependency_id, availability) =
        ("bonjour", windows_service_status("Bonjour Service").await);

    match availability {
        Availability::Available => AirPlayRequirement::Ready,
        Availability::AvailableButNotRunning => AirPlayRequirement::NeedsAction {
            dependency_id,
            reason: "not_running",
        },
        Availability::Unavailable => AirPlayRequirement::NeedsAction {
            dependency_id,
            reason: "missing",
        },
        Availability::UnableToCheck => AirPlayRequirement::UnableToCheck {
            detail: format!("Unable to check the {dependency_id} dependency."),
        },
    }
}

async fn install_dependency(dependency_id: &str) -> Result<String> {
    #[cfg(all(target_os = "windows", not(feature = "windows_store")))]
    {
        return install_windows_dependency(dependency_id).await;
    }

    #[cfg(all(target_os = "windows", feature = "windows_store"))]
    {
        let _ = dependency_id;
        return Err(anyhow!(
            "Install actions are not available in the Microsoft Store build"
        ));
    }

    #[cfg(target_os = "linux")]
    {
        return install_linux_dependency(dependency_id).await;
    }

    #[allow(unreachable_code)]
    Err(anyhow!("No install action is available on this platform"))
}

#[cfg(target_os = "linux")]
async fn check_linux_dependencies() -> Result<Vec<DependencyStatus>> {
    let avahi = check_avahi_status().await;

    #[cfg(not(feature = "flatpak"))]
    let udev_rules = check_udev_rules_installed().await.unwrap_or_else(|err| {
        warn!("failed to check udev rules: {err:#}");
        Availability::UnableToCheck
    });

    #[allow(unused_mut)]
    let mut dependencies = vec![DependencyStatus {
        id: "avahi",
        name: "Avahi Daemon",
        description: "Required for AirPlay, optional backend for wireless device discovery.",
        optional: true,
        availability: avahi,
        action_text: if avahi == Availability::AvailableButNotRunning {
            "Start"
        } else {
            "Install"
        },
        action_mode: "install",
        documentation_url: "",
    }];

    #[cfg(not(feature = "flatpak"))]
    dependencies.push(DependencyStatus {
        id: "udev_rules",
        name: "UDEV rules",
        description: "Optional USB permissions for devices in recovery mode.",
        optional: true,
        availability: udev_rules,
        action_text: "View Instructions",
        action_mode: "install",
        documentation_url: "",
    });

    Ok(dependencies)
}

#[cfg(target_os = "linux")]
async fn check_avahi_status() -> Availability {
    #[cfg(feature = "flatpak")]
    return match tokio::task::spawn_blocking(|| {
        cpp!(unsafe [] -> bool as "bool" {
            return idescriptor_is_avahi_available();
        })
    })
    .await
    {
        Ok(true) => Availability::Available,
        Ok(false) => Availability::AvailableButNotRunning,
        Err(err) => {
            warn!("failed to check Avahi over D-Bus: {err}");
            Availability::UnableToCheck
        }
    };

    #[cfg(not(feature = "flatpak"))]
    return linux_service_status("avahi-daemon.service", &["avahi-browse", "avahi-daemon"])
        .await
        .unwrap_or_else(|err| {
            warn!("failed to check avahi: {err:#}");
            Availability::UnableToCheck
        });
}

#[cfg(all(target_os = "linux", not(feature = "flatpak")))]
fn udev_rules_are_supported(content: &str) -> bool {
    content.contains("SUBSYSTEM==\"usb\"")
        && content.contains("ATTR{idVendor}==\"05ac\"")
        && content.contains("MODE=\"0660\"")
        && content.contains("GROUP=\"idevice\"")
}

#[cfg(all(target_os = "linux", not(feature = "flatpak")))]
async fn check_udev_rules_installed() -> Result<Availability> {
    use std::time::Duration;

    let content = match tokio::fs::read_to_string("/etc/udev/rules.d/99-idevice.rules").await {
        Ok(content) => content,
        Err(err) => {
            log::debug!("unable to read idevice udev rules: {err}");
            return Ok(Availability::Unavailable);
        }
    };

    if !udev_rules_are_supported(&content) {
        return Ok(Availability::Unavailable);
    }

    let groups_output = match tokio::time::timeout(
        Duration::from_secs(3),
        tokio::process::Command::new("groups").output(),
    )
    .await
    {
        Ok(Ok(output)) if output.status.success() => output,
        Ok(Ok(output)) => {
            log::debug!("groups command failed with status {}", output.status);
            return Ok(Availability::Unavailable);
        }
        Ok(Err(err)) => {
            log::debug!("failed to run groups command: {err}");
            return Ok(Availability::Unavailable);
        }
        Err(_) => {
            log::debug!("groups command timed out");
            return Ok(Availability::Unavailable);
        }
    };

    let groups = String::from_utf8_lossy(&groups_output.stdout);
    if groups.split_whitespace().any(|group| group == "idevice") {
        Ok(Availability::Available)
    } else {
        Ok(Availability::Unavailable)
    }
}

#[cfg(all(target_os = "linux", not(feature = "flatpak")))]
async fn linux_service_status(service: &str, binaries: &[&str]) -> Result<Availability> {
    if command_success("systemctl", &["is-active", "--quiet", service]).await {
        return Ok(Availability::Available);
    }

    if binaries.iter().any(|binary| executable_in_path(binary)) {
        return Ok(Availability::AvailableButNotRunning);
    }

    Ok(Availability::Unavailable)
}

#[cfg(target_os = "linux")]
async fn install_linux_dependency(dependency_id: &str) -> Result<String> {
    match dependency_id {
        "udev_rules" => {
            Ok("You can read the UDEV.md guide (https://github.com/iDescriptor/iDescriptor/blob/main/UDEV.md) for manual configuration.".to_string())
        }
        "avahi" => {
            if cfg!(feature = "flatpak") {
                bail!("Starting host services is not available in the Flatpak build");
            }

            if executable_in_path("systemctl") && executable_in_path("pkexec") {
                tokio::process::Command::new("pkexec")
                    .args(["systemctl", "enable", "--now", "avahi-daemon.service"])
                    .status()
                    .await
                    .context("Failed to start Avahi with pkexec")?;
                Ok("Avahi start command finished. Refresh the check if the status did not update automatically.".to_string())
            } else {
                Ok(
                    "Install and start the avahi daemon with your system package manager."
                        .to_string(),
                )
            }
        }
        _ => bail!("Unknown dependency: {dependency_id}"),
    }
}

#[cfg(target_os = "windows")]
async fn check_windows_dependencies() -> Result<Vec<DependencyStatus>> {
    let (bonjour, apple_mobile, winfsp) = tokio::join!(
        windows_service_status("Bonjour Service"),
        windows_service_status("Apple Mobile Device Service"),
        windows_service_status("WinFsp.Launcher"),
    );

    let bonjour_action = windows_dependency_action("bonjour", bonjour);
    let apple_action = windows_dependency_action("apple_mobile_device_support", apple_mobile);
    let winfsp_action = windows_dependency_action("winfsp", winfsp);

    Ok(vec![
        DependencyStatus {
            id: "apple_mobile_device_support",
            name: "Apple Mobile Device Support",
            description: "Required for iOS device communication.",
            optional: false,
            availability: apple_mobile,
            action_text: apple_action.0,
            action_mode: apple_action.1,
            documentation_url: apple_action.2,
        },
        DependencyStatus {
            id: "bonjour",
            name: "Bonjour Service",
            description: "Required for AirPlay, optional backend for wireless device discovery.",
            optional: true,
            availability: bonjour,
            action_text: bonjour_action.0,
            action_mode: bonjour_action.1,
            documentation_url: bonjour_action.2,
        },
        DependencyStatus {
            id: "winfsp",
            name: "WinFsp",
            description: "Optional. Required for mounting the device as a drive.",
            optional: true,
            availability: winfsp,
            action_text: winfsp_action.0,
            action_mode: winfsp_action.1,
            documentation_url: winfsp_action.2,
        },
    ])
}

#[cfg(target_os = "windows")]
fn windows_dependency_action(
    _dependency_id: &str,
    _availability: Availability,
) -> (&'static str, &'static str, &'static str) {
    #[cfg(feature = "windows_store")]
    {
        let documentation_url = match _dependency_id {
            "bonjour" => {
                "https://github.com/iDescriptor/iDescriptor/blob/main/docs/WINDOWS_DEPENDENCIES.md#bonjour"
            }
            "apple_mobile_device_support" => {
                "https://github.com/iDescriptor/iDescriptor/blob/main/docs/WINDOWS_DEPENDENCIES.md#apple-mobile-device-support"
            }
            "winfsp" => {
                "https://github.com/iDescriptor/iDescriptor/blob/main/docs/WINDOWS_DEPENDENCIES.md#winfsp"
            }
            _ => "",
        };
        return ("View Instructions", "instructions", documentation_url);
    }

    #[cfg(not(feature = "windows_store"))]
    {
        let action_text = if _availability == Availability::AvailableButNotRunning {
            "Start"
        } else {
            "Install"
        };
        (action_text, "install", "")
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn windows_dependency_actions_match_the_distribution() {
        let dependencies = [
            (
                "bonjour",
                "https://github.com/iDescriptor/iDescriptor/blob/dev/docs/WINDOWS_DEPENDENCIES.md#bonjour",
            ),
            (
                "apple_mobile_device_support",
                "https://github.com/iDescriptor/iDescriptor/blob/dev/docs/WINDOWS_DEPENDENCIES.md#apple-mobile-device-support",
            ),
            (
                "winfsp",
                "https://github.com/iDescriptor/iDescriptor/blob/dev/docs/WINDOWS_DEPENDENCIES.md#winfsp",
            ),
        ];

        for (dependency_id, documentation_url) in dependencies {
            let missing = windows_dependency_action(dependency_id, Availability::Unavailable);
            let stopped =
                windows_dependency_action(dependency_id, Availability::AvailableButNotRunning);

            #[cfg(feature = "windows_store")]
            {
                assert_eq!(
                    missing,
                    ("View Instructions", "instructions", documentation_url)
                );
                assert_eq!(stopped, missing);
            }

            #[cfg(not(feature = "windows_store"))]
            {
                let _ = documentation_url;
                assert_eq!(missing, ("Install", "install", ""));
                assert_eq!(stopped, ("Start", "install", ""));
            }
        }
    }
}

#[cfg(all(test, target_os = "linux", not(feature = "flatpak")))]
mod linux_tests {
    use super::*;

    #[test]
    fn accepts_group_scoped_udev_permissions() {
        let rules = r#"SUBSYSTEM=="usb", ATTR{idVendor}=="05ac", ATTR{idProduct}=="1222|1227|1280|1281|1881", MODE="0660", GROUP="idevice""#;

        assert!(udev_rules_are_supported(rules));
    }

    #[test]
    fn rejects_world_writable_udev_permissions() {
        let rules = r#"SUBSYSTEM=="usb", ATTR{idVendor}=="05ac", MODE="0666", GROUP="idevice""#;

        assert!(!udev_rules_are_supported(rules));
    }
}

#[cfg(target_os = "windows")]
struct ServiceHandle(SC_HANDLE);

#[cfg(target_os = "windows")]
impl Drop for ServiceHandle {
    fn drop(&mut self) {
        unsafe {
            CloseServiceHandle(self.0);
        }
    }
}

#[cfg(target_os = "windows")]
async fn windows_service_status(service: &str) -> Availability {
    let service_name = service.to_string();
    let service_to_query = service_name.clone();

    match tokio::task::spawn_blocking(move || windows_service_status_native(&service_to_query))
        .await
    {
        Ok(status) => status,
        Err(err) => {
            error!("Windows service check task failed for {service_name}: {err}");
            Availability::UnableToCheck
        }
    }
}

#[cfg(target_os = "windows")]
fn windows_service_status_native(service: &str) -> Availability {
    let service_name: Vec<u16> = service.encode_utf16().chain(Some(0)).collect();

    unsafe {
        let manager = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT);
        if manager.is_null() {
            let error_code = GetLastError();
            warn!("Unable to open the Windows Service Control Manager: {error_code}");
            return Availability::UnableToCheck;
        }
        let _manager = ServiceHandle(manager);

        let service_handle = OpenServiceW(manager, service_name.as_ptr(), SERVICE_QUERY_STATUS);
        if service_handle.is_null() {
            let error_code = GetLastError();
            if error_code == ERROR_SERVICE_DOES_NOT_EXIST {
                log::debug!("Windows service is not installed: {service}");
                return Availability::Unavailable;
            }

            warn!("Unable to open Windows service {service}: {error_code}");
            return Availability::UnableToCheck;
        }
        let _service = ServiceHandle(service_handle);

        let mut status = MaybeUninit::<SERVICE_STATUS_PROCESS>::zeroed();
        let mut bytes_needed = 0_u32;
        if QueryServiceStatusEx(
            service_handle,
            SC_STATUS_PROCESS_INFO,
            status.as_mut_ptr().cast(),
            std::mem::size_of::<SERVICE_STATUS_PROCESS>() as u32,
            &mut bytes_needed,
        ) == 0
        {
            let error_code = GetLastError();
            warn!("Unable to query Windows service {service}: {error_code}");
            return Availability::UnableToCheck;
        }

        if status.assume_init().dwCurrentState == SERVICE_RUNNING {
            Availability::Available
        } else {
            Availability::AvailableButNotRunning
        }
    }
}

#[cfg(all(target_os = "windows", not(feature = "windows_store")))]
async fn install_windows_dependency(dependency_id: &str) -> Result<String> {
    match dependency_id {
        "bonjour" => {
            if windows_service_status("Bonjour Service").await
                == Availability::AvailableButNotRunning
            {
                start_windows_service("Bonjour Service").await?;
                Ok("Bonjour Service started successfully.".to_string())
            } else {
                run_bundled_elevated_script("install-bonjour.ps1")
                    .await
                    .map(|_| "Bonjour installation completed.".to_string())
            }
        }
        "apple_mobile_device_support" => {
            if windows_service_status("Apple Mobile Device Service").await
                == Availability::AvailableButNotRunning
            {
                start_windows_service("Apple Mobile Device Service").await?;
                Ok("Apple Mobile Device Service started successfully.".to_string())
            } else {
                run_bundled_elevated_script("install-apple-drivers.ps1")
                    .await
                    .map(|_| "Apple Mobile Device Support installation completed.".to_string())
            }
        }
        "winfsp" => {
            if windows_service_status("WinFsp.Launcher").await
                == Availability::AvailableButNotRunning
            {
                start_windows_service("WinFsp.Launcher").await?;
                Ok("WinFsp service started successfully.".to_string())
            } else {
                run_bundled_elevated_script("install-win-fsp.silent.bat")
                    .await
                    .map(|_| "WinFsp installation completed.".to_string())
            }
        }
        _ => bail!("Unknown dependency: {dependency_id}"),
    }
}

// Bonjour installation is handled by install-bonjour.ps1 for direct distributions.
#[cfg(any())]
async fn install_bonjour() -> Result<String> {
    use md5::{Digest, Md5};

    const BONJOUR_URL: &str =
        "https://github.com/tempx-x/bonjour-sdk/raw/refs/heads/main/bonjoursdksetup.exe";
    const BONJOUR_MD5: &str = "4ff2aae8205aec31b06743782cfcadce";

    log::info!("downloading Bonjour SDK installer");
    let bytes = reqwest::get(BONJOUR_URL)
        .await
        .context("Failed to start Bonjour download")?
        .bytes()
        .await
        .context("Failed to read Bonjour download")?;

    let digest = format!("{:x}", Md5::digest(&bytes));
    if digest != BONJOUR_MD5 {
        bail!("Bonjour installer checksum mismatch");
    }

    let temp_dir =
        std::env::temp_dir().join(format!("idescriptor-bonjour-{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&temp_dir)
        .await
        .with_context(|| format!("Failed to create {}", temp_dir.display()))?;

    let exe_path = temp_dir.join("bonjoursdksetup.exe");
    let msi_path = temp_dir.join("Bonjour64.msi");
    tokio::fs::write(&exe_path, bytes)
        .await
        .with_context(|| format!("Failed to write {}", exe_path.display()))?;

    let entries =
        compress_tools::tokio_support::list_archive_files(tokio::fs::File::open(&exe_path).await?)
            .await
            .context("Failed to inspect Bonjour installer archive")?;
    let msi_entry = entries
        .into_iter()
        .find(|entry| entry.to_ascii_lowercase().ends_with("bonjour64.msi"))
        .ok_or_else(|| anyhow!("Bonjour64.msi was not found inside the installer"))?;

    let source = tokio::fs::File::open(&exe_path).await?;
    let target = tokio::fs::File::create(&msi_path).await?;
    compress_tools::tokio_support::uncompress_archive_file(source, target, &msi_entry)
        .await
        .context("Failed to extract Bonjour64.msi")?;

    run_powershell_elevated(&format!(
        "& \"$env:SystemRoot\\System32\\msiexec.exe\" /i '{}'; if ($LASTEXITCODE -notin @(0, 1641, 3010)) {{ exit $LASTEXITCODE }}; exit 0",
        powershell_quote_path(&msi_path)
    ))
    .await?;

    Ok("Bonjour installation completed.".to_string())
}

#[cfg(all(target_os = "windows", not(feature = "windows_store")))]
async fn run_bundled_elevated_script(script_name: &str) -> Result<()> {
    let exe_dir = std::env::current_exe()
        .context("Failed to locate current executable")?
        .parent()
        .ok_or_else(|| anyhow!("Failed to locate application directory"))?
        .to_path_buf();
    let script_path = exe_dir.join(script_name);
    if !script_path.exists() {
        bail!("Installer script was not found: {}", script_path.display());
    }

    let command = if script_name.ends_with(".ps1") {
        format!("& '{}'", powershell_quote_path(&script_path))
    } else {
        format!(
            "& '{}'; exit $LASTEXITCODE",
            powershell_quote_path(&script_path)
        )
    };

    run_powershell_elevated(&command).await
}

#[cfg(all(target_os = "windows", not(feature = "windows_store")))]
async fn run_powershell_elevated(command: &str) -> Result<()> {
    use base64::Engine;

    let encoded_command = base64::engine::general_purpose::STANDARD.encode(
        command
            .encode_utf16()
            .flat_map(|unit| unit.to_le_bytes())
            .collect::<Vec<_>>(),
    );
    let elevation_command = format!(
        "$process = Start-Process -FilePath 'powershell.exe' -Verb RunAs -Wait -PassThru -ArgumentList @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-EncodedCommand', '{encoded_command}'); exit $process.ExitCode"
    );

    let status = tokio::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &elevation_command,
        ])
        .status()
        .await
        .context("Failed to launch elevated PowerShell command")?;
    if status.success() {
        Ok(())
    } else {
        bail!("Elevated command failed with status {status}");
    }
}

#[cfg(all(target_os = "windows", not(feature = "windows_store")))]
async fn start_windows_service(service: &str) -> Result<()> {
    let service = service.replace('\'', "''");
    run_powershell_elevated(&format!(
        "$ErrorActionPreference = 'Stop'; Set-Service -Name '{service}' -StartupType Automatic; Start-Service -Name '{service}'"
    ))
    .await
}

#[cfg(all(target_os = "windows", not(feature = "windows_store")))]
fn powershell_quote_path(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\'', "''")
}

#[cfg(target_os = "linux")]
async fn command_success(program: &str, args: &[&str]) -> bool {
    tokio::process::Command::new(program)
        .args(args)
        .status()
        .await
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
fn executable_in_path(binary: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };

    std::env::split_paths(&paths).any(|dir| {
        let candidate = dir.join(binary);
        candidate.is_file()
    })
}
