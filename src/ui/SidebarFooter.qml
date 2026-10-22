// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl
import QtQuick.Dialogs
import QtQuick.Layouts
import "." as App
import "./base"

Item {
    id: root

    Layout.fillWidth: true
    Layout.preferredHeight: 41

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 1
            color: App.Theme.sidebarDivider
        }

        RowLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.leftMargin: 7
            Layout.rightMargin: 7
            spacing: 1

            IconImage {
                visible: Qt.platform.os === "osx"
                source: "qrc:/resources/icons/plain-icon.svg"
                color: App.Theme.icon
                sourceSize.width: 40
                sourceSize.height: 40
                Layout.preferredWidth: 40
                Layout.preferredHeight: 40
                Layout.leftMargin: 2
                Layout.rightMargin: 3
                fillMode: Image.PreserveAspectFit
                smooth: true
                mipmap: true
            }

            Label {
                text: qsTr("v%1").arg(settingsManager.current_version())
                color: App.Theme.textMuted
                font.pixelSize: 10
            }

            Item { Layout.fillWidth: true }

            IconToolButton {
                id: audioReceiverButton

                implicitWidth: 27
                implicitHeight: 27
                iconSize: 16
                enabled: AirplayImp.mode !== 1
                         && AirplayImp.state !== 1
                         && AirplayImp.state !== 4
                icon.source: "qrc:/resources/icons/airplay-alt.svg"
                toolTipText: {
                    if (AirplayImp.mode === 1)
                        return qsTr("Close Screen Mirroring before starting the audio receiver")
                    if (AirplayImp.state === 5)
                        return qsTr("AirPlay audio receiver failed: %1").arg(AirplayImp.lastError)
                    if (AirplayImp.audioActive)
                        return AirplayImp.clientName.length > 0
                                ? qsTr("Receiving AirPlay audio from %1").arg(AirplayImp.clientName)
                                : qsTr("Receiving AirPlay audio")
                    if (AirplayImp.mode === 2)
                        return qsTr("AirPlay audio receiver is listening")
                    return qsTr("Enable AirPlay audio receiver")
                }
                onClicked: {
                    if (AirplayImp.mode === 2)
                        AirplayImp.cleanup()
                    else if (!AirplayImp.start_audio())
                        audioReceiverError.open()
                }

                Rectangle {
                    anchors.right: parent.right
                    anchors.bottom: parent.bottom
                    anchors.rightMargin: 2
                    anchors.bottomMargin: 2
                    width: 7
                    height: 7
                    radius: width / 2
                    visible: AirplayImp.mode === 2 || AirplayImp.state === 5
                    color: {
                        if (AirplayImp.state === 5)
                            return "#e5484d"
                        if (AirplayImp.audioActive)
                            return "#30a46c"
                        if (AirplayImp.state === 1 || AirplayImp.state === 4)
                            return "#f5a524"
                        return audioReceiverButton.palette.highlight
                    }
                    border.width: 1
                    border.color: audioReceiverButton.palette.window
                }
            }

            Loader {
                active: Qt.platform.os === "linux" || Qt.platform.os === "windows"
                visible: active
                sourceComponent: Component {
                    RowLayout {
                        spacing: 1

                        Component.onCompleted: IFuseManager.refresh()

                        Connections {
                            target: iFuse

                            function onStateChanged() {
                                IFuseManager.refresh()
                            }
                        }

                        Connections {
                            target: IFuseManager

                            function onUnmountFinished(mountPath, success, error) {
                                if (success)
                                    return

                                App.Helpers.messageBox(
                                    root,
                                    qsTr("Unmount Failed"),
                                    qsTr("Failed to unmount iFuse at %1. Please try again. %2")
                                        .arg(mountPath)
                                        .arg(error),
                                    MessageDialog.Ok
                                )
                            }
                        }

                        Repeater {
                            model: IFuseManager.mountPoints

                            delegate: IconToolButton {
                                required property string modelData
                                implicitWidth: 27
                                implicitHeight: 27
                                iconSize: 16
                                enabled: IFuseManager.busyPath.length === 0
                                icon.source: "qrc:/resources/icons/clarity_hard-disk-solid-alerted.svg"
                                toolTipText: IFuseManager.busyPath === modelData
                                             ? qsTr("Unmounting iFuse at %1…").arg(modelData)
                                             : qsTr("Unmount iFuse at %1").arg(modelData)
                                onClicked: IFuseManager.unmount(modelData)
                            }
                        }
                    }
                }
            }

            IconToolButton {
                id: activityButton

                implicitWidth: 27
                implicitHeight: 27
                iconSize: 16
                icon.source: "qrc:/resources/icons/uim_process.svg"
                toolTipText: qsTr("Activity")
                onClicked: {
                    const globalPos = activityButton.mapToGlobal(0, 0)
                    StatusWindow.toggle(Window.window, globalPos,
                                        activityButton.width, activityButton.height)
                }
            }

            IconToolButton {
                implicitWidth: 27
                implicitHeight: 27
                iconSize: 16
                icon.source: "qrc:/resources/icons/mdi_github.svg"
                toolTipText: qsTr("Open project on GitHub")
                onClicked: Qt.openUrlExternally(App.Constants.repoUrl)
            }

            IconToolButton {
                implicitWidth: 27
                implicitHeight: 27
                iconSize: 16
                icon.source: "qrc:/resources/icons/mingcute_settings-7-line.svg"
                toolTipText: qsTr("Settings")
                onClicked: App.Settings.open()
            }
        }
    }

    Component.onCompleted: {
        StatusWindow.registerOpener(Window.window, activityButton)
    }
    Component.onDestruction: StatusWindow.unregisterOpener(activityButton)

    MessageDialog {
        id: audioReceiverError
        title: qsTr("AirPlay Audio Receiver")
        text: AirplayImp.lastError.length > 0
              ? AirplayImp.lastError
              : qsTr("Failed to start the AirPlay audio receiver.")
    }

    Connections {
        target: AirplayImp

        function onBackendFailed(code, detail) {
            if (AirplayImp.mode === 2)
                audioReceiverError.open()
        }
    }
}
