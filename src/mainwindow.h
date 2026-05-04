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

#ifndef MAINWINDOW_H
#define MAINWINDOW_H
#include "ZDownloader.h"
#include "ZUpdater.h"
#include "devicesleepwarningwidget.h"
#include "iDescriptor-ui.h"
#include "iDescriptor.h"
#include "iomanagerclient.h"
#include "ztabwidget.h"
#include <QAction>
#include <QApplication>
#include <QDateTime>
#include <QDesktopServices>
#include <QEvent>
#include <QHBoxLayout>
#include <QLabel>
#include <QMainWindow>
#include <QMenu>
#include <QMenuBar>
#include <QMessageBox>
#include <QStack>
#include <QStackedWidget>
#include <QTimer>
#include <QVBoxLayout>
#include <QWidget>

class DeviceManagerWidget;

class MainWindow : public QMainWindow
{
    Q_OBJECT

public:
    static MainWindow *sharedInstance();
    MainWindow(QWidget *parent = nullptr);
    ~MainWindow();
    ZUpdater *m_updater = nullptr;
    void raiseDeviceTab();
    void showConnectedDevicesTab();
    void showWelcomeTab();
    void handleShowSleepyDeviceWarning();
public slots:
    void updateNoDevicesConnected();

private slots:
    void showAbout();

private:
    void createMenus();
    void retranslateUi();
    void retranslateMenus();
    void retranslateTabs();
    void retranslateStatusBar();

    ZTabWidget *m_ZTabWidget;
    DeviceManagerWidget *m_deviceManager;
    QStackedWidget *m_mainStackedWidget;
    QLabel *m_connectedDeviceCountLabel;
    QLabel *m_titleLabel;
    QPushButton *m_minBtn;
    QPushButton *m_maxBtn;
    QPushButton *m_closeBtn;
    QWidget *m_titleBar;
    QWidget *m_contentArea;
    QHBoxLayout *m_titleBarLayout;

    QMenu *m_helpMenu = nullptr;
    QAction *m_aboutAction = nullptr;

    ZTab *m_iDeviceTab = nullptr;
    ZTab *m_appsTab = nullptr;
    ZTab *m_toolboxTab = nullptr;
    ZTab *m_jailbrokenTab = nullptr;

    ZIconWidget *m_settingsButton = nullptr;
    ZIconWidget *m_githubButton = nullptr;
    ZIconWidget *m_welcomeMenuSwitch = nullptr;

    int m_connectedDeviceCount = 0;
    // Tracks the welcome-menu-switch tooltip phrasing. Initialized to
    // false to mirror the historical initial tooltip value, which the
    // click handler keeps in sync from there on.
    bool m_welcomeTabActive = false;

protected:
    void closeEvent(QCloseEvent *event) override;
    void changeEvent(QEvent *event) override;
};
#endif // MAINWINDOW_H
