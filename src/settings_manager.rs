// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

use cpp::cpp;
use qmetaobject::prelude::*;
use qttypes::{QStringList, QVariantList};

cpp! {{
    #include <QDebug>
    #include <QDir>
    #include <QLocale>
    #include <QSettings>
    #include <QStandardPaths>
    #include <QStringList>
    #include <QVariantList>
    #include <QVariantMap>
    #include <algorithm>

    static const QString SEEN_DEVICE_PREFIX_RS = QStringLiteral("seenDevices/");

    static QSettings &settings_manager_settings()
    {
        static QSettings settings;
        return settings;
    }

    static QString settings_manager_home_path()
    {
        return QStandardPaths::writableLocation(QStandardPaths::HomeLocation)
            + QStringLiteral("/.idescriptor");
    }

    static const QStringList &settings_manager_supported_languages()
    {
        static const QStringList languages = {
            QStringLiteral("af"), QStringLiteral("ar"), QStringLiteral("ca"),
            QStringLiteral("cs"), QStringLiteral("da"), QStringLiteral("de"),
            QStringLiteral("el"), QStringLiteral("en"), QStringLiteral("es"),
            QStringLiteral("fi"), QStringLiteral("fr"), QStringLiteral("he"),
            QStringLiteral("hu"), QStringLiteral("it"), QStringLiteral("ja"),
            QStringLiteral("ko"), QStringLiteral("nl"), QStringLiteral("no"),
            QStringLiteral("pl"), QStringLiteral("pt"), QStringLiteral("ro"),
            QStringLiteral("ru"), QStringLiteral("sr"), QStringLiteral("sv"),
            QStringLiteral("tr"), QStringLiteral("uk"), QStringLiteral("vi")
        };
        return languages;
    }

    static QString settings_manager_default_language()
    {
        if (QLocale::system().language() == QLocale::Chinese) {
            if (QLocale::system().territory() == QLocale::HongKong
                || QLocale::system().territory() == QLocale::Macao
                || QLocale::system().territory() == QLocale::Taiwan) {
                return QStringLiteral("zh_TW");
            }
            return QStringLiteral("zh_CN");
        }

        const QString language = QLocale::system().name().section(QChar('_'), 0, 0).toLower();
        if (settings_manager_supported_languages().contains(language)) {
            return language;
        }
        return QStringLiteral("en");
    }

    static QString settings_manager_normalize_language(QString language)
    {
        QString normalized = language.trimmed().toLower();
        if (normalized == QStringLiteral("german")) {
            return QStringLiteral("de");
        }
        if (normalized == QStringLiteral("english")) {
            return QStringLiteral("en");
        }
        if (normalized == QStringLiteral("traditional chinese")
            || normalized == QStringLiteral("zh-tw")
            || normalized == QStringLiteral("zh_tw")
            || normalized == QStringLiteral("zh-hant")
            || normalized == QStringLiteral("zh_hant")
            || normalized == QStringLiteral("zh-hk")
            || normalized == QStringLiteral("zh_hk")
            || normalized == QStringLiteral("zh-mo")
            || normalized == QStringLiteral("zh_mo")) {
            return QStringLiteral("zh_TW");
        }
        if (normalized == QStringLiteral("chinese")
            || normalized == QStringLiteral("simplified chinese")
            || normalized == QStringLiteral("zh")
            || normalized == QStringLiteral("zh-cn")
            || normalized == QStringLiteral("zh_hans")
            || normalized == QStringLiteral("zh-hans")) {
            return QStringLiteral("zh_CN");
        }
        if (normalized.startsWith(QStringLiteral("de"))) {
            return QStringLiteral("de");
        }
        if (normalized.startsWith(QStringLiteral("zh_cn"))) {
            return QStringLiteral("zh_CN");
        }

        normalized.replace(QChar('-'), QChar('_'));
        const QString languageCode = normalized.section(QChar('_'), 0, 0);
        if (settings_manager_supported_languages().contains(languageCode)) {
            return languageCode;
        }
        return QStringLiteral("en");
    }
}}

#[cfg(target_os = "macos")]
pub(crate) fn idevice_default_pairing_file(mac_address: &str) -> Option<String> {
    let mac_address = QString::from(mac_address);
    let pairing_file = cpp!(unsafe [mac_address as "QString"] -> QString as "QString" {
        QSettings settings;
        const QString prefix = QStringLiteral("_macos_idevice_");
        const QString exactKey = prefix + mac_address;
        if (settings.contains(exactKey)) {
            return settings.value(exactKey, QString()).toString();
        }

        // handle mac address normalization
        // 44:F2:1B:95:46:06
        // 44-f2-1b-95-46-06
        // 44f21b954606
        auto normalizeMacAddress = [](const QString &value) {
            QString normalized;
            for (const QChar character : value) {
                const QChar lower = character.toLower();
                if ((lower >= QChar('0') && lower <= QChar('9')) ||
                    (lower >= QChar('a') && lower <= QChar('f'))) {
                    normalized.append(lower);
                }
            }
            return normalized;
        };

        const QString normalizedMacAddress = normalizeMacAddress(mac_address);
        for (const QString &key : settings.allKeys()) {
            if (key.startsWith(prefix) &&
                normalizeMacAddress(key.mid(prefix.length())) == normalizedMacAddress) {
                return settings.value(key, QString()).toString();
            }
        }
        return QString();
    });

    let pairing_file = pairing_file.to_string();
    (!pairing_file.is_empty()).then_some(pairing_file)
}

