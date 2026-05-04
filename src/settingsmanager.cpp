/*
 * iDescriptor: A free and open-source idevice management tool.
 *
 * Copyright (C) 2025 Uncore <https://github.com/uncor3>
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published
 * by the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program. If not, see <https://www.gnu.org/licenses/>.
 */

#include "settingsmanager.h"
#include "settingswidget.h"
#include <QDebug>
#include <QDir>
#include <QSettings>
#include <QStandardPaths>

#ifdef WIN32
#include "platform/windows/win_common.h"
#endif

const QString SEEN_DEVICE_PREFIX = "seenDevices/";

SettingsManager *SettingsManager::sharedInstance()
{
    static SettingsManager instance;
    return &instance;
}

void SettingsManager::showSettingsDialog()
{
    if (m_dialog) {
        m_dialog->raise();
        m_dialog->activateWindow();
        return;
    }

    m_dialog = new SettingsWidget();
    m_dialog->setWindowTitle(tr("Settings - iDescriptor"));
    m_dialog->setModal(true);
    m_dialog->setAttribute(Qt::WA_DeleteOnClose);
    connect(m_dialog, &QObject::destroyed, [this]() { m_dialog = nullptr; });

    m_dialog->show();
}

SettingsManager::SettingsManager(QObject *parent) : QObject{parent}
{
    m_settings = new QSettings(this);
}

void SettingsManager::clear()
{
    m_settings->clear();
    m_settings->sync();
}

QString SettingsManager::devdiskimgpath() const
{
    return m_settings
        ->value("devdiskimgpath",
                SettingsManager::homePath() + "/devdiskimages")
        .toString();
}

void SettingsManager::setDevDiskImgPath(const QString &path)
{
    m_settings->setValue("devdiskimgpath", path);
    m_settings->sync();
}

QString SettingsManager::mkDevDiskImgPath() const
{
    QString path = devdiskimgpath();
    QDir dir(path);
    if (!dir.exists()) {
        dir.mkpath(path);
        return path;
    }
    return path;
}

QString SettingsManager::homePath()
{
    return QStandardPaths::writableLocation(QStandardPaths::HomeLocation) +
           "/.idescriptor";
}

bool SettingsManager::autoCheckUpdates() const
{
    return m_settings->value("autoCheckUpdates", true).toBool();
}

void SettingsManager::setAutoCheckUpdates(bool enabled)
{
    m_settings->setValue("autoCheckUpdates", enabled);
    m_settings->sync();
}

bool SettingsManager::autoRaiseWindow() const
{
    return m_settings->value("autoRaiseWindow", true).toBool();
}

void SettingsManager::setAutoRaiseWindow(bool enabled)
{
    m_settings->setValue("autoRaiseWindow", enabled);
    m_settings->sync();
}

bool SettingsManager::switchToNewDevice() const
{
    return m_settings->value("switchToNewDevice", true).toBool();
}

void SettingsManager::setSwitchToNewDevice(bool enabled)
{
    m_settings->setValue("switchToNewDevice", enabled);
    m_settings->sync();
}

bool SettingsManager::autoConnectWirelessDevices() const
{
    return m_settings->value("autoConnectWirelessDevices", true).toBool();
}

void SettingsManager::setAutoConnectWirelessDevices(bool enabled)
{
    m_settings->setValue("autoConnectWirelessDevices", enabled);
    m_settings->sync();
}

bool SettingsManager::autoEnableWifiConnections() const
{
    return m_settings->value("autoEnableWifiConnections", true).toBool();
}

void SettingsManager::setAutoEnableWifiConnections(bool enabled)
{
    m_settings->setValue("autoEnableWifiConnections", enabled);
    m_settings->sync();
}

#ifndef __APPLE__
bool SettingsManager::unmountiFuseOnExit() const
{
    return m_settings->value("unmountiFuseOnExit", false).toBool();
}

void SettingsManager::setUnmountiFuseOnExit(bool enabled)
{
    m_settings->setValue("unmountiFuseOnExit", enabled);
    m_settings->sync();
}
#endif

bool SettingsManager::useUnsecureBackend() const
{
    return m_settings->value("useUnsecureBackend-ipatool", false).toBool();
}

void SettingsManager::setUseUnsecureBackend(bool enabled)
{
    m_settings->setValue("useUnsecureBackend-ipatool", enabled);
    m_settings->sync();
}

