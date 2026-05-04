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

#include "settingswidget.h"
#include "mainwindow.h"
#include "settingsmanager.h"
#include "translationmanager.h"
#include <QCheckBox>
#include <QComboBox>
#include <QDialog>
#include <QEvent>
#include <QFileDialog>
#include <QFrame>
#include <QGroupBox>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QMessageBox>
#include <QPushButton>
#include <QScrollArea>
#include <QSpinBox>
#include <QStandardPaths>
#include <QTimer>
#include <QVBoxLayout>

#ifdef WIN32
#include "platform/windows/win_common.h"
#include <QOperatingSystemVersion>
#endif

// Locale-independent identifiers used as combo userData. They survive
// language changes and are also what gets persisted via SettingsManager,
// so the saved value remains meaningful across locales.
#define IDESC_THEME_SYSTEM_DEFAULT "System Default"
#ifdef WIN32
#define IDESC_BACKDROP_AUTO "Auto"
#endif

SettingsWidget::SettingsWidget(QWidget *parent) : QDialog{parent}
{
#ifdef WIN32
    m_backDropTypeLabel = nullptr;
    m_backDropTypeCombo = nullptr;
    m_disableMicaCheckBox = nullptr;
#endif
    setupUI();
    retranslateUi();
    loadSettings();
    connectSignals();
    // due to scrollbar add 10px on windows
#ifdef WIN32
    resize(sizeHint().width() + 10, sizeHint().height());
    setupWinWindow(this);
#endif
}

