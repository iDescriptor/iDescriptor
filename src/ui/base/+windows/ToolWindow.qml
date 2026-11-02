// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import ".." as App
import "./"

DefaultWindow {
    id: root
    required property string udid
    required property var device
    property bool auto_close: true
    //TODO: we may remove this if we find out how
    // macOS does that reveal animation used in iPhone Mirroring app
    property bool customChrome: false

    Connections {
        target: App.DeviceContext
        enabled: root.auto_close

        function onDeviceRemoved(removedUdid) {
            if (root.udid === removedUdid)
                root.close()
        }
    }
}
