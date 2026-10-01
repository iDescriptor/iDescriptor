// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

#[cfg(target_os = "linux")]
use crate::{RUNTIME, qt_threading::QtThreading};
use cpp::cpp;
#[cfg(target_os = "linux")]
use log::{debug, warn};
#[cfg(target_os = "linux")]
use macros::QtThreading;
use qmetaobject::prelude::*;

cpp! {{
    #include <QCoreApplication>
    #include <QGuiApplication>
    #include <QMetaObject>
    #include <QPalette>
    #include <QStyleHints>

    namespace {
    bool detectSystemDarkMode()
    {
        if (const auto *application =
                qobject_cast<QGuiApplication *>(QCoreApplication::instance())) {
            const Qt::ColorScheme colorScheme =
                application->styleHints()->colorScheme();
            if (colorScheme != Qt::ColorScheme::Unknown) {
                return colorScheme == Qt::ColorScheme::Dark;
            }

            const QPalette palette = application->palette();
            return palette.color(QPalette::WindowText).lightnessF()
                > palette.color(QPalette::Window).lightnessF();
        }
        return false;
    }
    } // namespace
}}

#[allow(non_snake_case)]
#[cfg_attr(target_os = "linux", derive(QtThreading))]
#[derive(QObject, Default)]
pub struct SystemAppearance {
    base: qt_base_class!(trait QObject),

    dark_mode: qt_property!(bool; NOTIFY darkModeChanged),
    darkModeChanged: qt_signal!(),

    portal_color_scheme: qt_property!(i32; NOTIFY portalColorSchemeChanged),
    portalColorSchemeChanged: qt_signal!(),

    portal_available: qt_property!(bool; NOTIFY portalAvailableChanged),
    portalAvailableChanged: qt_signal!(),

    qt_dark_mode: bool,
    apply_qt_dark_mode: qt_method!(fn(&mut self, dark: bool)),
}

impl SystemAppearance {
    pub fn initialize(&mut self) {
        self.apply_qt_dark_mode(Self::detect_system_dark_mode());
        self.initialize_qt_observers();

        #[cfg(target_os = "linux")]
        self.initialize_portal();
    }

    fn apply_qt_dark_mode(&mut self, dark: bool) {
        self.qt_dark_mode = dark;
        self.update_dark_mode();
    }

    fn detect_system_dark_mode() -> bool {
        cpp!(unsafe [] -> bool as "bool" {
            return detectSystemDarkMode();
        })
    }

    fn initialize_qt_observers(&mut self) {
        let appearance = self.get_cpp_object();
        cpp!(unsafe [appearance as "QObject *"] {
            if (auto *application =
                    qobject_cast<QGuiApplication *>(QCoreApplication::instance())) {
                QObject::connect(application->styleHints(),
                                 &QStyleHints::colorSchemeChanged, appearance,
                                 [appearance]() {
                                     QMetaObject::invokeMethod(
                                         appearance, "apply_qt_dark_mode",
                                         Qt::DirectConnection,
                                         Q_ARG(bool, detectSystemDarkMode()));
                                 });
                QT_WARNING_PUSH
                QT_WARNING_DISABLE_DEPRECATED
                QObject::connect(application, &QGuiApplication::paletteChanged,
                                 appearance, [appearance]() {
                                     QMetaObject::invokeMethod(
                                         appearance, "apply_qt_dark_mode",
                                         Qt::DirectConnection,
                                         Q_ARG(bool, detectSystemDarkMode()));
                                 });
                QT_WARNING_POP
            }
        });
    }

    fn set_portal_state(&mut self, available: bool, color_scheme: i32) {
        let color_scheme = color_scheme.clamp(0, 2);

        if self.portal_available != available {
            self.portal_available = available;
            self.portalAvailableChanged();
        }
        if self.portal_color_scheme != color_scheme {
            self.portal_color_scheme = color_scheme;
            self.portalColorSchemeChanged();
        }
        self.update_dark_mode();
    }

    fn update_dark_mode(&mut self) {
        let dark_mode = match self.portal_color_scheme {
            1 => true,
            2 => false,
            _ => self.qt_dark_mode,
        };
        if self.dark_mode != dark_mode {
            self.dark_mode = dark_mode;
            self.darkModeChanged();
        }
    }

    #[cfg(target_os = "linux")]
    fn initialize_portal(&mut self) {
        let q_thread = self.qt_thread();
        RUNTIME.spawn(async move {
            if let Err(error) = monitor_portal(q_thread.clone()).await {
                warn!("SystemAppearance portal monitor stopped: {error:#}");
                q_thread.queue(|appearance| appearance.set_portal_state(false, 0));
            }
        });
    }
}

#[cfg(target_os = "linux")]
const PORTAL_SERVICE: &str = "org.freedesktop.portal.Desktop";
#[cfg(target_os = "linux")]
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
#[cfg(target_os = "linux")]
const PORTAL_SETTINGS_INTERFACE: &str = "org.freedesktop.portal.Settings";
#[cfg(target_os = "linux")]
const APPEARANCE_NAMESPACE: &str = "org.freedesktop.appearance";
#[cfg(target_os = "linux")]
const COLOR_SCHEME_KEY: &str = "color-scheme";

