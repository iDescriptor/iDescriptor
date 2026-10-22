// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Qt5Compat.GraphicalEffects
import "../base"
import "../"
import "../../"

Item {
    id: root
    required property var app

    signal backRequested()

    property var details: null
    property string errorText: ""
    property string iconSource: app && app.logoUrl ? app.logoUrl : ""
    readonly property string bundleId: app && app.bundleId ? app.bundleId : ""
    readonly property string appName: details ? (details.trackName || details.trackCensoredName || app.name || "") : (app.name || "")
    readonly property string developerName: details ? (details.artistName || details.sellerName || "") : ""
    readonly property string categoryName: details ? (details.primaryGenreName || (details.genres && details.genres.length ? details.genres[0] : "")) : ""
    readonly property string priceText: details ? (details.formattedPrice || (details.price === 0 ? qsTr("Free") : "")) : ""

    function fetchDetails() {
        stateView.viewState = StateView.State.Loading
        errorText = ""
        details = null

        Helpers.fetch_app(root.bundleId, function(appDetails, error) {
            if (error || !appDetails) {
                root.errorText = error || qsTr("Failed to fetch app details.")
                stateView.errorText = root.errorText
                stateView.viewState = StateView.State.Error
                return
            }

            root.details = appDetails
            root.iconSource = appDetails.artworkUrl512 || appDetails.artworkUrl100 || root.iconSource
            stateView.viewState = StateView.State.Content
        })
    }

    function formatBytes(bytesText) {
        var bytes = Number(bytesText || 0)
        if (!bytes) return ""
        return Helpers.formatSize(bytes)
    }

    function ratingText(value) {
        var rating = Number(value || 0)
        return rating > 0 ? rating.toFixed(1) : "--"
    }

    Component.onCompleted: fetchDetails()
    onBundleIdChanged: if (bundleId.length) fetchDetails()

    InstallAppPopup {
        id: installPopup
        bundleId: root.bundleId
        appName: root.appName
        iconSource: root.iconSource
        appDescription: (root.details && root.details.description) || root.app.description || ""
        anchors.centerIn: parent
    }

    GetIpaPopup {
        id: getIpaPopup
        bundleId: root.bundleId
        appName: root.appName
        iconSource: root.iconSource
        appDescription: (root.details && root.details.description) || root.app.description || ""
        anchors.centerIn: parent
    }

    StateView {
        id: stateView
        anchors.fill: parent
        autoSwitchContent: false
        retryable: true
        cancelable: true
        cancelText: qsTr("Back")
        onRetryRequested: root.fetchDetails()
        onCancelRequested: root.backRequested()

        contentItem: ColumnLayout {
            anchors.fill: parent
            spacing: 0

            RowLayout {
                Layout.fillWidth: true
                Layout.margins: 5

                IconToolButton {
                    icon.source: "qrc:/resources/icons/material-symbols_arrow-left-alt.svg"
                    toolTipText: qsTr("Back")
                    onClicked: root.backRequested()
                }

                Item { Layout.fillWidth: true }
            }

            ScrollView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                contentWidth: availableWidth

                ColumnLayout {
                    width: Math.min(980, stateView.width)
                    anchors.horizontalCenter: parent.horizontalCenter
                    spacing: 0

                Item {
                    id: shelfWrapper
                    Layout.fillWidth: true
                    Layout.preferredHeight: 300
                    clip: true

                    Rectangle {
                        anchors.fill: parent
                        color: "#f5f5f7"
                    }

                    Image {
                        id: shelfIcon
                        width: shelfWrapper.width
                        anchors.centerIn: parent
                        source: root.iconSource
                        fillMode: Image.PreserveAspectCrop
                        opacity: 0
                    }

                    FastBlur {
                        anchors.centerIn: parent
                        width: shelfIcon.width
                        height: shelfIcon.height
                        source: shelfIcon
                        radius: 64
                        opacity: 0.48
                        scale: 1.45
                        rotation: -18
                    }

                    Rectangle {
                        anchors.fill: parent
                        gradient: Gradient {
                            GradientStop { position: 0.0; color: Qt.rgba(1, 1, 1, 0.05) }
                            GradientStop { position: 0.72; color: Qt.rgba(0, 0, 0, 0) }
                            GradientStop { position: 1.0; color: palette.window }
                        }
                    }

                    RowLayout {
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.bottom: parent.bottom
                        anchors.margins: 28
                        spacing: 22

                        IconLoader {
                            iconSource: root.iconSource
                            radius: 22
                            Layout.preferredWidth: 132
                            Layout.preferredHeight: 132
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            Layout.minimumWidth: 0
                            spacing: 7

                            Label {
                                Layout.fillWidth: true
                                text: root.appName
                                font.pixelSize: 34
                                font.bold: true
                                wrapMode: Text.WordWrap
                                maximumLineCount: 2
                            }

                            Label {
                                Layout.fillWidth: true
                                text: root.categoryName
                                color: "#6e6e73"
                                font.pixelSize: 16
                                elide: Text.ElideRight
                            }

                            Label {
                                Layout.fillWidth: true
                                text: [root.developerName, root.priceText].filter(function(v) { return v && v.length > 0 }).join(" · ")
                                color: "#6e6e73"
                                elide: Text.ElideRight
                            }

                            RowLayout {
                                spacing: 10

                                Button {
                                    text: qsTr("Install")
                                    onClicked: installPopup.open()
                                }

                                Button {
                                    text: qsTr("Get IPA")
                                    onClicked: getIpaPopup.open()
                                }
                            }
                        }
                    }
                }

                RowLayout {
                    Layout.fillWidth: true
                    Layout.margins: 22
                    spacing: 14

                    Repeater {
                        model: [
                            { label: qsTr("Rating"), value: ratingText(details ? details.averageUserRating : 0), sub: qsTr("%1 ratings").arg(details ? (details.userRatingCount || 0) : 0) },
                            { label: qsTr("Age"), value: details ? (details.trackContentRating || details.contentAdvisoryRating || "--") : "--", sub: qsTr("Years") },
                            { label: qsTr("Version"), value: details ? (details.version || "--") : "--", sub: qsTr("Latest") },
                            { label: qsTr("Size"), value: formatBytes(details ? details.fileSizeBytes : 0), sub: qsTr("Download") }
                        ]

                        delegate: Rectangle {
                            Layout.fillWidth: true
                            Layout.preferredHeight: 92
                            radius: 12
                            color: Theme.softBg

                            ColumnLayout {
                                anchors.centerIn: parent
                                spacing: 3

                                Label {
                                    Layout.alignment: Qt.AlignHCenter
                                    text: modelData.label
                                    color: "#6e6e73"
                                    font.pixelSize: 12
                                    font.bold: true
                                }

                                Label {
                                    Layout.alignment: Qt.AlignHCenter
                                    text: modelData.value
                                    font.pixelSize: 20
                                    font.bold: true
                                }

                                Label {
                                    Layout.alignment: Qt.AlignHCenter
                                    text: modelData.sub
                                    color: "#86868b"
                                    font.pixelSize: 11
                                }
                            }
                        }
                    }
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.margins: 24
                    spacing: 24

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 8

                        Label {
                            text: qsTr("What's New")
                            font.pixelSize: 22
                            font.bold: true
                        }

                        Label {
                            Layout.fillWidth: true
                            text: details ? (details.releaseNotes || qsTr("No release notes available.")) : ""
                            wrapMode: Text.WordWrap
                        }
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 8

                        Label {
                            text: qsTr("Screenshots")
                            font.pixelSize: 22
                            font.bold: true
                        }

                        ListView {
                            Layout.fillWidth: true
                            Layout.preferredHeight: details && details.screenshotUrls && details.screenshotUrls.length ? 360 : 48
                            orientation: ListView.Horizontal
                            spacing: 14
                            clip: true
                            model: details && details.screenshotUrls ? details.screenshotUrls : []

                            delegate: Rectangle {
                                width: 180
                                height: 340
                                radius: 20
                                color: "#f5f5f7"
                                border.color: "#d2d2d7"
                                clip: true

                                Image {
                                    anchors.fill: parent
                                    source: modelData
                                    fillMode: Image.PreserveAspectCrop
                                    asynchronous: true
                                }
                            }

                            Label {
                                anchors.verticalCenter: parent.verticalCenter
                                visible: parent.count === 0
                                text: qsTr("No screenshots available.")
                                color: "#6e6e73"
                            }
                        }
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 8

                        Label {
                            text: qsTr("Description")
                            font.pixelSize: 22
                            font.bold: true
                        }

                        Label {
                            Layout.fillWidth: true
                            text: details ? (details.description || "") : ""
                            wrapMode: Text.WordWrap
                        }
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 8

                        Label {
                            text: qsTr("Information")
                            font.pixelSize: 22
                            font.bold: true
                        }

                        GridLayout {
                            Layout.fillWidth: true
                            columns: 2
                            columnSpacing: 24
                            rowSpacing: 10

                            Label { text: qsTr("Seller"); color: "#6e6e73" }
                            Label { Layout.fillWidth: true; text: details ? (details.sellerName || "--") : "--"; wrapMode: Text.WordWrap }
                            Label { text: qsTr("Bundle ID"); color: "#6e6e73" }
                            Label { Layout.fillWidth: true; text: root.bundleId; wrapMode: Text.WrapAnywhere }
                            Label { text: qsTr("Minimum iOS"); color: "#6e6e73" }
                            Label { text: details ? (details.minimumOsVersion || "--") : "--" }
                            Label { text: qsTr("Languages"); color: "#6e6e73" }
                            Label {
                                Layout.fillWidth: true
                                text: details && details.languageCodesISO2A ? details.languageCodesISO2A.slice(0, 8).join(", ") : "--"
                                wrapMode: Text.WordWrap
                            }
                        }
                    }
                    }
                }
            }
        }
    }
}
