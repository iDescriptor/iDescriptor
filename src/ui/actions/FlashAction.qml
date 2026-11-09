// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import "../base"
import "../" as App

FlashActionWindow {
    id: root

    required property string firmwareIdentifier
    required property string firmwareVersion
    required property string firmwareBuild
    required property string firmwarePath

    // Keep display information after the normal-mode DeviceContext entry disappears.
    property string targetName: ""
    property string targetDetail: ""
    property bool ownsRestore: false
    property var restoreState: ({})
    readonly property string phase: restoreState.phase || "ready"
    readonly property bool running: root.ownsRestore && FlashManager.busy
    hasActiveRestore: root.running
    readonly property bool finished: phase === "completed"
                                     || phase === "failed"
                                     || phase === "cancelled"
    readonly property var stageLabels: [
        qsTr("Validate"), qsTr("Device"), qsTr("Filesystem"), qsTr("Signing"),
        qsTr("Boot"), qsTr("RestoreOS"), qsTr("Install"), qsTr("Finish")
    ]

    function formatBytes(bytes) {
        const value = Number(bytes || 0)
        if (value <= 0)
            return qsTr("0 B")
        const units = [qsTr("B"), qsTr("KB"), qsTr("MB"), qsTr("GB")]
        let amount = value
        let unit = 0
        while (amount >= 1024 && unit < units.length - 1) {
            amount /= 1024
            ++unit
        }
        return "%1 %2".arg(amount.toFixed(unit >= 2 ? 1 : 0)).arg(units[unit])
    }

    function progressDetail() {
        const component = root.restoreState.component || ""
        const sent = Number(root.restoreState.bytesSent || 0)
        const total = Number(root.restoreState.bytesTotal || 0)
        const speed = Number(root.restoreState.speed || 0)
        if (!component.length && speed <= 0)
            return ""
        let value = component
        if (total > 0)
            value += qsTr(" • %1 of %2").arg(root.formatBytes(sent)).arg(root.formatBytes(total))
        else if (sent > 0)
            value += qsTr(" • %1").arg(root.formatBytes(sent))
        if (speed > 0)
            value += qsTr(" • %1/s").arg(root.formatBytes(speed))
        return value
    }

    function requestRestore() {
        if ((!root.udid.length && !root.ecid.length) || FlashManager.busy)
            return
        App.Helpers.messageBox(
            root.contentItem,
            qsTr("Erase and Restore This Device?"),
            qsTr("This permanently erases all data on %1 and installs iOS %2 (%3). Keep the device connected until the restore finishes.")
                .arg(root.targetName).arg(root.firmwareVersion).arg(root.firmwareBuild),
            MessageDialog.Yes | MessageDialog.No,
            function(button) {
                if (button !== MessageDialog.Yes)
                    return
                if (!FlashManager.start_flash(root.udid, root.ecid,
                                              root.firmwarePath, root.firmwareIdentifier)) {
                    App.Helpers.showError(root.contentItem, qsTr("The restore could not be started. Check that the firmware is still present and verified."))
                    return
                }
                root.ownsRestore = true
                root.restoreState = FlashManager.state
            })
    }

    Component.onCompleted: {
        root.targetName = root.udid.length ? root.device.info.DeviceName : root.device.info.marketing_name
        root.targetDetail = root.udid.length ? qsTr("USB • %1").arg(root.udid)
                                           : qsTr("ECID %1").arg(root.ecid)
        if (!FlashManager.busy) {
            FlashManager.reset()
            root.restoreState = FlashManager.state
        }
    }



    Connections {
        target: FlashManager
        function onStateChanged() {
            if (!root.ownsRestore)
                return
            root.restoreState = FlashManager.state
            // Retain the resolved ECID for a retry after the original UDID disappears.
            if (root.restoreState.targetEcid)
                root.ecid = root.restoreState.targetEcid
            if (activityLog.count > 0)
                activityLog.positionViewAtEnd()
        }
        function onFlashFinished(success, cancelled, error) {
            if (root.ownsRestore) {
                root.restoreState = FlashManager.state
                root.ownsRestore = false
            }
        }
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 24
        spacing: 14

        RowLayout {
            Layout.fillWidth: true
            spacing: 12

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 2
                Label {
                    Layout.fillWidth: true
                    text: root.phase === "completed" ? qsTr("Restore Complete")
                          : root.phase === "failed" ? qsTr("Restore Failed")
                          : root.phase === "cancelled" ? qsTr("Restore Cancelled")
                          : root.running ? qsTr("Restoring %1").arg(root.firmwareIdentifier)
                          : qsTr("Erase and Restore %1").arg(root.firmwareIdentifier)
                    color: App.Theme.text
                    font.pixelSize: 22
                    font.weight: Font.DemiBold
                }
                Label {
                    Layout.fillWidth: true
                    text: root.running
                          ? qsTr("Do not disconnect the cable. The device may restart several times.")
                          : qsTr("Install a verified Apple firmware file using a wired USB connection.")
                    color: App.Theme.textMuted
                    font.pixelSize: 13
                    wrapMode: Text.WordWrap
                }
            }

            Rectangle {
                Layout.preferredWidth: 9
                Layout.preferredHeight: 9
                radius: 5
                color: root.phase === "completed" ? App.Theme.systemGreen
                       : root.phase === "failed" ? App.Theme.systemRed
                       : root.phase === "cancelled" ? App.Theme.systemOrange
                       : root.running ? App.Theme.systemBlue : App.Theme.textMuted
            }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 12

            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 92
                radius: 9
                color: App.Theme.groupedBackground
                border.color: App.Theme.separator
                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: 12
                    spacing: 5
                    Label { text: qsTr("FIRMWARE"); color: App.Theme.textMuted; font.pixelSize: 10; font.weight: Font.DemiBold }
                    Label { text: qsTr("iOS %1 • %2").arg(root.firmwareVersion).arg(root.firmwareBuild); color: App.Theme.text; font.pixelSize: 14; font.weight: Font.DemiBold }
                    Label { Layout.fillWidth: true; text: root.firmwarePath; color: App.Theme.textMuted; font.pixelSize: 11; elide: Text.ElideMiddle }
                }
            }

            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 92
                radius: 9
                color: App.Theme.groupedBackground
                border.color: App.Theme.separator
                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: 12
                    spacing: 5
                    Label { text: qsTr("TARGET DEVICE"); color: App.Theme.textMuted; font.pixelSize: 10; font.weight: Font.DemiBold }
                    Label {
                        Layout.fillWidth: true
                        text: root.targetName
                        color: App.Theme.text
                        font.pixelSize: 14
                        font.weight: Font.DemiBold
                        elide: Text.ElideRight
                    }
                    Label {
                        Layout.fillWidth: true
                        text: root.targetDetail
                        color: App.Theme.textMuted
                        font.pixelSize: 11
                        elide: Text.ElideMiddle
                    }
                }
            }
        }

        Rectangle {
            Layout.fillWidth: true
            implicitHeight: progressLayout.implicitHeight + 22
            radius: 9
            color: App.Theme.softBg
            border.color: App.Theme.separator

            ColumnLayout {
                id: progressLayout
                anchors.fill: parent
                anchors.margins: 11
                spacing: 8

                RowLayout {
                    Layout.fillWidth: true
                    Label {
                        Layout.fillWidth: true
                        text: root.restoreState.stage || qsTr("Ready to restore")
                        color: App.Theme.text
                        font.pixelSize: 13
                        font.weight: Font.DemiBold
                    }
                    Label {
                        visible: root.running
                        text: qsTr("Step %1 of %2").arg(root.restoreState.stageIndex || 1).arg(root.restoreState.stageCount || 8)
                        color: App.Theme.textMuted
                        font.pixelSize: 11
                    }
                }
                Label {
                    Layout.fillWidth: true
                    text: root.restoreState.detail || qsTr("Ready to restore the selected device.")
                    color: root.phase === "failed" ? App.Theme.systemRed : App.Theme.textMuted
                    font.pixelSize: 11
                    wrapMode: Text.WordWrap
                }
                ProgressBar {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 7
                    from: 0
                    to: 1
                    value: Number(root.restoreState.progress || 0)
                    indeterminate: root.running && !!root.restoreState.indeterminate
                }
                Label {
                    Layout.fillWidth: true
                    visible: text.length > 0
                    text: root.progressDetail()
                    color: App.Theme.textMuted
                    font.pixelSize: 10
                    elide: Text.ElideMiddle
                }
                RowLayout {
                    Layout.fillWidth: true

                    spacing: 5
                    Repeater {
                        model: root.stageLabels
                        delegate: Rectangle {
                            required property string modelData
                            required property int index
                            Layout.fillWidth: true
                            Layout.preferredHeight: 22
                            radius: 5
                            color: root.phase === "completed" ? App.Theme.systemGreen
                                   : (index + 1) < Number(root.restoreState.stageIndex || 0) ? App.Theme.systemGreen
                                   : (index + 1) === Number(root.restoreState.stageIndex || 0) ? App.Theme.systemBlue
                                   : App.Theme.elevatedSurface
                            Label {
                                anchors.centerIn: parent
                                text: qsTr("%1. %2").arg(index + 1).arg(modelData)
                                color: (index + 1) <= Number(root.restoreState.stageIndex || 0) ? "white" : App.Theme.textMuted
                                font.pixelSize: 9
                                elide: Text.ElideRight
                                width: parent.width - 6
                                horizontalAlignment: Text.AlignHCenter
                            }
                        }
                    }
                }
            }
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.fillHeight: true
            radius: 8
            color: App.Theme.rowSurface
            border.color: App.Theme.separator
            clip: true

            ColumnLayout {
                anchors.fill: parent
                anchors.margins: 10
                spacing: 6
                RowLayout {
                    Layout.fillWidth: true
                    Label { text: qsTr("Restore Activity"); color: App.Theme.text; font.weight: Font.DemiBold }
                    Item { Layout.fillWidth: true }
                    IconToolButton {
                        text: qsTr("Copy Logs")
                        toolTipText: text
                        Accessible.name: text
                        icon.source: "qrc:/resources/icons/idescriptor-copy-logs.svg"
                        enabled: activityLog.count > 0
                        onClicked: QmlUtils.copy_to_clipboard(FlashManager.logs_text())
                    }
                }
                ListView {
                    id: activityLog
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    model: FlashManager.log_model
                    clip: true
                    spacing: 3
                    ScrollBar.vertical: ScrollBar {}
                    onCountChanged: if (count > 0) positionViewAtEnd()
                    delegate: RowLayout {
                        required property string timestamp
                        required property string level
                        required property string message
                        width: ListView.view.width
                        spacing: 8
                        Label { text: timestamp; color: App.Theme.textMuted; font.family: Qt.platform.os === "windows" ? "Consolas" : "Menlo"; font.pixelSize: 10 }
                        Label {
                            text: level.toUpperCase()
                            color: level === "error" ? App.Theme.systemRed
                                   : level === "warning" ? App.Theme.systemOrange : App.Theme.systemBlue
                            font.family: Qt.platform.os === "windows" ? "Consolas" : "Menlo"
                            font.pixelSize: 9
                            font.weight: Font.DemiBold
                            Layout.preferredWidth: 54
                        }
                        Label { Layout.fillWidth: true; text: message; color: App.Theme.textMuted; font.family: Qt.platform.os === "windows" ? "Consolas" : "Menlo"; font.pixelSize: 10; wrapMode: Text.WrapAnywhere }
                    }
                    Label {
                        anchors.centerIn: parent
                        visible: activityLog.count === 0
                        text: qsTr("Detailed restore logs will appear here.")
                        color: App.Theme.textMuted
                        font.pixelSize: 11
                    }
                }
            }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 10
            Label {
                Layout.fillWidth: true
                visible: !root.running && !root.udid.length && !root.ecid.length
                text: qsTr("Connect a %1 over USB, in normal, recovery, or DFU mode.").arg(root.firmwareIdentifier)
                color: App.Theme.systemOrange
                font.pixelSize: 11
                wrapMode: Text.WordWrap
            }
            Item { Layout.fillWidth: true; visible: root.running || root.udid.length > 0 || root.ecid.length > 0 }
            Button {
                visible: root.running
                enabled: !!root.restoreState.canCancel
                text: qsTr("Cancel Restore")
                onClicked: FlashManager.cancel_flash()
            }
            Button {
                visible: !root.running && !root.finished
                enabled: !FlashManager.busy && (root.udid.length > 0 || root.ecid.length > 0)
                text: qsTr("Erase and Restore")
                onClicked: root.requestRestore()
            }
            Button {
                visible: root.finished && root.phase !== "completed"
                enabled: !FlashManager.busy && (root.udid.length > 0 || root.ecid.length > 0)
                text: qsTr("Try Again")
                onClicked: root.requestRestore()
            }
        }
    }
}