void SettingsWidget::setupUI()
{
    auto *mainLayout = new QVBoxLayout(this);
    mainLayout->setContentsMargins(0, 0, 0, 0);
    mainLayout->setSpacing(15);

    // Create scroll area for the settings
    auto *scrollArea = new QScrollArea();
    auto *scrollWidget = new QWidget();
    auto *scrollLayout = new QVBoxLayout(scrollWidget);
    scrollLayout->setSpacing(35);
    scrollLayout->setContentsMargins(10, 10, 10, 10);

    // === GENERAL SETTINGS ===
    m_generalGroup = new QGroupBox();
    auto *generalLayout = new QVBoxLayout(m_generalGroup);

    // Download path
    auto *downloadLayout = new QHBoxLayout();
    m_downloadPathLabel = new QLabel();
    downloadLayout->addWidget(m_downloadPathLabel);
    m_downloadPathEdit = new QLineEdit();
    m_downloadPathEdit->setReadOnly(true);
    m_downloadPathEdit->setMaximumWidth(300);
    downloadLayout->addWidget(m_downloadPathEdit);
    m_browseButton = new QPushButton();
    downloadLayout->addWidget(m_browseButton);
    generalLayout->addLayout(downloadLayout);

    // Wireless file server port
    auto *portLayout = new QHBoxLayout();
    m_wirelessFileServerPortLabel = new QLabel();
    portLayout->addWidget(m_wirelessFileServerPortLabel);
    m_wirelessFileServerPort = new QSpinBox();
    m_wirelessFileServerPort->setRange(1024, 65535);
    portLayout->addWidget(m_wirelessFileServerPort);
    portLayout->addStretch();
    generalLayout->addLayout(portLayout);

    // Unmount iFuse drives on exit (not implemented on macOS)
    // TODO: Implement
#ifndef __APPLE__
    m_unmount_iFuseDrives = new QCheckBox();
    generalLayout->addWidget(m_unmount_iFuseDrives);
#endif

    connect(m_browseButton, &QPushButton::clicked, this,
            &SettingsWidget::onBrowseButtonClicked);

    // Auto-check for updates
    m_autoUpdateCheck = new QCheckBox();
    generalLayout->addWidget(m_autoUpdateCheck);

    m_autoEnableWifiConnections = new QCheckBox();
    generalLayout->addWidget(m_autoEnableWifiConnections);

    // Theme selection
    auto *themeLayout = new QHBoxLayout();
    m_themeLabel = new QLabel();
    themeLayout->addWidget(m_themeLabel);
    m_themeCombo = new QComboBox();

    /* FIXME: Theme control needs to be implemented */
    // userData carries the locale-independent identifier persisted by
    // SettingsManager. Display text is localised in retranslateUi().
    m_themeCombo->addItem(QString(), QStringLiteral(IDESC_THEME_SYSTEM_DEFAULT));

    themeLayout->addWidget(m_themeCombo);
    themeLayout->addStretch();
    generalLayout->addLayout(themeLayout);

    // Language selection
    auto *languageLayout = new QHBoxLayout();
    m_languageLabel = new QLabel();
    languageLayout->addWidget(m_languageLabel);
    m_languageCombo = new QComboBox();
    for (const QString &code : TranslationManager::availableLocaleCodes()) {
        // Display name is filled in retranslateUi(); userData is the locale
        // code, which is what we persist and look up by.
        m_languageCombo->addItem(QString(), code);
    }
    languageLayout->addWidget(m_languageCombo);
    languageLayout->addStretch();
    generalLayout->addLayout(languageLayout);

#ifdef WIN32
    QOperatingSystemVersion osVersion = QOperatingSystemVersion::current();
    if (osVersion >= QOperatingSystemVersion::Windows11) {
        auto *backDropTypeLayout = new QHBoxLayout();
        m_backDropTypeLabel = new QLabel();
        backDropTypeLayout->addWidget(m_backDropTypeLabel);
        m_backDropTypeCombo = new QComboBox();

        // "Auto" => no override; other entries carry the WIN_BACKDROP int.
        m_backDropTypeCombo->addItem(QString(),
                                     QStringLiteral(IDESC_BACKDROP_AUTO));
        m_backDropTypeCombo->addItem(QString(), static_cast<int>(MICA));
        m_backDropTypeCombo->addItem(QString(), static_cast<int>(MICA_ALT));
        m_backDropTypeCombo->addItem(QString(), static_cast<int>(ACRYLIC));

        backDropTypeLayout->addWidget(m_backDropTypeCombo);
        backDropTypeLayout->addStretch();

        generalLayout->addLayout(backDropTypeLayout);

        m_disableMicaCheckBox = new QCheckBox();
        generalLayout->addWidget(m_disableMicaCheckBox);
    }
#endif

    scrollLayout->addWidget(m_generalGroup);

    // === DEVICE CONNECTION SETTINGS ===
    m_deviceGroup = new QGroupBox();
    auto *deviceLayout = new QVBoxLayout(m_deviceGroup);

    m_autoRaiseWindow = new QCheckBox();
    deviceLayout->addWidget(m_autoRaiseWindow);

    m_switchToNewDevice = new QCheckBox();
    deviceLayout->addWidget(m_switchToNewDevice);

    m_autoConnectWirelessDevices = new QCheckBox();
    deviceLayout->addWidget(m_autoConnectWirelessDevices);

    // Connection timeout
    auto *timeoutLayout = new QHBoxLayout();
    m_connectionTimeoutLabel = new QLabel();
    timeoutLayout->addWidget(m_connectionTimeoutLabel);
    m_connectionTimeout = new QSpinBox();
    m_connectionTimeout->setRange(5, 60);
    timeoutLayout->addWidget(m_connectionTimeout);
    timeoutLayout->addStretch();
    deviceLayout->addLayout(timeoutLayout);

    scrollLayout->addWidget(m_deviceGroup);

    // === SECURITY SETTINGS ===
    m_securityGroup = new QGroupBox();
    auto *securityLayout = new QVBoxLayout(m_securityGroup);

    m_useUnsecureBackend = new QCheckBox();
    securityLayout->addWidget(m_useUnsecureBackend);
    scrollLayout->addWidget(m_securityGroup);

    // === JAILBROKEN SETTINGS ===
    m_jailbrokenGroup = new QGroupBox();
    auto *jailbrokenLayout = new QVBoxLayout(m_jailbrokenGroup);

    // Default jailbroken root password
    auto *passwordLayout = new QHBoxLayout();
    m_defaultJailbrokenRootPasswordLabel = new QLabel();
    passwordLayout->addWidget(m_defaultJailbrokenRootPasswordLabel);
    m_defaultJailbrokenRootPassword = new QLineEdit();
    m_defaultJailbrokenRootPassword->setEchoMode(QLineEdit::PasswordEchoOnEdit);
    m_defaultJailbrokenRootPassword->setMaximumWidth(200);
    passwordLayout->addWidget(m_defaultJailbrokenRootPassword);
    passwordLayout->addStretch();
    jailbrokenLayout->addLayout(passwordLayout);

    scrollLayout->addWidget(m_jailbrokenGroup);

    // === AirPlay SETTINGS ===
    m_airplayGroup = new QGroupBox();
    auto *airplayLayout = new QVBoxLayout(m_airplayGroup);

    auto *fpsLayout = new QHBoxLayout();

    m_fpsLabel = new QLabel();
    m_fpsComboBox = new QComboBox();
    // The FPS values are numeric and locale-independent; no tr() needed.
    m_fpsComboBox->addItems({"24", "30", "60", "120"});

    fpsLayout->addWidget(m_fpsLabel);
    fpsLayout->addWidget(m_fpsComboBox);
    fpsLayout->addStretch();
    airplayLayout->addLayout(fpsLayout);

    m_noHoldCheckbox = new QCheckBox();
    airplayLayout->addWidget(m_noHoldCheckbox);

#ifdef __linux__
    m_useLegacyPortsCheckbox = new QCheckBox();
    airplayLayout->addWidget(m_useLegacyPortsCheckbox);

    m_showV4L2CheckBox = new QCheckBox();
    airplayLayout->addWidget(m_showV4L2CheckBox);
#endif

    scrollLayout->addWidget(m_airplayGroup);

    // === MISCELLANEOUS SETTINGS ===
    m_miscGroup = new QGroupBox();
    auto *miscLayout = new QVBoxLayout(m_miscGroup);

    auto *iconSizeBaseMultiplierLayout = new QHBoxLayout();
    m_iconSizeBaseMultiplier = new QDoubleSpinBox();
    m_iconSizeBaseMultiplier->setRange(1.0, 5.0);
    m_iconSizeBaseMultiplier->setSingleStep(0.1);
    m_iconSizeBaseMultiplier->setDecimals(1);
    m_iconSizeBaseMultiplier->setSuffix("x");

    m_iconSizeBaseMultiplierLabel = new QLabel();
    iconSizeBaseMultiplierLayout->addWidget(m_iconSizeBaseMultiplierLabel);
    iconSizeBaseMultiplierLayout->addWidget(m_iconSizeBaseMultiplier);
    iconSizeBaseMultiplierLayout->addStretch();
    miscLayout->addLayout(iconSizeBaseMultiplierLayout);

    scrollLayout->addWidget(m_miscGroup);

    scrollLayout->addSpacing(30);

    // Add a footer Author & Version & app info & app description
    m_footerLabel = new QLabel();
    m_footerLabel->setAlignment(Qt::AlignCenter);
    m_footerLabel->setStyleSheet("color: gray; font-size: 8pt;");
    scrollLayout->addWidget(m_footerLabel);

    // Add stretch to push everything to the top
    scrollLayout->addStretch();

    scrollArea->setWidget(scrollWidget);
    scrollArea->setWidgetResizable(true);
    scrollArea->setFrameStyle(QFrame::NoFrame);
    scrollArea->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);

    // == BUTTONS ===
    auto *buttonLayout = new QHBoxLayout();

    m_checkUpdatesButton = new QPushButton();
    m_resetButton = new QPushButton();
    m_applyButton = new QPushButton();

    buttonLayout->addWidget(m_checkUpdatesButton);
    buttonLayout->addWidget(m_resetButton);
    buttonLayout->addWidget(m_applyButton);
    buttonLayout->setContentsMargins(10, 10, 10, 10);

    mainLayout->addWidget(scrollArea);
    mainLayout->addLayout(buttonLayout);

    // Connect button signals
    connect(m_checkUpdatesButton, &QPushButton::clicked, this,
            &SettingsWidget::onCheckUpdatesClicked);
    connect(m_resetButton, &QPushButton::clicked, this,
            &SettingsWidget::onResetToDefaultsClicked);
    connect(m_applyButton, &QPushButton::clicked, this,
            &SettingsWidget::onApplyClicked);
}