#[cfg(target_os = "linux")]
async fn monitor_portal(
    q_thread: crate::qt_threading::QtThread<SystemAppearance>,
) -> anyhow::Result<()> {
    use dbus::{
        arg::{RefArg, Variant},
        message::MatchRule,
    };
    use futures::StreamExt;

    let (resource, connection) =
        tokio::task::spawn_blocking(|| dbus_tokio::connection::new_session_sync()).await??;

    let mut resource_task = tokio_util::task::AbortOnDropHandle::new(RUNTIME.spawn(resource));

    let setting_rule = MatchRule::new_signal(PORTAL_SETTINGS_INTERFACE, "SettingChanged")
        .with_sender(PORTAL_SERVICE)
        .with_path(PORTAL_PATH);
    let (setting_match, mut setting_stream) =
        connection
            .add_match(setting_rule)
            .await?
            .stream::<(String, String, Variant<Box<dyn RefArg>>)>();

    let owner_rule = MatchRule::new_signal("org.freedesktop.DBus", "NameOwnerChanged")
        .with_sender("org.freedesktop.DBus")
        .with_path("/org/freedesktop/DBus");
    let (owner_match, mut owner_stream) =
        connection
            .add_match(owner_rule)
            .await?
            .stream::<(String, String, String)>();

    refresh_portal(&connection, q_thread.clone()).await;

    let monitor_result = loop {
        tokio::select! {
            setting = setting_stream.next() => {
                let Some((_message, (name_space, key, Variant(value)))) = setting else {
                    break Err(anyhow::anyhow!("portal SettingChanged stream ended"));
                };
                if name_space == APPEARANCE_NAMESPACE && key == COLOR_SCHEME_KEY {
                    let color_scheme = portal_color_scheme(value.as_ref());
                    q_thread.queue(move |appearance| {
                        appearance.set_portal_state(true, color_scheme);
                    });
                }
            }
            owner = owner_stream.next() => {
                let Some((_message, (name, _old_owner, new_owner))) = owner else {
                    break Err(anyhow::anyhow!("D-Bus NameOwnerChanged stream ended"));
                };
                if name != PORTAL_SERVICE {
                    continue;
                }
                if new_owner.is_empty() {
                    q_thread.queue(|appearance| appearance.set_portal_state(false, 0));
                } else {
                    refresh_portal(&connection, q_thread.clone()).await;
                }
            }
            resource_result = &mut resource_task => {
                let error = match resource_result {
                    Ok(error) => anyhow::anyhow!("lost D-Bus connection: {error}"),
                    Err(error) => anyhow::anyhow!("D-Bus I/O task failed: {error}"),
                };
                break Err(error);
            }
        }
    };

    if let Err(error) = connection.remove_match(setting_match.token()).await {
        debug!("Failed to remove SystemAppearance portal match: {error}");
    }
    if let Err(error) = connection.remove_match(owner_match.token()).await {
        debug!("Failed to remove SystemAppearance service-owner match: {error}");
    }
    monitor_result
}

#[cfg(target_os = "linux")]
async fn refresh_portal(
    connection: &std::sync::Arc<dbus::nonblock::SyncConnection>,
    q_thread: crate::qt_threading::QtThread<SystemAppearance>,
) {
    use dbus::{
        arg::{RefArg, Variant},
        nonblock::Proxy,
    };
    use std::time::Duration;

    let proxy = Proxy::new(
        PORTAL_SERVICE,
        PORTAL_PATH,
        Duration::from_secs(5),
        connection.clone(),
    );
    let result: Result<(Variant<Box<dyn RefArg>>,), dbus::Error> = proxy
        .method_call(
            PORTAL_SETTINGS_INTERFACE,
            "Read",
            (APPEARANCE_NAMESPACE, COLOR_SCHEME_KEY),
        )
        .await;

    match result {
        Ok((Variant(value),)) => {
            let color_scheme = portal_color_scheme(value.as_ref());
            debug!("SystemAppearance portal color scheme: {color_scheme}");
            q_thread.queue(move |appearance| {
                appearance.set_portal_state(true, color_scheme);
            });
        }
        Err(error) => {
            debug!("SystemAppearance portal is unavailable: {error}");
            q_thread.queue(|appearance| appearance.set_portal_state(false, 0));
        }
    }
}

#[cfg(target_os = "linux")]
fn portal_color_scheme(value: &dyn dbus::arg::RefArg) -> i32 {
    value
        .as_u64()
        .filter(|color_scheme| *color_scheme <= 2)
        .map_or(0, |color_scheme| color_scheme as i32)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::portal_color_scheme;
    use dbus::arg::{RefArg, Variant};

    #[test]
    fn unwraps_nested_portal_color_scheme_variant() {
        let color_scheme: Box<dyn RefArg> = Box::new(Variant(Box::new(Variant(
            Box::new(1_u32) as Box<dyn RefArg>
        )) as Box<dyn RefArg>));

        assert_eq!(portal_color_scheme(color_scheme.as_ref()), 1);
    }

    #[test]
    fn rejects_unknown_portal_color_scheme() {
        assert_eq!(portal_color_scheme(&3_u32), 0);
    }
}
