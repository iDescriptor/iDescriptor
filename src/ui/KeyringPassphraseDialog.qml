// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "./base"
import "." as App

AnimatedDialog {
    id: dialog

    modal: true
    focus: true
    standardButtons: Dialog.NoButton
    closePolicy: Popup.NoAutoClose
    anchors.centerIn: Overlay.overlay
    width: Math.min(Overlay.overlay.width - 32, 430)
    height: 330

    property string passphrase: ""
    property string errorText: ""
    property bool busy: false
    property bool passphraseVisible: false
    property bool rememberPassphrase: Qt.platform.os === "windows"

    onOpened: {
        passphrase = ""
        errorText = ""
        busy = false
        passphraseVisible = false
        rememberPassphrase = Qt.platform.os === "windows"
        Qt.callLater(function() { passphraseField.forceActiveFocus() })
    }

    Connections {
        target: apps

        function onKeyringPassphraseFinished(success, error) {
            if (!dialog.visible)
                return
            dialog.busy = false
            if (success) {
                dialog.passphrase = ""
                dialog.close()
            } else {
                dialog.errorText = error || qsTr("Could not unlock the saved ipatool account.")
                passphraseField.selectAll()
                passphraseField.forceActiveFocus()
            }
        }
    }

    contentItem: ColumnLayout {
        anchors.fill: parent
        anchors.margins: 24
        spacing: 12

        Text {
            Layout.fillWidth: true
            text: qsTr("Unlock Saved App Store Account")
            color: App.Theme.text
            font.pixelSize: 18
            font.weight: Font.DemiBold
            horizontalAlignment: Text.AlignHCenter
        }

        Text {
            Layout.fillWidth: true
            text: qsTr("Enter the separate keyring passphrase used to encrypt ~/.ipatool/account.")
            color: App.Theme.textMuted
            font.pixelSize: 12
            wrapMode: Text.WordWrap
            horizontalAlignment: Text.AlignHCenter
        }

        TextField {
            id: passphraseField
            Layout.fillWidth: true
            Layout.preferredHeight: 38
            enabled: !dialog.busy
            text: dialog.passphrase
            placeholderText: qsTr("Keyring passphrase")
            echoMode: dialog.passphraseVisible ? TextInput.Normal : TextInput.Password
            onTextChanged: {
                dialog.passphrase = text
                dialog.errorText = ""
            }
            onAccepted: unlockButton.clicked()
        }

        Text {
            Layout.fillWidth: true
            visible: dialog.errorText.length > 0
            text: dialog.errorText
            color: App.Theme.dangerText
            font.pixelSize: 11
            wrapMode: Text.WordWrap
        }

        RowLayout {
            Layout.fillWidth: true

            CheckBox {
                visible: Qt.platform.os === "windows"
                checked: dialog.rememberPassphrase
                text: qsTr("Remember on this computer")
                onToggled: dialog.rememberPassphrase = checked
            }

            Item { Layout.fillWidth: true }

            Button {
                text: dialog.passphraseVisible ? qsTr("Hide") : qsTr("Show")
                enabled: dialog.passphrase.length > 0 && !dialog.busy
                onClicked: dialog.passphraseVisible = !dialog.passphraseVisible
            }
        }

        Item { Layout.fillHeight: true }

        RowLayout {
            Layout.fillWidth: true

            Button {
                text: qsTr("Continue Signed Out")
                enabled: !dialog.busy
                onClicked: {
                    dialog.passphrase = ""
                    apps.keyring_passphrase_cancelled()
                    dialog.close()
                }
            }

            Item { Layout.fillWidth: true }

            Button {
                id: unlockButton
                text: qsTr("Unlock")
                highlighted: true
                enabled: dialog.passphrase.length > 0 && !dialog.busy
                onClicked: {
                    dialog.busy = true
                    apps.unlock_keyring(dialog.passphrase, dialog.rememberPassphrase)
                }
            }
        }
    }
}
