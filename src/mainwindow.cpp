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

#include "mainwindow.h"
#include "appswidget.h"
#include "devicemanagerwidget.h"
#include "ifusediskunmountbutton.h"
#include "ifusemanager.h"
#include "jailbrokenwidget.h"
#include "releasechangelogdialog.h"
#include "toolboxwidget.h"
#include "welcomewidget.h"
#include <unistd.h>

#include "appcontext.h"
#include "networkdeviceswidget.h"
#include "settingsmanager.h"
#include "statusballoon.h"

#ifdef WIN32
#include "platform/windows/win_common.h"
#endif

#ifdef ENABLE_RECOVERY_DEVICE_SUPPORT
void handleCallbackRecovery(const irecv_device_event_t *event, void *userData)
{

    switch (event->type) {
    case IRECV_DEVICE_ADD:
        qDebug() << "Recovery device added: ";
        QMetaObject::invokeMethod(AppContext::sharedInstance(),
                                  "addRecoveryDevice", Qt::QueuedConnection,
                                  Q_ARG(uint64_t, event->device_info->ecid));
        break;
    case IRECV_DEVICE_REMOVE:
        qDebug() << "Recovery device removed: ";
        QMetaObject::invokeMethod(AppContext::sharedInstance(),
                                  "removeRecoveryDevice", Qt::QueuedConnection,
                                  Q_ARG(uint64_t, event->device_info->ecid));
        break;
    default:
        printf("Unhandled recovery event: %d\n", event->type);
    }
}
irecv_device_event_context_t context;
#endif

void handleCallback(int code, const QString &udid, const QString &info)
{
    qDebug() << "Device event: "
             << (code == 1   ? "Connected"
                 : code == 2 ? "Disconnected"
                 : code == 3 ? "Pairing Pending"
                 : code == 4 ? "Pairing Failed"
                             : "Unknown")
             << ", UDID: " << udid;
    AddType addType;

    switch (code) {
    case 1: { // fully connected
        addType = AddType::Regular;
        break;
    }
    case 2: { // disconnected
        QMetaObject::invokeMethod(
            AppContext::sharedInstance(), "removeDevice", Qt::QueuedConnection,
            Q_ARG(iDescriptor::Uniq, iDescriptor::Uniq(udid)));
        return;
    }
    case 3: { // pairing pending
        addType = AddType::Pairing;
        break;
    }
    case 4: { // pairing failed
        addType = AddType::FailedToPair;
        break;
    }
    }

    QMetaObject::invokeMethod(
        AppContext::sharedInstance(), "addDevice", Qt::QueuedConnection,
        Q_ARG(iDescriptor::Uniq, iDescriptor::Uniq(udid)),
        Q_ARG(iDescriptor::IdeviceConnectionType,
              static_cast<iDescriptor::IdeviceConnectionType>(
                  iDescriptor::CONNECTION_USB)),
        Q_ARG(AddType, addType), Q_ARG(QString, info));
}

MainWindow *MainWindow::sharedInstance()
{
    static MainWindow instance;
    return &instance;
}