#[cfg(target_os = "macos")]
pub(crate) fn cache_idevice_default_pairing_file(mac_address: &str, pairing_file: &str) {
    let mac_address = QString::from(mac_address);
    let pairing_file = QString::from(pairing_file);
    cpp!(unsafe [mac_address as "QString", pairing_file as "QString"] {
        QSettings settings;
        settings.setValue(QStringLiteral("_macos_idevice_") + mac_address, pairing_file);
        settings.sync();
    });
}

#[allow(non_snake_case)]
#[derive(QObject, Default)]
pub struct SettingsManager {
    base: qt_base_class!(trait QObject),

    favoritePlacesChanged: qt_signal!(),
    recentLocationsChanged: qt_signal!(),

    clear: qt_method!(fn(&self)),
    home_path: qt_method!(fn(&self) -> QString),
    dev_disk_img_path: qt_method!(fn(&self) -> QString),
    set_dev_disk_img_path: qt_method!(fn(&self, path: QString)),
    mk_dev_disk_img_path: qt_method!(fn(&self) -> QString),
    ipa_download_path: qt_method!(fn(&self) -> QString),
    set_ipa_download_path: qt_method!(fn(&self, path: QString)),
    mk_ipa_download_path: qt_method!(fn(&self) -> QString),
    last_gallery_export_folder: qt_method!(fn(&self) -> QString),
    set_last_gallery_export_folder: qt_method!(fn(&self, folder: QString)),
    backup_root_path: qt_method!(fn(&self) -> QString),
    set_backup_root_path: qt_method!(fn(&self, path: QString)),
    mk_backup_root_path: qt_method!(fn(&self) -> QString),
    clear_keys: qt_method!(fn(&mut self, key_prefix: QString)),

    save_favorite_place:
        qt_method!(fn(&mut self, path: QString, alias: QString, key_prefix: QString)),
    remove_favorite_place: qt_method!(fn(&mut self, key_prefix: QString, path: QString)),
    get_favorite_places: qt_method!(fn(&self, key_prefix: QString) -> QVariantList),

    save_recent_location:
        qt_method!(fn(&mut self, latitude: QString, longitude: QString, name: QString)),
    get_recent_locations: qt_method!(fn(&self) -> QVariantList),
    clear_recent_locations: qt_method!(fn(&self)),

    auto_check_updates: qt_method!(fn(&self) -> bool),
    set_auto_check_updates: qt_method!(fn(&self, enabled: bool)),
    z_linux_window: qt_method!(fn(&self) -> bool),
    set_z_linux_window: qt_method!(fn(&self, enabled: bool)),
    auto_raise_window: qt_method!(fn(&self) -> bool),
    set_auto_raise_window: qt_method!(fn(&self, enabled: bool)),
    switch_to_new_device: qt_method!(fn(&self) -> bool),
    set_switch_to_new_device: qt_method!(fn(&self, enabled: bool)),
    auto_connect_wireless_devices: qt_method!(fn(&self) -> bool),
    set_auto_connect_wireless_devices: qt_method!(fn(&self, enabled: bool)),
    auto_enable_wifi_connections: qt_method!(fn(&self) -> bool),
    set_auto_enable_wifi_connections: qt_method!(fn(&self, enabled: bool)),
    upgrade_to_wireless_on_disconnect: qt_method!(fn(&self) -> bool),
    set_upgrade_to_wireless_on_disconnect: qt_method!(fn(&self, enabled: bool)),
    unmount_ifuse_on_exit: qt_method!(fn(&self) -> bool),
    set_unmount_ifuse_on_exit: qt_method!(fn(&self, enabled: bool)),
    gallery_backend: qt_method!(fn(&self) -> i32),
    set_gallery_backend: qt_method!(fn(&self, backend: i32)),
    network_discovery_backend: qt_method!(fn(&self) -> QString),
    set_network_discovery_backend: qt_method!(fn(&self, backend: QString)),
    theme: qt_method!(fn(&self) -> QString),
    set_theme: qt_method!(fn(&self, theme: QString)),
    window_effect: qt_method!(fn(&self) -> QString),
    set_window_effect: qt_method!(fn(&self, effect: QString)),
    is_window_effect_choice_presented: qt_method!(fn(&self) -> bool),
    set_is_window_effect_choice_presented: qt_method!(fn(&self, presented: bool)),
    language: qt_method!(fn(&self) -> QString),
    set_language: qt_method!(fn(&self, language: QString)),
    connection_timeout: qt_method!(fn(&self) -> i32),
    set_connection_timeout: qt_method!(fn(&self, seconds: i32)),
    wireless_file_server_port: qt_method!(fn(&self) -> i32),
    set_wireless_file_server_port: qt_method!(fn(&self, port: i32)),
    show_keychain_dialog: qt_method!(fn(&self) -> bool),
    set_show_keychain_dialog: qt_method!(fn(&self, show: bool)),
    default_jailbroken_root_password: qt_method!(fn(&self) -> QString),
    set_default_jailbroken_root_password: qt_method!(fn(&self, password: QString)),
    reset_to_defaults: qt_method!(fn(&self)),