QString SettingsManager::theme() const
{
    return m_settings->value("theme", "System Default").toString();
}

void SettingsManager::setTheme(const QString &theme)
{
    m_settings->setValue("theme", theme);
    m_settings->sync();
}

QString SettingsManager::language() const
{
    return m_settings->value("language", QString()).toString();
}

void SettingsManager::setLanguage(const QString &code)
{
    if (language() == code) {
        return;
    }
    m_settings->setValue("language", code);
    m_settings->sync();
    emit languageChanged(code);
}

int SettingsManager::connectionTimeout() const
{
    return m_settings->value("connectionTimeout", 30).toInt();
}

void SettingsManager::setConnectionTimeout(int seconds)
{
    m_settings->setValue("connectionTimeout", seconds);
    m_settings->sync();
}

int SettingsManager::wirelessFileServerPort() const
{
    return m_settings->value("wirelessFileServerPort", 8080).toInt();
}

void SettingsManager::setWirelessFileServerPort(int port)
{
    m_settings->setValue("wirelessFileServerPort", port);
    m_settings->sync();
}

bool SettingsManager::showKeychainDialog() const
{
    return m_settings->value("showKeychainDialog", true).toBool();
}

void SettingsManager::setShowKeychainDialog(bool show)
{
    m_settings->setValue("showKeychainDialog", show);
    m_settings->sync();
}

QString SettingsManager::defaultJailbrokenRootPassword() const
{
    return m_settings->value("defaultJailbrokenRootPassword", "alpine")
        .toString();
}

void SettingsManager::setDefaultJailbrokenRootPassword(const QString &password)
{
    m_settings->setValue("defaultJailbrokenRootPassword", password);
    m_settings->sync();
}

void SettingsManager::doIfEnabled(Setting setting, std::function<void()> action)
{
    bool shouldExecute = false;

    switch (setting) {
    case Setting::AutoRaiseWindow:
        shouldExecute = autoRaiseWindow();
        break;
    case Setting::SwitchToNewDevice:
        shouldExecute = switchToNewDevice();
        break;
    case Setting::AutoCheckUpdates:
        shouldExecute = autoCheckUpdates();
        break;
#ifndef __APPLE__
    case Setting::UnmountiFuseOnExit:
        shouldExecute = unmountiFuseOnExit();
        break;
#endif
    default:
        qWarning() << "Unhandled setting in doIfEnabled";
        return;
    }

    qDebug() << "enabled" << switchToNewDevice();
    if (shouldExecute && action) {
        action();
    }
}

void SettingsManager::resetToDefaults()
{
    setDevDiskImgPath(SettingsManager::homePath() + "/devdiskimages");
    setAutoCheckUpdates(true);
    setAutoRaiseWindow(true);
    setSwitchToNewDevice(true);
    setAutoConnectWirelessDevices(true);
    setAutoEnableWifiConnections(true);
#ifndef __APPLE__
    setUnmountiFuseOnExit(false);
#endif
    setUseUnsecureBackend(false);
    setTheme("System Default");
    setConnectionTimeout(30);
    setShowKeychainDialog(true);
    setDefaultJailbrokenRootPassword("alpine");
    setIconSizeBaseMultiplier(1.0);
    setAirplayFps(60);
    setAirplayNoHold(true);
    setWirelessFileServerPort(8080);
#ifdef __linux__
    setAirplayUseLegacyPorts(true);
    setShowV4L2(false);
#endif
    setIsSleepyDeviceWarningDismissed(false);

#ifdef WIN32
    setWinBackdropType(ACRYLIC);
    setDisableMica(false);
#endif
    setLanguage(QString());
}

void SettingsManager::saveFavoritePlace(const QString &path,
                                        const QString &alias,
                                        const QString &keyPrefix)
{
    if (path.isEmpty() || alias.isEmpty()) {
        qWarning() << "Cannot save favorite place with empty path or alias";
        return;
    }

    // Use a key that encodes the path properly
    QString key = keyPrefix + QString::fromLatin1(path.toUtf8().toBase64());
    m_settings->setValue(key, QStringList() << path << alias);
    m_settings->sync();

    qDebug() << "Saved favorite place (AFC2):" << alias << "(" << path << ")";
    emit favoritePlacesChanged();
}