MainWindow::MainWindow(QWidget *parent) : QMainWindow(parent)
{
    setMinimumSize(MIN_MAIN_WINDOW_SIZE);
    resize(MIN_MAIN_WINDOW_SIZE);
    setContentsMargins(0, 0, 0, 0);

    QWidget *centralWidget = new QWidget(this);
    setCentralWidget(centralWidget);
    auto mainLayout = new QVBoxLayout(centralWidget);
    mainLayout->setContentsMargins(0, 0, 0, 0);

    auto core = AppContext::sharedInstance()->core;

    connect(core, &CXX::Core::device_event, this,
            [this](uint32_t code, const QString &udid, const QString &info) {
                handleCallback(code, udid, info);
            });

    connect(core, &CXX::Core::sleepy_time_detected, this,
            &MainWindow::handleShowSleepyDeviceWarning);

    AppContext::sharedInstance()->core->init();

    m_ZTabWidget = new ZTabWidget(this);
#ifdef __APPLE__
    setupMacOSWindow(this);
    setAttribute(Qt::WA_ContentsMarginsRespectsSafeArea, false);
#endif
    mainLayout->addWidget(m_ZTabWidget);
#ifdef WIN32
    setupWinWindow(this);
#endif

    m_mainStackedWidget = new QStackedWidget();
    WelcomeWidget *welcomePage = new WelcomeWidget(this);
    m_deviceManager = new DeviceManagerWidget(this);

    m_mainStackedWidget->addWidget(welcomePage);
    m_mainStackedWidget->addWidget(m_deviceManager);

    connect(m_deviceManager, &DeviceManagerWidget::updateNoDevicesConnected,
            this, &MainWindow::updateNoDevicesConnected);

    m_iDeviceTab = m_ZTabWidget->addTab(m_mainStackedWidget, tr("iDevice"));
    m_appsTab = m_ZTabWidget->addTab(AppsWidget::sharedInstance(), tr("Apps"));
    m_toolboxTab =
        m_ZTabWidget->addTab(ToolboxWidget::sharedInstance(), tr("Toolbox"));
    auto *jailbrokenWidget = new JailbrokenWidget(this);
    m_jailbrokenTab = m_ZTabWidget->addTab(jailbrokenWidget, tr("Jailbroken"));
    m_ZTabWidget->finalizeStyles();

    connect(
        m_appsTab, &ZTab::clicked, this,
        [this](int index) { AppsWidget::sharedInstance()->init(); },
        Qt::SingleShotConnection);

    // settings button
    m_settingsButton = new ZIconWidget(
        QIcon(":/resources/icons/MingcuteSettings7Line.png"), tr("Settings"));
    m_settingsButton->setCursor(Qt::PointingHandCursor);
    connect(m_settingsButton, &ZIconWidget::clicked, this, [this]() {
        SettingsManager::sharedInstance()->showSettingsDialog();
    });

    m_githubButton =
        new ZIconWidget(QIcon(":/resources/icons/MdiGithub.png"),
                        tr("%1 on GitHub").arg(qApp->applicationName()));
    m_githubButton->setCursor(Qt::PointingHandCursor);
    connect(m_githubButton, &ZIconWidget::clicked, this,
            []() { QDesktopServices::openUrl(QUrl(REPO_URL)); });

    m_connectedDeviceCountLabel = new QLabel(
        tr("%1: no devices").arg(qApp->applicationName()));
    m_connectedDeviceCountLabel->setContentsMargins(0, 0, 0, 0);
    m_connectedDeviceCountLabel->setStyleSheet(
        "QLabel:hover { background-color : #13131319; }");

    QWidget *statusbar = new QWidget();
    QHBoxLayout *statusLayout = new QHBoxLayout(statusbar);
    statusLayout->setContentsMargins(10, 0, 10, 0);
    statusLayout->setSpacing(1);
    statusbar->setObjectName("StatusBar");
    statusbar->setStyleSheet(
        "QWidget#StatusBar { background-color: transparent; }");
    statusLayout->addWidget(m_connectedDeviceCountLabel);

    StatusBalloon *statusBalloon = StatusBalloon::sharedInstance();

    statusLayout->addWidget(statusBalloon->getButton());

    m_welcomeMenuSwitch = new ZIconWidget(
        QIcon(":/resources/icons/LetsIconsHorizontalDownLeftMainLight.png"),
        tr("Switch to Welcome Menu"));
    connect(m_welcomeMenuSwitch, &ZIconWidget::clicked, this, [this]() {
        if (m_mainStackedWidget->currentIndex() != 0) {
            m_welcomeTabActive = true;
            m_welcomeMenuSwitch->setToolTip(tr("Switch to Connected Devices"));
            return showWelcomeTab();
        }
        m_welcomeTabActive = false;
        m_welcomeMenuSwitch->setToolTip(tr("Switch to Welcome Menu"));
        showConnectedDevicesTab();
    });

    connect(m_ZTabWidget, &ZTabWidget::currentChanged, this,
            [this](int index) {
                if (index != 0) {
                    return m_welcomeMenuSwitch->hide();
                }

                m_welcomeMenuSwitch->show();
            });

    statusLayout->addWidget(m_welcomeMenuSwitch);
    statusLayout->addStretch(1);

    QLabel *appVersionLabel = new QLabel(QString("v%1").arg(APP_VERSION));
    appVersionLabel->setContentsMargins(5, 0, 5, 0);
    appVersionLabel->setStyleSheet(
        "QLabel:hover { background-color : #13131319; }");
    statusLayout->addWidget(appVersionLabel);
    statusLayout->addWidget(githubButton);
    statusLayout->addWidget(settingsButton);

    mainLayout->addWidget(statusbar);

#ifdef __linux__
    QList<QString> mounted_iFusePaths = iFuseManager::getMountPoints();

    for (const QString &path : mounted_iFusePaths) {
        auto *p = new iFuseDiskUnmountButton(path);

        statusLayout->addWidget(p);
        connect(p, &iFuseDiskUnmountButton::clicked, this,
                [this, p, path, statusLayout]() {
                    bool ok = iFuseManager::linuxUnmount(path);
                    if (!ok) {
                        QMessageBox::warning(
                            nullptr, tr("Unmount Failed"),
                            tr("Failed to unmount iFuse at %1. Please "
                               "try again.")
                                .arg(path));
                        return;
                    }
                    statusLayout->removeWidget(p);
                    p->deleteLater();
                });
    }
#endif

#ifdef ENABLE_RECOVERY_DEVICE_SUPPORT
    irecv_error_t res_recovery =
        irecv_device_event_subscribe(&context, handleCallbackRecovery, nullptr);

    if (res_recovery != IRECV_E_SUCCESS) {
        qDebug() << "ERROR: Unable to subscribe to recovery device events. "
                    "Error code:"
                 << res_recovery;
    }
    qDebug() << "Subscribed to recovery device events successfully.";
#endif

    createMenus();

    UpdateProcedure updateProcedure;
    bool packageManagerManaged = false;
    bool isPortable = false;
    bool skipPrerelease = true;
#ifdef WIN32
    isPortable = !is_iDescriptorInstalled();
    qDebug() << "isPortable=" << isPortable;
#endif

    /*
    struct UpdateProcedure {
        bool openFile;
        bool openFileDir;
        bool quitApp;
        QString boxInformativeText;
        QString boxText;
    };
    */
    switch (ZUpdater::detectPlatform()) {
    case Platform::Windows:
        updateProcedure = UpdateProcedure{
            !isPortable,
            isPortable,
            !isPortable,
            isPortable
                ? tr("New portable version downloaded, app location will "
                     "be shown after this message")
                : tr("The application will now quit to install the update."),
            isPortable
                ? tr("New portable version downloaded")
                : tr("Do you want to install the downloaded update now?"),
        };
        break;
        // todo: adjust for pkg managers
    case Platform::MacOS:
        updateProcedure = UpdateProcedure{
            true,
            false,
            true,
            tr("The application will now quit and open .dmg file downloaded "
               "to \"Downloads\" from there you can drag it to Applications "
               "to install."),
            tr("Update downloaded would you like to quit and install the "
               "update?"),
        };
        break;
    case Platform::Linux:
        // currently only on linux (arch aur) is enabled
#ifdef PACKAGE_MANAGER_MANAGED
        packageManagerManaged = true;
#endif
        updateProcedure = UpdateProcedure{
            true,
            false,
            true,
            tr("AppImages we ship are not updateable. New version is "
               "downloaded to \"Downloads\". You can start using the new "
               "version by launching it from there. You can delete this "
               "AppImage version if you like."),
            tr("Update downloaded would you like to quit and open the new "
               "version?"),
        };
        break;
    default:
        updateProcedure = UpdateProcedure{
            false, false, false, "", "",
        };
    }

    m_updater = new ZUpdater("iDescriptor/iDescriptor", APP_VERSION,
                             "iDescriptor", updateProcedure, isPortable,
                             packageManagerManaged, skipPrerelease, this);
#if defined(PACKAGE_MANAGER_MANAGED) && defined(__linux__)
    m_updater->setPackageManagerManagedMessage(
        tr("You seem to have installed %1 using a package manager. "
           "Please use %2 to update it.")
            .arg(qApp->applicationName(), PACKAGE_MANAGER_HINT));
#endif

    QString lastAppVersion = SettingsManager::sharedInstance()->appVersion();
    bool shouldShowReleaseChangelog = lastAppVersion != APP_VERSION;
    SettingsManager::sharedInstance()->setAppVersion(APP_VERSION);

    if (shouldShowReleaseChangelog) {
        connect(
            m_updater, &ZUpdater::dataAvailable, this,
            [this](const QJsonDocument data, bool isUpdateAvailable) {
                if (!isUpdateAvailable) {
                    ReleaseChangelogDialog dialog(data, this);
                    dialog.exec();
                }
            },
            Qt::SingleShotConnection);
    }

    SettingsManager::sharedInstance()->doIfEnabled(
        SettingsManager::Setting::AutoCheckUpdates, [this]() {
            qDebug() << "Checking for updates...";
            m_updater->checkForUpdates();
        });

    // ═══════════════════════════════════════════════════════════════════════
    //  Upgrade to wireless when a "WIRED" device is removed
    // ═══════════════════════════════════════════════════════════════════════
    connect(
        AppContext::sharedInstance(), &AppContext::deviceRemoved, this,
        [](const QString &udid, const std::string &wifiMacAddress,
           bool wasWireless) {
            if (wasWireless)
                return;
            QString wifiMac = QString::fromStdString(wifiMacAddress);
            NetworkDevice dev =
                NetworkDeviceProvider::sharedInstance()->getNetworkDeviceByMac(
                    wifiMac);
            if (!dev.isValid()) {
                qDebug() << "No valid network device found for UDID" << udid
                         << "with Wi-Fi MAC" << wifiMac
                         << "Not trying to upgrade to wireless connection.";
                return;
            }

            qDebug()
                << "Trying to upgrade device to wireless connection for UDID"
                << udid;
            // FIXME: maybe ignore iOS 15 and lower?
            AppContext::sharedInstance()->tryToConnectToNetworkDevice(dev);
        });

    // ═══════════════════════════════════════════════════════════════════════
    //  Add a wireless device
    // ═══════════════════════════════════════════════════════════════════════
    connect(
        NetworkDeviceProvider::sharedInstance(),
        &NetworkDeviceProvider::deviceAdded, this,
        [this](const NetworkDevice &device) {
            if (!SettingsManager::sharedInstance()
                     ->autoConnectWirelessDevices())
                return;
            if (auto existingDevice =
                    AppContext::sharedInstance()->getDeviceByMacAddress(
                        device.macAddress)) {
                if (existingDevice->deviceInfo.isWireless) {
                    qDebug()
                        << "Ignoring wireless device with MAC:"
                        << device.macAddress << "as it's already initialized";

                } else {
                    qDebug() << "Prefering wired connection on device with MAC:"
                             << device.macAddress;
                }

                return;
            }
            AppContext::sharedInstance()->tryToConnectToNetworkDevice(device);
        });
}

