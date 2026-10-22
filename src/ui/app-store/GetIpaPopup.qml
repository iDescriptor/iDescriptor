// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import "../base"
import "../"

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
    width: 440
    title: qsTr("Get IPA")
    standardButtons: Dialog.NoButton
    closePolicy: root.downloading ? Popup.NoAutoClose : Popup.CloseOnPressOutside

    property string taskId: ""
    property bool downloading: false
    property real progress: 0
    property string stateText: ""
    property string outputPath: ""
    property string errorText: ""
    property string completedPath: ""
    readonly property bool completed: completedPath.length > 0
    readonly property string progressText: progress >= 0
                                                   ? qsTr("%1%").arg(Math.round(progress * 100))
                                                   : ""

    function resetState() {
        taskId = ""
        downloading = false
        progress = 0
        stateText = ""
        errorText = ""
        completedPath = ""
        outputPath = settingsManager.ipa_download_path()
    }

    function clearAppMetadata() {
        // Invalidate pending requests, including an earlier request for the same app.
        metadataRequestGeneration += 1
        resolvedBundleId = ""
        resolvedIconSource = ""
        resolvedDescription = ""
    }

    function resolveAppMetadata() {
        clearAppMetadata()
        if (bundleId.length === 0 || (iconSource.length > 0 && appDescription.length > 0))
            return

        const requestedBundleId = bundleId
        const requestGeneration = metadataRequestGeneration
        Helpers.fetch_app(requestedBundleId, function(app, error) {
            if (root.metadataRequestGeneration !== requestGeneration
                    || root.bundleId !== requestedBundleId || error || !app)
                return
            root.resolvedIconSource = app.artworkUrl100 || app.artworkUrl512 || app.artworkUrl60 || ""
            root.resolvedDescription = app.description || ""
            root.resolvedBundleId = requestedBundleId
        })
    }

    function requestClose() {
        if (downloading) {
            cancelConfirmation.open()
            return
        }
        close()
    }

    function startDownload() {
        errorText = ""
        completedPath = ""
        progress = -1
        stateText = qsTr("Preparing download...")

        const id = apps.download_ipa(bundleId, outputPath)
        if (!id || !id.length) {
            progress = 0
            errorText = qsTr("The App Store service is not initialized.")
            stateText = errorText
            return
        }

        taskId = id
        downloading = true
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
        downloading = false
    }

    Keys.onEscapePressed: function(event) {
        root.requestClose()
        event.accepted = true
    }

    Overlay.modal: Rectangle {
        color: Qt.rgba(0, 0, 0, 0.35)
    }

    FolderDialog {
        id: outputDialog
        title: qsTr("Choose download folder")
        onAccepted: root.outputPath = QmlUtils.url_to_path(selectedFolder)
    }

    MessageDialog {
        id: cancelConfirmation
        title: qsTr("Cancel download?")
        text: qsTr("The IPA download is still in progress. Do you want to cancel it and close this dialog?")
        buttons: MessageDialog.Yes | MessageDialog.No
        onButtonClicked: function(button, role) {
            if (button !== MessageDialog.Yes)
                return

            const id = root.taskId
            root.taskId = ""
            root.downloading = false
            if (id.length)
                apps.cancel_task(id)
            root.close()
        }
    }

    Connections {
        target: apps

        function onDownloadIpaProgress(taskId, progress) {
            if (taskId !== root.taskId)
                return
            root.downloading = true
            root.progress = progress
            root.stateText = qsTr("Downloading IPA...")
        }

        function onDownloadIpaFinished(taskId, success, path, error) {
            if (taskId !== root.taskId)
                return

            root.taskId = ""
            root.downloading = false
            root.progress = success ? 1 : 0
            root.completedPath = success ? path : ""
            root.errorText = success ? "" : (error || qsTr("Download failed."))
            root.stateText = success ? qsTr("Saved IPA") : root.errorText
        }
    }

    contentItem: ColumnLayout {
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
                    text: root.appName
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
            implicitHeight: destinationRow.implicitHeight + 28
            radius: 14
            color: Theme.groupedBackground
            border.color: Theme.separator
            border.width: 1

            RowLayout {
                id: destinationRow
                anchors.fill: parent
                anchors.margins: 14
                spacing: 12


                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 2

                    Label {
                        text: qsTr("Save to")
                        color: Theme.text
                        font.weight: Font.DemiBold
                    }

                    Label {
                        Layout.fillWidth: true
                        text: root.outputPath
                        color: Theme.textMuted
                        font.pixelSize: 12
                        elide: Text.ElideMiddle
                    }
                }

                Button {
                    text: qsTr("Choose…")
                    flat: true
                    enabled: !root.downloading
                    onClicked: outputDialog.open()
                }
            }
        }

        Rectangle {
            Layout.fillWidth: true
            implicitHeight: statusColumn.implicitHeight + 28
            radius: 14
            visible: root.downloading || root.stateText.length > 0
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
                spacing: 9

                RowLayout {
                    Layout.fillWidth: true

                    Label {
                        Layout.fillWidth: true
                        text: root.completed
                              ? qsTr("Download complete")
                              : root.stateText
                        color: root.errorText.length ? Theme.dangerText : Theme.text
                        font.weight: Font.DemiBold
                        wrapMode: Text.WordWrap
                    }

                    Label {
                        visible: root.downloading && root.progress >= 0
                        text: root.progressText
                        color: Theme.textMuted
                    }
                }

                ProgressBar {
                    Layout.fillWidth: true
                    visible: root.downloading
                    indeterminate: root.progress < 0
                    from: 0
                    to: 1
                    value: Math.max(0, root.progress)
                }

                Label {
                    Layout.fillWidth: true
                    visible: root.completed || root.errorText.length > 0
                    text: root.completed
                          ? qsTr("Saved to %1").arg(root.completedPath)
                          : root.errorText
                    color: root.errorText.length ? Theme.dangerText : Theme.textMuted
                    font.pixelSize: 12
                    wrapMode: Text.WrapAnywhere
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

            Button {
                text: qsTr("Open Folder")
                flat: true
                visible: root.completed
                enabled: !root.downloading
                onClicked: Qt.openUrlExternally(Helpers.toFileUrl(root.outputPath))
            }

            Item { Layout.fillWidth: true }

            Button {
                text: root.downloading ? qsTr("Cancel") : qsTr("Close")
                flat: true
                onClicked: root.requestClose()
            }

            Button {
                text: root.completed ? qsTr("Download Again") : qsTr("Download")
                highlighted: true
                enabled: !root.downloading && root.bundleId.length > 0
                onClicked: root.startDownload()
            }
        }
    }
}
