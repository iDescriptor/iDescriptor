// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import QtQuick.Controls.impl
import iDescriptor
import "." as App
import "./base"


Item {
    id: root

    property var query
    property bool loading: true
    required property var device
    property var udid: device.udid
    property var info: device.info
    readonly property bool isMainPage: nav.depth <= 1
    // FIXME: should be synced with the backend
    readonly property int hiddenAlbumId: -4
    property int selectedAlbumCount: 0
    property var albumExportSelection: []
    property var is_init: false
    property var pendingAlbumExportRequests: ({})

    signal gallerySizeQueried(real size)

    Component.onCompleted: {
        console.log("DeviceGallery.qml: Component.onCompleted")
        query = serviceFactory.create_query_backend(root.udid, device.connectionId, info.ios_version_major)
        if (query) {
            query.init(settingsManager.gallery_backend());
        } else {
            root.gallerySizeQueried(0)
            // FIXME:show error
            console.error("Query is null after create_query_backend")
        }
    }

    ListModel {
        id: albumModel
    }

    function openAlbum(id) {
        nav.push(albumContentsComponent, {
            albumId: id,
        })
    }

    function goBack() {
        if (nav.depth > 1) {
            nav.pop()
        }
    }

    function updateSelectedAlbumCount() {
        let count = 0
        for (let i = 0; i < albumModel.count; i++) {
            if (albumModel.get(i).selected)
                count += 1
        }
        root.selectedAlbumCount = count
    }

    function albumExists(albumId) {
        for (let i = 0; i < albumModel.count; i++) {
            if (albumModel.get(i).albumId === albumId)
                return true
        }
        return false
    }

    function chooseAlbumExportDestination(albums) {
        if (!albums || albums.length === 0)
            return

        root.albumExportSelection = albums
        const savedFolder = settingsManager.last_gallery_export_folder()
        if (savedFolder.length > 0)
            albumExportDialog.currentFolder = savedFolder
        albumExportDialog.open()
    }

    function startAlbumExports(destinationRoot) {
        const albums = root.albumExportSelection
        if (!albums || albums.length === 0)
            return

        for (let i = 0; i < albums.length; i++) {
            const album = albums[i]
            const requestId = QmlUtils.generate_uuid()
            const albumName = album.fileName || qsTr("Album")
            const destinationDir = QmlUtils.join_path(destinationRoot, QmlUtils.safe_path_segment(albumName))
            root.pendingAlbumExportRequests[requestId] = {
                albumId: album.albumId,
                albumName: albumName,
                destinationDir: destinationDir
            }
            query.resolve_album_export(requestId, album.albumId, albumName)
        }

        root.albumExportSelection = []
    }

    Connections {
        target: query

        function onGallerySizeQueried(size) {
            root.gallerySizeQueried(size)
        }

        function onStateChanged() {
            if (query.state.init && !root.is_init) {
                root.is_init = true
                query.read_albums()
            }
        }

        function onAlbumsChanged() {
            console.log(JSON.stringify(query.albums))
            albumModel.clear()
            root.selectedAlbumCount = 0

            query.albums.forEach((jsonStr) => {
                const obj = JSON.parse(jsonStr)

                albumModel.append({
                    albumId : obj.album_id ?  obj.album_id : -99,
                    fileName: obj.album_name,
                    filePath: obj.file_path,
                    itemCount: obj.item_count === undefined || obj.item_count === null ? 0 : obj.item_count,
                    dateTime: new Date(),
                    selected: false,
                    thumbVersion: 0
                })
            })
        }

        function onReloadFinished(success, revision, error) {
            if (!success || nav.depth <= 1 || !nav.currentItem)
                return

            if (!root.albumExists(nav.currentItem.albumId)) {
                root.goBack()
                albumRemovedDialog.open()
            }
        }

        function onAlbumExportResolved(requestId, albumId, albumName, items) {
            const pending = root.pendingAlbumExportRequests[requestId]
            if (!pending)
                return

            delete root.pendingAlbumExportRequests[requestId]
            if (!items || items.length === 0)
                return

            App.StatusWindow.addProcess(
                requestId,
                qsTr("Exporting %1").arg(albumName),
                "Export",
                items.length,
                pending.destinationDir
            )
            ioManager.start_gallery_export(root.udid, requestId, items, pending.destinationDir)
        }
    }

    Connections {
        target : imageLoader

        function onThumbnailReady(path, rowHint) {
            const item = albumModel.get(rowHint)
            if (item && item.filePath == path) {
                albumModel.setProperty(rowHint, "thumbVersion", item.thumbVersion + 1)
            }
        }
    }

    StateView {
        id: galleryStateView
        anchors.fill: parent
        autoSwitchContent: false
        viewState: query.state.err
                   ? StateView.State.Error
                   : query.reloading || !query.state.init
                     ? StateView.State.Loading
                     : StateView.State.Content
        errorText: query.state.err ? query.state.err : ""
        retryable: true
        onRetryRequested: query.reload()
        contentItem : ColumnLayout {
            anchors.fill : parent
            StackView {
                id: nav
                Layout.fillWidth: true
                Layout.fillHeight: true
                //padding
                Layout.margins:10
                Layout.bottomMargin: 0
                initialItem: mainPageComponent
                clip: true

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

        }
    }

    Component {
        id: mainPageComponent

        Item {
            id: albumListPage

            function albumAt(index) {
                const row = albumModel.get(index)
                return {
                    albumId: row.albumId,
                    fileName: row.fileName,
                    itemCount: row.itemCount,
                }
            }

            function selectedAlbums() {
                const albums = []
                for (let i = 0; i < albumModel.count; i++) {
                    if (albumModel.get(i).selected)
                        albums.push(albumAt(i))
                }
                return albums
            }

            function allAlbums() {
                const albums = []
                for (let i = 0; i < albumModel.count; i++)
                    albums.push(albumAt(i))
                return albums
            }

            ColumnLayout {
                anchors.fill: parent
                // anchors.margins: 5

                RowLayout {
                    Layout.fillWidth: true

                    Button {
                        text: qsTr("Import")
                        onClicked: App.Toolbox.toolClicked(5, false)
                    }

                    IconToolButton {
                        icon.source: "qrc:/resources/icons/ic_outline-refresh.svg"
                        enabled: !query.reloading
                        toolTipText: query.reloading
                                     ? qsTr("Refreshing gallery")
                                     : qsTr("Refresh")
                        onClicked: query.reload()
                    }

                    Item { Layout.fillWidth: true }

                    Button {
                        text: qsTr("Export Selected")
                        enabled: root.selectedAlbumCount > 0
                        onClicked: root.chooseAlbumExportDestination(albumListPage.selectedAlbums())
                    }

                    Button {
                        text: qsTr("Export All")
                        enabled: albumModel.count > 0
                        // TODO: Ask the community whether Export All should include duplicate assets from overlapping albums such as Recents/Favorites. For now we export every album folder as-is without duplicate checks.
                        onClicked: {
                            const all = albumListPage.allAlbums()
                            const totalItems = all.reduce((sum, album) => sum + album.itemCount, 0)
                            App.Helpers.messageBox(
                                root,
                                qsTr("Export All"),
                                qsTr("Are you sure you want to export all %1 items from %2 albums?").arg(totalItems).arg(all.length),
                                MessageDialog.Yes | MessageDialog.No,
                                function(button) {
                                    if (button === MessageDialog.Yes) {
                                        root.chooseAlbumExportDestination(all)
                                    }
                                }
                            )
                        }
                    }
                }

                Label {
                    Layout.fillWidth: true
                    Layout.leftMargin: 4
                    text: qsTr("Albums")
                    color: Theme.text
                    font.pixelSize: 28
                    font.bold: true
                }

                Item {
                    id: galleryPane
                    Layout.fillWidth: true
                    Layout.fillHeight: true

                    GridView {
                        id: gallery
                        anchors.fill: parent
                        cellWidth: 250
                        cellHeight: 292

                        clip: true
                        model: albumModel
                        ScrollBar.vertical: ScrollBar {
                            id: galleryScrollBar
                            policy: ScrollBar.AsNeeded
                        }
                        delegate: ItemDelegate {
                            width: 240
                            height: 284
                            highlighted: selected
                            background: Rectangle {
                                color: "transparent"
                            }
                            MouseArea {
                                anchors.fill: parent
                                onDoubleClicked: {
                                    root.openAlbum(albumId)
                                }
                            }

                            Rectangle {
                                id: albumCover
                                anchors.top: parent.top
                                anchors.horizontalCenter: parent.horizontalCenter
                                width: 240
                                height: 240
                                radius: 8
                                clip: true

                                Image {
                                    cache: false
                                    anchors.fill: parent
                                    asynchronous: true
                                    source: "image://thumb/" + encodeURIComponent(filePath)
                                            + "?udid=" + encodeURIComponent(root.udid)
                                            + "&afc2=false&index=" + index
                                            + "&v=" + thumbVersion
                                    fillMode: Image.PreserveAspectCrop
                                    sourceSize.width: 240 * Screen.devicePixelRatio
                                    sourceSize.height: 240 * Screen.devicePixelRatio
                                }

                                Rectangle {
                                    anchors.fill: parent
                                    visible: albumId === root.hiddenAlbumId
                                    color: Theme.controlFill

                                    IconImage {
                                        anchors.centerIn: parent
                                        width: 100
                                        height: 100
                                        source: "qrc:/resources/icons/clarity_eye-hide-line.svg"
                                        color: Theme.icon
                                    }
                                }

                                Rectangle {
                                    anchors.fill: parent
                                    color: selected ? Theme.accent : "transparent"
                                    opacity: 0.3
                                }
                            }

                            Column {
                                anchors.top: albumCover.bottom
                                anchors.topMargin: 6
                                anchors.left: parent.left
                                anchors.right: parent.right
                                spacing: 1

                                Text {
                                    width: parent.width
                                    text: fileName
                                    font.pixelSize: 14
                                    font.bold: true
                                    color: Theme.text
                                    elide: Text.ElideRight
                                }

                                Text {
                                    width: parent.width
                                    text: itemCount
                                    font.pixelSize: 13
                                    color: Theme.textMuted
                                    elide: Text.ElideRight
                                }
                            }
                        }
                    }

                    RubberBandSelection {
                        anchors.fill: parent
                        anchors.rightMargin: galleryScrollBar.visible ? galleryScrollBar.width : 0
                        targetView: gallery
                        itemCount: albumModel.count
                        selectableItemWidth: 240
                        selectableItemHeight: 284
                        isItemSelected: (index) => albumModel.get(index).selected
                        setItemSelected: (index, selected) => albumModel.setProperty(index, "selected", selected)
                        onSelectionUpdated: root.updateSelectedAlbumCount()
                    }
                }
            }
        }
    }

    Component {
        id: albumContentsComponent

        AlbumContents {
            query: root.query
            device: root.device
            onGoBack : root.goBack()
        }
    }

    FolderDialog {
        id: albumExportDialog
        title: qsTr("Choose Export Folder")
        onAccepted: {
            const destinationRoot = QmlUtils.url_to_path(selectedFolder)
            settingsManager.set_last_gallery_export_folder(selectedFolder.toString())
            root.startAlbumExports(destinationRoot)
        }
    }

    MessageDialog {
        id: albumRemovedDialog
        title: qsTr("Album unavailable")
        text: qsTr("This album is no longer available on the device.")
    }
}
