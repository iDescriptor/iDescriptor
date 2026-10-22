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
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
    anchors.centerIn: Overlay.overlay
    width: 380
    height: Math.min(Overlay.overlay.height - 32, usesEncryptedAccountFile ? 650 : 460)

    property string email: ""
    property string password: ""
    property string keyringPassphrase: ""
    property string keyringPassphraseConfirmation: ""
    property bool keyringPassphraseVisible: false
    property bool rememberKeyringPassphrase: Qt.platform.os === "windows"
    property string activeRequestId: ""
    property string credentialError: ""
    property string codeError: ""
    property string systemError: ""
    property bool credentialsLoading: false
    property bool codeLoading: false
    property bool passwordVisible: false
    property bool closingAfterSuccess: false
    property bool codeErrorActive: false
    property string credentialFocusTarget: ""
    property int codeFocusIndex: -1
    property string queuedCode: ""
    property var codeDigits: ["", "", "", "", "", ""]

    readonly property bool usesEncryptedAccountFile: apps.state.usesEncryptedAccountFile || false
    readonly property bool keyringPassphraseReady: !usesEncryptedAccountFile
                                                   || (keyringPassphrase.length > 0
                                                       && keyringPassphrase === keyringPassphraseConfirmation)
    readonly property bool credentialsReady: email.trim().length > 0
                                             && password.length > 0
                                             && keyringPassphraseReady
    readonly property bool codeReady: codeDigits.join("").length === 6
    readonly property bool waitingForCode: activeRequestId.length > 0

    function resetFlow() {
        email = ""
        password = ""
        keyringPassphrase = ""
        keyringPassphraseConfirmation = ""
        keyringPassphraseVisible = false
        rememberKeyringPassphrase = Qt.platform.os === "windows"
        activeRequestId = ""
        credentialError = ""
        codeError = ""
        systemError = ""
        credentialsLoading = false
        codeLoading = false
        passwordVisible = false
        closingAfterSuccess = false
        queuedCode = ""
        clearCode()
        if (nav.depth > 1)
            nav.pop()
    }

    function startSignIn() {
        if (!credentialsReady || credentialsLoading)
            return

        credentialError = ""
        systemError = ""
        credentialsLoading = true
        apps.sign_in(
            email.trim(),
            password,
            keyringPassphrase,
            rememberKeyringPassphrase
        )
    }

    function submitCode() {
        if (!codeReady || codeLoading)
            return

        codeError = ""
        codeLoading = true
        if (waitingForCode) {
            apps.auth_code_received(activeRequestId, codeDigits.join(""))
        } else {
            queuedCode = codeDigits.join("")
            apps.sign_in(
                email.trim(),
                password,
                keyringPassphrase,
                rememberKeyringPassphrase
            )
        }
    }

    function cancelPendingCode() {
        if (!waitingForCode)
            return

        const requestId = activeRequestId
        activeRequestId = ""
        apps.auth_code_cancelled(requestId)
    }

    function clearCode() {
        codeDigits = ["", "", "", "", "", ""]
        codeErrorActive = false
    }

    function focusCodeBox(index) {
        codeFocusIndex = -1
        codeFocusIndex = Math.max(0, Math.min(5, index))
    }

    function focusCredentialField(name) {
        credentialFocusTarget = ""
        credentialFocusTarget = name
    }

    function setCodeDigit(index, value) {
        const digit = value.replace(/[^0-9]/g, "").slice(-1)
        const digits = codeDigits.slice()
        digits[index] = digit
        codeDigits = digits

        if (digit.length > 0 && index < 5)
            focusCodeBox(index + 1)
        if (codeDigits.join("").length === 6)
            submitCode()
    }

    function clearCodeDigit(index) {
        const digits = codeDigits.slice()
        if (digits[index].length > 0) {
            digits[index] = ""
            codeDigits = digits
            return
        }

        if (index > 0) {
            digits[index - 1] = ""
            codeDigits = digits
            focusCodeBox(index - 1)
        }
    }

    function showCodeError(errorText) {
        codeLoading = false
        codeError = errorText
        clearCode()
        codeErrorActive = true
        codeErrorTimer.restart()
        focusCodeBox(0)
    }

    function handleFailure(errorText) {
        credentialsLoading = false
        codeLoading = false

        if (waitingForCode || nav.depth > 1) {
            activeRequestId = ""
            queuedCode = ""
            showCodeError(errorText)
            return
        }

        credentialError = errorText
        focusCredentialField("password")
    }

    onAboutToShow: {
        resetFlow()
        Qt.callLater(function() { dialog.focusCredentialField("email") })
    }

    onClosed: {
        if (!closingAfterSuccess)
            cancelPendingCode()
        password = ""
        keyringPassphrase = ""
        keyringPassphraseConfirmation = ""
        credentialsLoading = false
        codeLoading = false
    }

    Timer {
        id: codeErrorTimer
        interval: App.Theme.fastAnimation
        repeat: false
        onTriggered: codeErrorActive = false
    }

    Connections {
        target: apps

        function onAuthCodeRequested(requestId, requestedEmail) {
            if (!dialog.visible) {
                apps.auth_code_cancelled(requestId)
                return
            }

            dialog.credentialsLoading = false
            dialog.codeLoading = false
            dialog.activeRequestId = requestId
            dialog.email = requestedEmail || dialog.email
            dialog.codeError = ""
            dialog.systemError = ""

            if (dialog.queuedCode.length === 6) {
                const code = dialog.queuedCode
                dialog.queuedCode = ""
                apps.auth_code_received(requestId, code)
                return
            }

            dialog.clearCode()
            if (nav.depth === 1)
                nav.push(codePageComponent)
            Qt.callLater(function() { dialog.focusCodeBox(0) })
        }

        function onSignInFinished(success, error) {
            if (!dialog.visible)
                return

            if (success) {
                dialog.closingAfterSuccess = true
                dialog.activeRequestId = ""
                dialog.close()
                return
            }

            dialog.handleFailure(error || qsTr("Sign in failed."))
        }
    }

    contentItem: StackView {
        id: nav
        anchors.fill: parent
        anchors.margins: 24
        clip: true
        initialItem: credentialsPageComponent

        pushEnter: Transition {
            PropertyAnimation { property: "x"; from: nav.width; to: 0; duration: 320; easing.type: Easing.OutCubic }
        }
        pushExit: Transition {
            PropertyAnimation { property: "x"; from: 0; to: -nav.width; duration: 320; easing.type: Easing.OutCubic }
            PropertyAnimation { property: "opacity"; from: 1; to: 0.55; duration: 320; easing.type: Easing.OutCubic }
        }
        popEnter: Transition {
            PropertyAnimation { property: "x"; from: -nav.width; to: 0; duration: 280; easing.type: Easing.OutCubic }
            PropertyAnimation { property: "opacity"; from: 0.55; to: 1; duration: 280; easing.type: Easing.OutCubic }
        }
        popExit: Transition {
            PropertyAnimation { property: "x"; from: 0; to: nav.width; duration: 280; easing.type: Easing.OutCubic }
        }
    }

    Component {
        id: credentialsPageComponent

        StateView {
            id: credentialsState
            autoSwitchContent: false
            retryable: true
            viewState: dialog.systemError.length > 0 ? StateView.State.Error : StateView.State.Content
            errorText: dialog.systemError
            onRetryRequested: {
                dialog.systemError = ""
                dialog.startSignIn()
            }

            contentItem: ColumnLayout {
                anchors.fill: parent
                spacing: 10

                Item { Layout.fillHeight: true }

                Text {
                    Layout.fillWidth: true
                    text: qsTr("Sign in with your Apple Account")
                    color: App.Theme.text
                    font.pixelSize: 16
                    font.weight: Font.DemiBold
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.WordWrap
                }

                Text {
                    Layout.fillWidth: true
                    text: qsTr("Use your account to search and install App Store apps.")
                    color: App.Theme.textMuted
                    font.pixelSize: 12
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.WordWrap
                    lineHeight: 1.4
                }

                Item { Layout.preferredHeight: 6 }

                TextField {
                    id: emailField
                    Layout.fillWidth: true
                    Layout.preferredHeight: 36
                    enabled: !dialog.credentialsLoading
                    opacity: dialog.credentialsLoading ? 0.6 : 1
                    text: dialog.email
                    placeholderText: qsTr("Apple Account")
                    placeholderTextColor: App.Theme.textMuted
                    color: App.Theme.text
                    selectedTextColor: App.Theme.textSelected
                    selectionColor: App.Theme.selection
                    topPadding: 0
                    bottomPadding: 0
                    verticalAlignment: TextInput.AlignVCenter
                    inputMethodHints: Qt.ImhEmailCharactersOnly
                    onTextChanged: {
                        dialog.email = text
                        dialog.credentialError = ""
                    }
                    onAccepted: passwordField.forceActiveFocus()

                    Connections {
                        target: dialog
                        function onCredentialFocusTargetChanged() {
                            if (dialog.credentialFocusTarget === "email")
                                emailField.forceActiveFocus()
                        }
                    }

                    background: Item {
                        Rectangle {
                            anchors.fill: parent
                            anchors.margins: -2
                            radius: App.Theme.sidebarCornerRadius + 2
                            visible: emailField.activeFocus
                            color: "transparent"
                            border.color: App.Theme.focus
                            border.width: 2
                        }

                        Rectangle {
                            anchors.fill: parent
                            radius: App.Theme.sidebarCornerRadius
                            color: App.Theme.controlFill
                            border.color: emailField.activeFocus ? App.Theme.accent : App.Theme.controlStroke
                            border.width: 1
                        }
                    }
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 4

                    TextField {
                        id: passwordField
                        Layout.fillWidth: true
                        Layout.preferredHeight: 36
                        enabled: !dialog.credentialsLoading
                        opacity: dialog.credentialsLoading ? 0.6 : 1
                        text: dialog.password
                        rightPadding: 42
                        placeholderText: qsTr("Password")
                        placeholderTextColor: App.Theme.textMuted
                        color: App.Theme.text
                        selectedTextColor: App.Theme.textSelected
                        selectionColor: App.Theme.selection
                        topPadding: 0
                        bottomPadding: 0
                        verticalAlignment: TextInput.AlignVCenter
                        echoMode: dialog.passwordVisible ? TextInput.Normal : TextInput.Password
                        onTextChanged: {
                            dialog.password = text
                            dialog.credentialError = ""
                        }
                        onAccepted: {
                            if (dialog.usesEncryptedAccountFile)
                                keyringPassphraseField.forceActiveFocus()
                            else
                                dialog.startSignIn()
                        }

                        Connections {
                            target: dialog
                            function onCredentialFocusTargetChanged() {
                                if (dialog.credentialFocusTarget === "password")
                                    passwordField.forceActiveFocus()
                            }
                        }

                        Button {
                            id: passwordVisibilityButton
                            anchors.right: parent.right
                            anchors.rightMargin: 4
                            anchors.verticalCenter: parent.verticalCenter
                            width: 30
                            height: 28
                            enabled: passwordField.enabled
                            icon.source: dialog.passwordVisible
                                         ? "qrc:/resources/icons/clarity_eye-hide-line.svg"
                                         : "qrc:/resources/icons/clarity_eye-line.svg"
                            icon.width: 17
                            icon.height: 17
                            icon.color: App.Theme.icon
                            onClicked: dialog.passwordVisible = !dialog.passwordVisible
                            background: Rectangle {
                                radius: 8
                                color: parent.down ? App.Theme.pressed
                                      : parent.hovered ? App.Theme.hover
                                                       : "transparent"
                            }
                        }

                        background: Item {
                            Rectangle {
                                anchors.fill: parent
                                anchors.margins: -2
                                radius: App.Theme.sidebarCornerRadius + 2
                                visible: passwordField.activeFocus
                                color: "transparent"
                                border.color: App.Theme.focus
                                border.width: 2
                            }

                            Rectangle {
                                anchors.fill: parent
                                radius: App.Theme.sidebarCornerRadius
                                color: App.Theme.controlFill
                                border.color: dialog.credentialError.length > 0 ? App.Theme.dangerText
                                            : passwordField.activeFocus ? App.Theme.accent
                                                                        : App.Theme.controlStroke
                                border.width: 1
                            }
                        }
                    }

                    Text {
                        Layout.fillWidth: true
                        visible: dialog.credentialError.length > 0
                        text: dialog.credentialError
                        color: App.Theme.dangerText
                        font.pixelSize: 11
                        wrapMode: Text.WordWrap
                    }
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    visible: dialog.usesEncryptedAccountFile
                    spacing: 6

                    Text {
                        Layout.fillWidth: true
                        text: qsTr("Keyring passphrase")
                        color: App.Theme.text
                        font.pixelSize: 12
                        font.weight: Font.DemiBold
                    }

                    Text {
                        Layout.fillWidth: true
                        text: qsTr("This separate passphrase encrypts the account file shared with ipatool.")
                        color: App.Theme.textMuted
                        font.pixelSize: 11
                        wrapMode: Text.WordWrap
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 6

                        TextField {
                            id: keyringPassphraseField
                            Layout.fillWidth: true
                            Layout.preferredHeight: 36
                            enabled: !dialog.credentialsLoading
                            text: dialog.keyringPassphrase
                            placeholderText: qsTr("Create a keyring passphrase")
                            echoMode: dialog.keyringPassphraseVisible ? TextInput.Normal : TextInput.Password
                            color: App.Theme.text
                            placeholderTextColor: App.Theme.textMuted
                            onTextChanged: {
                                dialog.keyringPassphrase = text
                                dialog.credentialError = ""
                            }
                            onAccepted: keyringPassphraseConfirmationField.forceActiveFocus()
                        }

                        Button {
                            Layout.preferredHeight: 36
                            text: qsTr("Generate")
                            enabled: !dialog.credentialsLoading
                            onClicked: {
                                const generated = apps.generate_keyring_passphrase()
                                if (!generated.length)
                                    return
                                dialog.keyringPassphrase = generated
                                dialog.keyringPassphraseConfirmation = generated
                                dialog.keyringPassphraseVisible = true
                            }
                        }
                    }

                    TextField {
                        id: keyringPassphraseConfirmationField
                        Layout.fillWidth: true
                        Layout.preferredHeight: 36
                        enabled: !dialog.credentialsLoading
                        text: dialog.keyringPassphraseConfirmation
                        placeholderText: qsTr("Confirm keyring passphrase")
                        echoMode: dialog.keyringPassphraseVisible ? TextInput.Normal : TextInput.Password
                        color: App.Theme.text
                        placeholderTextColor: App.Theme.textMuted
                        onTextChanged: {
                            dialog.keyringPassphraseConfirmation = text
                            dialog.credentialError = ""
                        }
                        onAccepted: dialog.startSignIn()
                    }

                    Text {
                        Layout.fillWidth: true
                        visible: dialog.keyringPassphraseConfirmation.length > 0
                                 && dialog.keyringPassphrase !== dialog.keyringPassphraseConfirmation
                        text: qsTr("The keyring passphrases do not match.")
                        color: App.Theme.dangerText
                        font.pixelSize: 11
                    }

                    RowLayout {
                        Layout.fillWidth: true

                        CheckBox {
                            visible: Qt.platform.os === "windows"
                            checked: dialog.rememberKeyringPassphrase
                            text: qsTr("Remember on this computer")
                            onToggled: dialog.rememberKeyringPassphrase = checked
                        }

                        Item { Layout.fillWidth: true }

                        Button {
                            text: qsTr("Copy")
                            enabled: dialog.keyringPassphrase.length > 0
                            onClicked: QmlUtils.copy_to_clipboard(dialog.keyringPassphrase)
                        }

                        Button {
                            text: dialog.keyringPassphraseVisible ? qsTr("Hide") : qsTr("Show")
                            enabled: dialog.keyringPassphrase.length > 0
                            onClicked: dialog.keyringPassphraseVisible = !dialog.keyringPassphraseVisible
                        }
                    }
                }

                Text {
                    Layout.fillWidth: true
                    text: qsTr("Credentials are passed to Apple's sign-in service and stored by the local ipatool keyring.")
                    color: App.Theme.textMuted
                    font.pixelSize: 11
                    wrapMode: Text.WordWrap
                    lineHeight: 1.35
                }

                StateView {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 42
                    autoSwitchContent: false
                    retryable: false
                    viewState: dialog.credentialsLoading ? StateView.State.Loading : StateView.State.Content

                    contentItem: Button {
                        text: qsTr("Continue")
                        enabled: dialog.credentialsReady
                        onClicked: dialog.startSignIn()

                        contentItem: Text {
                            text: parent.text
                            color: parent.enabled ? App.Theme.textSelected : App.Theme.textMuted
                            font.pixelSize: 13
                            font.weight: Font.DemiBold
                            horizontalAlignment: Text.AlignHCenter
                            verticalAlignment: Text.AlignVCenter
                        }

                        background: Rectangle {
                            radius: 18
                            color: !parent.enabled ? App.Theme.controlFill
                                  : parent.down ? App.Theme.accentPressed
                                                : parent.hovered ? App.Theme.accentHover
                                                                 : App.Theme.accent
                            border.color: parent.enabled ? "transparent" : App.Theme.controlStroke
                            border.width: parent.enabled ? 0 : 1
                        }
                    }
                }

                Text {
                    id: forgotPasswordLink
                    Layout.alignment: Qt.AlignHCenter
                    text: qsTr("Forgot password?")
                    color: App.Theme.accent
                    font.pixelSize: 12
                    font.underline: forgotMouse.containsMouse

                    MouseArea {
                        id: forgotMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: Qt.openUrlExternally("https://iforgot.apple.com/")
                    }
                }

                Item { Layout.fillHeight: true }
            }
        }
    }

    Component {
        id: codePageComponent

        Loader {
            id: codePageLoader
            sourceComponent: StateView {
                id: codeState
                autoSwitchContent: false
                retryable: true
                viewState: dialog.systemError.length > 0 ? StateView.State.Error : StateView.State.Content
                errorText: dialog.systemError
                onRetryRequested: {
                    dialog.systemError = ""
                    dialog.submitCode()
                }

                contentItem: ColumnLayout {
                    anchors.fill: parent
                    spacing: 14

                    Button {
                        Layout.alignment: Qt.AlignLeft
                        Layout.preferredWidth: 32
                        Layout.preferredHeight: 32
                        enabled: !dialog.codeLoading
                        icon.source: "qrc:/resources/icons/material-symbols_arrow-left-alt.svg"
                        icon.width: 18
                        icon.height: 18
                        icon.color: App.Theme.icon
                        onClicked: {
                            dialog.cancelPendingCode()
                            nav.pop()
                        }
                        background: Rectangle {
                            radius: width / 2
                            color: parent.down ? App.Theme.pressed
                                  : parent.hovered ? App.Theme.hover
                                                   : App.Theme.controlFill
                            border.color: App.Theme.controlStroke
                            border.width: 1
                        }
                    }

                    Item { Layout.fillHeight: true }

                    Text {
                        Layout.fillWidth: true
                        text: qsTr("Enter the code from your other device")
                        color: App.Theme.text
                        font.pixelSize: 16
                        font.weight: Font.DemiBold
                        horizontalAlignment: Text.AlignHCenter
                        wrapMode: Text.WordWrap
                    }

                    Text {
                        Layout.fillWidth: true
                        text: qsTr("A verification code was requested for %1.").arg(dialog.email)
                        color: App.Theme.textMuted
                        font.pixelSize: 12
                        horizontalAlignment: Text.AlignHCenter
                        wrapMode: Text.WordWrap
                        lineHeight: 1.4
                    }

                    RowLayout {
                        Layout.alignment: Qt.AlignHCenter
                        spacing: 6
                        opacity: dialog.codeLoading ? 0.6 : 1

                        Repeater {
                            model: 6

                            delegate: TextField {
                                id: otpField
                                required property int index

                                Layout.preferredWidth: 40
                                Layout.preferredHeight: 48
                                enabled: !dialog.codeLoading
                                text: dialog.codeDigits[index]
                                color: App.Theme.text
                                selectedTextColor: App.Theme.textSelected
                                selectionColor: App.Theme.selection
                                horizontalAlignment: Text.AlignHCenter
                                verticalAlignment: Text.AlignVCenter
                                font.pixelSize: 20
                                font.weight: Font.DemiBold
                                inputMethodHints: Qt.ImhDigitsOnly
                                validator: IntValidator { bottom: 0; top: 9 }
                                maximumLength: 1
                                onTextEdited: dialog.setCodeDigit(index, text)
                                Keys.onPressed: function(event) {
                                    if (event.key === Qt.Key_Backspace) {
                                        dialog.clearCodeDigit(index)
                                        event.accepted = true
                                    } else if (event.key === Qt.Key_Left && index > 0) {
                                        dialog.focusCodeBox(index - 1)
                                        event.accepted = true
                                    } else if (event.key === Qt.Key_Right && index < 5) {
                                        dialog.focusCodeBox(index + 1)
                                        event.accepted = true
                                    } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
                                        dialog.submitCode()
                                        event.accepted = true
                                    }
                                }

                                Connections {
                                    target: dialog
                                    function onCodeFocusIndexChanged() {
                                        if (dialog.codeFocusIndex === otpField.index)
                                            otpField.forceActiveFocus()
                                    }
                                }

                                background: Item {
                                    Rectangle {
                                        anchors.fill: parent
                                        anchors.margins: -2
                                        radius: App.Theme.sidebarCornerRadius + 2
                                        visible: otpField.activeFocus
                                        color: "transparent"
                                        border.color: App.Theme.focus
                                        border.width: 2
                                    }

                                    Rectangle {
                                        anchors.fill: parent
                                        radius: App.Theme.sidebarCornerRadius
                                        color: App.Theme.controlFill
                                        border.color: dialog.codeErrorActive ? App.Theme.dangerText
                                                    : otpField.activeFocus ? App.Theme.accent
                                                                           : App.Theme.controlStroke
                                        border.width: 1
                                    }
                                }
                            }
                        }
                    }

                    Text {
                        Layout.fillWidth: true
                        visible: dialog.codeError.length > 0
                        text: dialog.codeError
                        color: App.Theme.dangerText
                        font.pixelSize: 11
                        horizontalAlignment: Text.AlignHCenter
                        wrapMode: Text.WordWrap
                    }

                    Text {
                        Layout.alignment: Qt.AlignHCenter
                        visible: false
                        text: qsTr("Resend code")
                        color: App.Theme.accent
                        font.pixelSize: 12
                    }

                    StateView {
                        Layout.fillWidth: true
                        Layout.preferredHeight: 42
                        autoSwitchContent: false
                        retryable: false
                        viewState: dialog.codeLoading ? StateView.State.Loading : StateView.State.Content

                        contentItem: Button {
                            text: qsTr("Verify")
                            enabled: dialog.codeReady
                            onClicked: dialog.submitCode()

                            contentItem: Text {
                                text: parent.text
                                color: parent.enabled ? App.Theme.textSelected : App.Theme.textMuted
                                font.pixelSize: 13
                                font.weight: Font.DemiBold
                                horizontalAlignment: Text.AlignHCenter
                                verticalAlignment: Text.AlignVCenter
                            }

                            background: Rectangle {
                                radius: 18
                                color: !parent.enabled ? App.Theme.controlFill
                                      : parent.down ? App.Theme.accentPressed
                                                    : parent.hovered ? App.Theme.accentHover
                                                                     : App.Theme.accent
                                border.color: parent.enabled ? "transparent" : App.Theme.controlStroke
                                border.width: parent.enabled ? 0 : 1
                            }
                        }
                    }

                    Item { Layout.fillHeight: true }
                }
            }
        }
    }
}
