// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "./base"
import "." as App

Item {
    id: root

    required property string udid
    required property var info

    function v(key, fallback) {
        if (!info) return fallback
        const val = info[key]
        if (val === undefined || val === null || val === "") return fallback
        return val
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 24
        spacing: 18

        RowLayout {
            Layout.fillWidth: true
            spacing: 18

            Image {
                source: v("placeholder_path", "qrc:/resources/icons/iphone_gen1_placeholder.png")
                sourceSize: Qt.size(400, 600)
                fillMode: Image.PreserveAspectFit
                smooth: true
                mipmap: true
                Layout.preferredWidth: 210
                Layout.preferredHeight: 280
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 10

                Label {
                    Layout.fillWidth: true
                    text: v("display_name", qsTr("Recovery Device"))
                    color: App.Theme.text
                    font.pixelSize: 28
                    font.bold: true
                    elide: Text.ElideRight
                }

                Label {
                    Layout.fillWidth: true
                    text: qsTr("%1 mode").arg(v("mode", qsTr("Recovery")))
                    color: App.Theme.textMuted
                    font.pixelSize: 14
                    elide: Text.ElideRight
                }

                RowLayout {
                    Button {
                        text: qsTr("Go Flash")
                        enabled: root.v("ecid", "") !== ""
                        onClicked: {
                            //FIXME: we need enums and we need to pass ecid
                            Toolbox.toolClicked(21, false, false)
                        }
                    }

                    Button {
                        text: qsTr("Exit Recovery Mode")
                        highlighted: true
                        enabled: v("ecid", "") !== ""
                        onClicked: {
                            const success = core.exit_recovery_mode(v("ecid", ""))
                            if (success) {
                                App.Helpers.showInfo(
                                    root,
                                    qsTr("The command to exit recovery mode was sent successfully. The device should restart shortly."))
                            } else {
                                App.Helpers.showError(
                                    root,
                                    qsTr("Failed to exit recovery mode. This could be due to USB permissions."))
                            }
                        }
                    }
                }
            }
        }

        SectionBox {
            Layout.fillWidth: true
            padding: 12

            GridLayout {
                columns: 4
                columnSpacing: 16
                rowSpacing: 10
                anchors.fill: parent

                Label { text: qsTr("Model:"); font.bold: true }
                CopyableText { text: v("model_identifier", v("hardware_model", qsTr("Unknown"))); elide: Text.ElideRight; Layout.fillWidth: true }
                Label { text: qsTr("Board:"); font.bold: true }
                CopyableText { text: v("board", v("srtg", qsTr("Unknown"))); elide: Text.ElideRight; Layout.fillWidth: true }

                Label { text: qsTr("Marketing Name:"); font.bold: true }
                CopyableText { text: v("marketing_name", qsTr("Unknown")); elide: Text.ElideRight; Layout.fillWidth: true }
                Label { text: qsTr("Mode:"); font.bold: true }
                CopyableText { text: v("mode", qsTr("Unknown")); elide: Text.ElideRight; Layout.fillWidth: true }

                Label { text: qsTr("ECID:"); font.bold: true }
                CopyableText { text: v("ecid", qsTr("Unknown")); elide: Text.ElideMiddle; Layout.fillWidth: true }
                Label { text: qsTr("Serial Number:"); font.bold: true }
                CopyableText { text: v("srnm", qsTr("Unknown")); elide: Text.ElideRight; Layout.fillWidth: true }

                Label { text: qsTr("CPID:"); font.bold: true }
                CopyableText { text: v("cpid", qsTr("Unknown")); elide: Text.ElideRight; Layout.fillWidth: true }
                Label { text: qsTr("BDID:"); font.bold: true }
                CopyableText { text: v("bdid", qsTr("Unknown")); elide: Text.ElideRight; Layout.fillWidth: true }

                Label { text: qsTr("Vendor ID:"); font.bold: true }
                CopyableText { text: "0x" + Number(v("vendor_id", 0)).toString(16); elide: Text.ElideRight; Layout.fillWidth: true }
                Label { text: qsTr("Product ID:"); font.bold: true }
                CopyableText { text: "0x" + Number(v("product_id", 0)).toString(16); elide: Text.ElideRight; Layout.fillWidth: true }
            }
        }

        SectionBox {
            Layout.fillWidth: true
            padding: 12
            visible: v("serial_string", "") !== ""

            ColumnLayout {
                anchors.fill: parent
                spacing: 8

                Label {
                    text: qsTr("Recovery Descriptor")
                    color: App.Theme.text
                    font.bold: true
                }

                CopyableText {
                    Layout.fillWidth: true
                    text: v("serial_string", "")
                    color: App.Theme.textMuted
                    wrapMode: Text.WrapAnywhere
                }
            }
        }

        Item { Layout.fillHeight: true }
    }
}
