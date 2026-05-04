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

#ifndef SETTINGSMANAGER_H
#define SETTINGSMANAGER_H

#include <QDialog>
#include <QObject>
#include <QPair>
#include <QSettings>
#include <QString>
#include <QStringList>
#include <functional>

#ifdef WIN32
#include "platform/windows/win_common.h"
#endif

class SettingsManager : public QObject
{
    Q_OBJECT

public:
    static SettingsManager *sharedInstance();

    // Settings keys
    enum class Setting {
        DevDiskImgPath,
        AutoCheckUpdates,
        AutoRaiseWindow,
        SwitchToNewDevice,
        UnmountiFuseOnExit,
        Theme,
        ConnectionTimeout,
        Language,
    };
    static QString homePath();
    QString devdiskimgpath() const;
    void setDevDiskImgPath(const QString &path);
    QString mkDevDiskImgPath() const;
    void clearKeys(const QString &keyPrefix);

    void saveFavoritePlace(const QString &path, const QString &alias,
                           const QString &keyPrefix);
    void removeFavoritePlace(const QString &keyPrefix, const QString &path);
    QList<QPair<QString, QString>>
    getFavoritePlaces(const QString &keyPrefix) const;
    void showSettingsDialog();

    // Recently used locations
    void saveRecentLocation(const QString &latitude, const QString &longitude,
                            const QString &name = QString());
    QList<QVariantMap> getRecentLocations() const;
    void clearRecentLocations();

    bool autoCheckUpdates() const;
    void setAutoCheckUpdates(bool enabled);

    bool autoRaiseWindow() const;
    void setAutoRaiseWindow(bool enabled);

    bool switchToNewDevice() const;
    void setSwitchToNewDevice(bool enabled);

    bool autoConnectWirelessDevices() const;
    void setAutoConnectWirelessDevices(bool enabled);

    bool autoEnableWifiConnections() const;
    void setAutoEnableWifiConnections(bool enabled);

#ifndef __APPLE__
    bool unmountiFuseOnExit() const;
    void setUnmountiFuseOnExit(bool enabled);
#endif
    bool useUnsecureBackend() const;
    void setUseUnsecureBackend(bool enabled);

    QString theme() const;
    void setTheme(const QString &theme);

    QString language() const;
    void setLanguage(const QString &code);

    int connectionTimeout() const;
    void setConnectionTimeout(int seconds);

    int wirelessFileServerPort() const;
    void setWirelessFileServerPort(int port);

    bool showKeychainDialog() const;
    void setShowKeychainDialog(bool show);

    QString defaultJailbrokenRootPassword() const;
    void setDefaultJailbrokenRootPassword(const QString &password);

    // Utility method for conditional execution
    void doIfEnabled(Setting setting, std::function<void()> action);

    // Reset to defaults
    void resetToDefaults();

    void clear();

    QString appVersion();
    void setAppVersion(const QString &version);

    double iconSizeBaseMultiplier() const;
    void setIconSizeBaseMultiplier(double multiplier);

    int airplayFps() const;
    void setAirplayFps(int fps);

    bool airplayNoHold() const;
    void setAirplayNoHold(bool noHold);

#ifdef __linux__
    bool airplayUseLegacyPorts() const;
    void setAirplayUseLegacyPorts(bool enabled);
    bool showV4L2() const;
    void setShowV4L2(bool show);
#endif

#ifdef __APPLE__
    void setIdeviceDefaultPairingFile(const QString &macAddress,
                                      const QString &pairingFile);
    QString getIdeviceDefaultPairingFile(const QString &macAddress) const;
    QMap<QString, QString> getAllIdeviceDefaultPairingFiles() const;
#endif

#ifdef WIN32
    void setWinBackdropType(WIN_BACKDROP type);
    WIN_BACKDROP winBackdropType() const;

    bool disableMica() const;
    void setDisableMica(bool disabled);
#endif

    bool isSleepyDeviceWarningDismissed() const;
    void setIsSleepyDeviceWarningDismissed(bool dismissed);
    void dismissSleepyDeviceWarning();

    bool hasSeenDevice(const QString &udid) const;
    void setHasSeenDevice(const QString &udid, bool seen = true);
    QStringList seenDeviceUdids() const;
    void clearSeenDevices();

signals:
    void favoritePlacesChanged();
    void recentLocationsChanged();
    void languageChanged(const QString &code);

private:
    QDialog *m_dialog;
    explicit SettingsManager(QObject *parent = nullptr);
    QSettings *m_settings;

    static const QString FAVORITE_PREFIX;
};

#endif // SETTINGSMANAGER_H