void SettingsWidget::retranslateUi()
{
    setWindowTitle(tr("Settings"));

    // === GENERAL ===
    m_generalGroup->setTitle(tr("General"));
    m_downloadPathLabel->setText(tr("Download Path:"));
    m_browseButton->setText(tr("Browse..."));
    m_wirelessFileServerPortLabel->setText(tr("Wireless File Server Port:"));
    m_wirelessFileServerPort->setToolTip(
        tr("The starting port for the wireless file server. If this port is "
           "unavailable, it will try the next 10 ports."));

#ifndef __APPLE__
    m_unmount_iFuseDrives->setText(tr("Unmount iFuse drives on exit"));
#endif

    m_autoUpdateCheck->setText(tr("Automatically check for updates"));
    m_autoEnableWifiConnections->setText(
        tr("Automatically enable Wi-Fi connections"));

    m_themeLabel->setText(tr("Theme:"));
    for (int i = 0; i < m_themeCombo->count(); ++i) {
        const QString id = m_themeCombo->itemData(i).toString();
        if (id == QLatin1String(IDESC_THEME_SYSTEM_DEFAULT)) {
            m_themeCombo->setItemText(i, tr("System Default"));
        } else {
            // Future-proof fallback: display the identifier itself.
            m_themeCombo->setItemText(i, id);
        }
    }

    m_languageLabel->setText(tr("Language:"));
    for (int i = 0; i < m_languageCombo->count(); ++i) {
        const QString code = m_languageCombo->itemData(i).toString();
        m_languageCombo->setItemText(i, TranslationManager::displayName(code));
    }

#ifdef WIN32
    if (m_backDropTypeLabel) {
        m_backDropTypeLabel->setText(tr("Backdrop Type:"));
    }
    if (m_backDropTypeCombo) {
        for (int i = 0; i < m_backDropTypeCombo->count(); ++i) {
            const QVariant data = m_backDropTypeCombo->itemData(i);
            if (data.typeId() == QMetaType::QString
                && data.toString() == QLatin1String(IDESC_BACKDROP_AUTO)) {
                //: Backdrop type that defers to the platform default.
                m_backDropTypeCombo->setItemText(i, tr("Auto", "Backdrop type"));
            } else {
                const int value = data.toInt();
                if (value == static_cast<int>(MICA)) {
                    m_backDropTypeCombo->setItemText(i, tr("Mica"));
                } else if (value == static_cast<int>(MICA_ALT)) {
                    m_backDropTypeCombo->setItemText(i, tr("Mica Alt"));
                } else if (value == static_cast<int>(ACRYLIC)) {
                    m_backDropTypeCombo->setItemText(i, tr("Acrylic"));
                }
            }
        }
    }
    if (m_disableMicaCheckBox) {
        m_disableMicaCheckBox->setText(
            tr("Disable Mica effects (also disables WinUI styles)"));
    }
#endif

    // === DEVICE CONNECTION ===
    m_deviceGroup->setTitle(tr("Device Connection"));
    m_autoRaiseWindow->setText(
        tr("Auto-raise main window on device connection"));
    m_switchToNewDevice->setText(tr("Switch to newly connected device"));
    m_autoConnectWirelessDevices->setText(
        tr("Automatically connect to wireless devices"));
    m_connectionTimeoutLabel->setText(tr("Connection Timeout:"));
    m_connectionTimeout->setSuffix(tr(" seconds"));

    // === SECURITY ===
    m_securityGroup->setTitle(tr("Security"));
    m_useUnsecureBackend->setText(
        tr("Use unsecure backend for app store (ipatool)"));
    m_useUnsecureBackend->setToolTip(
        tr("Enabling this may put your Apple account at risk but you don't "
           "have to deal with Apple keychain."));

    // === JAILBROKEN ===
    m_jailbrokenGroup->setTitle(tr("Jailbroken"));
    m_defaultJailbrokenRootPasswordLabel->setText(
        tr("Default Jailbroken Root Password:"));
    m_defaultJailbrokenRootPassword->setToolTip(
        tr("Default password used for SSH root authentication on jailbroken "
           "devices: Default is 'alpine'."));

    // === AIRPLAY ===
    m_airplayGroup->setTitle(tr("AirPlay"));
    m_fpsLabel->setText(tr("Fps:"));
    m_fpsComboBox->setToolTip(
        tr("Set the fps for AirPlay. Go with 30 fps if have an older device."));
    m_noHoldCheckbox->setText(tr("Allow New Connections to Take Over"));

#ifdef __linux__
    m_useLegacyPortsCheckbox->setText(tr("Use legacy ports"));
    m_useLegacyPortsCheckbox->setToolTip(
        tr("Use legacy ports, refer to AIRPLAY.md for more information."));
    m_showV4L2CheckBox->setText(tr("Show V4L2 Button on AirPlay Widget"));
#endif

    // === MISCELLANEOUS ===
    m_miscGroup->setTitle(tr("Miscellaneous"));
    m_iconSizeBaseMultiplierLabel->setText(tr("Icon Size Base Multiplier:"));
    m_iconSizeBaseMultiplier->setToolTip(
        tr("Adjust the base multiplier for icon sizes. This affects how large "
           "icons appear throughout the application. Requires restart to take "
           "effect."));

    // === FOOTER ===
    m_footerLabel->setText(
        tr("iDescriptor v%1\n"
           "A free, open-source, and cross-platform iDevice management tool.\n"
           "\xC2\xA9 2026 See AUTHORS for details. Licensed under AGPLv3.")
            .arg(QStringLiteral(APP_VERSION)));

    // === BUTTONS ===
    m_checkUpdatesButton->setText(tr("Check for Updates"));
    m_resetButton->setText(tr("Reset Settings"));
    m_applyButton->setText(tr("Apply"));
}