void SettingsManager::removeFavoritePlace(const QString &keyPrefix,
                                          const QString &path)
{
    // Use the same encoding as in saveFavoritePlace
    QString key = keyPrefix + QString::fromLatin1(path.toUtf8().toBase64());
    qDebug() << "Attempting to remove favorite place with key:" << key;
    if (m_settings->contains(key)) {
        m_settings->remove(key);
        m_settings->sync();
        qDebug() << "Removed favorite place:" << path;
        emit favoritePlacesChanged();
    }
}

QList<QPair<QString, QString>>
SettingsManager::getFavoritePlaces(const QString &keyPrefix) const
{
    QList<QPair<QString, QString>> favorites;

    // Get all keys that start with the specified prefix
    QStringList allKeys = m_settings->allKeys();
    QStringList favoriteKeys = allKeys.filter(keyPrefix);

    qDebug() << "Found favorite keys:" << favoriteKeys;

    for (const QString &key : favoriteKeys) {
        QStringList value = m_settings->value(key).toStringList();
        if (value.size() >= 2) {
            QString path = value[0];
            QString alias = value[1];
            if (!path.isEmpty() && !alias.isEmpty()) {
                favorites.append(qMakePair(path, alias));
                qDebug() << "Loaded favorite:" << alias << "->" << path;
            }
        }
    }

    // Sort by alias for consistent ordering
    std::sort(
        favorites.begin(), favorites.end(),
        [](const QPair<QString, QString> &a, const QPair<QString, QString> &b) {
            return a.second.toLower() < b.second.toLower();
        });

    return favorites;
}

void SettingsManager::clearKeys(const QString &keyPrefix)
{
    QStringList allKeys = m_settings->allKeys();
    QStringList favoriteKeys = allKeys.filter(keyPrefix);

    for (const QString &key : favoriteKeys) {
        m_settings->remove(key);
    }

    m_settings->sync();

    emit favoritePlacesChanged();
}

void SettingsManager::saveRecentLocation(const QString &latitude,
                                         const QString &longitude,
                                         const QString &name)
{
    // Get existing recent locations
    QList<QVariantMap> recentLocations = getRecentLocations();

    // Create new location entry
    QVariantMap newLocation;
    newLocation["latitude"] = latitude;
    newLocation["longitude"] = longitude;

    // Add to front of list
    recentLocations.prepend(newLocation);

    // Keep only last 10 locations
    while (recentLocations.size() > 10) {
        recentLocations.removeLast();
    }

    // Save to settings
    QVariantList variantList;
    for (const QVariantMap &loc : recentLocations) {
        variantList.append(loc);
    }

    m_settings->setValue("recentLocations", variantList);
    m_settings->sync();

    qDebug() << "Saved recent location:" << latitude << "," << longitude;

    emit recentLocationsChanged();
}

QList<QVariantMap> SettingsManager::getRecentLocations() const
{
    QList<QVariantMap> recentLocations;

    QVariantList variantList = m_settings->value("recentLocations").toList();

    for (const QVariant &item : variantList) {
        if (item.canConvert<QVariantMap>()) {
            recentLocations.append(item.toMap());
        }
    }

    return recentLocations;
}

void SettingsManager::clearRecentLocations()
{
    m_settings->remove("recentLocations");
    m_settings->sync();
}

QString SettingsManager::appVersion()
{
    return m_settings->value("__APP_VERSION__", "").toString();
}

void SettingsManager::setAppVersion(const QString &version)
{
    m_settings->setValue("__APP_VERSION__", version);
    m_settings->sync();
}

double SettingsManager::iconSizeBaseMultiplier() const
{
    return m_settings->value("iconSizeBaseMultiplier", 1.0).toDouble();
}

void SettingsManager::setIconSizeBaseMultiplier(double multiplier)
{
    m_settings->setValue("iconSizeBaseMultiplier", multiplier);
    m_settings->sync();
}

int SettingsManager::airplayFps() const
{
    return m_settings->value("airplayFps", 60).toInt();
}

void SettingsManager::setAirplayFps(int fps)
{
    m_settings->setValue("airplayFps", fps);
    m_settings->sync();
}

bool SettingsManager::airplayNoHold() const
{
    return m_settings->value("airplayNoHold", true).toBool();
}
void SettingsManager::setAirplayNoHold(bool noHold)
{
    m_settings->setValue("airplayNoHold", noHold);
    m_settings->sync();
}

