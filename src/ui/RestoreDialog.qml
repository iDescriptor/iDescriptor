// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import "./base"
import "." as App

AnimatedDialog {
    id: dialog

    required property string backupRoot
    // the connected device the backup will be restored onto
    required property string targetUdid
    required property string targetName

    // the backup to restore from, it can belong to a different device
    property string selectedBackupUdid: ""
    property string selectedBackupName: ""
    property bool selectedBackupEncrypted: false
    readonly property bool restoringFromOtherDevice: selectedBackupUdid !== ""
        && selectedBackupUdid !== targetUdid

    readonly property int settingsPage: 0
    readonly property int resultPage: 1
    readonly property bool dismissalLocked: operationInProgress

    property int contentPage: settingsPage
    property bool operationInProgress: false
    property bool operationCompleted: false
    property bool operationSucceeded: false

    modal: true
    focus: true
    standardButtons: Dialog.NoButton
    closePolicy: operationInProgress
        ? Popup.NoAutoClose
        : Popup.CloseOnEscape | Popup.CloseOnPressOutside
    width: Math.min(parent ? parent.width - 48 : 520, 520)
    height: 570
    padding: 24

    function resetFlow() {
        contentPage = settingsPage
        operationInProgress = false
        operationCompleted = false
        operationSucceeded = false
        rebootToggle.checked = true
        copyToggle.checked = false
        restoreSettingsToggle.checked = true
        systemToggle.checked = true
        removeToggle.checked = false
        passwordField.text = ""
        stateView.errorText = qsTr("The restore operation could not be completed.")
        stateView.viewState = StateView.State.Content

        // prefer the device's own backup, otherwise fall back to the first one
        let index = backupCombo.indexOfValue(targetUdid)
        if (index < 0 && backupCombo.count > 0)
            index = 0
        backupCombo.currentIndex = index
        selectBackupAt(index)
    }

    function selectBackupAt(index) {
        if (index < 0) {
            selectedBackupUdid = ""
            selectedBackupName = ""
            selectedBackupEncrypted = false
            return
        }

        const udid = backupCombo.valueAt(index)
        const metadata = backupManager.get_backup_metadata(udid, backupRoot)
        if (!metadata || !metadata.success) {
            App.Helpers.showError(dialog, qsTr("The backup metadata could not be read."))
            selectedBackupUdid = ""
            selectedBackupName = ""
            selectedBackupEncrypted = false
            return
        }

        selectedBackupUdid = udid
        selectedBackupName = backupCombo.textAt(index) || udid
        selectedBackupEncrypted = metadata.IsEncrypted === true
        passwordField.text = ""
    }

    function backupLabel(udid, deviceName, version) {
        let label = deviceName || udid
        if (udid === targetUdid)
            label = qsTr("%1 (this device)").arg(label)
        if (version)
            label = qsTr("%1 · iOS %2").arg(label).arg(version)
        return label
    }

    function requestClose() {
        if (!operationInProgress)
            dialog.close()
    }

    function requestRestore() {
        if (!selectedBackupUdid || !targetUdid || operationInProgress)
            return

        if (selectedBackupEncrypted && passwordField.text.length === 0) {
            App.Helpers.showWarning(dialog, qsTr("Enter the backup password before restoring this encrypted backup."))
            passwordField.forceActiveFocus()
            return
        }

        App.Helpers.messageBox(
            dialog,
            qsTr("Confirm Restore"),
            restoringFromOtherDevice
                ? qsTr("Restore the backup of “%1” onto “%2”? This backup was made on a different device. Existing data on “%2” will be replaced.")
                    .arg(selectedBackupName || selectedBackupUdid)
                    .arg(targetName || targetUdid)
                : qsTr("Restore “%1” to its matching device? Existing device data may be replaced.")
                    .arg(selectedBackupName || selectedBackupUdid),
            MessageDialog.Yes | MessageDialog.No,
            function(button) {
                if (button === MessageDialog.Yes)
                    dialog.startRestore()
            })
    }

    function startRestore() {
        if (!selectedBackupUdid || !targetUdid || operationInProgress)
            return

        contentPage = resultPage
        operationInProgress = true
        operationCompleted = false
        operationSucceeded = false
        stateView.viewState = StateView.State.Loading

        backupManager.start_restore(
            backupRoot,
            targetUdid,
            selectedBackupUdid,
            selectedBackupEncrypted ? passwordField.text : "",
            rebootToggle.checked,
            copyToggle.checked,
            !restoreSettingsToggle.checked,
            systemToggle.checked,
            removeToggle.checked)
    }

    onOpened: resetFlow()

    Keys.onEscapePressed: function(event) {
        dialog.requestClose()
        event.accepted = true
    }

    Connections {
        target: backupManager

        function onOperationFinished(operation, operationUdid, success, errorCode, errorString) {
            if (operation !== "restore"
                    || operationUdid !== dialog.targetUdid
                    || !dialog.operationInProgress)
                return

            dialog.operationInProgress = false
            dialog.operationCompleted = true
            dialog.operationSucceeded = success

            if (success) {
                stateView.viewState = StateView.State.Content
                App.Helpers.showInfo(
                    dialog,
                    qsTr("“%1” was restored successfully.")
                        .arg(dialog.selectedBackupName || dialog.selectedBackupUdid))
            } else {
                if (errorCode === 211) {
                    stateView.errorText = qsTr("Find My iPhone must be turned off before this backup can be restored. On the device, open Settings, tap your name, then choose Find My → Find My iPhone and turn it off.")
                    App.Helpers.messageBox(
                        dialog,
                        qsTr("Turn Off Find My iPhone"),
                        stateView.errorText)
                } else {
                    stateView.errorText = errorString || qsTr("iDescriptor could not restore “%1”. Keep the device connected and try again.")
                        .arg(dialog.selectedBackupName || dialog.selectedBackupUdid)
                }
                stateView.viewState = StateView.State.Error
            }
        }
    }

    Overlay.modal: Rectangle {
        color: Qt.rgba(0, 0, 0, App.Theme.darkMode ? 0.52 : 0.30)
    }

    contentItem: ColumnLayout {
        spacing: 16

        Label {
            Layout.fillWidth: true
            text: dialog.operationInProgress
                ? qsTr("Restoring %1").arg(dialog.selectedBackupName || dialog.selectedBackupUdid)
                : dialog.operationSucceeded
                    ? qsTr("Restore Complete")
                    : qsTr("Restore Backup")
            color: App.Theme.text
            font.pixelSize: 20
            font.weight: Font.DemiBold
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
        }

        StateView {
            id: stateView
            Layout.fillWidth: true
            Layout.fillHeight: true
            autoSwitchContent: false
            retryable: dialog.operationCompleted && !dialog.operationSucceeded
            cancelable: dialog.operationCompleted && !dialog.operationSucceeded
            retryText: qsTr("Try Again")
            cancelText: qsTr("Close")
            onRetryRequested: dialog.requestRestore()
            onCancelRequested: dialog.requestClose()

            contentItem: StackLayout {
                anchors.fill: parent
                currentIndex: dialog.contentPage

                ScrollView {
                    clip: true
                    contentWidth: availableWidth

                    ColumnLayout {
                        width: parent.width
                        spacing: 14

                        Label {
                            Layout.fillWidth: true
                            text: qsTr("Choose a backup to restore onto “%1”.")
                                .arg(dialog.targetName || dialog.targetUdid)
                            color: App.Theme.text
                            wrapMode: Text.WordWrap
                        }

                        ComboBox {
                            id: backupCombo
                            Layout.fillWidth: true
                            model: backupManager.backup_model
                            valueRole: "udid"
                            textRole: "device_name"
                            // use the dialog's selection instead of currentText to avoid a binding loop
                            displayText: dialog.selectedBackupUdid === ""
                                ? qsTr("No backups available")
                                : dialog.backupLabel(dialog.selectedBackupUdid, dialog.selectedBackupName, "")
                            onActivated: function(index) {
                                dialog.selectBackupAt(index)
                            }

                            delegate: ItemDelegate {
                                required property int index
                                required property string udid
                                required property string device_name
                                required property string version

                                width: backupCombo.width
                                highlighted: backupCombo.highlightedIndex === index
                                contentItem: ColumnLayout {
                                    spacing: 1

                                    Label {
                                        Layout.fillWidth: true
                                        text: dialog.backupLabel(udid, device_name, version)
                                        color: App.Theme.text
                                        elide: Text.ElideRight
                                    }

                                    Label {
                                        Layout.fillWidth: true
                                        text: udid
                                        color: App.Theme.textMuted
                                        font.pixelSize: 11
                                        elide: Text.ElideMiddle
                                    }
                                }
                            }
                        }

                        Label {
                            Layout.fillWidth: true
                            visible: dialog.restoringFromOtherDevice
                            text: qsTr("This backup was made on a different device. Its data, apps and settings will replace those on “%1”. The backup cannot be newer than the iOS version installed on this device.")
                                .arg(dialog.targetName || dialog.targetUdid)
                            color: App.Theme.textMuted
                            font.pixelSize: 12
                            wrapMode: Text.WordWrap
                        }

                        Frame {
                            Layout.fillWidth: true
                            padding: 12

                            background: Rectangle {
                                radius: 6
                                color: Qt.rgba(0.95, 0.64, 0.12, App.Theme.darkMode ? 0.16 : 0.12)
                                border.color: Qt.rgba(0.95, 0.64, 0.12, 0.55)
                            }

                            contentItem: Label {
                                text: qsTr("Before restoring, turn off Find My iPhone on the device in Settings → [your name] → Find My → Find My iPhone.")
                                color: App.Theme.text
                                wrapMode: Text.WordWrap
                            }
                        }

                        Label {
                            Layout.fillWidth: true
                            text: qsTr("Restore Settings")
                            color: App.Theme.textMuted
                            font.pixelSize: 12
                        }

                        CheckBox {
                            id: rebootToggle
                            Layout.fillWidth: true
                            text: qsTr("Restart device after restore")
                            checked: true
                        }

                        CheckBox {
                            id: copyToggle
                            Layout.fillWidth: true
                            text: qsTr("Create a safety copy before restoring")
                            checked: false
                        }

                        CheckBox {
                            id: restoreSettingsToggle
                            Layout.fillWidth: true
                            text: qsTr("Restore device settings from backup")
                            checked: true
                        }

                        CheckBox {
                            id: systemToggle
                            Layout.fillWidth: true
                            text: qsTr("Restore system files")
                            checked: true
                        }

                        CheckBox {
                            id: removeToggle
                            Layout.fillWidth: true
                            text: qsTr("Remove items not restored")
                            checked: false
                        }

                        CheckBox {
                            id: encryptedBackup
                            Layout.fillWidth: true
                            text: qsTr("Encrypted backup")
                            checked: dialog.selectedBackupEncrypted
                            enabled: false
                        }

                        TextField {
                            id: passwordField
                            Layout.fillWidth: true
                            visible: dialog.selectedBackupEncrypted
                            echoMode: TextInput.Password
                            placeholderText: qsTr("Backup password")
                            onAccepted: dialog.requestRestore()
                        }

                        RowLayout {
                            Layout.fillWidth: true
                            Layout.topMargin: 8
                            spacing: 10

                            Item { Layout.fillWidth: true }

                            Button {
                                text: qsTr("Cancel")
                                onClicked: dialog.requestClose()
                            }

                            Button {
                                text: qsTr("Restore")
                                highlighted: true
                                enabled: dialog.selectedBackupUdid !== ""
                                    && (!dialog.selectedBackupEncrypted || passwordField.text.length > 0)
                                onClicked: dialog.requestRestore()
                            }
                        }
                    }
                }

                ColumnLayout {
                    spacing: 14

                    Item { Layout.fillHeight: true }

                    Label {
                        Layout.fillWidth: true
                        text: qsTr("The backup was restored successfully. The device may restart to finish applying the restored data.")
                        color: App.Theme.text
                        horizontalAlignment: Text.AlignHCenter
                        wrapMode: Text.WordWrap
                    }

                    Button {
                        Layout.alignment: Qt.AlignHCenter
                        text: qsTr("Close")
                        enabled: dialog.operationCompleted
                        highlighted: true
                        onClicked: dialog.requestClose()
                    }

                    Item { Layout.fillHeight: true }
                }
            }
        }

        ProgressBar {
            Layout.fillWidth: true
            visible: dialog.operationInProgress
            indeterminate: true
        }

        Label {
            Layout.fillWidth: true
            visible: dialog.operationInProgress
            text: qsTr("Keep the device connected until the restore finishes.")
            color: App.Theme.textMuted
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
        }
    }
}
