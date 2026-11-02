// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtMultimedia
import "./base"
import "." as App

AnimatedDialog {
    id: root

    required property var device
    property int iosVersion: root.device.info.ios_version_major
    modal: true
    focus: true
    closePolicy: root.operationInProgress
        ? Popup.NoAutoClose
        : Popup.CloseOnEscape | Popup.CloseOnPressOutside
    anchors.centerIn: parent
    width: Math.min(parent ? parent.width - 32 : 500, 500)
    height: root.contentIndex === 1
        ? Math.min(parent ? parent.height - 32 : 680, 680)
        : 220

    property string version: ""
    property string statusText: qsTr("Please wait...")
    property string developerModeError: ""
    property int contentIndex: 0
    property bool didHandle: false
    property bool tryAnywayEnabled: true
    property bool operationInProgress: false

    signal handled(bool success, bool forced)
    signal preparationFailed(string message)

    function showError(message) {
        root.operationInProgress = false
        stateView.errorText = message
        stateView.viewState = StateView.State.Error
        root.preparationFailed(message)
    }

    function start() {
        didHandle = false
        open()
        retry()
    }

    function retry() {
        root.operationInProgress = true
        stateView.viewState = StateView.State.Loading
        contentIndex = 0
        statusText = qsTr("Please wait...")
        App.Helpers.setTimeout(function() {
            root.check()
        }, 200)
    }

    function check() {
        if (root.iosVersion >= 17) {
            checkDeveloperMode()
            return
        }

        checkDeveloperDiskImage()
    }

    function checkDeveloperMode() {
        root.operationInProgress = true
        stateView.viewState = StateView.State.Content
        contentIndex = 0
        statusText = qsTr("Checking Developer Mode...")

        App.Helpers.connectOnce(root.device.service_manager.developerModeStatusChecked, function(enabled) {
            if (enabled) {
                // TODO: not sure if we need to enable dev mode
                // to mount personalized image or cryptex,
                // most likely Cryptex does not need dev mode to be enabled
                if (root.iosVersion >= 17)
                    preparePersonalizedImage()
                else
                    checkDeveloperDiskImage()
                return
            }

            contentIndex = 1
            stateView.viewState = StateView.State.Content
            revealDeveloperModeOption()
        })

        root.device.service_manager.check_developer_mode_status()
    }

    function preparePersonalizedImage() {
        root.operationInProgress = true
        stateView.viewState = StateView.State.Content
        contentIndex = 0
        statusText = qsTr("Preparing developer image…")
        App.Helpers.connectOnce(root.device.service_manager.developerImagePrepared, function(success, error) {
            if (!success) {
                showError(error)
                return
            }
            finishWithSuccess(true, false)
        })
        root.device.service_manager.prepare_developer_image("auto", settingsManager.dev_disk_img_path(), settingsManager.developer_image_revision())
    }

    Connections {
        target: root.device.service_manager
        function onDeveloperImageProgress(stage, progress) {
            const labels = {
                checking: qsTr("Checking developer support…"),
                connecting: qsTr("Connecting to developer services…"),
                downloading: qsTr("Downloading developer image…"),
                personalizing: qsTr("Personalizing developer image…"),
                identifiers: qsTr("Reading device personalization identifiers…"),
                nonce: qsTr("Reading device Cryptex nonce…"),
                signing: qsTr("Requesting personalization ticket from Apple…"),
                installing: qsTr("Uploading and installing developer image…"),
                install_reply: qsTr("Waiting for device installation…"),
                uploading: qsTr("Uploading developer image…"),
                mounting: qsTr("Mounting developer image…"),
                verifying: qsTr("Verifying developer image…")
            }
            statusText = labels[stage] || qsTr("Preparing developer image…")
            if (stage === "downloading" || stage === "uploading")
                statusText += " " + Math.round(progress * 100) + "%"
        }
    }

    function revealDeveloperModeOption() {
        root.operationInProgress = true
        developerModeError = ""

        App.Helpers.connectOnce(root.device.service_manager.developerModeOptionRevealed, function(revealed) {
            root.operationInProgress = false
            if (!revealed)
                developerModeError = qsTr("Could not reveal Developer Mode automatically. You can still follow the steps below or try anyway.")
        })

        root.device.service_manager.reveal_developer_mode_option_in_ui()
    }

    function checkDeveloperDiskImage() {
        root.operationInProgress = true
        // FIXME: we dont have developer disk images for ios 6 and below
        if (root.iosVersion <= 5) {
            showError(qsTr("Developer disk image is not available for this iOS version. Please use a device with iOS 6 or above."))
            return
        }

        stateView.viewState = StateView.State.Content
        contentIndex = 0

        App.Helpers.connectOnce(root.device.service_manager.mountedImageRetrieved, function(success, isLocked, signature, sigLength) {
            if (!success) {
                if (isLocked) {
                    showError(qsTr("The device appears to be locked. Please unlock the device and try again."))
                    return
                }

                showError(qsTr("Failed to retrieve mounted developer disk image info."))
                return
            }

            if (sigLength > 0) {
                finishWithSuccess(false, false)
                return
            }

            prepareCompatibleImage()
        })

        root.device.service_manager.get_mounted_image()
    }

    function prepareCompatibleImage() {
        root.operationInProgress = true
        const info = DevImgsManager.get_best_compatible_version(root.iosVersion, settingsManager.dev_disk_img_path())
        root.version = info.version || ""

        if (!info.found || root.version.length === 0) {
            // FIXME: we need to disable the retry button in this case
            showError(qsTr("There is no compatible developer disk image available for this iOS version."))
            return
        }

        if (info.exists) {
            mountVersion(root.version)
            return
        }

        statusText = qsTr("Downloading compatible developer disk image...")

        //FIXME: if downloadedVersion !== root.version
        //this will hang forever
        App.Helpers.connectOnce(DevImgsManager.imageDownloadFinished, function(downloadedVersion, index, success, error) {
            DevImgsManager.handle_download_finished(downloadedVersion)

            if (downloadedVersion !== root.version)
                return

            if (!success) {
                showError(error && error.length > 0 ? error : qsTr("Failed to download compatible developer disk image."))
                return
            }

            mountVersion(root.version)
        })

        if (!DevImgsManager.download_image(root.version, 0, settingsManager.dev_disk_img_path()))
            showError(qsTr("Failed to start developer disk image download."))
    }

    /* try to mount a specific version */
    function mountVersion(version) {
        root.operationInProgress = true
        statusText = qsTr("Mounting...")

        const locations = DevImgsManager.get_locations_for_version(version, settingsManager.dev_disk_img_path())
        if (!locations.exists || !locations.dmg || !locations.sig) {
            showError(qsTr("The developer disk image is missing. Please download it first."))
            return
        }

        App.Helpers.connectOnce(root.device.service_manager.devImageMounted, function(mountedVersion, success, isLocked) {
            if (!success) {
                if (isLocked) {
                    showError(qsTr("Failed to mount developer disk image.\nThe device appears to be locked. Please unlock the device and try again."))
                    return
                }

                showError(qsTr("Failed to mount developer disk image.\nPlease ensure the device is unlocked and using a genuine cable."))
                return
            }

            finishWithSuccess(true, false)
        })

        root.device.service_manager.mount_dev_image(version, locations.dmg, locations.sig)
    }

    /*
        waiting is sometimes required because services
        may not become available
        as soon as the img is mounted
    */
    function finishWithSuccess(wait, forced) {
        root.operationInProgress = true
        statusText = qsTr("Developer disk image is ready.")
        stateView.viewState = StateView.State.Content
        contentIndex = 0
        App.Helpers.setTimeout(function() {
            root.didHandle = true
            root.handled(true, forced)
            root.close()
        }, wait ? 3000 : 250)
    }

    function finishWithFailure() {
        if (root.didHandle)
            return

        root.operationInProgress = false
        root.didHandle = true
        root.handled(false, false)
    }

    onRejected: root.finishWithFailure()

    Behavior on height {
        NumberAnimation {
            duration: 220
            easing.type: Easing.OutCubic
        }
    }

    background: Rectangle {
        radius: 10
        color: palette.window
        border.color: Qt.rgba(0, 0, 0, 0.14)
        border.width: 1
    }

    contentItem: StateView {
        id: stateView
        autoSwitchContent: false
        retryable: root.iosVersion < 17
        cancelable: true
        errorText: qsTr("Failed to prepare Developer Mode.")
        onRetryRequested: root.retry()
        onCancelRequested: root.reject()

        contentItem: StackLayout {
            anchors.fill: parent
            currentIndex: root.contentIndex

            ColumnLayout {
                anchors.fill: parent
                anchors.margins: 20
                spacing: 12

                Item { Layout.fillHeight: true }

                Label {
                    Layout.fillWidth: true
                    text: root.statusText
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.WordWrap
                }

                BusyIndicator {
                    Layout.alignment: Qt.AlignHCenter
                    running: root.contentIndex === 0
                }

                Item { Layout.fillHeight: true }
            }

            ColumnLayout {
                anchors.fill: parent
                anchors.margins: 18
                spacing: 10

                Label {
                    Layout.fillWidth: true
                    text: qsTr("Enable Developer Mode")
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.WordWrap
                    font.pixelSize: 18
                    font.bold: true
                }

                Label {
                    Layout.fillWidth: true
                    text: qsTr("Developer Mode is required before this feature can continue. Enable it in the Settings app on your device, then retry the action.")
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.WordWrap
                    color: palette.text
                }

                Label {
                    Layout.fillWidth: true
                    visible: root.developerModeError.length > 0
                    text: root.developerModeError
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.WordWrap
                    color: "#d97706"
                }

                Video {
                    id: tutorialVideo
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    Layout.minimumHeight: 320
                    Layout.preferredHeight: 420
                    source: "qrc:/resources/dev-mode.mp4"
                    fillMode: VideoOutput.PreserveAspectFit
                    loops: MediaPlayer.Infinite
                    onVisibleChanged: {
                        if (visible && stateView.viewState === StateView.State.Content)
                            play()
                        else
                            pause()
                    }
                    Component.onCompleted: {
                        if (visible)
                            play()
                    }
                }

                Button {
                    Layout.alignment: Qt.AlignHCenter
                    text: qsTr("Try Anyway")
                    visible: root.tryAnywayEnabled
                    onClicked: root.finishWithSuccess(false, true)
                }
            }
        }
    }
}
