// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl
import QtQuick.Dialogs
import QtQuick.Layouts
import "../base"
import "../" as App

ToolWindow {
    id: root
    width: 860
    height: 660
    minimumWidth: 740
    minimumHeight: 540
    title: qsTr("Backups - iDescriptor")

    property string backupRoot: settingsManager.mk_backup_root_path()
    readonly property var state: backupManager.state
    readonly property bool busy: backupManager.busy
    readonly property bool loading: state.loading
    property string restoreTargetUdid: ""
    property string restoreTargetName: ""
    // any backup (including ones from other devices) can be restored onto a connected device
    readonly property bool hasAnyBackup: backupManager.backup_model.rowCount() > 0
    property string eraseUdid: ""
    property string eraseName: ""
    property var deviceBackupSections: ({})
    function deviceName(device) {
        if (!device)
            return qsTr("Unknown")

        if (device.info && device.info.marketing_name)
            return device.info.marketing_name

        if (device.text)
            return device.text

        return device.udid || qsTr("Unknown")
    }

    function shortUdid(value) {
        if (!value || value.length <= 14)
            return value || ""
        return value.slice(0, 10) + "..."
    }

    function unknownBackupCount() {
        let total = 0
        for (let i = 0; i < backupManager.backup_model.count; ++i) {
            const item = backupManager.backup_model.get(i)
            if (!App.DeviceContext.getDevice(item.udid))
                total += 1
        }
        return total
    }


    // udid is the connected device to restore onto, the backup is picked in RestoreDialog
    function selectBackupForRestore(udid, title) {
        root.restoreTargetUdid = udid
        root.restoreTargetName = title || udid
        Qt.callLater(function() {
            restoreDialog.open()
        })
    }


    function openBackupDetails(udid, title) {
        const metadata = backupManager.get_backup_metadata(udid, root.backupRoot)
        if (metadata && metadata.success && metadata.IsEncrypted === true) {
            root.openBackupDetailsWithoutDevice(udid, title, true)
            return
        }

        nav.push(detailsHostComponent, {
            sectionUdid: udid,
            sectionTitle: title || udid
        })
    }

    function openBackupDetailsWithoutDevice(udid, title, encrypted) {
        nav.push(detailsHostComponentWithoutDevice, {
            udid,
            title: title || udid,
            encrypted
        })
    }

    function openBackupAction(udid, title, iconPath, wireless, forceFullBackup) {
        nav.push(backupActionHostComponent, {
            sectionUdid: udid,
            sectionTitle: title || udid,
            sectionIconPath: iconPath || "qrc:/resources/icons/iphone_gen1.svg",
            sectionWireless: wireless === true,
            sectionForceFullBackup: forceFullBackup === true
        })
    }

    // function refreshBackups() {
    //     backupManager.refresh(backupRoot)
    // }

    function chooseBackupRoot(path) {
        backupRoot = path
        settingsManager.set_backup_root_path(path)
        backupManager.init(path)
    }

    function showExperimentalBackupWarning() {
        if (!settingsManager.backup_experimental_warning_acknowledged())
            experimentalBackupDialog.open()
    }

    Component.onCompleted: {
        backupManager.init(root.backupRoot)
        Qt.callLater(root.showExperimentalBackupWarning)
        // refreshBackups()
    }

    onClosing: function(close) {
        if ((eraseDialog.visible && eraseDialog.dismissalLocked)
                || (restoreDialog.visible && restoreDialog.dismissalLocked)) {
            close.accepted = false
            return
        }
        App.ClosingHandler.handler("backup", close, root)
    }

    Connections {
        target: backupManager

        function onOperationFinished(operation, udid, success, errorCode, errorString) {
            if (operation !== "restore")
                return

            const section = root.deviceBackupSections[udid]
            if (!section)
                return

            // TODO: maybe show a toast or something
            section.handleReset()
        }

        function onProgressUpdate(udid, progress) {
            const section = root.deviceBackupSections[udid]
            if (!section || !section.isRestoring)
                return

            section.updateProgress(progress)
        }
    }

    FolderDialog {
        id: backupRootDialog
        title: qsTr("Select Backup Directory")
        onAccepted: root.chooseBackupRoot(QmlUtils.url_to_path(selectedFolder))
    }

    AnimatedDialog {
        id: experimentalBackupDialog
        anchors.centerIn: Overlay.overlay
        modal: true
        focus: true
        title: qsTr("Experimental Backup Feature")
        standardButtons: Dialog.NoButton
        closePolicy: Popup.NoAutoClose
        width: Math.min(root.width - 48, 520)
        height: 450
        padding: 24

        Overlay.modal: Rectangle {
            color: Qt.rgba(0, 0, 0, App.Theme.darkMode ? 0.52 : 0.30)
        }

        contentItem: ColumnLayout {
            spacing: 16

            Rectangle {
                Layout.alignment: Qt.AlignHCenter
                Layout.preferredWidth: 64
                Layout.preferredHeight: 64
                radius: width / 2
                color: Qt.rgba(1, 0.59, 0, App.Theme.darkMode ? 0.20 : 0.12)

                Label {
                    anchors.centerIn: parent
                    text: "!"
                    color: App.Theme.systemOrange
                    font.pixelSize: 34
                    font.bold: true
                }
            }

            Label {
                Layout.fillWidth: true
                text: qsTr("Backups and restores are experimental")
                color: App.Theme.text
                font.pixelSize: 20
                font.bold: true
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.WordWrap
            }

            Label {
                Layout.fillWidth: true
                text: qsTr("Proceed with care. Unexpected device, connection, or storage problems may cause an incomplete backup, a failed restore, or data loss. Keep another trusted backup and do not rely on iDescriptor as the only copy of important data.")
                color: App.Theme.text
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.WordWrap
            }

            Label {
                Layout.fillWidth: true
                text: qsTr("This notice will only be shown once. Pressing OK confirms that you understand the risks.")
                color: App.Theme.textMuted
                font.pixelSize: 12
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.WordWrap
            }

            Item {
                Layout.fillHeight: true
            }

            Button {
                Layout.alignment: Qt.AlignHCenter
                Layout.preferredWidth: 120
                text: qsTr("OK")
                highlighted: true
                onClicked: {
                    settingsManager.set_backup_experimental_warning_acknowledged(true)
                    experimentalBackupDialog.close()
                }
            }
        }
    }

    App.RestoreDialog {
        id: restoreDialog
        anchors.centerIn: Overlay.overlay
        backupRoot: root.backupRoot
        targetUdid: root.restoreTargetUdid
        targetName: root.restoreTargetName
        onClosed: {
            root.restoreTargetUdid = ""
            root.restoreTargetName = ""
        }
    }

    App.EraseDialog {
        id: eraseDialog
        anchors.centerIn: Overlay.overlay
        udid: root.eraseUdid
        displayName: root.eraseName
        backupRoot: root.backupRoot
        onClosed: {
            root.eraseUdid = ""
            root.eraseName = ""
        }
    }

    Component {
        id: deviceBackupSection

        ColumnLayout {
            id: sectionRoot
            property bool backupExists:false
            property string sectionUdid: ""
            property string sectionTitle: ""
            property string sectionIconPath: ""
            property bool sectionWireless: false
            property bool isRestoring: false
            property real progress: 0.0
            property bool cancelRequested: false


            signal detailsRequested(string udid, string title)
            signal backupRequested(string udid, string title, string iconPath, bool wireless, bool forceFullBackup)

            function handleReset() {
                isRestoring = false
                progress = 0.0
                cancelRequested = false
            }

            function updateProgress(value, time_remaining) {
                sectionRoot.progress = value
            }

            Layout.fillWidth: true
            spacing: 0

            ItemDelegate {
                Layout.fillWidth: true
                implicitHeight: 56
                onClicked: {
                    if (sectionRoot.backupExists) {
                        sectionRoot.detailsRequested(sectionRoot.sectionUdid, sectionRoot.sectionTitle)
                    }
                }


                contentItem : StackLayout {
                    currentIndex: sectionRoot.isRestoring ? 1 : 0

                    // Normal state
                    RowLayout {
                        anchors.fill: parent
                        spacing: 12


                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 1

                            Label {
                                Layout.fillWidth: true
                                text: sectionTitle
                                color: App.Theme.text
                                font.weight: Font.DemiBold
                                elide: Text.ElideRight
                            }

                            Label {
                                Layout.fillWidth: true
                                text: sectionRoot.backupExists ? qsTr("Backup exists") : qsTr("No backup")
                                color: App.Theme.textMuted
                                font.pixelSize: 12
                            }
                        }

                        Button {
                            text: qsTr("Restore")
                            highlighted: sectionRoot.backupExists
                            enabled: !root.busy && root.hasAnyBackup
                            visible: root.hasAnyBackup
                            onClicked: {
                                root.selectBackupForRestore(sectionRoot.sectionUdid, sectionRoot.sectionTitle)
                            }
                        }

                        Button {
                            text: !sectionRoot.backupExists ? qsTr("Back Up Now") : qsTr("Update Backup")
                            highlighted: !sectionRoot.backupExists
                            enabled: !root.busy
                            onClicked: {
                              sectionRoot.backupRequested(
                                  sectionRoot.sectionUdid,
                                  sectionRoot.sectionTitle,
                                  sectionRoot.sectionIconPath,
                                  sectionRoot.sectionWireless,
                                  !sectionRoot.backupExists
                              )
                            }
                        }
                        Button {
                            text: qsTr("Erase")
                            enabled: !root.busy
                            palette.text: App.Theme.dangerText
                            onClicked: {
                                root.eraseUdid = sectionRoot.sectionUdid
                                root.eraseName = sectionRoot.sectionTitle
                                Qt.callLater(function() {
                                    eraseDialog.open()
                                })
                            }
                        }

                        Button {
                            visible: sectionRoot.backupExists
                            text: qsTr("Details")
                            onClicked: {
                                sectionRoot.detailsRequested(sectionRoot.sectionUdid, sectionRoot.sectionTitle)
                            }
                        }
                    }

                    // Restore in progress state
                    RowLayout {
                        id: progressRow
                        anchors.fill: parent
                        spacing: 12


                        property string infoText: {
                            switch (true) {
                                case sectionRoot.cancelRequested:
                                    return qsTr("Cancelling…")
                                case sectionRoot.isRestoring:
                                    return qsTr("Restoring…")
                                default:
                                    return ""
                            }
                        }

                        Label {
                            Layout.fillWidth: true
                            text: sectionTitle
                            color: sectionRoot.cancelRequested ? App.Theme.textMuted : App.Theme.text
                            font.weight: Font.DemiBold
                            elide: Text.ElideRight
                        }

                        Label {
                            Layout.fillWidth: true
                            text: progressRow.infoText
                            color: App.Theme.textMuted
                            font.pixelSize: 12
                        }

                        ProgressBar {
                            Layout.fillWidth: true
                            Layout.preferredHeight: 24
                            from: 0
                            value: sectionRoot.progress
                            to: 1
                        }

                        Spinner {
                            running: true
                            Layout.preferredWidth: 24
                            Layout.preferredHeight: 24
                        }

                        Button {
                            text: sectionRoot.cancelRequested ? qsTr("Cancelling…") : qsTr("Cancel")
                            highlighted: true
                            enabled: !sectionRoot.cancelRequested
                            onClicked: {
                                // FIXME: wireup
                                //prompt first
                                sectionRoot.cancelRequested = true
                                const cancelSuccess = backupManager.cancel_operation(sectionUdid)
                                if (cancelSuccess) {
                                    sectionRoot.handleReset()
                                } else {
                                    console.warn("Failed to cancel operation for udid:", sectionUdid)
                                }
                            }
                        }
                    }
                }
            }

        }
    }

    StackView {
        id: nav
        anchors.fill: parent
        initialItem: mainPageComponent
        clip: true

        pushEnter: Transition {
            PropertyAnimation { property: "x"; from: root.width; to: 0; duration: 320; easing.type: Easing.OutCubic }
        }
        pushExit: Transition {
            PropertyAnimation { property: "x"; from: 0; to: -root.width; duration: 320; easing.type: Easing.OutCubic }
            PropertyAnimation { property: "opacity"; from: 1; to: 0.55; duration: 320; easing.type: Easing.OutCubic }
        }
        popEnter: Transition {
            PropertyAnimation { property: "x"; from: -root.width; to: 0; duration: 280; easing.type: Easing.OutCubic }
            PropertyAnimation { property: "opacity"; from: 0.55; to: 1; duration: 280; easing.type: Easing.OutCubic }
        }
        popExit: Transition {
            PropertyAnimation { property: "x"; from: 0; to: nav.width; duration: 280; easing.type: Easing.OutCubic }
        }
    }

    Component {
        id: mainPageComponent

        StateView {
            id: stateView
            anchors.fill: parent
            viewState: StateView.State.Loading

        contentItem: ColumnLayout {
            anchors.fill: parent
            anchors.margins: 24
            spacing: 18

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 4

                Label {
                    Layout.fillWidth: true
                    text: qsTr("Backups")
                    color: App.Theme.text
                    font.pixelSize: 28
                    font.weight: Font.DemiBold
                    elide: Text.ElideRight
                }

                Label {
                    Layout.fillWidth: true
                    text: qsTr("Manage local backups for devices. You can back up, restore, and erase devices from this interface.")
                    color: App.Theme.textMuted
                    font.pixelSize: 14
                    elide: Text.ElideRight
                }
            }

            LocationSelector {
                Layout.fillWidth: true
                labelText: qsTr("Backup Location")
                location: root.backupRoot
                changeEnabled: !root.busy
                onChangeRequested: backupRootDialog.open()
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 10

                Label {
                    Layout.fillWidth: true
                    text: qsTr("Backups")
                    color: App.Theme.textMuted
                    font.pixelSize: 12
                }

                Button {
                    text: qsTr("Refresh")
                    enabled: !root.loading && !root.busy
                    onClicked: {
                        backupManager.init(backupRoot)
                    }
                }
            }

            StateView {
                id: deviceListStateView
                Layout.fillHeight: true
                Layout.fillWidth: true
                autoSwitchContent: false
                autoSwitchDelay: 300
                requestedViewState: root.state.loading ? StateView.State.Loading : StateView.State.Content
                contentItem : Loader {
                    anchors.fill: parent
                    active: !root.state.loading
                    sourceComponent: ScrollView {
                        id: backupsScroll
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        anchors.fill: parent
                        clip: true
                        contentWidth: availableWidth

                        Text {
                            anchors.centerIn: parent
                            text: qsTr("Backups and connected devices will appear here")
                            color: palette.text
                            visible: App.DeviceContext.devices.count === 0 && backupManager.backup_model.rowCount() === 0
                        }

                        ColumnLayout {
                            width: backupsScroll.availableWidth
                            spacing: 15

                            ColumnLayout {
                                Layout.fillWidth: true
                                Repeater {

                                    model: App.DeviceContext.devices
                                    Loader {
                                        id: deviceBackupLoader
                                        Layout.fillWidth: true
                                        width: parent.width
                                        sourceComponent: deviceBackupSection
                                        onLoaded: {
                                            item.sectionUdid = model.udid
                                            item.sectionTitle = model.info.marketing_name
                                            item.sectionIconPath = model.info.placeholder_path
                                            item.sectionWireless = model.info.is_wireless === true
                                            item.backupExists = backupManager.does_backup_exist_for_udid(model.udid)
                                            item.detailsRequested.connect(root.openBackupDetails)
                                            item.backupRequested.connect(root.openBackupAction)
                                            root.deviceBackupSections[model.udid] = item
                                        }
                                    }
                                }
                            }

                            //Unknown Devices
                            ColumnLayout {
                                Layout.fillWidth: true

                                Repeater {
                                    model: backupManager.backup_model

                                    delegate: ItemDelegate {
                                        Layout.fillWidth: true
                                        width: parent.width
                                        // FIXME: this can be handled better with O(1) lookup
                                        visible: !App.DeviceContext.getDevice(model.udid)
                                        height: visible ? implicitHeight : 0
                                        onClicked: {
                                            root.openBackupDetailsWithoutDevice(model.udid)
                                        }
                                        contentItem: Label {
                                            text: model.udid
                                            color: App.Theme.text
                                            elide: Text.ElideRight
                                        }
                                    }
                                }
                            }
                        }

                    }
                }
            }
        }
    }
    }

    Component {
        id: detailsHostComponent

        Item {
            id: itemRoot
            required property string sectionUdid
            required property string sectionTitle

            Loader {
                anchors.fill: parent
                sourceComponent: App.BackupDetails {
                    udid: itemRoot.sectionUdid
                    backupRoot: root.backupRoot
                    title: itemRoot.sectionTitle
                    onBackRequested: nav.pop()
                }
            }
        }
    }

    Component {
        id: backupActionHostComponent

        Item {
            id: itemRoot
            required property string sectionUdid
            required property string sectionTitle
            required property string sectionIconPath
            required property bool sectionWireless
            required property bool sectionForceFullBackup

            Loader {
                anchors.fill: parent
                sourceComponent: App.BackupAction {
                    udid: itemRoot.sectionUdid
                    backupRoot: root.backupRoot
                    title: itemRoot.sectionTitle
                    iconPath: itemRoot.sectionIconPath
                    wireless: itemRoot.sectionWireless
                    initialForceFullBackup: itemRoot.sectionForceFullBackup
                    onBackRequested: nav.pop()
                    onDoneRequested: nav.pop()
                    onBackupRootSelected: function(path) {
                        root.chooseBackupRoot(path)
                    }
                    onDetailsRequested: function() {
                        root.openBackupDetails(itemRoot.sectionUdid, itemRoot.sectionTitle)
                    }
                    onBackupFinished: backupManager.init(root.backupRoot)
                }
            }
        }
    }


    Component {
        id: detailsHostComponentWithoutDevice
        Item {
            id: itemRoot
            required property string udid
            required property string title
            property bool encrypted: false
            Loader {
                anchors.fill: parent
                sourceComponent: App.BackupDetailsWithoutDevice {
                    udid: itemRoot.udid
                    backupRoot: root.backupRoot
                    title: itemRoot.title
                    encrypted: itemRoot.encrypted
                    onBackRequested: nav.pop()
                }
            }
        }
    }
}
