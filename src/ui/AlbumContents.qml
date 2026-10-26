// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import iDescriptor
import "." as App
import "./base"

Item {
    id: root
    required property var query
    property bool loading: true
    required property var device
    property var udid: device.udid
    required property var albumId
    property int mediaFilter: 0
    property bool mostRecentFirst: true
    property int selectedFileCount: 0
    property var pendingExportPaths: []
    property string pendingExportTitle: ""
    property string errorMessage: ""
    readonly property int preferredTileSize: 178
    readonly property int tileSpacing: 4

    signal goBack()


    function loadAlbumContents() {
        console.log("loading album contents")
        if (albumId === undefined || albumId === null)
            return

        loading = true
        errorMessage = ""
        albumContentsModel.clear()
        selectedFileCount = 0
        query.query_album(albumId, mediaFilter, mostRecentFirst)
    }

    function reloadAlbumContents() {
        loading = true
        errorMessage = ""
        query.reload()
    }

    onAlbumIdChanged: loadAlbumContents()
    onMediaFilterChanged: loadAlbumContents()
    onMostRecentFirstChanged: loadAlbumContents()

    ListModel {
        id: albumContentsModel
    }

    function updateSelectedFileCount() {
        let count = 0
        for (let i = 0; i < albumContentsModel.count; i++) {
            if (albumContentsModel.get(i).selected)
                count += 1
        }
        root.selectedFileCount = count
    }

    function selectedFilePaths() {
        const paths = []
        for (let i = 0; i < albumContentsModel.count; i++) {
            const item = albumContentsModel.get(i)
            if (item.selected)
                paths.push(item.filePath)
        }
        return paths
    }

    function currentFilePaths() {
        const paths = []
        for (let i = 0; i < albumContentsModel.count; i++)
            paths.push(albumContentsModel.get(i).filePath)
        return paths
    }

    function chooseExportDestination(paths, title) {
        if (!paths || paths.length === 0)
            return

        root.pendingExportPaths = paths
        root.pendingExportTitle = title
        const savedFolder = settingsManager.last_gallery_export_folder()
        if (savedFolder.length > 0)
            exportDialog.currentFolder = savedFolder
        exportDialog.open()
    }

    function startExport(destinationDir) {
        const paths = root.pendingExportPaths
        if (!paths || paths.length === 0)
            return

        const jobId = QmlUtils.generate_uuid()
        const title = root.pendingExportTitle || qsTr("Exporting Files")
        App.StatusWindow.addProcess(jobId, title, "Export", paths.length, destinationDir)
        ioManager.start_gallery_export(root.udid, jobId, paths, destinationDir)
        root.pendingExportPaths = []
        root.pendingExportTitle = ""
    }

    Connections {
        target: query

        function onReloadFinished(success, revision, error) {
            if (success) {
                root.loadAlbumContents()
                return
            }

            root.loading = false
            root.errorMessage = error || qsTr("Failed to reload the gallery.")
        }

        function onAlbumQueried(id, mediaFilter, mostRecentFirst, items) {
            if (id !== albumId || mediaFilter !== root.mediaFilter || mostRecentFirst !== root.mostRecentFirst || !items) return
            albumContentsModel.clear()
            root.selectedFileCount = 0

            for (const item of items) {
                albumContentsModel.append({
                    fileName : item,
                    filePath : item,
                    thumbVersion : 0,
                    selected : false
                })
            }

            root.errorMessage = ""
            root.loading = false
        }

        function onAlbumQueryFailed(id, mediaFilter, mostRecentFirst, error) {
            if (id !== albumId || mediaFilter !== root.mediaFilter || mostRecentFirst !== root.mostRecentFirst)
                return

            root.loading = false
            root.errorMessage = error || qsTr("Failed to load the album contents.")

        }
    }

    Connections {
        target : imageLoader

        function onThumbnailReady(path, rowHint) {
            const item = albumContentsModel.get(rowHint)
            if (item && item.filePath == path) {
                albumContentsModel.setProperty(rowHint, "thumbVersion", item.thumbVersion + 1)
            }
        }
    }

    StateView {
        id: stateView
        anchors.fill: parent
        autoSwitchContent: false
        viewState: root.loading || query.reloading
                   ? StateView.State.Loading
                   : root.errorMessage.length > 0
                     ? StateView.State.Error
                     : StateView.State.Content
        errorText: root.errorMessage
        retryable: true
        cancelable: true
        cancelText: qsTr("Back")
        onRetryRequested: root.loadAlbumContents()
        onCancelRequested: root.goBack()
        contentItem : ColumnLayout {
            anchors.fill : parent

            RowLayout {
                Layout.fillWidth: true
                IconToolButton {
                    icon.source: "qrc:/resources/icons/material-symbols_arrow-left-alt.svg"
                    enabled : nav.depth > 1
                    toolTipText: qsTr("Back")
                    onClicked : root.goBack()
                }

                IconToolButton {
                    icon.source: "qrc:/resources/icons/ic_outline-refresh.svg"
                    enabled: !query.reloading
                    toolTipText: query.reloading
                                 ? qsTr("Refreshing album contents")
                                 : qsTr("Refresh")
                    onClicked: root.reloadAlbumContents()
                }

                Item { Layout.fillWidth: true }

                ComboBox {
                    textRole: "label"
                    valueRole: "value"
                    model: [
                        { value: 0, label: qsTr("All") },
                        { value: 1, label: qsTr("Images") },
                        { value: 2, label: qsTr("Videos") }
                    ]
                    currentIndex: Math.max(0, indexOfValue(root.mediaFilter))
                    onActivated: root.mediaFilter = currentValue
                }

                ComboBox {
                    textRole: "label"
                    valueRole: "value"
                    model: [
                        { value: true, label: qsTr("Most Recent") },
                        { value: false, label: qsTr("Oldest First") }
                    ]
                    currentIndex: Math.max(0, indexOfValue(root.mostRecentFirst))
                    onActivated: root.mostRecentFirst = currentValue
                }

                Button {
                    text: qsTr("Export Selected (%1)").arg(root.selectedFileCount)
                    enabled: root.selectedFileCount > 0
                    onClicked: root.chooseExportDestination(root.selectedFilePaths(), qsTr("Exporting Selected Items"))
                }

                Button {
                    text: qsTr("Export All")
                    enabled: albumContentsModel.count > 0
                    onClicked: root.chooseExportDestination(root.currentFilePaths(), qsTr("Exporting Files"))
                }
            }

            Item {
                id: galleryPane
                Layout.fillWidth: true
                Layout.fillHeight: true

                GridView {
                    id: gallery
                    anchors.fill: parent
                    anchors.margins: 8
                    readonly property int columnCount: Math.max(
                        2,
                        Math.floor((width + root.tileSpacing)
                                   / (root.preferredTileSize + root.tileSpacing)))
                    readonly property real tileSize:
                        Math.floor(width / columnCount) - root.tileSpacing
                    cellWidth: tileSize + root.tileSpacing
                    cellHeight: tileSize + root.tileSpacing
                    clip: true
                    model: albumContentsModel
                    ScrollBar.vertical: ScrollBar {
                        id: galleryScrollBar
                        policy: ScrollBar.AsNeeded
                    }

                    delegate: ItemDelegate {
                        width: gallery.tileSize
                        height: gallery.tileSize
                        highlighted: selected
                        background: Rectangle {
                            color: "transparent"
                        }
                        MouseArea {
                            id: tileMouseArea
                            anchors.fill: parent
                            hoverEnabled: true
                            onDoubleClicked: {
                                const comp = Qt.createComponent("PreviewWindow.qml")

                                if (comp.status === Component.Ready) {
                                    const navigationPaths = root.currentFilePaths()
                                    const win = comp.createObject(root,{
                                        filePath,
                                        udid : root.udid,
                                        afcClient: root.device.afcClient,
                                        row: index,
                                        navigationPaths,
                                        navigationIndex: index
                                    })
                                    if (win !== null) {
                                        win.show()
                                    } else {
                                        console.error("createObject failed:", comp.errorString())
                                    }

                                } else if (comp.status === Component.Error) {
                                    console.error("Component failed to load:", comp.errorString())
                                }
                            }
                        }

                        Rectangle {
                            anchors.fill: parent
                            color: selected ? "#4FC3F7" : "transparent"
                            opacity : 0.3
                            z : 1
                        }

                        Image {
                            cache: false
                            asynchronous: true
                            anchors.fill: parent
                            source: "image://thumb/" + encodeURIComponent(filePath)
                                    + "?udid=" + root.udid
                                    + "&afc2=false&index=" + index
                                    + "&v=" + thumbVersion
                            fillMode: Image.PreserveAspectCrop
                            sourceSize.width: 240 * Screen.devicePixelRatio
                            sourceSize.height: 240 * Screen.devicePixelRatio
                        }

                        Rectangle {
                            anchors.bottom: parent.bottom
                            anchors.left: parent.left
                            anchors.right: parent.right
                            height: 40
                            visible: tileMouseArea.containsMouse
                            z: 2
                            gradient: Gradient {
                                GradientStop { position: 0; color: "transparent" }
                                GradientStop { position: 1; color: "#B0000000" }
                            }

                            Text {
                                anchors.left: parent.left
                                anchors.right: parent.right
                                anchors.bottom: parent.bottom
                                anchors.margins: 6
                                text: filePath
                                font.pixelSize: 10
                                color: "white"
                                elide: Text.ElideMiddle
                            }
                        }
                    }
                }

                Column {
                    anchors.centerIn: parent
                    spacing: 6
                    visible: !root.loading
                             && !query.reloading
                             && albumContentsModel.count === 0

                    Label {
                        anchors.horizontalCenter: parent.horizontalCenter
                        text: qsTr("No Photos or Videos")
                        font.pixelSize: 20
                        font.weight: Font.DemiBold
                        color: palette.text
                    }

                    Label {
                        anchors.horizontalCenter: parent.horizontalCenter
                        text: qsTr("This album is empty.")
                        color: palette.mid
                    }
                }

                RubberBandSelection {
                    anchors.fill: gallery
                    anchors.rightMargin: galleryScrollBar.visible ? galleryScrollBar.width : 0
                    targetView: gallery
                    itemCount: albumContentsModel.count
                    selectableItemWidth: gallery.cellWidth
                    selectableItemHeight: gallery.cellHeight
                    isItemSelected: (index) => albumContentsModel.get(index).selected
                    setItemSelected: (index, selected) => albumContentsModel.setProperty(index, "selected", selected)
                    onSelectionUpdated: root.updateSelectedFileCount()
                }
            }
        }
    }

    FolderDialog {
        id: exportDialog
        title: qsTr("Choose Export Folder")
        onAccepted: {
            const destinationDir = QmlUtils.url_to_path(selectedFolder)
            settingsManager.set_last_gallery_export_folder(selectedFolder.toString())
            root.startExport(destinationDir)
        }
    }
}