    current_version: qt_method!(fn(&self) -> QString),
    build_description: qt_method!(fn(&self) -> QString),
    app_version: qt_method!(fn(&self) -> QString),
    set_app_version: qt_method!(fn(&self, version: QString)),
    airplay_fps: qt_method!(fn(&self) -> i32),
    set_airplay_fps: qt_method!(fn(&self, fps: i32)),
    airplay_width: qt_method!(fn(&self) -> i32),
    set_airplay_width: qt_method!(fn(&self, width: i32)),
    airplay_height: qt_method!(fn(&self) -> i32),
    set_airplay_height: qt_method!(fn(&self, height: i32)),
    airplay_refresh_rate: qt_method!(fn(&self) -> i32),
    set_airplay_refresh_rate: qt_method!(fn(&self, refresh_rate: i32)),
    airplay_h265: qt_method!(fn(&self) -> bool),
    set_airplay_h265: qt_method!(fn(&self, enabled: bool)),
    airplay_overscanned: qt_method!(fn(&self) -> bool),
    set_airplay_overscanned: qt_method!(fn(&self, enabled: bool)),
    airplay_audio_start_on_launch: qt_method!(fn(&self) -> bool),
    set_airplay_audio_start_on_launch: qt_method!(fn(&self, enabled: bool)),
    airplay_no_hold: qt_method!(fn(&self) -> bool),
    set_airplay_no_hold: qt_method!(fn(&self, no_hold: bool)),
    airplay_use_legacy_ports: qt_method!(fn(&self) -> bool),
    set_airplay_use_legacy_ports: qt_method!(fn(&self, enabled: bool)),
    show_v4l2: qt_method!(fn(&self) -> bool),
    set_show_v4l2: qt_method!(fn(&self, show: bool)),

    set_idevice_default_pairing_file:
        qt_method!(fn(&self, mac_address: QString, pairing_file: QString)),
    get_idevice_default_pairing_file: qt_method!(fn(&self, mac_address: QString) -> QString),
    get_all_idevice_default_pairing_files: qt_method!(fn(&self) -> QVariantList),

    is_sleepy_device_warning_dismissed: qt_method!(fn(&self) -> bool),
    set_is_sleepy_device_warning_dismissed: qt_method!(fn(&self, dismissed: bool)),
    dismiss_sleepy_device_warning: qt_method!(fn(&self)),
    backup_experimental_warning_acknowledged: qt_method!(fn(&self) -> bool),
    set_backup_experimental_warning_acknowledged: qt_method!(fn(&self, acknowledged: bool)),
    local_network_onboarding_shown: qt_method!(fn(&self) -> bool),
    set_local_network_onboarding_shown: qt_method!(fn(&self, shown: bool)),
    has_seen_device: qt_method!(fn(&self, udid: QString) -> bool),
    set_has_seen_device: qt_method!(fn(&self, udid: QString, seen: bool)),
    seen_device_udids: qt_method!(fn(&self) -> QStringList),
    clear_seen_devices: qt_method!(fn(&self)),
}

#[allow(non_snake_case)]
impl SettingsManager {
    pub(crate) fn unmount_ifuse_on_exit_enabled() -> bool {
        read_bool("unmountiFuseOnExit", false)
    }

    pub(crate) fn airplay_fps_value() -> u8 {
        read_i32("airplayFps", 60).clamp(1, u8::MAX as i32) as u8
    }

    pub(crate) fn airplay_width_value() -> u16 {
        read_i32("airplayWidth", 1920).clamp(1, u16::MAX as i32) as u16
    }

    pub(crate) fn airplay_height_value() -> u16 {
        read_i32("airplayHeight", 1080).clamp(1, u16::MAX as i32) as u16
    }

    pub(crate) fn airplay_refresh_rate_value() -> u8 {
        read_i32("airplayRefreshRate", 60).clamp(1, u8::MAX as i32) as u8
    }

    pub(crate) fn airplay_h265_enabled() -> bool {
        read_bool("airplayH265", false)
    }

    pub(crate) fn airplay_overscanned_enabled() -> bool {
        read_bool("airplayOverscanned", false)
    }

    pub(crate) fn airplay_use_legacy_ports_enabled() -> bool {
        read_bool("airplayUseLegacyPorts", true)
    }

    pub(crate) fn airplay_audio_start_on_launch_enabled() -> bool {
        read_bool("airplayAudioStartOnLaunch", false)
    }

    pub fn clear_all() {
        cpp!(unsafe [] {
            auto &settings = settings_manager_settings();
            settings.clear();
            settings.sync();
        });
    }

    fn clear(&self) {
        Self::clear_all();
    }

    fn home_path(&self) -> QString {
        cpp!(unsafe [] -> QString as "QString" {
            return settings_manager_home_path();
        })
    }

