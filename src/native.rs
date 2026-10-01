// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

use cpp::cpp;
use qmetaobject::{QString, QmlEngine};

#[cfg(target_os = "macos")]
const APP_ICON_PATH: &str = ":/packaging/shared/resources/app-icon/icon.icns";

#[cfg(not(target_os = "macos"))]
const APP_ICON_PATH: &str = ":/packaging/shared/resources/app-icon/icon.png";

cpp! {{
    #include <QQuickStyle>
    #include <QQuickWindow>
    #include <QQmlContext>
    #include <QtQml/qqml.h>
    #include <QLoggingCategory>
    #include <QtGui/QGuiApplication>
    #include <QFont>
    #include <QQmlFileSelector>
    #include <QIcon>
    #include <QMessageBox>
    #include <QOperatingSystemVersion>

    #include "src/live_reload.cpp"
}}

pub fn configure_application(application_version: QString) {
    cpp!(unsafe [application_version as "QString"] {
        #define FLUENTUI_BUILD_STATIC_LIB 1
        #ifdef WIN32
            // ::SetUnhandledExceptionFilter(MyUnhandledExceptionFilter);
            qputenv("QT_QPA_PLATFORM", "windows:darkmode=2");
        #endif

        #ifdef Q_OS_WINDOWS
            QQuickStyle::setStyle("FluentWinUI3");
        #else
            QQuickStyle::setStyle("IDescriptorStyle");
            #ifdef Q_OS_MACOS
                QQuickStyle::setFallbackStyle("macOS");
            #elif defined(Q_OS_LINUX)
                QQuickStyle::setFallbackStyle("Fusion");
            #else
                QQuickStyle::setFallbackStyle("Basic");
            #endif
        #endif
        #ifndef Q_OS_LINUX
            // rsplay uses qml6glsink so we have to use opengl
            // Linux is fine with QT_QPA_PLATFORM=xcb
            QQuickWindow::setGraphicsApi(QSGRendererInterface::OpenGL);
        #endif

        // QCoreApplication::setAttribute(Qt::AA_UseOpenGLES);
        #if (QT_VERSION < QT_VERSION_CHECK(6, 0, 0))
            QApplication::setAttribute(Qt::AA_EnableHighDpiScaling);
            QApplication::setAttribute(Qt::AA_UseHighDpiPixmaps);
        #if (QT_VERSION >= QT_VERSION_CHECK(5, 14, 0))
            QApplication::setHighDpiScaleFactorRoundingPolicy(
                Qt::HighDpiScaleFactorRoundingPolicy::PassThrough);
        #endif
        #endif
            QCoreApplication::setOrganizationName("iDescriptor");
            QCoreApplication::setApplicationName("iDescriptor");
            QCoreApplication::setApplicationVersion(application_version);
    });
}

pub fn show_settings_reset_message() {
    cpp!(unsafe [] {
        QMessageBox::information(
            nullptr,
            QStringLiteral("Settings Reset"),
            QStringLiteral(
                "All application settings have been reset to their default values."
            )
        );
    });
}

pub fn local_network_privacy_required() -> bool {
    cpp!(unsafe [] -> bool as "bool" {
        #ifdef Q_OS_MACOS
            return QOperatingSystemVersion::current()
                >= QOperatingSystemVersion(QOperatingSystemVersion::MacOS, 15);
        #else
            return false;
        #endif
    })
}

#[cfg(target_os = "windows")]
pub fn set_default_font() {
    cpp!(unsafe [] {
        QGuiApplication::setFont(QFont("Segoe UI"));
    });
}

#[cfg(not(target_os = "windows"))]
pub fn add_resource_style_import_path(engine: &mut QmlEngine) {
    let engine_ptr = engine.cpp_ptr();

    cpp!(unsafe [engine_ptr as "QQmlApplicationEngine *"] {
        engine_ptr->addImportPath(":/src/ui/styles");
    });
}

#[cfg(all(debug_assertions, not(target_os = "windows")))]
pub fn add_style_import_path(engine: &mut QmlEngine, style_import_path: QString) {
    let engine_ptr = engine.cpp_ptr();

    cpp!(unsafe [
        engine_ptr as "QQmlApplicationEngine *",
        style_import_path as "QString"
    ] {
        engine_ptr->addImportPath(style_import_path);
    });
}

pub fn initialize_engine(engine: &mut QmlEngine) {
    let engine_ptr = engine.cpp_ptr();

    cpp!(unsafe [engine_ptr as "QQmlApplicationEngine *"] {

        static QQmlFileSelector* s_fileSelector = nullptr;
        if (!s_fileSelector) {
            s_fileSelector = new QQmlFileSelector(engine_ptr, engine_ptr);
        }
    });

    let app_icon_path = QString::from(APP_ICON_PATH);

    cpp!(unsafe [app_icon_path as "QString"] {
        QGuiApplication::setWindowIcon(QIcon(app_icon_path));
    });
}

// FIXME: workaround to find FluentUI
// in dev builds
#[cfg(all(debug_assertions, target_os = "windows"))]
pub fn add_development_import_path(engine: &mut QmlEngine) {
    let engine_ptr = engine.cpp_ptr();

    cpp!(unsafe [engine_ptr as "QQmlApplicationEngine *"] {
        engine_ptr->addImportPath("C:/Qt/6.9.3/mingw_64/qml");
    });
}

pub fn initialize_live_reload(engine: &mut QmlEngine, ui_path: QString, entry_path: QString) {
    let engine_ptr = engine.cpp_ptr();

    cpp!(unsafe [
        engine_ptr as "QQmlApplicationEngine *",
        ui_path as "QString",
        entry_path as "QString"
    ] {
        init_live_reload(engine_ptr, ui_path, entry_path);
    });
}
