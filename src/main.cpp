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

#include "iDescriptor.h"
#include "mainwindow.h"
#include "settingsmanager.h"
#include "translationmanager.h"
#include <QApplication>
#include <QDebug>
#include <QDir>
#include <QFile>
#include <QMessageBox>
#include <QStyleFactory>
#include <QtGlobal>
#include <stdlib.h>
#ifdef WIN32
#include "platform/windows/win_common.h"
#endif

int main(int argc, char *argv[])
{
    QApplication a(argc, argv);
    QCoreApplication::setOrganizationName("iDescriptor");
    QCoreApplication::setApplicationName("iDescriptor");
    QCoreApplication::setApplicationVersion(APP_VERSION);
    TranslationManager::sharedInstance()->initFromSettings();
    if (a.arguments().contains("--reset-settings")) {
        SettingsManager::sharedInstance()->clear();
        QMessageBox::information(
            nullptr,
            QCoreApplication::translate("main", "Settings Reset"),
            QCoreApplication::translate(
                "main",
                "All application settings have been reset to their default values."));
    }
#ifdef WIN32

    bool enableMica = !a.arguments().contains("--disable-mica") &&
                      !SettingsManager::sharedInstance()->disableMica();
    if (!enableMica) {
        SettingsManager::sharedInstance()->setDisableMica(true);
        qDebug() << "Mica effect disabled";
    }

    QApplication::setEffectEnabled(Qt::UI_AnimateCombo, false);

    QOperatingSystemVersion osVersion = QOperatingSystemVersion::current();
    if (enableMica && osVersion >= QOperatingSystemVersion::Windows11) {
        QFile styleFile(detectDarkModeWindows() ? ":/resources/win.dark.qcss"
                                                : ":/resources/win.light.qcss");
        if (styleFile.open(QFile::ReadOnly | QFile::Text)) {
            const QString style = QString::fromUtf8(styleFile.readAll())
                                      .arg(COLOR_ACCENT_BLUE.name());
            qDebug() << "Loaded Windows style sheet successfully.";
            a.setStyleSheet(style);
        }
    } else {
        qDebug() << "Not applying WinUI stylesheet.";
    }

    QString appPath = QCoreApplication::applicationDirPath();
    QString gstPluginPath =
        QDir::toNativeSeparators(appPath + "/gstreamer-1.0");
    QString gstPluginScannerPath = QDir::toNativeSeparators(
        appPath + "/gstreamer-1.0/libexec/gst-plugin-scanner.exe");

    const char *oldPath = getenv("PATH");
    QString newPath = appPath + ";" + QString(oldPath);
    qputenv("PATH", newPath.toUtf8());

    qputenv("GST_PLUGIN_PATH", gstPluginPath.toUtf8());
    qDebug() << "GST_PLUGIN_PATH=" << gstPluginPath;
    qputenv("GST_REGISTRY_REUSE_PLUGIN_SCANNER", "no");
    qDebug() << "GST_REGISTRY_REUSE_PLUGIN_SCANNER=no";
    qputenv("GST_PLUGIN_SYSTEM_PATH", gstPluginPath.toUtf8());
    qDebug() << "GST_PLUGIN_SYSTEM_PATH=" << gstPluginPath;
    qputenv("GST_DEBUG", "GST_PLUGIN_LOADING:5");
    qDebug() << "GST_DEBUG=GST_PLUGIN_LOADING:5";
    qputenv("GST_PLUGIN_SCANNER_1_0", gstPluginScannerPath.toUtf8());
    qDebug() << "GST_PLUGIN_SCANNER_1_0=" << gstPluginScannerPath;
#endif
#ifdef __APPLE__
    QString appPath = QCoreApplication::applicationDirPath();
    QString frameworksPath =
        QDir::toNativeSeparators(appPath + "/../Frameworks");
    QString gstPluginPath =
        QDir::toNativeSeparators(frameworksPath + "/gstreamer");
    QString gstPluginScannerPath =
        QDir::toNativeSeparators(frameworksPath + "/gst-plugin-scanner");

    setenv("GST_PLUGIN_PATH", gstPluginPath.toUtf8().constData(), 1);
    setenv("GST_PLUGIN_SYSTEM_PATH", gstPluginPath.toUtf8().constData(), 1);
    setenv("GST_PLUGIN_SCANNER", gstPluginScannerPath.toUtf8().constData(), 1);
#endif
    MainWindow *w = MainWindow::sharedInstance();
    w->show();
    return a.exec();
}