    fn dev_disk_img_path(&self) -> QString {
        cpp!(unsafe [] -> QString as "QString" {
            auto &settings = settings_manager_settings();
            return settings.value(
                QStringLiteral("devdiskimgpath"),
                settings_manager_home_path() + QStringLiteral("/devdiskimages")
            ).toString();
        })
    }

    fn set_dev_disk_img_path(&self, path: QString) {
        cpp!(unsafe [path as "QString"] {
            auto &settings = settings_manager_settings();
            settings.setValue(QStringLiteral("devdiskimgpath"), path);
            settings.sync();
        });
    }

    fn mk_dev_disk_img_path(&self) -> QString {
        cpp!(unsafe [] -> QString as "QString" {
            auto &settings = settings_manager_settings();
            QString path = settings.value(
                QStringLiteral("devdiskimgpath"),
                settings_manager_home_path() + QStringLiteral("/devdiskimages")
            ).toString();
            QDir dir(path);
            if (!dir.exists()) {
                dir.mkpath(path);
            }
            return path;
        })
    }

    pub(crate) fn ipa_download_path(&self) -> QString {
        cpp!(unsafe [] -> QString as "QString" {
            auto &settings = settings_manager_settings();
            return settings.value(
                QStringLiteral("ipaDownloadPath"),
                settings_manager_home_path() + QStringLiteral("/ipa")
            ).toString();
        })
    }

    fn set_ipa_download_path(&self, path: QString) {
        cpp!(unsafe [path as "QString"] {
            auto &settings = settings_manager_settings();
            settings.setValue(QStringLiteral("ipaDownloadPath"), path);
            settings.sync();
        });
    }

    fn mk_ipa_download_path(&self) -> QString {
        cpp!(unsafe [] -> QString as "QString" {
            auto &settings = settings_manager_settings();
            QString path = settings.value(
                QStringLiteral("ipaDownloadPath"),
                settings_manager_home_path() + QStringLiteral("/ipa")
            ).toString();
            QDir dir(path);
            if (!dir.exists()) {
                dir.mkpath(path);
            }
            return path;
        })
    }

    fn last_gallery_export_folder(&self) -> QString {
        read_string("lastGalleryExportFolder", "")
    }

    fn set_last_gallery_export_folder(&self, folder: QString) {
        // Keep the folder URL as supplied by FolderDialog, including URL escaping.
        // Export callers separately normalize it to a local path via QmlUtils.
        write_string("lastGalleryExportFolder", folder);
    }

    fn backup_root_path(&self) -> QString {
        cpp!(unsafe [] -> QString as "QString" {
            auto &settings = settings_manager_settings();
            return settings.value(
                QStringLiteral("backupRootPath"),
                settings_manager_home_path() + QStringLiteral("/backups")
            ).toString();
        })
    }

    fn set_backup_root_path(&self, path: QString) {
        cpp!(unsafe [path as "QString"] {
            auto &settings = settings_manager_settings();
            settings.setValue(QStringLiteral("backupRootPath"), path);
            settings.sync();
        });
    }

    fn mk_backup_root_path(&self) -> QString {
        cpp!(unsafe [] -> QString as "QString" {
            auto &settings = settings_manager_settings();
            QString path = settings.value(
                QStringLiteral("backupRootPath"),
                settings_manager_home_path() + QStringLiteral("/backups")
            ).toString();
            QDir dir(path);
            if (!dir.exists()) {
                dir.mkpath(path);
            }
            return path;
        })
    }

    fn save_favorite_place(&mut self, path: QString, alias: QString, key_prefix: QString) {
        let changed = cpp!(unsafe [path as "QString", alias as "QString", key_prefix as "QString"] -> bool as "bool" {
            if (path.isEmpty() || alias.isEmpty()) {
                qWarning() << "Cannot save favorite place with empty path or alias";
                return false;
            }

            auto &settings = settings_manager_settings();
            QString key = key_prefix + QString::fromLatin1(path.toUtf8().toBase64());
            settings.setValue(key, QStringList() << path << alias);
            settings.sync();
            qDebug() << "Saved favorite place (AFC2):" << alias << "(" << path << ")";
            return true;
        });

        if changed {
            self.favoritePlacesChanged();
        }
    }

    fn remove_favorite_place(&mut self, key_prefix: QString, path: QString) {
        let changed = cpp!(unsafe [key_prefix as "QString", path as "QString"] -> bool as "bool" {
            auto &settings = settings_manager_settings();
            QString key = key_prefix + QString::fromLatin1(path.toUtf8().toBase64());
            qDebug() << "Attempting to remove favorite place with key:" << key;
            if (!settings.contains(key)) {
                return false;
            }
            settings.remove(key);
            settings.sync();
            qDebug() << "Removed favorite place:" << path;
            return true;
        });

        if changed {
            self.favoritePlacesChanged();
        }
    }

