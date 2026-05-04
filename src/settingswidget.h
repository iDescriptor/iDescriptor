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

#ifndef SETTINGSWIDGET_H
#define SETTINGSWIDGET_H

#include <QCheckBox>
#include <QComboBox>
#include <QDialog>
#include <QGroupBox>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QSpinBox>
#include <QWidget>

QT_BEGIN_NAMESPACE
class QEvent;
QT_END_NAMESPACE

class SettingsWidget : public QDialog
{
    Q_OBJECT

public:
    explicit SettingsWidget(QWidget *parent = nullptr);

protected:
    void changeEvent(QEvent *event) override;

private slots:
    void onBrowseButtonClicked();
    void onCheckUpdatesClicked();
    void onResetToDefaultsClicked();
    void onApplyClicked();
    void onSettingChanged();

private:
    void setupUI();
    void loadSettings();
    void saveSettings();
    void connectSignals();
    void resetToDefaults();
    void retranslateUi();

    // UI Elements
    // General
    QGroupBox *m_generalGroup;
    QLabel *m_downloadPathLabel;
    QLineEdit *m_downloadPathEdit;
    QPushButton *m_browseButton;
    QLabel *m_wirelessFileServerPortLabel;
    QSpinBox *m_wirelessFileServerPort;
    QCheckBox *m_autoUpdateCheck;
    QLabel *m_themeLabel;
    QComboBox *m_themeCombo;
    QLabel *m_languageLabel;
    QComboBox *m_languageCombo;
    QCheckBox *m_autoRaiseWindow;
    QCheckBox *m_switchToNewDevice;
    QCheckBox *m_autoEnableWifiConnections;
#ifndef __APPLE__
    QCheckBox *m_unmount_iFuseDrives;
#endif
    QCheckBox *m_useUnsecureBackend;
    // Device Connection
    QGroupBox *m_deviceGroup;
    QCheckBox *m_autoConnectWirelessDevices;
    QLabel *m_connectionTimeoutLabel;
    QSpinBox *m_connectionTimeout;

    // Security
    QGroupBox *m_securityGroup;

    // Jailbroken
    QGroupBox *m_jailbrokenGroup;
    QLabel *m_defaultJailbrokenRootPasswordLabel;
    QLineEdit *m_defaultJailbrokenRootPassword;

    // Miscellaneous
    QGroupBox *m_miscGroup;
    QLabel *m_iconSizeBaseMultiplierLabel;
    QDoubleSpinBox *m_iconSizeBaseMultiplier;

    // Airplay
    QGroupBox *m_airplayGroup;
    QLabel *m_fpsLabel;
    QComboBox *m_fpsComboBox;
    QCheckBox *m_noHoldCheckbox;

#ifdef __linux__
    QCheckBox *m_useLegacyPortsCheckbox;
    QCheckBox *m_showV4L2CheckBox;
#endif

#ifdef WIN32
    QLabel *m_backDropTypeLabel;
    QComboBox *m_backDropTypeCombo;
    QCheckBox *m_disableMicaCheckBox;
#endif

    // Footer
    QLabel *m_footerLabel;

    // Buttons
    QPushButton *m_checkUpdatesButton;
    QPushButton *m_resetButton;
    QPushButton *m_applyButton;

    bool m_restartRequired = false;
};

#endif // SETTINGSWIDGET_H