void SettingsWidget::changeEvent(QEvent *event)
{
    if (event->type() == QEvent::LanguageChange) {
        retranslateUi();
    }
    QDialog::changeEvent(event);
}

void SettingsWidget::loadSettings()
{
    SettingsManager *sm = SettingsManager::sharedInstance();

    m_downloadPathEdit->setText(sm->devdiskimgpath());
    m_autoUpdateCheck->setChecked(sm->autoCheckUpdates());
    m_autoRaiseWindow->setChecked(sm->autoRaiseWindow());
    m_switchToNewDevice->setChecked(sm->switchToNewDevice());
    m_autoEnableWifiConnections->setChecked(sm->autoEnableWifiConnections());
    m_autoConnectWirelessDevices->setChecked(sm->autoConnectWirelessDevices());
    m_wirelessFileServerPort->setValue(sm->wirelessFileServerPort());

#ifndef __APPLE__
    m_unmount_iFuseDrives->setChecked(sm->unmountiFuseOnExit());
#endif

    // Theme combo: match by locale-independent userData identifier so the
    // saved value keeps resolving correctly across language changes.
    const QString currentTheme = sm->theme();
    int themeIndex = m_themeCombo->findData(currentTheme);
    if (themeIndex == -1) {
        // Backwards-compatible fallback for values stored before the
        // identifier scheme landed (which used the displayed text).
        themeIndex = m_themeCombo->findText(currentTheme);
    }
    if (themeIndex != -1) {
        m_themeCombo->setCurrentIndex(themeIndex);
    }

    // Language combo: select the persisted locale code.
    const QString currentLocale =
        TranslationManager::sharedInstance()->currentLocaleCode();
    const int languageIndex = m_languageCombo->findData(currentLocale);
    m_languageCombo->setCurrentIndex(languageIndex >= 0 ? languageIndex : 0);

    m_connectionTimeout->setValue(sm->connectionTimeout());
    m_useUnsecureBackend->setChecked(sm->useUnsecureBackend());
    m_defaultJailbrokenRootPassword->setText(
        sm->defaultJailbrokenRootPassword());

    // Disable apply button initially
    m_applyButton->setEnabled(false);

    m_iconSizeBaseMultiplier->setValue(sm->iconSizeBaseMultiplier());
    m_fpsComboBox->setCurrentText(QString::number(sm->airplayFps()));
    m_noHoldCheckbox->setChecked(sm->airplayNoHold());
#ifdef __linux__
    m_useLegacyPortsCheckbox->setChecked(sm->airplayUseLegacyPorts());
    m_showV4L2CheckBox->setChecked(sm->showV4L2());
#endif

#ifdef WIN32
    if (m_backDropTypeCombo) {
        const int typeValue = static_cast<int>(sm->winBackdropType());
        const int index = m_backDropTypeCombo->findData(typeValue);
        if (index != -1) {
            m_backDropTypeCombo->setCurrentIndex(index);
        } else {
            m_backDropTypeCombo->setCurrentIndex(0);
        }
    }
    if (m_disableMicaCheckBox) {
        m_disableMicaCheckBox->setChecked(sm->disableMica());
    }
#endif
}