    fn get_favorite_places(&self, key_prefix: QString) -> QVariantList {
        cpp!(unsafe [key_prefix as "QString"] -> QVariantList as "QVariantList" {
            auto &settings = settings_manager_settings();
            QList<QPair<QString, QString>> favorites;
            QStringList favoriteKeys = settings.allKeys().filter(key_prefix);

            qDebug() << "Found favorite keys:" << favoriteKeys;

            for (const QString &key : favoriteKeys) {
                QStringList value = settings.value(key).toStringList();
                if (value.size() >= 2) {
                    QString path = value[0];
                    QString alias = value[1];
                    if (!path.isEmpty() && !alias.isEmpty()) {
                        favorites.append(qMakePair(path, alias));
                        qDebug() << "Loaded favorite:" << alias << "->" << path;
                    }
                }
            }

            std::sort(
                favorites.begin(), favorites.end(),
                [](const QPair<QString, QString> &a, const QPair<QString, QString> &b) {
                    return a.second.toLower() < b.second.toLower();
                });

            QVariantList result;
            for (const auto &favorite : favorites) {
                QVariantMap item;
                item.insert(QStringLiteral("path"), favorite.first);
                item.insert(QStringLiteral("alias"), favorite.second);
                result.append(item);
            }
            return result;
        })
    }

    fn clear_keys(&mut self, key_prefix: QString) {
        cpp!(unsafe [key_prefix as "QString"] {
            auto &settings = settings_manager_settings();
            QStringList favoriteKeys = settings.allKeys().filter(key_prefix);
            for (const QString &key : favoriteKeys) {
                settings.remove(key);
            }
            settings.sync();
        });
        self.favoritePlacesChanged();
    }

    fn save_recent_location(&mut self, latitude: QString, longitude: QString, name: QString) {
        cpp!(unsafe [latitude as "QString", longitude as "QString", name as "QString"] {
            Q_UNUSED(name);

            auto &settings = settings_manager_settings();
            QVariantList recentLocations = settings.value(QStringLiteral("recentLocations")).toList();

            QVariantMap newLocation;
            newLocation.insert(QStringLiteral("latitude"), latitude);
            newLocation.insert(QStringLiteral("longitude"), longitude);

            recentLocations.prepend(newLocation);
            while (recentLocations.size() > 10) {
                recentLocations.removeLast();
            }

            settings.setValue(QStringLiteral("recentLocations"), recentLocations);
            settings.sync();
            qDebug() << "Saved recent location:" << latitude << "," << longitude;
        });
        self.recentLocationsChanged();
    }

    fn get_recent_locations(&self) -> QVariantList {
        cpp!(unsafe [] -> QVariantList as "QVariantList" {
            auto &settings = settings_manager_settings();
            return settings.value(QStringLiteral("recentLocations")).toList();
        })
    }

    fn clear_recent_locations(&self) {
        cpp!(unsafe [] {
            auto &settings = settings_manager_settings();
            settings.remove(QStringLiteral("recentLocations"));
            settings.sync();
        });
    }

    fn auto_check_updates(&self) -> bool {
        read_bool("autoCheckUpdates", true)
    }

    fn set_auto_check_updates(&self, enabled: bool) {
        write_bool("autoCheckUpdates", enabled);
    }

    pub fn z_linux_window(&self) -> bool {
        read_bool("ZLinuxWindow", false)
    }

    fn set_z_linux_window(&self, enabled: bool) {
        write_bool("ZLinuxWindow", enabled);
    }

    fn auto_raise_window(&self) -> bool {
        read_bool("autoRaiseWindow", true)
    }

    fn set_auto_raise_window(&self, enabled: bool) {
        write_bool("autoRaiseWindow", enabled);
    }

    fn switch_to_new_device(&self) -> bool {
        read_bool("switchToNewDevice", true)
    }

    fn set_switch_to_new_device(&self, enabled: bool) {
        write_bool("switchToNewDevice", enabled);
    }

    fn auto_connect_wireless_devices(&self) -> bool {
        read_bool("autoConnectWirelessDevices", true)
    }

    fn set_auto_connect_wireless_devices(&self, enabled: bool) {
        write_bool("autoConnectWirelessDevices", enabled);
    }

    fn auto_enable_wifi_connections(&self) -> bool {
        read_bool("autoEnableWifiConnections", true)
    }

    fn set_auto_enable_wifi_connections(&self, enabled: bool) {
        write_bool("autoEnableWifiConnections", enabled);
    }

    fn upgrade_to_wireless_on_disconnect(&self) -> bool {
        read_bool("upgradeToWirelessOnDisconnect", true)
    }

    fn set_upgrade_to_wireless_on_disconnect(&self, enabled: bool) {
        write_bool("upgradeToWirelessOnDisconnect", enabled);
    }

    fn unmount_ifuse_on_exit(&self) -> bool {
        Self::unmount_ifuse_on_exit_enabled()
    }

    fn set_unmount_ifuse_on_exit(&self, enabled: bool) {
        write_bool("unmountiFuseOnExit", enabled);
    }

    fn gallery_backend(&self) -> i32 {
        read_i32("galleryBackend", 1).clamp(0, 2)
    }

    fn set_gallery_backend(&self, backend: i32) {
        write_i32("galleryBackend", backend.clamp(0, 2));
    }