#ifdef __linux__
bool SettingsManager::airplayUseLegacyPorts() const
{
    return m_settings->value("airplayUseLegacyPorts", true).toBool();
}

void SettingsManager::setAirplayUseLegacyPorts(bool enabled)
{
    m_settings->setValue("airplayUseLegacyPorts", enabled);
    m_settings->sync();
}

bool SettingsManager::showV4L2() const
{
    return m_settings->value("showV4L2", false).toBool();
}

void SettingsManager::setShowV4L2(bool show)
{
    m_settings->setValue("showV4L2", show);
    m_settings->sync();
}
#endif

/*
    only needed on macOS because
    we cannot read /var/db/lockdown without root perm
    so caching must be implemented
*/
#ifdef __APPLE__
void SettingsManager::setIdeviceDefaultPairingFile(const QString &macAddress,
                                                   const QString &pairingFile)
{
    m_settings->setValue("_macos_idevice_" + macAddress, pairingFile);
    m_settings->sync();
}

QString
SettingsManager::getIdeviceDefaultPairingFile(const QString &macAddress) const
{
    return m_settings->value("_macos_idevice_" + macAddress, "").toString();
}

QMap<QString, QString> SettingsManager::getAllIdeviceDefaultPairingFiles() const
{
    QMap<QString, QString> macAddresses;
    QStringList allKeys = m_settings->allKeys();
    for (const QString &key : allKeys) {
        if (key.startsWith("_macos_idevice_")) {
            QString macAddress = key.mid(QString("_macos_idevice_").length());
            macAddresses[macAddress] = m_settings->value(key, "").toString();
        }
    }
    return macAddresses;
}
#endif

#ifdef WIN32
void SettingsManager::setWinBackdropType(WIN_BACKDROP type)
{
    m_settings->setValue("winBackdropType", static_cast<int>(type));
    m_settings->sync();
}

WIN_BACKDROP SettingsManager::winBackdropType() const
{
    return static_cast<WIN_BACKDROP>(
        m_settings->value("winBackdropType", static_cast<int>(ACRYLIC))
            .toInt());
}

bool SettingsManager::disableMica() const
{
    return m_settings->value("disableMica", false).toBool();
}

void SettingsManager::setDisableMica(bool disabled)
{
    m_settings->setValue("disableMica", disabled);
    m_settings->sync();
}
#endif

bool SettingsManager::isSleepyDeviceWarningDismissed() const
{
    return m_settings->value("sleepyDeviceWarningDismissed", false).toBool();
}

void SettingsManager::dismissSleepyDeviceWarning()
{
    m_settings->setValue("sleepyDeviceWarningDismissed", true);
    m_settings->sync();
}

void SettingsManager::setIsSleepyDeviceWarningDismissed(bool dismissed)
{
    m_settings->setValue("sleepyDeviceWarningDismissed", dismissed);
    m_settings->sync();
}

bool SettingsManager::hasSeenDevice(const QString &udid) const
{
    const QString trimmed = udid.trimmed();
    if (trimmed.isEmpty()) {
        return false;
    }
    return m_settings->value(SEEN_DEVICE_PREFIX + trimmed, false).toBool();
}

void SettingsManager::setHasSeenDevice(const QString &udid, bool seen)
{
    const QString trimmed = udid.trimmed();
    if (trimmed.isEmpty()) {
        return;
    }

    const QString key = SEEN_DEVICE_PREFIX + trimmed;
    if (seen) {
        m_settings->setValue(key, true);
    } else {
        m_settings->remove(key);
    }
    m_settings->sync();
}

QStringList SettingsManager::seenDeviceUdids() const
{
    QStringList udids;
    const QStringList allKeys = m_settings->allKeys();

    for (const QString &key : allKeys) {
        if (!key.startsWith(SEEN_DEVICE_PREFIX)) {
            continue;
        }

        if (m_settings->value(key, false).toBool()) {
            udids.append(key.mid(SEEN_DEVICE_PREFIX.length()));
        }
    }

    return udids;
}

void SettingsManager::clearSeenDevices()
{
    const QStringList allKeys = m_settings->allKeys();
    for (const QString &key : allKeys) {
        if (key.startsWith(SEEN_DEVICE_PREFIX)) {
            m_settings->remove(key);
        }
    }
    m_settings->sync();
}