void SettingsWidget::connectSignals()
{
    // Connect all checkboxes and combos for immediate feedback
    connect(m_autoUpdateCheck, &QCheckBox::toggled, this,
            &SettingsWidget::onSettingChanged);
    connect(m_autoRaiseWindow, &QCheckBox::toggled, this,
            &SettingsWidget::onSettingChanged);
    connect(m_switchToNewDevice, &QCheckBox::toggled, this,
            &SettingsWidget::onSettingChanged);
    connect(m_autoEnableWifiConnections, &QCheckBox::toggled, this,
            &SettingsWidget::onSettingChanged);
    connect(m_autoConnectWirelessDevices, &QCheckBox::toggled, this,
            &SettingsWidget::onSettingChanged);
#ifndef __APPLE__
    connect(m_unmount_iFuseDrives, &QCheckBox::toggled, this,
            &SettingsWidget::onSettingChanged);
#endif
    connect(m_themeCombo, QOverload<int>::of(&QComboBox::currentIndexChanged),
            this, &SettingsWidget::onSettingChanged);
    connect(m_languageCombo,
            QOverload<int>::of(&QComboBox::currentIndexChanged), this,
            &SettingsWidget::onSettingChanged);
    connect(m_connectionTimeout, QOverload<int>::of(&QSpinBox::valueChanged),
            this, &SettingsWidget::onSettingChanged);
    connect(m_wirelessFileServerPort,
            QOverload<int>::of(&QSpinBox::valueChanged), this,
            &SettingsWidget::onSettingChanged);

    connect(m_iconSizeBaseMultiplier,
            QOverload<double>::of(&QDoubleSpinBox::valueChanged), this,
            [this]() {
                m_restartRequired = true;
                onSettingChanged();
            });

    connect(m_useUnsecureBackend, &QCheckBox::toggled, this, [this]() {
        // since this is unsafe if its being enabled, show a warning
        if (m_useUnsecureBackend->isChecked()) {
            auto reply = QMessageBox::warning(
                this, tr("Warning"),
                tr("Enabling this will not encrypt your Apple account which "
                   "is a "
                   "security risk. Are you sure you want to enable this?"),
                QMessageBox::Yes | QMessageBox::No, QMessageBox::No);

            if (reply == QMessageBox::Yes) {
                m_restartRequired = true;
                onSettingChanged();
            } else {
                m_useUnsecureBackend->setChecked(false);
            }
        } else {
            m_restartRequired = true;
            onSettingChanged();
        }
    });

    connect(m_defaultJailbrokenRootPassword, &QLineEdit::textChanged, this,
            &SettingsWidget::onSettingChanged);
    connect(m_fpsComboBox, QOverload<int>::of(&QComboBox::currentIndexChanged),
            this, &SettingsWidget::onSettingChanged);
    connect(m_noHoldCheckbox, &QCheckBox::toggled, this,
            &SettingsWidget::onSettingChanged);
#ifdef __linux__
    connect(m_useLegacyPortsCheckbox, &QCheckBox::toggled, this,
            &SettingsWidget::onSettingChanged);
    connect(m_showV4L2CheckBox, &QCheckBox::toggled, this,
            &SettingsWidget::onSettingChanged);
#endif
#ifdef WIN32
    if (m_backDropTypeCombo) {
        connect(m_backDropTypeCombo,
                QOverload<int>::of(&QComboBox::currentIndexChanged), this,
                [this]() {
                    m_restartRequired = true;
                    onSettingChanged();
                });
    }
    if (m_disableMicaCheckBox) {
        connect(m_disableMicaCheckBox, &QCheckBox::toggled, this, [this]() {
            m_restartRequired = true;
            onSettingChanged();
        });
    }
#endif
}