    pub fn network_discovery_backend(&self) -> QString {
        normalize_network_discovery_backend(read_string("networkDiscoveryBackend", "auto"))
        // normalize_network_discovery_backend(read_string("networkDiscoveryBackend", "pure_rust"))
    }

    fn set_network_discovery_backend(&self, backend: QString) {
        Self::set_network_discovery_backend_value(backend);
    }

    pub(crate) fn set_network_discovery_backend_value(backend: QString) {
        write_string(
            "networkDiscoveryBackend",
            normalize_network_discovery_backend(backend),
        );
    }

    fn theme(&self) -> QString {
        read_string("theme", "system")
    }

    fn set_theme(&self, theme: QString) {
        write_string("theme", theme);
    }

    fn window_effect(&self) -> QString {
        normalize_window_effect(read_string("windowEffect", "normal"))
    }

    fn set_window_effect(&self, effect: QString) {
        write_string("windowEffect", normalize_window_effect(effect));
    }

    fn is_window_effect_choice_presented(&self) -> bool {
        read_bool("windowEffectChoicePresented", false)
    }

    fn set_is_window_effect_choice_presented(&self, presented: bool) {
        write_bool("windowEffectChoicePresented", presented);
    }

    pub fn language(&self) -> QString {
        cpp!(unsafe [] -> QString as "QString" {
            auto &settings = settings_manager_settings();
            return settings_manager_normalize_language(settings.value(
                QStringLiteral("language"),
                settings_manager_default_language()
            ).toString());
        })
    }

    fn set_language(&self, language: QString) {
        let language = cpp!(unsafe [language as "QString"] -> QString as "QString" {
            return settings_manager_normalize_language(language);
        });
        write_string("language", language);
    }

    fn connection_timeout(&self) -> i32 {
        read_i32("connectionTimeout", 30)
    }

    fn set_connection_timeout(&self, seconds: i32) {
        write_i32("connectionTimeout", seconds);
    }

    fn wireless_file_server_port(&self) -> i32 {
        wireless_file_server_port() as i32
    }

    fn set_wireless_file_server_port(&self, port: i32) {
        write_i32("wirelessFileServerPort", port);
    }

    fn show_keychain_dialog(&self) -> bool {
        read_bool("showKeychainDialog", true)
    }

    fn set_show_keychain_dialog(&self, show: bool) {
        write_bool("showKeychainDialog", show);
    }

    fn default_jailbroken_root_password(&self) -> QString {
        read_string("defaultJailbrokenRootPassword", "alpine")
    }

    fn set_default_jailbroken_root_password(&self, password: QString) {
        write_string("defaultJailbrokenRootPassword", password);
    }

    fn reset_to_defaults(&self) {
        self.set_last_gallery_export_folder(QString::default());
        self.set_dev_disk_img_path(QString::from(format!(
            "{}/devdiskimages",
            self.home_path().to_string()
        )));
        self.set_backup_root_path(QString::from(format!(
            "{}/backups",
            self.home_path().to_string()
        )));
        self.set_ipa_download_path(QString::from(format!(
            "{}/ipa",
            self.home_path().to_string()
        )));
        self.set_auto_check_updates(true);
        self.set_z_linux_window(false);
        self.set_auto_raise_window(true);
        self.set_switch_to_new_device(true);
        self.set_auto_connect_wireless_devices(true);
        self.set_auto_enable_wifi_connections(true);
        self.set_upgrade_to_wireless_on_disconnect(true);
        self.set_unmount_ifuse_on_exit(false);
        self.set_gallery_backend(1);
        self.set_network_discovery_backend(QString::from("auto"));
        self.set_theme(QString::from("system"));
        self.set_window_effect(QString::from("normal"));
        self.set_language(default_language());
        self.set_connection_timeout(30);
        self.set_show_keychain_dialog(true);
        self.set_default_jailbroken_root_password(QString::from("alpine"));
        self.set_airplay_fps(60);
        self.set_airplay_width(1920);
        self.set_airplay_height(1080);
        self.set_airplay_refresh_rate(60);
        self.set_airplay_h265(false);
        self.set_airplay_overscanned(false);
        self.set_airplay_audio_start_on_launch(false);
        self.set_airplay_no_hold(true);
        self.set_wireless_file_server_port(8080);
        self.set_airplay_use_legacy_ports(true);
        self.set_show_v4l2(false);
        self.set_is_sleepy_device_warning_dismissed(false);
        self.set_backup_experimental_warning_acknowledged(false);
        self.set_local_network_onboarding_shown(false);
    }

    fn current_version(&self) -> QString {
        QString::from(env!("CARGO_PKG_VERSION"))
    }

    fn build_description(&self) -> QString {
        QString::from(crate::updater::build_description())
    }

    fn app_version(&self) -> QString {
        read_string("__APP_VERSION__", "")
    }

    fn set_app_version(&self, version: QString) {
        write_string("__APP_VERSION__", version);
    }

    fn airplay_fps(&self) -> i32 {
        read_i32("airplayFps", 60)
    }

