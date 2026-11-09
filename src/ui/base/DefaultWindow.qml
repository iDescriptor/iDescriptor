// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

import QtQuick
import QtQuick.Controls
import "../"

Window {
    id: window
    /* these are required because FluWindow has them but DefaultWindow doesn't */
    property string _effect: "____"
    property bool showMaximize: false
    property bool showMinimize: false
    property bool showClose: false
    property bool auto_close: true
    property bool autoDestroy: true
    property bool autoVisible: false
    property bool fitsAppBarWindows: true
    property bool setupMacOSWindowStyle: false
    color: Theme.windowBackground
    palette: Theme.palette

    Shortcut {
        sequences: [StandardKey.Close]
        enabled: Qt.platform.os === "osx"
        context: Qt.WindowShortcut
        onActivated: window.close()
    }


    Component.onCompleted : {
        if (Qt.platform.os === "osx" && window.setupMacOSWindowStyle) {
            console.log("Setting up macOS window on update")
            QmlUtils.setup_tool_window(window)
        }
    }
}
