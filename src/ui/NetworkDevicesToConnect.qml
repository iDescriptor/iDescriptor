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

    readonly property int statusResetDelay: 2500
    // property var networkDeviceCards : ({})
    ListModel { id: deviceModel }

    property string statusText: {
        switch (NetworkDeviceProvider.state) {
            case NetworkDeviceProvider.Loading:
                return qsTr("Network device provider is loading")
            case NetworkDeviceProvider.Failed:
                return qsTr("Network device provider failed to start")
            default:
                return deviceModel.count === 0 ? qsTr("No network devices found") : qsTr("Found %1 network device(s)").arg(deviceModel.count)
        }
    }
    function normalizeDevice(mac, dev) {
        const productType = dev.productType ? dev.productType : ""
        const productVersion = dev.productVersion ? dev.productVersion : ""
        const isIPhone = productType.startsWith("iPhone")
        const isIPad = productType.startsWith("iPad")
        const hasKnownFamily = isIPhone || isIPad
        const parsedProductType = hasKnownFamily ? QmlUtils.get_device_name(productType) : ""

        return {
            mac: mac || dev.macAddress || "",
            name: dev.name || qsTr("Unknown device"),
            productType,
            parsedProductType,
            productVersion,
            iconSource: hasKnownFamily
                        ? QmlUtils.get_device_icon_path(productType)
                        : "qrc:/resources/icons/unknown.svg",
            address: dev.address || dev.ip || "",
            port: dev.port || "",
            raw: dev,

            // UI state
            state: "idle",           // idle|connecting|failed|noPairing|connected|alreadyExists
            stateText: "",
            buttonText: qsTr("Connect"),
            buttonEnabled: true,
            statusResetToken: 0
        }
    }

    function indexByMac(mac) {
        for (var i = 0; i < deviceModel.count; i++) {
            if (deviceModel.get(i).mac === mac) return i
        }
        return -1
    }

    function indexByIp(ip) {
        for (var i = 0; i < deviceModel.count; i++) {
            if (deviceModel.get(i).address === ip) return i
        }
        return -1
    }

    function setStatusAtIndex(i, state) {
        const resetToken = deviceModel.get(i).statusResetToken + 1
        deviceModel.setProperty(i, "statusResetToken", resetToken)

        if (state === "failed") {
            deviceModel.setProperty(i, "state", "failed")
            deviceModel.setProperty(i, "buttonText", qsTr("Failed to connect"))
            deviceModel.setProperty(i, "buttonEnabled", true)
        } else if (state === "noPairing") {
            deviceModel.setProperty(i, "state", "noPairing")
            deviceModel.setProperty(i, "buttonText", qsTr("No pairing file"))
            deviceModel.setProperty(i, "buttonEnabled", true)
        } else if (state === "connecting") {
            deviceModel.setProperty(i, "state", "connecting")
            deviceModel.setProperty(i, "buttonText", qsTr("Connecting..."))
            deviceModel.setProperty(i, "buttonEnabled", false)
        } else if (state === "connected") {
            deviceModel.setProperty(i, "state", "connected")
            deviceModel.setProperty(i, "buttonText", qsTr("Connected"))
            deviceModel.setProperty(i, "buttonEnabled", false)
        } else if (state === "alreadyExists") {
            deviceModel.setProperty(i, "state", "alreadyExists")
            deviceModel.setProperty(i, "buttonText", qsTr("Already connected"))
            deviceModel.setProperty(i, "buttonEnabled", false)
        } else {
            deviceModel.setProperty(i, "state", "idle")
            deviceModel.setProperty(i, "buttonText", qsTr("Connect"))
            deviceModel.setProperty(i, "buttonEnabled", true)
        }

        if (state !== "idle" && state !== "connecting")
            scheduleStatusReset(i, resetToken)
    }

    function scheduleStatusReset(i, resetToken) {
        const item = deviceModel.get(i)
        const mac = item.mac
        const ip = item.address

        App.Helpers.setTimeout(function() {
            const currentIndex = mac ? root.indexByMac(mac) : root.indexByIp(ip)
            if (currentIndex < 0)
                return

            if (deviceModel.get(currentIndex).statusResetToken !== resetToken)
                return

            root.setStatusAtIndex(currentIndex, "idle")
        }, root.statusResetDelay)
    }

    function setStatusForMac(mac, state) {
        var i = indexByMac(mac)
        if (i < 0) {
            console.log("setStatusForMac: No device found with MAC:", mac)
            return false
        }

        setStatusAtIndex(i, state)
        return true
    }

    function setStatusForIp(ip, state) {
        var i = indexByIp(ip)
        if (i < 0) return false

        setStatusAtIndex(i, state)
        return true
    }

    function evalDevices() {
        const devices = NetworkDeviceProvider.getNetworkDevices()
        if (!devices)
            return

        const keys = Object.keys(devices)
        for (let i = 0; i < keys.length; ++i)
            root.handleDeviceAdded(devices[keys[i]], i)
    }


    function handleDeviceAdded(device, index) {
        var mac = device.macAddress || device.mac || ""
        if (!mac) return

        var i = root.indexByMac(mac)
        if (i < 0) {
            deviceModel.append(root.normalizeDevice(mac, device))
        }

        if (settingsManager.auto_connect_wireless_devices()) {
            App.Helpers.setTimeout(()=>{
                App.DeviceContext.tryToConnectToNetworkDevice(mac, device.address, false)
            }, index * 100)
        }
    }

    Connections {
        target: NetworkDeviceProvider

        function onDeviceAdded(device) {
            root.handleDeviceAdded(device, 1)
        }

        function onDeviceRemoved(macAddress) {
            var i = root.indexByMac(macAddress)
            if (i >= 0) deviceModel.remove(i, 1)
        }
    }

    Connections {
        target: core

        function onInitFailed(macAddress) {
            root.setStatusForMac(macAddress, "failed")
        }

        function onCustomInitFailed(ip, macAddress, error) {
            console.log("Custom network device initialization failed:", ip, macAddress, error)
            if (!macAddress || !root.setStatusForMac(macAddress, "failed")) {
                root.setStatusForIp(ip, "failed")
            }
        }
    }

    Connections {
        target: App.DeviceContext

        function onInitStarted(mac) {
            root.setStatusForMac(mac, "connecting")
        }

        function onDeviceAdded(udid, mac) {
            root.setStatusForMac(mac, "connected")
        }
        function onDeviceAlreadyExistsMAC(mac) {
            console.log("Device with MAC:", mac, "already exists. Setting status to 'alreadyExists'");
            root.setStatusForMac(mac, "alreadyExists")
        }

        function onNoPairingFileForWirelessDevice(macAddress) {
            root.setStatusForMac(macAddress, "noPairing")
        }
    }

    Component.onCompleted: {
        root.evalDevices()
    }

    //eval interval, every 30 seconds
    Timer {
        id: evalTimer
        interval: 30000
        repeat: true
        running: true
        onTriggered: root.evalDevices()
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 6
        spacing: 10

        RowLayout {
            Layout.fillWidth: true
            spacing: 3

            Item {
                visible: retryButton.visible
                Layout.preferredWidth: retryButton.implicitWidth
            }

            Label {
                Layout.fillWidth: true
                Layout.alignment: Qt.AlignVCenter
                text: root.statusText
                horizontalAlignment: Text.AlignHCenter
                font.pointSize: 12
                font.weight: Font.Medium
                wrapMode: Text.WordWrap
            }

            Button {
                id: retryButton
                Layout.alignment: Qt.AlignVCenter
                visible: NetworkDeviceProvider.state === NetworkDeviceProvider.Failed
                enabled: NetworkDeviceProvider.state === NetworkDeviceProvider.Failed
                text: qsTr("Retry")
                onClicked: NetworkDeviceProvider.restartBrowsing()
            }
        }

        SectionBox {
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.maximumWidth: 600
            padding: 0

            ColumnLayout {
                spacing: 8

                Label {
                    Layout.topMargin: 10
                    Layout.leftMargin: 10
                    Layout.rightMargin: 10
                    text: qsTr("Network Devices")
                    font.pointSize: 14
                    // font.weight: Font.Bold
                }

                ScrollView {
                    id: deviceScroll
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    contentWidth: availableWidth
                    contentHeight: scrollContent.height
                    clip: true
                    ScrollBar.horizontal.policy: ScrollBar.AlwaysOff

                    Item {
                        id: scrollContent
                        width: deviceScroll.availableWidth
                        height: deviceColumn.implicitHeight + 24

                        Column {
                            id: deviceColumn
                            x: 12
                            y: 12
                            width: Math.max(0, parent.width - 24)
                            spacing: 12

                            Repeater {
                            model: deviceModel

                            delegate: Item {
                                id: deviceSectionBox
                                width: parent.width
                                height: deviceRow.implicitHeight

                                RowLayout {
                                    id: deviceRow
                                    anchors.left: parent.left
                                    anchors.right: parent.right
                                    spacing: 10

                                    Label {
                                        text: "●"
                                        font.pointSize: 14
                                        color: App.Theme.accent
                                    }

                                    ColumnLayout {
                                        id: content
                                        Layout.fillWidth: true


                                        spacing: Qt.platform.os === "windows" ? 3 : 0

                                        RowLayout {
                                            Layout.fillWidth: true
                                            spacing: 4

                                            Image {
                                                id: deviceIcon
                                                Layout.preferredWidth: 30
                                                Layout.preferredHeight: 40
                                                source: iconSource
                                                fillMode: Image.PreserveAspectFit
                                                smooth: true
                                                mipmap: true
                                            }


                                            RowLayout {
                                                Layout.fillWidth: true
                                                Layout.minimumWidth: 0

                                                Label {
                                                    text: name
                                                    wrapMode: Text.NoWrap
                                                    elide: Text.ElideRight
                                                }

                                                Rectangle {
                                                    visible: !!parsedProductType
                                                    implicitWidth: productTypeLabel.implicitWidth + 16
                                                    implicitHeight: productTypeLabel.implicitHeight + 8
                                                    radius: implicitHeight / 2
                                                    color: productVersion ? App.Theme.selectionSoft : App.Theme.softBg
                                                    border.color: productVersion ? App.Theme.selectionStroke : App.Theme.separator
                                                    border.width: 1

                                                    Label {
                                                        id: productTypeLabel
                                                        anchors.centerIn: parent
                                                        text: parsedProductType
                                                        color: parsedProductType ? App.Theme.accent : App.Theme.textMuted
                                                        font.pointSize: 9
                                                        font.weight: Font.Medium
                                                    }
                                                }

                                                Item {
                                                    Layout.fillWidth: true
                                                }
                                            }

                                            ToolButton {
                                                id: sectionMenuButton
                                                Layout.minimumWidth: implicitWidth
                                                enabled: !!address
                                                icon.source: "qrc:/resources/icons/mi_options-vertical.svg"
                                                icon.color: palette.text
                                                onClicked: sectionMenu.open()

                                                background: Rectangle {
                                                    color: "transparent"
                                                }

                                                Menu {
                                                    id: sectionMenu

                                                    MenuItem {
                                                        text: qsTr("Connect via custom pairing file")
                                                        onTriggered: pairingFileDialog.open()
                                                    }
                                                }
                                            }
                                        }

                                        RowLayout {
                                            Layout.fillWidth: true
                                            spacing: 12

                                            Label {
                                                text: productVersion ? `iOS: ${productVersion}`
                                                                        : qsTr("iOS unavailable")
                                                font.pointSize: 9
                                                font.weight: Font.Medium
                                            }

                                            Label {
                                                Layout.fillWidth: true
                                                Layout.minimumWidth: 0
                                                text: qsTr("IP: %1").arg(address || "-")
                                                elide: Text.ElideRight
                                                font.pointSize: 9
                                                font.weight: Font.Medium
                                            }

                                            Button {
                                                Layout.minimumWidth: implicitWidth
                                                text: buttonText
                                                enabled: buttonEnabled
                                                onClicked: {
                                                    root.setStatusForMac(mac, "connecting")
                                                    App.DeviceContext.tryToConnectToNetworkDevice(mac, address, true)
                                                }
                                            }
                                        }

                                        Rectangle {
                                            Layout.fillWidth: true
                                            height: 1
                                            color: App.Theme.sidebarDivider
                                        }
                                    }
                                }

                                FileDialog {
                                    id: pairingFileDialog
                                    title: qsTr("Choose pairing file")
                                    fileMode: FileDialog.OpenFile
                                    nameFilters: [qsTr("Property List files (*.plist)")]
                                    onAccepted: {
                                        var path = QmlUtils.url_to_path(selectedFile)
                                        if (!path || !address) return

                                        App.DeviceContext.tryToConnectToNetworkDeviceCustom(address, path)
                                        root.setStatusForIp(address, "connecting")
                                    }
                                }
                            }
                        }

                            Item { width: 1; height: 1 } // spacer
                        }
                    }
                }
            }
        }
    }
}