    fn set_airplay_fps(&self, fps: i32) {
        write_i32("airplayFps", fps.clamp(1, u8::MAX as i32));
    }

    fn airplay_width(&self) -> i32 {
        read_i32("airplayWidth", 1920)
    }

    fn set_airplay_width(&self, width: i32) {
        write_i32("airplayWidth", width.clamp(1, u16::MAX as i32));
    }

    fn airplay_height(&self) -> i32 {
        read_i32("airplayHeight", 1080)
    }

    fn set_airplay_height(&self, height: i32) {
        write_i32("airplayHeight", height.clamp(1, u16::MAX as i32));
    }

    fn airplay_refresh_rate(&self) -> i32 {
        read_i32("airplayRefreshRate", 60)
    }

    fn set_airplay_refresh_rate(&self, refresh_rate: i32) {
        write_i32("airplayRefreshRate", refresh_rate.clamp(1, u8::MAX as i32));
    }

    fn airplay_h265(&self) -> bool {
        Self::airplay_h265_enabled()
    }

    fn set_airplay_h265(&self, enabled: bool) {
        write_bool("airplayH265", enabled);
    }

    fn airplay_overscanned(&self) -> bool {
        Self::airplay_overscanned_enabled()
    }

    fn set_airplay_overscanned(&self, enabled: bool) {
        write_bool("airplayOverscanned", enabled);
    }

    fn airplay_audio_start_on_launch(&self) -> bool {
        Self::airplay_audio_start_on_launch_enabled()
    }

    fn set_airplay_audio_start_on_launch(&self, enabled: bool) {
        write_bool("airplayAudioStartOnLaunch", enabled);
    }

    fn airplay_no_hold(&self) -> bool {
        read_bool("airplayNoHold", false)
    }

    fn set_airplay_no_hold(&self, no_hold: bool) {
        write_bool("airplayNoHold", no_hold);
    }

    fn airplay_use_legacy_ports(&self) -> bool {
        read_bool("airplayUseLegacyPorts", true)
    }

    fn set_airplay_use_legacy_ports(&self, enabled: bool) {
        write_bool("airplayUseLegacyPorts", enabled);
    }

    fn show_v4l2(&self) -> bool {
        read_bool("showV4L2", false)
    }

    fn set_show_v4l2(&self, show: bool) {
        write_bool("showV4L2", show);
    }

    //FIXME: we should be using this on macOS
    fn set_idevice_default_pairing_file(&self, mac_address: QString, pairing_file: QString) {
        cpp!(unsafe [mac_address as "QString", pairing_file as "QString"] {
            auto &settings = settings_manager_settings();
            settings.setValue(QStringLiteral("_macos_idevice_") + mac_address, pairing_file);
            settings.sync();
        });
    }

    fn get_idevice_default_pairing_file(&self, mac_address: QString) -> QString {
        cpp!(unsafe [mac_address as "QString"] -> QString as "QString" {
            auto &settings = settings_manager_settings();
            return settings.value(QStringLiteral("_macos_idevice_") + mac_address, QString()).toString();
        })
    }

    fn get_all_idevice_default_pairing_files(&self) -> QVariantList {
        cpp!(unsafe [] -> QVariantList as "QVariantList" {
            auto &settings = settings_manager_settings();
            QVariantList result;
            const QString prefix = QStringLiteral("_macos_idevice_");
            for (const QString &key : settings.allKeys()) {
                if (!key.startsWith(prefix)) {
                    continue;
                }

                QVariantMap item;
                item.insert(QStringLiteral("macAddress"), key.mid(prefix.length()));
                item.insert(QStringLiteral("pairingFile"), settings.value(key, QString()).toString());
                result.append(item);
            }
            return result;
        })
    }

    fn is_sleepy_device_warning_dismissed(&self) -> bool {
        read_bool("sleepyDeviceWarningDismissed", false)
    }

    fn dismiss_sleepy_device_warning(&self) {
        self.set_is_sleepy_device_warning_dismissed(true);
    }

    fn set_is_sleepy_device_warning_dismissed(&self, dismissed: bool) {
        write_bool("sleepyDeviceWarningDismissed", dismissed);
    }

    fn backup_experimental_warning_acknowledged(&self) -> bool {
        read_bool("backupExperimentalWarningAcknowledged", false)
    }

    fn set_backup_experimental_warning_acknowledged(&self, acknowledged: bool) {
        write_bool("backupExperimentalWarningAcknowledged", acknowledged);
    }

    fn local_network_onboarding_shown(&self) -> bool {
        read_bool("localNetworkOnboardingShown", false)
    }

    fn set_local_network_onboarding_shown(&self, shown: bool) {
        write_bool("localNetworkOnboardingShown", shown);
    }

    fn has_seen_device(&self, udid: QString) -> bool {
        cpp!(unsafe [udid as "QString"] -> bool as "bool" {
            const QString trimmed = udid.trimmed();
            if (trimmed.isEmpty()) {
                return false;
            }
            auto &settings = settings_manager_settings();
            return settings.value(SEEN_DEVICE_PREFIX_RS + trimmed, false).toBool();
        })
    }