void MainWindow::createMenus()
{
    m_helpMenu = menuBar()->addMenu(QString());

    m_aboutAction = new QAction(this);
    // setMenuRole(AboutRole) ensures macOS moves the action to the
    // application menu automatically while other platforms keep it under
    // the Help menu.
    m_aboutAction->setMenuRole(QAction::AboutRole);
    connect(m_aboutAction, &QAction::triggered, this, &MainWindow::showAbout);
    m_helpMenu->addAction(m_aboutAction);

    retranslateMenus();
}

void MainWindow::retranslateMenus()
{
    if (m_helpMenu) {
        m_helpMenu->setTitle(tr("&Help"));
    }
    if (m_aboutAction) {
        m_aboutAction->setText(
            tr("&About %1").arg(qApp->applicationName()));
    }
}

void MainWindow::retranslateTabs()
{
    if (m_iDeviceTab) {
        m_iDeviceTab->setText(tr("iDevice"));
    }
    if (m_appsTab) {
        m_appsTab->setText(tr("Apps"));
    }
    if (m_toolboxTab) {
        m_toolboxTab->setText(tr("Toolbox"));
    }
    if (m_jailbrokenTab) {
        m_jailbrokenTab->setText(tr("Jailbroken"));
    }
}

void MainWindow::retranslateStatusBar()
{
    if (m_settingsButton) {
        m_settingsButton->setToolTip(tr("Settings"));
    }
    if (m_githubButton) {
        m_githubButton->setToolTip(
            tr("%1 on GitHub").arg(qApp->applicationName()));
    }
    if (m_welcomeMenuSwitch) {
        // The tooltip describes the action the user can perform next:
        // when the welcome page is visible, the next action is to jump
        // to the connected-devices view, and vice versa.
        m_welcomeMenuSwitch->setToolTip(
            m_welcomeTabActive ? tr("Switch to Connected Devices")
                               : tr("Switch to Welcome Menu"));
    }
    if (m_connectedDeviceCountLabel) {
        if (m_connectedDeviceCount == 0) {
            m_connectedDeviceCountLabel->setText(
                tr("%1: no devices").arg(qApp->applicationName()));
        } else {
            // %n triggers Qt's plural handling so translators can provide
            // singular/plural forms for "device(s) connected".
            m_connectedDeviceCountLabel->setText(
                tr("%1: %n device(s) connected", "", m_connectedDeviceCount)
                    .arg(qApp->applicationName()));
        }
    }
}