void SettingsWidget::onBrowseButtonClicked()
{
    QString dir = QFileDialog::getExistingDirectory(
        this, tr("Select Download Directory"), m_downloadPathEdit->text(),
        QFileDialog::ShowDirsOnly);

    if (!dir.isEmpty()) {
        m_downloadPathEdit->setText(dir);
        onSettingChanged();
    }
}

void SettingsWidget::onCheckUpdatesClicked()
{
    m_checkUpdatesButton->setText(tr("Checking..."));
    m_checkUpdatesButton->setEnabled(false);

    connect(
        MainWindow::sharedInstance()->m_updater, &ZUpdater::dataAvailable, this,
        [this](const QJsonDocument data, bool isUpdateAvailable) {
            if (!isUpdateAvailable) {
                QMessageBox::information(this, tr("No Updates"),
                                         tr("You are using the latest version "
                                            "of iDescriptor."));
            }
            m_checkUpdatesButton->setText(tr("Check for Updates"));
            m_checkUpdatesButton->setEnabled(true);
        },
        Qt::SingleShotConnection);

    MainWindow::sharedInstance()->m_updater->checkForUpdates();
}

void SettingsWidget::onResetToDefaultsClicked()
{
    auto reply = QMessageBox::question(
        this, tr("Reset Settings"),
        tr("Are you sure you want to reset all settings to their default "
           "values?"),
        QMessageBox::Yes | QMessageBox::No, QMessageBox::No);

    if (reply == QMessageBox::Yes) {
        resetToDefaults();
    }
}

