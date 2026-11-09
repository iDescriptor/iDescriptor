// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

import QtQuick
import ".." as App

DefaultWindow {
    id: root
    required property string udid
    required property string ecid
    required property var device
    property bool hasActiveRestore: false

    // A restore deliberately disconnects the normal-mode device. Its job owns
    // reconnect timeouts; device removal must never close this window.
    onClosing: function(close) {
        if (root.hasActiveRestore)
            App.ClosingHandler.handler("flash", close, root)
    }

    Component.onCompleted: {
        if (Qt.platform.os === "osx")
            QmlUtils.setup_tool_window(root)
    }
}