void MainWindow::retranslateUi()
{
    retranslateMenus();
    retranslateTabs();
    retranslateStatusBar();
}

void MainWindow::changeEvent(QEvent *event)
{
    if (event->type() == QEvent::LanguageChange) {
        retranslateUi();
    }
    QMainWindow::changeEvent(event);
}

void MainWindow::showAbout()
{
    QMessageBox::about(this,
                       tr("About %1").arg(qApp->applicationName()),
                       tr("A free, open-source, and cross-platform "
                          "iDevice management tool."));
}

void MainWindow::updateNoDevicesConnected()
{
    qDebug() << "No devices connected? "
             << AppContext::sharedInstance()->noDevicesConnected();
    if (AppContext::sharedInstance()->noDevicesConnected()) {
        m_connectedDeviceCount = 0;
        retranslateStatusBar();
        return m_mainStackedWidget->setCurrentIndex(0); // Show Welcome page
    }
    m_connectedDeviceCount =
        AppContext::sharedInstance()->getConnectedDeviceCount();
    retranslateStatusBar();
    m_mainStackedWidget->setCurrentIndex(1); // Show device list page
}

void MainWindow::closeEvent(QCloseEvent *event)
{
    if (StatusBalloon::sharedInstance()->hasActiveProcesses()) {
        auto reply = QMessageBox::question(
            this, tr("Transfers in Progress"),
            tr("There are import/export operations in progress.\n"
               "Do you really want to quit? This will cancel them."),
            QMessageBox::Yes | QMessageBox::No, QMessageBox::No);

        if (reply == QMessageBox::No) {
            event->ignore();
            return;
        }
        IOManagerClient::sharedInstance()->cancelAllJobs();
    }

    QMainWindow::closeEvent(event);
}