    fn set_has_seen_device(&self, udid: QString, seen: bool) {
        cpp!(unsafe [udid as "QString", seen as "bool"] {
            const QString trimmed = udid.trimmed();
            if (trimmed.isEmpty()) {
                return;
            }

            auto &settings = settings_manager_settings();
            const QString key = SEEN_DEVICE_PREFIX_RS + trimmed;
            if (seen) {
                settings.setValue(key, true);
            } else {
                settings.remove(key);
            }
            settings.sync();
        });
    }

    fn seen_device_udids(&self) -> QStringList {
        cpp!(unsafe [] -> QStringList as "QStringList" {
            auto &settings = settings_manager_settings();
            QStringList udids;
            for (const QString &key : settings.allKeys()) {
                if (!key.startsWith(SEEN_DEVICE_PREFIX_RS)) {
                    continue;
                }

                if (settings.value(key, false).toBool()) {
                    udids.append(key.mid(SEEN_DEVICE_PREFIX_RS.length()));
                }
            }
            return udids;
        })
    }

    fn clear_seen_devices(&self) {
        cpp!(unsafe [] {
            auto &settings = settings_manager_settings();
            for (const QString &key : settings.allKeys()) {
                if (key.startsWith(SEEN_DEVICE_PREFIX_RS)) {
                    settings.remove(key);
                }
            }
            settings.sync();
        });
    }
}

fn read_bool(key: &str, default_value: bool) -> bool {
    let key = QString::from(key);
    cpp!(unsafe [key as "QString", default_value as "bool"] -> bool as "bool" {
        return settings_manager_settings().value(key, default_value).toBool();
    })
}

#[allow(dead_code)]
fn has_setting(key: &str) -> bool {
    let key = QString::from(key);
    cpp!(unsafe [key as "QString"] -> bool as "bool" {
        return settings_manager_settings().contains(key);
    })
}

pub fn airplay_uxplay_args() -> Vec<String> {
    let fps = read_i32("airplayFps", 60).clamp(1, 255);
    let mut args = vec!["uxplay".to_string(), "-fps".to_string(), fps.to_string()];

    if read_bool("airplayNoHold", true) {
        args.push("-nohold".to_string());
    }

    #[cfg(target_os = "linux")]
    if read_bool("airplayUseLegacyPorts", true) {
        args.push("-p".to_string());
    }

    args
}

pub(crate) fn wireless_file_server_port() -> u16 {
    read_i32("wirelessFileServerPort", 8080).clamp(1, u16::MAX as i32) as u16
}

pub(crate) fn companion_client_instance_id() -> String {
    let current = read_string("companionClientInstanceID", "").to_string();
    if uuid::Uuid::parse_str(&current).is_ok() {
        return current;
    }

    let generated = uuid::Uuid::new_v4().to_string();
    write_string(
        "companionClientInstanceID",
        QString::from(generated.clone()),
    );
    generated
}

fn write_bool(key: &str, value: bool) {
    let key = QString::from(key);
    cpp!(unsafe [key as "QString", value as "bool"] {
        auto &settings = settings_manager_settings();
        settings.setValue(key, value);
        settings.sync();
    });
}

fn read_i32(key: &str, default_value: i32) -> i32 {
    let key = QString::from(key);
    cpp!(unsafe [key as "QString", default_value as "int"] -> i32 as "int" {
        return settings_manager_settings().value(key, default_value).toInt();
    })
}

fn write_i32(key: &str, value: i32) {
    let key = QString::from(key);
    cpp!(unsafe [key as "QString", value as "int"] {
        auto &settings = settings_manager_settings();
        settings.setValue(key, value);
        settings.sync();
    });
}

fn read_string(key: &str, default_value: &str) -> QString {
    let key = QString::from(key);
    let default_value = QString::from(default_value);
    cpp!(unsafe [key as "QString", default_value as "QString"] -> QString as "QString" {
        return settings_manager_settings().value(key, default_value).toString();
    })
}

fn normalize_network_discovery_backend(backend: QString) -> QString {
    let backend = backend.to_string();
    let supported = matches!(backend.as_str(), "auto" | "pure_rust")
        || cfg!(target_os = "linux") && backend == "avahi"
        || cfg!(any(target_os = "windows", target_os = "macos")) && backend == "bonjour";
    if supported {
        QString::from(backend)
    } else {
        log::warn!("unsupported saved network discovery backend '{backend}', using auto");
        QString::from("auto")
    }
}

fn normalize_window_effect(effect: QString) -> QString {
    cpp!(unsafe [effect as "QString"] -> QString as "QString" {
        const QString normalized = effect.trimmed().toLower();
        if (normalized == QStringLiteral("acrylic")) {
            return QStringLiteral("acrylic");
        }
        return QStringLiteral("normal");
    })
}

fn default_language() -> QString {
    cpp!(unsafe [] -> QString as "QString" {
        return settings_manager_default_language();
    })
}

fn write_string(key: &str, value: QString) {
    let key = QString::from(key);
    cpp!(unsafe [key as "QString", value as "QString"] {
        auto &settings = settings_manager_settings();
        settings.setValue(key, value);
        settings.sync();
    });
}
