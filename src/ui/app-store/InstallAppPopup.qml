// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import "../"
import "../base"

AnimatedDialog {
    id: root
    required property string bundleId
    required property string appName
    property string iconSource: ""
    property string appDescription: ""
    property string resolvedDescription: ""
    property string resolvedBundleId: ""
    property int metadataRequestGeneration: 0
    readonly property string descriptionPreview: Helpers.appDescriptionPreview(
        (resolvedBundleId === bundleId ? resolvedDescription : "") || appDescription)
    property string resolvedIconSource: ""
    readonly property string effectiveIconSource: iconSource.length > 0
                                                   ? iconSource
                                                   : (resolvedBundleId === bundleId ? resolvedIconSource : "")

    padding: 24

    modal: true
    focus: true
    width: 540
    title: qsTr("Install App")
    standardButtons: Dialog.NoButton
    closePolicy: root.installing ? Popup.NoAutoClose : Popup.CloseOnPressOutside

    readonly property bool hasDevice: DeviceContext.devices.count > 0
    property int selectedDeviceIndex: hasDevice ? 0 : -1
    readonly property var selectedDevice:
        selectedDeviceIndex >= 0 && selectedDeviceIndex < DeviceContext.devices.count
        ? DeviceContext.devices.get(selectedDeviceIndex)
        : null
    property string taskId: ""
    property bool installing: false
    property real progress: 0
    property string stateText: ""
    property string errorText: ""
    property string phase: ""
    property string resolvedAppName: ""
    readonly property string effectiveAppName: root.appName.length > 0
                                                ? root.appName
                                                : (root.resolvedBundleId === root.bundleId ? root.resolvedAppName : "")
    readonly property bool completed: !installing && phase === "done" && !errorText.length
    readonly property string progressText: progress >= 0
                                                   ? qsTr("%1%").arg(Math.round(progress * 100))
                                                   : ""

    function clearAppMetadata() {
        // Invalidate pending requests, including an earlier request for the same app.
        metadataRequestGeneration += 1
        resolvedBundleId = ""
        resolvedAppName = ""
        resolvedIconSource = ""
        resolvedDescription = ""
    }

    function resolveAppMetadata() {
        clearAppMetadata()
        if (bundleId.length === 0 || (appName.length > 0 && iconSource.length > 0 && appDescription.length > 0))
            return

        const requestedBundleId = bundleId
        const requestGeneration = metadataRequestGeneration
        Helpers.fetch_app(requestedBundleId, function(app, error) {
            // Ignore a response for an app that is no longer displayed.
            if (root.metadataRequestGeneration !== requestGeneration
                    || root.bundleId !== requestedBundleId || error || !app)
                return
            root.resolvedAppName = app.trackName || ""
            root.resolvedIconSource = app.artworkUrl100 || app.artworkUrl512 || app.artworkUrl60 || ""
            root.resolvedDescription = app.description || ""
            root.resolvedBundleId = requestedBundleId
        })
    }

    function resetState() {
        taskId = ""
        installing = false
        progress = 0
        stateText = ""
        errorText = ""
        phase = ""
        selectedDeviceIndex = hasDevice ? 0 : -1
    }

    function requestClose() {
        if (installing) {
            cancelConfirmation.open()
            return
        }
        close()
    }

    function startInstall() {
        if (!selectedDevice)
            return

        errorText = ""
        progress = -1
        phase = "download"
        stateText = qsTr("Preparing IPA download...")

        const id = apps.install_app(bundleId, selectedDevice.udid)
        if (!id || !id.length) {
            progress = 0
            errorText = qsTr("The App Store service is not initialized.")
            stateText = errorText
            return
        }

        taskId = id
        installing = true
    }

    onBundleIdChanged: {
        if (visible)
            resolveAppMetadata()
        else
            clearAppMetadata()
    }

    onAboutToShow: {
        resetState()
        resolveAppMetadata()
    }
    onClosed: {
        clearAppMetadata()
        if (taskId.length)
            apps.cancel_task(taskId)
        taskId = ""
        installing = false
    }

    Keys.onEscapePressed: function(event) {
        root.requestClose()
        event.accepted = true
    }

    Overlay.modal: Rectangle {
        color: Qt.rgba(0, 0, 0, 0.35)
    }

    MessageDialog {
        id: cancelConfirmation
        title: qsTr("Cancel installation?")
        text: qsTr("The IPA download or installation is still in progress. Do you want to cancel it and close this dialog?")
        buttons: MessageDialog.Yes | MessageDialog.No
        onButtonClicked: function(button, role) {
            if (button !== MessageDialog.Yes)
                return

            const id = root.taskId
            root.taskId = ""
            root.installing = false
            if (id.length)
                apps.cancel_task(id)
            root.close()
        }
    }

    Connections {
        target: apps

        function onInstallAppProgress(taskId, progress, phase) {
            if (taskId !== root.taskId)
                return

            root.installing = true
            root.progress = progress
            root.phase = phase
            root.stateText = phase === "install"
                    ? qsTr("Installing IPA on device...")
                    : qsTr("Downloading IPA...")
        }

        function onInstallAppFinished(taskId, success, error) {
            if (taskId !== root.taskId)
                return

            root.taskId = ""
            root.installing = false
            root.progress = success ? 1 : 0
            root.errorText = success ? "" : (error || qsTr("Installation failed."))
            root.phase = success ? "done" : "error"
            root.stateText = success ? qsTr("Installation finished") : root.errorText
        }
    }

    contentItem: StateView {
        viewState: root.effectiveAppName.length ? StateView.State.Content : StateView.State.Loading
        autoSwitchContent: false
        implicitHeight: installContent.implicitHeight

        contentItem: ColumnLayout {
            id: installContent
            anchors.fill: parent
            spacing: 18

            RowLayout {
                Layout.fillWidth: true
                spacing: 16

                IconLoader {
                    Layout.preferredWidth: 64
                    Layout.preferredHeight: 64
                    radius: 15
                    iconSource: root.effectiveIconSource
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 3

                    Label {
                        Layout.fillWidth: true
                        text: root.effectiveAppName
                        color: Theme.text
                        font.pixelSize: 20
                        font.weight: Font.DemiBold
                        elide: Text.ElideRight
                    }

                    Label {
                        Layout.fillWidth: true
                        text: root.bundleId
                        color: Theme.textMuted
                        font.pixelSize: 12
                        elide: Text.ElideMiddle
                    }

                    Label {
                        Layout.fillWidth: true
                        text: root.descriptionPreview
                        visible: text.length > 0
                        textFormat: Text.PlainText
                        color: Theme.textMuted
                        font.pixelSize: 13
                        wrapMode: Text.WordWrap
                        maximumLineCount: 2
                        elide: Text.ElideRight
                    }
                }
            }

            Rectangle {
                Layout.fillWidth: true
                implicitHeight: deviceColumn.implicitHeight + 28
                radius: 14
                color: Theme.groupedBackground
                border.color: Theme.separator
                border.width: 1

                ColumnLayout {
                    id: deviceColumn
                    anchors.fill: parent
                    anchors.margins: 14
                    spacing: 9

                    RowLayout {
                        Layout.fillWidth: true

                        Label {
                            Layout.fillWidth: true
                            text: qsTr("Install on")
                            color: Theme.text
                            font.weight: Font.DemiBold
                        }
                    }

                    ComboBox {
                        id: deviceCombo
                        Layout.fillWidth: true
                        enabled: root.hasDevice && !root.installing
                        model: DeviceContext.devices
                        textRole: "text"
                        valueRole: "udid"
                        currentIndex: root.selectedDeviceIndex

                        onActivated: function(index) {
                            root.selectedDeviceIndex = index
                        }

                        onCountChanged: {
                            if (count === 0)
                                root.selectedDeviceIndex = -1
                            else if (root.selectedDeviceIndex < 0 || root.selectedDeviceIndex >= count)
                                root.selectedDeviceIndex = 0
                        }
                    }

                    Label {
                        Layout.fillWidth: true
                        visible: !root.hasDevice
                        text: qsTr("Connect and trust an Apple device to continue.")
                        color: Theme.textMuted
                        font.pixelSize: 12
                        wrapMode: Text.WordWrap
                    }
                }
            }

            Rectangle {
                Layout.fillWidth: true
                implicitHeight: statusColumn.implicitHeight + 28
                radius: 14
                visible: root.installing || root.stateText.length > 0
                color: root.errorText.length
                       ? Qt.rgba(Theme.systemRed.r, Theme.systemRed.g, Theme.systemRed.b, Theme.darkMode ? 0.13 : 0.08)
                       : root.completed
                         ? Qt.rgba(Theme.systemGreen.r, Theme.systemGreen.g, Theme.systemGreen.b, Theme.darkMode ? 0.13 : 0.08)
                         : Theme.softBg
                border.color: root.errorText.length
                              ? Qt.rgba(Theme.systemRed.r, Theme.systemRed.g, Theme.systemRed.b, 0.28)
                              : root.completed
                                ? Qt.rgba(Theme.systemGreen.r, Theme.systemGreen.g, Theme.systemGreen.b, 0.28)
                                : Theme.separator
                border.width: 1

                ColumnLayout {
                    id: statusColumn
                    anchors.fill: parent
                    anchors.margins: 14
                    spacing: 10

                    RowLayout {
                        Layout.fillWidth: true

                        Label {
                            Layout.fillWidth: true
                            text: root.stateText
                            color: root.errorText.length ? Theme.dangerText : Theme.text
                            font.weight: Font.DemiBold
                            wrapMode: Text.WordWrap
                        }

                        Label {
                            visible: root.installing && root.progress >= 0
                            text: root.progressText
                            color: Theme.textMuted
                        }
                    }

                    ProgressBar {
                        Layout.fillWidth: true
                        visible: root.installing
                        indeterminate: root.progress < 0
                        from: 0
                        to: 1
                        value: Math.max(0, root.progress)
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        visible: root.installing
                        spacing: 8

                        Rectangle {
                            Layout.fillWidth: true
                            Layout.preferredHeight: 3
                            radius: 2
                            color: root.phase === "download" || root.phase === "install"
                                   ? Theme.systemBlue : Theme.separator
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            Layout.preferredHeight: 3
                            radius: 2
                            color: root.phase === "install" ? Theme.systemBlue : Theme.separator
                        }
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        visible: root.installing

                        Label {
                            Layout.fillWidth: true
                            text: qsTr("Download")
                            color: root.phase === "download" ? Theme.text : Theme.textMuted
                            font.pixelSize: 11
                        }

                        Label {
                            Layout.fillWidth: true
                            text: qsTr("Install")
                            color: root.phase === "install" ? Theme.text : Theme.textMuted
                            font.pixelSize: 11
                            horizontalAlignment: Text.AlignRight
                        }
                    }
                }
            }

            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 1
                color: Theme.separator
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 10

                Item { Layout.fillWidth: true }

                Button {
                    text: root.installing ? qsTr("Cancel") : qsTr("Close")
                    flat: true
                    onClicked: root.requestClose()
                }

                Button {
                    text: root.completed ? qsTr("Install Again") : qsTr("Install")
                    highlighted: true
                    enabled: !root.installing && root.bundleId.length > 0 && root.selectedDevice
                    onClicked: root.startInstall()
                }
            }
        }
    }
}