void SettingsWidget::onApplyClicked()
{
    saveSettings();
    QMessageBox::information(this, tr("Settings"),
                             m_restartRequired
                                 ? tr("Settings applied. Please restart "
                                      "the application for changes to "
                                      "take effect.")
                                 : tr("Settings applied."));
    m_restartRequired = false;
}

void SettingsWidget::onSettingChanged()
{
    // Enable apply button when settings change
    m_applyButton->setEnabled(true);
}

void SettingsWidget::saveSettings()
{
    SettingsManager *sm = SettingsManager::sharedInstance();

    sm->setDevDiskImgPath(m_downloadPathEdit->text());
    sm->setAutoCheckUpdates(m_autoUpdateCheck->isChecked());
    sm->setAutoRaiseWindow(m_autoRaiseWindow->isChecked());
    sm->setSwitchToNewDevice(m_switchToNewDevice->isChecked());
    sm->setAutoEnableWifiConnections(m_autoEnableWifiConnections->isChecked());
    sm->setAutoConnectWirelessDevices(
        m_autoConnectWirelessDevices->isChecked());
    sm->setWirelessFileServerPort(m_wirelessFileServerPort->value());

#ifndef __APPLE__
    sm->setUnmountiFuseOnExit(m_unmount_iFuseDrives->isChecked());
#endif
    sm->setUseUnsecureBackend(m_useUnsecureBackend->isChecked());

    // Persist the locale-independent identifier carried in userData so the
    // stored value keeps resolving across language changes.
    const QString themeId = m_themeCombo->currentData().toString();
    sm->setTheme(themeId.isEmpty() ? m_themeCombo->currentText() : themeId);
    sm->setConnectionTimeout(m_connectionTimeout->value());
    sm->setDefaultJailbrokenRootPassword(
        m_defaultJailbrokenRootPassword->text());

    sm->setIconSizeBaseMultiplier(m_iconSizeBaseMultiplier->value());

    sm->setAirplayFps(m_fpsComboBox->currentText().toInt());
    sm->setAirplayNoHold(m_noHoldCheckbox->isChecked());
#ifdef __linux__
    sm->setAirplayUseLegacyPorts(m_useLegacyPortsCheckbox->isChecked());
    sm->setShowV4L2(m_showV4L2CheckBox->isChecked());
#endif

    // Apply the selected locale. TranslationManager::setLocale persists the
    // value via SettingsManager and broadcasts a LanguageChange event.
    const QString newLocale = m_languageCombo->currentData().toString();
    if (newLocale
        != TranslationManager::sharedInstance()->currentLocaleCode()) {
        TranslationManager::sharedInstance()->setLocale(newLocale);
    }

    m_applyButton->setEnabled(false);

#ifdef WIN32
    if (m_backDropTypeCombo) {
        const QVariant data = m_backDropTypeCombo->currentData();
        // The "Auto" entry stores the literal string identifier; everything
        // else stores the WIN_BACKDROP int.
        if (data.typeId() == QMetaType::QString) {
            // AUTO = ACRYLIC
            sm->setWinBackdropType(static_cast<WIN_BACKDROP>(ACRYLIC));
        } else if (!data.isValid()) {
            sm->setWinBackdropType(static_cast<WIN_BACKDROP>(ACRYLIC));
        } else {
            sm->setWinBackdropType(static_cast<WIN_BACKDROP>(data.toInt()));
        }
    }
    if (m_disableMicaCheckBox) {
        sm->setDisableMica(m_disableMicaCheckBox->isChecked());
    }
#endif
}

void SettingsWidget::resetToDefaults()
{
    SettingsManager::sharedInstance()->resetToDefaults();

    // Reload UI with default values
    loadSettings();

    onSettingChanged();
}