void MainWindow::raiseDeviceTab()
{
    m_ZTabWidget->setCurrentIndex(0);
    raise();
    activateWindow();
}

void MainWindow::showConnectedDevicesTab()
{
    m_mainStackedWidget->setCurrentIndex(1);
}

void MainWindow::showWelcomeTab() { m_mainStackedWidget->setCurrentIndex(0); }

MainWindow::~MainWindow()
{
#ifdef ENABLE_RECOVERY_DEVICE_SUPPORT
    irecv_device_event_unsubscribe(context);
#endif
    delete m_updater;
    // sleep(2);
}

void MainWindow::handleShowSleepyDeviceWarning()
{
    static bool widgetAlreadyVisible = false;
    /* one minute cooldown to prevent spamming */
    static const int kMinIntervalMs = 60 * 1000;

    static QDateTime lastShown;
    const auto now = QDateTime::currentDateTimeUtc();

    if (lastShown.isValid() && lastShown.msecsTo(now) < kMinIntervalMs) {
        qDebug() << "Ignoring sleepy device warning dialog.";
        return;
    }

    lastShown = now;

    if (SettingsManager::sharedInstance()->isSleepyDeviceWarningDismissed() ||
        widgetAlreadyVisible) {
        return;
    }

    widgetAlreadyVisible = true;
    DeviceSleepWarningWidget(this).exec();
    widgetAlreadyVisible = false;
}