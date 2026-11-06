// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "../base"
import ".."

ToolWindow {
    id: root

    property var displayInfo: ({
    })
    property bool loading: false
    readonly property string manufacturer: root.value("manufacturer", "")
    readonly property string authStatus: root.value("auth_status", "unknown")

    function value(key, fallback) {
        if (!root.displayInfo)
            return fallback;

        const result = root.displayInfo[key];
        if (result === undefined || result === null || result === "")
            return fallback;

        return result;
    }

    function sourceText(source) {
        switch (source) {
        case "auth_certificate":
            return qsTr("Display authentication certificate");
        case "device_tree":
            return qsTr("Device tree (raw screen serial)");
        case "panel_id":
            return qsTr("Display controller Panel ID");
        default:
            return qsTr("Unavailable");
        }
    }

    function authText(status) {
        switch (status) {
        case "passed":
            return qsTr("Passed");
        case "failed":
            return qsTr("Failed");
        default:
            return qsTr("Not reported");
        }
    }

    function requestDisplayInfo() {
        root.loading = true;
        stateView.viewState = StateView.State.Loading;
        root.device.service_manager.get_display_info();
    }

    title: qsTr("Display - iDescriptor")
    width: 640
    height: 620
    minimumWidth: 560
    minimumHeight: 560

    onVisibleChanged: {
        if (visible)
            root.requestDisplayInfo()
    }

    Connections {
        function onDisplayInfoRetrieved(info) {
            root.displayInfo = info;
            root.loading = false;
            stateView.viewState = StateView.State.Content;
        }

        function onDisplayInfoFailed(error) {
            console.error("Display information refresh failed:", error);
            root.loading = false;
            stateView.errorText = qsTr("The display serial number could not be read. Make sure the device is connected and unlocked.");
            stateView.viewState = StateView.State.Error;
        }

        target: root.device.service_manager
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 20
        spacing: 14

        RowLayout {
            Layout.fillWidth: true
            spacing: 8

            ColumnLayout {
                spacing: 2

                Label {
                    text: qsTr("Display")
                    color: Theme.text
                    font.pixelSize: 24
                    font.weight: Font.DemiBold
                }

                Label {
                    text: qsTr("Screen manufacturer and serial number")
                    color: Theme.textMuted
                    font.pixelSize: 12
                }

            }

            Item {
                Layout.fillWidth: true
            }

            IconToolButton {
                Layout.alignment: Qt.AlignRight | Qt.AlignVCenter
                icon.source: "qrc:/resources/icons/ic_outline-refresh.svg"
                enabled: !root.loading
                ToolTip.visible: hovered
                ToolTip.delay: 400
                ToolTip.text: qsTr("Refresh display information")
                onClicked: root.requestDisplayInfo()
            }

        }

        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 1
            color: Theme.separator
        }

        StateView {
            id: stateView

            Layout.fillWidth: true
            Layout.fillHeight: true
            autoSwitchContent: false
            viewState: StateView.State.Loading
            errorText: qsTr("Display information could not be loaded.")
            onRetryRequested: root.requestDisplayInfo()

            contentItem: ColumnLayout {
                anchors.fill: parent
                spacing: 14

                Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: screenSerialColumn.implicitHeight + 24
                    radius: 12
                    color: Theme.rowSurface
                    border.color: Theme.separator
                    border.width: 1

                    ColumnLayout {
                        id: screenSerialColumn

                        anchors.fill: parent
                        anchors.margins: 12
                        spacing: 3

                        Label {
                            text: qsTr("Screen Serial Number")
                            color: Theme.textMuted
                            font.pixelSize: 11
                        }

                        CopyableText {
                            Layout.fillWidth: true
                            text: root.value("screen_serial", qsTr("Unavailable"))
                            color: Theme.text
                            font.bold: true
                            wrapMode: Text.WrapAnywhere
                        }

                    }

                }

                Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 112
                    radius: 14
                    color: Theme.elevatedSurface
                    border.color: Theme.separator
                    border.width: 1

                    ColumnLayout {
                        anchors.fill: parent
                        anchors.margins: 14
                        spacing: 5

                        Label {
                            text: qsTr("Manufacturer")
                            color: Theme.textMuted
                            font.pixelSize: 12
                        }

                        Label {
                            Layout.fillWidth: true
                            text: root.manufacturer.length ? root.manufacturer : qsTr("Unknown")
                            color: root.manufacturer.length ? Theme.text : Theme.textMuted
                            font.pixelSize: 30
                            font.weight: Font.DemiBold
                            elide: Text.ElideRight
                        }

                        Label {
                            Layout.fillWidth: true
                            text: root.manufacturer.length
                                  ? qsTr("Screen serial prefix %1").arg(root.value("prefix", ""))
                                  : qsTr("Screen serial prefix %1 is not in the known list yet").arg(root.value("prefix", ""))
                            color: Theme.textMuted
                            font.pixelSize: 12
                            elide: Text.ElideRight
                        }

                    }

                }

                GridLayout {
                    Layout.fillWidth: true
                    columns: 3
                    columnSpacing: 10
                    rowSpacing: 10

                    Repeater {
                        model: [{
                            "label": qsTr("Read From"),
                            "value": root.sourceText(root.value("source", "")),
                            "color": Theme.text
                        }, {
                            "label": qsTr("Display Authentication"),
                            "value": root.authText(root.authStatus),
                            "color": root.authStatus === "passed" ? Theme.systemGreen : root.authStatus === "failed" ? Theme.systemRed : Theme.textMuted
                        }, {
                            "label": qsTr("Authentication IC Serial"),
                            "value": root.value("auth_ic_serial", qsTr("Unavailable")),
                            "color": Theme.text
                        }]

                        delegate: Rectangle {
                            required property var modelData

                            Layout.fillWidth: true
                            Layout.preferredHeight: 72
                            radius: 12
                            color: Theme.rowSurface
                            border.color: Theme.separator
                            border.width: 1

                            ColumnLayout {
                                anchors.fill: parent
                                anchors.margins: 11
                                spacing: 3

                                Label {
                                    Layout.fillWidth: true
                                    text: modelData.label
                                    color: Theme.textMuted
                                    font.pixelSize: 11
                                    elide: Text.ElideRight
                                }

                                CopyableText {
                                    Layout.fillWidth: true
                                    text: modelData.value
                                    color: modelData.color
                                    font.bold: true
                                    elide: Text.ElideRight
                                }

                            }

                        }

                    }

                }

                Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: panelIdColumn.implicitHeight + 24
                    visible: root.value("panel_id", "") !== "" && root.value("panel_id", "") !== root.value("screen_serial", "")
                    radius: 12
                    color: Theme.rowSurface
                    border.color: Theme.separator
                    border.width: 1

                    ColumnLayout {
                        id: panelIdColumn

                        anchors.fill: parent
                        anchors.margins: 12
                        spacing: 3

                        Label {
                            text: qsTr("Panel ID")
                            color: Theme.textMuted
                            font.pixelSize: 11
                        }

                        CopyableText {
                            Layout.fillWidth: true
                            text: root.value("panel_id", "")
                            color: Theme.text
                            wrapMode: Text.WrapAnywhere
                        }

                    }

                }

                Label {
                    Layout.fillWidth: true
                    text: qsTr("The manufacturer is inferred from the screen serial prefix using community-reported data; Apple does not publish this mapping. A replacement screen can carry a copied serial number, so check Display Authentication as well.")
                    color: Theme.textMuted
                    font.pixelSize: 11
                    wrapMode: Text.WordWrap
                }

                Item {
                    Layout.fillHeight: true
                }

            }

        }

    }

}
