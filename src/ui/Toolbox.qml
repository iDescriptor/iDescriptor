// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

pragma Singleton

import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
import QtQuick.Controls.impl
import QtQuick.Dialogs
import "." as App

Item {
    id: root
    anchors.fill: parent

    MessageDialog {
        id: errorDialog
        title: qsTr("Error")
        text: ""
    }

    MessageDialog {
        id: infoDialog
        title: qsTr("Information")
        text: ""
    }

    property string currentDeviceUdid: App.DeviceContext.currentDeviceUdid
    property var toolWindows: ({})
    property var pendingUnpairs: ({})
    readonly property bool hasDevice: App.DeviceContext.devices && App.DeviceContext.devices.count > 0

    Component.onCompleted: Qt.callLater(root.syncNormalDeviceSelection)

    function showError(message) {
        errorDialog.text = message
        errorDialog.open()
    }

    function showInfo(message) {
        infoDialog.text = message
        infoDialog.open()
    }

    function confirmDeviceAction(action, title, message, removeDevice, udid) {
        App.Helpers.messageBox(
            root,
            title,
            message,
            MessageDialog.Yes | MessageDialog.No,
            function(button) {
                if (button === MessageDialog.Yes)
                    root.performDeviceAction(action, udid, removeDevice)
            })
    }

    function requestDeviceAction(action, udid) {
        if (!udid || !App.DeviceContext.getDevice(udid)) {
            root.showError(qsTr("The selected device is no longer connected."))
            return
        }

        switch (action) {
            case "restart":
                root.confirmDeviceAction(
                    action,
                    qsTr("Restart Device"),
                    qsTr("Are you sure you want to restart this device?"),
                    true,
                    udid)
                break
            case "shutdown":
                root.confirmDeviceAction(
                    action,
                    qsTr("Shut Down Device"),
                    qsTr("Are you sure you want to shut down this device?"),
                    true,
                    udid)
                break
            case "recovery":
                root.confirmDeviceAction(
                    action,
                    qsTr("Enter Recovery Mode"),
                    qsTr("Are you sure you want to put this device into recovery mode?"),
                    true,
                    udid)
                break
            case "unpair":
                root.confirmDeviceAction(
                    action,
                    qsTr("Unpair iDevice"),
                    qsTr("Are you sure you want to unpair this device? You will need to trust and pair it again before reconnecting."),
                    false,
                    udid)
                break
            case "unpairAndRemove":
                root.confirmDeviceAction(
                    "unpair",
                    qsTr("Unpair and Remove iDevice"),
                    qsTr("Are you sure you want to unpair this device and remove it from iDescriptor? You will need to trust and pair it again before reconnecting."),
                    true,
                    udid)
                break
            default:
                root.showError(qsTr("Unknown device action."))
        }
    }

    function performUnpair(device, udid, removeDevice) {
        if (root.pendingUnpairs[udid]) {
            root.showError(qsTr("An unpair operation is already in progress for this device."))
            return
        }

        const serviceManager = device.service_manager
        const completed = function(success, error) {
            serviceManager.unpairCompleted.disconnect(completed)
            delete root.pendingUnpairs[udid]

            if (!success) {
                root.showError(error && error.length
                    ? qsTr("Failed to unpair the device: %1").arg(error)
                    : qsTr("Failed to unpair the device."))
                return
            }

            root.showInfo(qsTr("The device was unpaired successfully."))
            if (removeDevice && App.DeviceContext.getDevice(udid))
                App.DeviceContext.removeDevice(udid)
        }

        root.pendingUnpairs[udid] = {
            serviceManager: serviceManager,
            completed: completed
        }
        serviceManager.unpairCompleted.connect(completed)
        serviceManager.unpair()
    }

    function performDeviceAction(action, udid, removeDevice) {
        const device = App.DeviceContext.getDevice(udid)
        if (!device) {
            root.showError(qsTr("The selected device is no longer connected."))
            return
        }

        let success = false
        switch (action) {
            case "restart":
                success = device.service_manager.restart()
                break
            case "shutdown":
                success = device.service_manager.shutdown()
                break
            case "recovery":
                success = device.service_manager.enter_recovery_mode()
                break
            case "unpair":
                root.performUnpair(device, udid, removeDevice)
                return
            default:
                showError(qsTr("Unknown device action."))
                return
        }

        if (!success)
            showError(qsTr("Failed to send the command to the device. Make sure it is connected and unlocked."))
        else {
            showInfo(qsTr("Action '%1' sent successfully.").arg(action))
            if (removeDevice) {
                App.DeviceContext.removeDevice(udid)
            }
        }
    }

    function createComp(loc, args = {}) {
        const comp = Qt.createComponent(loc)
        if (comp.status === Component.Ready) {
            const win = comp.createObject(root,args)
            if (win !== null) {
                let destructionScheduled = false
                let closeCheckScheduled = false
                win.closing.connect(function(closeEvent) {
                    if (closeCheckScheduled || destructionScheduled)
                        return

                    closeCheckScheduled = true
                    Qt.callLater(function() {
                        closeCheckScheduled = false
                        if (win.visible || destructionScheduled)
                            return

                        destructionScheduled = true
                        win.destroy(0)
                    })
                })
                win.show()
                return win
            } else {
                console.error("createObject failed:", comp.errorString())
            }

        } else if (comp.status === Component.Error) {
            console.error("Component failed to load:", comp.errorString())
        }

        return null
    }

    function focusToolWindow(win) {
        if (!win)
            return false

        win.show()
        win.raise()
        win.requestActivate()
        return true
    }



    function forgetToolWindow(key, win) {
        // An older closing window must not remove its replacement.
        if (root.toolWindows[key] === win)
            delete root.toolWindows[key]
    }

    // A lifetime observer is parented to the window so explicit destruction also
    // clears the registry, without relying on QObject.destroyed in QML.
    Component {
        id: toolWindowLifetime
        QtObject {
            property string registryKey
            property var trackedWindow
            Component.onDestruction: root.forgetToolWindow(registryKey, trackedWindow)
        }
    }

    function openToolWindow(loc, deviceBound, device, udid, extraArgs = {}) {
        const key = JSON.stringify([loc, deviceBound ? udid : ""])
        const existing = root.toolWindows[key]
        if (existing) {
            focusToolWindow(existing)
            return
        }

        const args = Object.assign({}, extraArgs, {
            device: device,
            udid: udid,
            auto_close: deviceBound
        })
        const win = createComp(loc, args)
        if (!win)
            return

        root.toolWindows[key] = win
        toolWindowLifetime.createObject(win, {
            registryKey: key,
            trackedWindow: win
        })
        win.visibleChanged.connect(function() {
            if (!win.visible)
                root.forgetToolWindow(key, win)
        })
    }

    // 0 Airplayer, 1 SimulateLocation, 2 LiveScreen, 3 QueryMobileGestalt, 4 DeveloperDiskImages,
    // 5 WirelessGalleryImport, 6 iFuse, 7 CableInfo, 8 NetworkDevices, 9 EnableDevMode,
    // 10 Restart, 11 Shutdown, 12 RecoveryMode, 13 EnableWifiConnections, 14 BackupManager,
    // 15 TransferSpeedTest, 16 Unpair, 17 UnpairAndRemove, 18 DetailedBatteryInfo,
    // 19 WiredGalleryImport, 20 ScreenMirroring, 21 FirmwareManager
    // signal toolClicked(int toolId, bool requiresDevice)
    function toolClicked(toolId, requiresDevice, wirelessNotAllowed) {
        const device = App.DeviceContext.getDevice(currentDeviceUdid)

        if (requiresDevice) {
            if (!device) {
                console.log("DEVICE DISAPPERED")
                return
            }
            if (wirelessNotAllowed && device.info.is_wireless) {
                showError(qsTr("This tool is not available for wireless devices. Please connect your device via USB."))
                return
            }
        }

        switch (toolId) {
            case 0:
                const gl_plugin_loaded = AirplayImp.load_gst_gl()
                if (!gl_plugin_loaded) {
                    switch (Qt.platform.os) {
                        case "linux":
                            errorDialog.text = qsTr("Failed to load gst gl plugin, make sure you have QT_QPA_PLATFORM=xcb env var set")
                            break;
                        case "windows":
                            errorDialog.text = qsTr("Failed to load gst gl plugin, make sure you can use OpenGL")
                            break;
                        case "macos":
                            errorDialog.text = qsTr("Failed to load gst gl plugin, make sure you can use OpenGL")
                            break;
                        default:
                            errorDialog.text = qsTr("Failed to load gst gl plugin")

                    }
                    errorDialog.open()
                    return;
                }

                root.openToolWindow("./tools/Airplay.qml", false, device, currentDeviceUdid)
                break;
            case 20:
                if (device.info.ios_version_major < 27) {
                    root.showInfo(qsTr("Screen Mirroring requires iOS 27 or later. Use AirPlay for this device."))
                    return
                }
                if (!AirplayImp.load_gst_gl()) {
                    root.showError(qsTr("Could not load the GStreamer Qt GL video plugin. Check your OpenGL and GStreamer installation."))
                    return
                }
                root.openToolWindow("./tools/ScreenMirroring.qml", true, device, currentDeviceUdid)
                break;
            case 1:
                root.openToolWindow("./tools/SimulateLocation.qml", true, device, currentDeviceUdid)
                break;
            case 2:
                root.openToolWindow("./tools/LiveScreen.qml", true, device, currentDeviceUdid)
                break;
            case 3:
                // FIXME: doesnt work iOS 17 and above
                root.openToolWindow("./tools/QueryMobileGestalt.qml", true, device, currentDeviceUdid)
                break;
            case 4:
                root.openToolWindow("./tools/DevDiskImages.qml", false, device, currentDeviceUdid)
                break;
            case 5:
                root.openToolWindow("./tools/WirelessGalleryImport.qml", false, device, currentDeviceUdid)
                break;
            case 6:
                root.openToolWindow("./tools/IFuse.qml", true, device, currentDeviceUdid)
                break;
            case 7:
                root.openToolWindow("./tools/CableInfo.qml", true, device, currentDeviceUdid)
                break;
            case 8:
                root.openToolWindow("./tools/NetworkDevices.qml", false, device, currentDeviceUdid)
                break;
            case 9: {
                const major_version = device.info.ios_version_major

                function startDevModeHelper() {
                    const component = Qt.createComponent("./DevModeHelper.qml")
                    if (component.status !== Component.Ready) {
                        root.showError(qsTr("Failed to load Developer Mode helper: %1").arg(component.errorString()))
                        return
                    }

                    const helper = component.createObject(root, {
                        device,
                        tryAnywayEnabled: false
                    })
                    if (!helper) {
                        root.showError(qsTr("Failed to create Developer Mode helper: %1").arg(component.errorString()))
                        return
                    }

                    let preparationFailed = false
                    helper.preparationFailed.connect(function(message) {
                        preparationFailed = true
                        root.showError(message)
                    })
                    helper.handled.connect(function(success) {
                        if (success) {
                            root.showInfo(helper.iosVersion >= 17
                                ? qsTr("Developer Mode is enabled on the selected device.")
                                : qsTr("A developer disk image is mounted on the selected device."))
                        } else if (!preparationFailed) {
                            root.showError(helper.iosVersion >= 17
                                ? qsTr("Developer Mode was not enabled. Complete the steps on the device and try again.")
                                : qsTr("A developer disk image could not be mounted."))
                        }

                        Qt.callLater(function() {
                            helper.destroy()
                        })
                    })
                    helper.start()
                }

                const title = major_version < 17
                    ? qsTr("Mount Developer Disk Image?")
                    : qsTr("Check Developer Mode?")
                const message = major_version < 17
                    ? qsTr("This tool will mount a developer disk image for you. Developer disk images are required to enable extra features on the device. Do you want to continue?")
                    : qsTr("This tool will check if Developer Mode is enabled on your device. Developer Mode is required to enable extra features on the device. Do you want to continue?")

                App.Helpers.messageBox(
                    root,
                    title,
                    message,
                    MessageDialog.Yes | MessageDialog.No,
                    function(button) {
                        if (button === MessageDialog.Yes)
                            startDevModeHelper()
                    })
                break
            }
            case 10:
                requestDeviceAction("restart", currentDeviceUdid)
                break;
            case 11:
                requestDeviceAction("shutdown", currentDeviceUdid)
                break;
            case 12:
                requestDeviceAction("recovery", currentDeviceUdid)
                break;
            case 13:
                App.DeviceContext.enableWifiConnections(device, root)
                break;
            case 14:
            // FIXME: BackupManager is no longer device bound which means each backup process
            // should listen for device connection changes
                root.openToolWindow("./tools/BackupManager.qml", false, device, currentDeviceUdid)
                break;
            case 15:
                root.openToolWindow("./tools/TransferSpeedTest.qml", true, device, currentDeviceUdid)
                break;
            case 16:
                requestDeviceAction("unpair", currentDeviceUdid)
                break;
            case 17:
                requestDeviceAction("unpairAndRemove", currentDeviceUdid)
                break;
            case 18:
                root.openToolWindow("./tools/BatteryInfo.qml", true, device, currentDeviceUdid, { info: device.info })
                break;
            case 19:
                root.openToolWindow("./tools/WiredGalleryImport.qml", true, device, currentDeviceUdid)
                break;
            case 21:
                root.openToolWindow("./tools/FirmwareManager.qml", false, device, currentDeviceUdid)
                break;
            default:
            console.log(`No tool for id ${toolId}`)
        }


    }

    function deviceSelectionChanged(udid) {
        if (udid && udid.length) {
            App.DeviceContext.setCurrentDevice(udid)
        }
    }

    function deviceIndexForUdid(udid) {
        if (!udid || !App.DeviceContext.getDevice(udid))
            return -1

        for (let i = 0; i < App.DeviceContext.devices.count; ++i) {
            if (App.DeviceContext.devices.get(i).udid === udid)
                return i
        }
        return -1
    }

    function syncNormalDeviceSelection() {
        const udid = App.DeviceContext.currentDeviceUdid

        // The visible destination and Toolbox's device context are independent.
        if (!udid) {
            if (!root.hasDevice) {
                deviceCombo.currentIndex = 0
            }
            return
        }

        const index = root.deviceIndexForUdid(udid)
        if (index < 0)
            return

        if (deviceCombo.currentIndex !== index) {
            deviceCombo.currentIndex = index
        }
    }

    Connections {
        target: App.DeviceContext

        function onCurrentDeviceUdidChanged() {
            root.syncNormalDeviceSelection()
        }
    }



    readonly property var mainToolsModel: ([
        {
            toolId: 0,
            title: qsTr("Airplayer"),
            description: qsTr("Cast your device screen"),
            requiresDevice: false,
            iconSource: "qrc:/resources/icons/material-symbols_airplay-outline-rounded.svg",
            visible: true
        },
        {
            toolId: 1,
            title: qsTr("Simulate Location"),
            description: qsTr("Simulate GPS location on your device"),
            requiresDevice: true,
            iconSource: "qrc:/resources/icons/material-symbols_location-on-outline.svg",
            visible: true
        },
        {
            toolId: 20,
            title: qsTr("Screen Mirroring"),
            description: qsTr("Stream your iPhone screen directly to iDescriptor"),
            requiresDevice: true,
            iconSource: "qrc:/resources/icons/pepicons-print_cellphone-eye.svg",
            visible: true
        },
        {
            toolId: 2,
            title: qsTr("Live Screen"),
            description: qsTr("View device screen in real-time"),
            requiresDevice: true,
            iconSource: "qrc:/resources/icons/pepicons-print_cellphone-eye.svg",
            visible: true
        },
        {
            toolId: 3,
            title: qsTr("Query Mobile Gestalt"),
            description: qsTr("Query device hardware information"),
            requiresDevice: true,
            iconSource: "qrc:/resources/icons/streamline_programming-browser-search-search-window-glass-app-code-programming-query-find-magnifying-apps.svg",
            visible: true
        },
        {
            toolId: 4,
            title: qsTr("Dev Disk Images"),
            description: qsTr("Manage developer disk images"),
            requiresDevice: false,
            iconSource: "qrc:/resources/icons/tabler_database-export.svg",
            visible: true
        },
        {
            toolId: 5,
            title: qsTr("Wireless Gallery Import"),
            description: qsTr("Import photos wirelessly to your iDevice (requires Shortcuts app)"),
            requiresDevice: false,
            iconSource: "qrc:/resources/icons/material-symbols_android-wifi-3-bar-plus.svg",
            visible: true
        },
        {
            toolId: 19,
            title: qsTr("Wired Gallery Import"),
            description: qsTr("Import photos and videos through iDescriptor Companion"),
            requiresDevice: true,
            wirelessNotAllowed: true,
            iconSource: "qrc:/resources/icons/material-symbols_cable-rounded.svg",
            visible: true
        },
        {
            toolId: 6,
            title: qsTr("iFuse Mount"),
            description: qsTr("Mount your iDevice's filesystem on your PC"),
            requiresDevice: true,
            iconSource: "qrc:/resources/icons/fuse.svg",
            visible: (Qt.platform.os !== "osx" && Qt.platform.os !== "darwin" && !iFuse.is_flatpak_build()),
            wirelessNotAllowed: true
        },
        {
            toolId: 7,
            title: qsTr("Cable Info"),
            description: qsTr("View detailed cable and connection info"),
            requiresDevice: true,
            iconSource: "qrc:/resources/icons/material-symbols_cable-rounded.svg",
            visible: true
        },
        {
            toolId: 18,
            title: qsTr("Detailed Battery Info"),
            description: qsTr("View detailed battery health and capacity information"),
            requiresDevice: true,
            iconSource: "qrc:/resources/icons/idescriptor-battery-details.svg",
            visible: true
        },
        {
            toolId: 8,
            title: qsTr("Network Devices"),
            description: qsTr("Discover and monitor devices on your network"),
            requiresDevice: false,
            iconSource: "qrc:/resources/icons/streamline_ultimate-multiple-users-network.svg",
            visible: true
        },
        {
            toolId: 14,
            title: qsTr("Backups"),
            description: qsTr("Back up and restore this device"),
            requiresDevice: false,
            iconSource: "qrc:/resources/icons/tabler_database-export.svg",
            visible: true
        },
        {
            toolId: 21,
            title: qsTr("Firmware Manager"),
            description: qsTr("Download Apple firmware and restore your device"),
            requiresDevice: false,
            iconSource: "qrc:/resources/icons/mdi_disk.svg",
            visible: true
        },
        {
            toolId: 15,
            title: qsTr("Transfer Speed Test"),
            description: qsTr("Measure upload and download speed to this device"),
            requiresDevice: true,
            iconSource: "qrc:/resources/icons/material-symbols_cable-rounded.svg",
            visible: true
        }
    ])

    readonly property var moreToolsModel: ([
        {
            toolId: 9,
            title: qsTr("Enable Dev Mode"),
            description: qsTr("Check or enable Developer Mode on this device"),
            requiresDevice: true,
            iconSource: "qrc:/resources/icons/mdi_disk.svg",
            visible: true
        },
        {
            toolId: 10,
            title: qsTr("Restart"),
            description: qsTr("Restart device services"),
            requiresDevice: true,
            iconSource: "qrc:/resources/icons/ic_twotone-restart-alt.svg",
            visible: true
        },
        {
            toolId: 11,
            title: qsTr("Shutdown"),
            description: qsTr("Shut down the device"),
            requiresDevice: true,
            iconSource: "qrc:/resources/icons/ic_outline-power-settings-new.svg",
            visible: true
        },
        {
            toolId: 12,
            title: qsTr("Recovery Mode"),
            description: qsTr("Enter device recovery mode"),
            requiresDevice: true,
            iconSource: "qrc:/resources/icons/hugeicons_wrench-01.svg",
            visible: true
        },
        {
            toolId: 13,
            title: qsTr("Enable Wi-Fi Connections"),
            description: qsTr("Make device connectable via Wi-Fi"),
            requiresDevice: true,
            wirelessNotAllowed: true,
            iconSource: "qrc:/resources/icons/streamline-freehand_charging-flash-wireless.svg",
            visible: true
        },
        {
            toolId: 16,
            title: qsTr("Unpair iDevice"),
            description: qsTr("Remove this computer's trust relationship with the device"),
            requiresDevice: true,
            iconSource: "qrc:/resources/icons/idescriptor-unpair.svg",
            visible: true
        },
        {
            toolId: 17,
            title: qsTr("Unpair and Remove iDevice"),
            description: qsTr("Unpair the device and remove it from iDescriptor"),
            requiresDevice: true,
            iconSource: "qrc:/resources/icons/idescriptor-unpair.svg",
            visible: true
        }
    ])

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // Device selection row
        RowLayout {
            Layout.fillWidth: true
            Layout.margins: 12
            spacing: 10

            Label {
                text: qsTr("Device:")
                Layout.alignment: Qt.AlignVCenter
            }

            ComboBox {
                id: deviceCombo
                Layout.minimumWidth: 230
                Layout.preferredWidth: 240
                enabled: root.hasDevice

                model: root.hasDevice ? App.DeviceContext.devices : [{ text: qsTr("No device connected"), udid: "" }]
                textRole: "text"
                valueRole: "udid"

                onActivated: (index) => {
                    console.log("Toolbox activated")
                    const udid = deviceCombo.currentValue || ""
                    root.deviceSelectionChanged(udid)
                }

                // onCountChanged: {
                //     // Model changes and DeviceContext selection updates can arrive
                //     // in the same event turn, so synchronize after both settle.
                //     Qt.callLater(root.syncNormalDeviceSelection)
                // }
            }

            Item { Layout.fillWidth: true }
        }

        ScrollView {
            id: scroll
            Layout.fillWidth: true
            Layout.fillHeight: true

            ColumnLayout {
                width: scroll.availableWidth
                spacing: 14
                Layout.margins: 0

                /* Section: Tools */
                Label {
                    text: qsTr("Tools")
                    font.bold: true
                    font.pixelSize: 14
                    leftPadding: 10
                }

                GridLayout {
                    id: mainGrid
                    Layout.margins: 10
                    Layout.fillWidth: true
                    columns: 3
                    columnSpacing: 10
                    rowSpacing: 10

                    Repeater {
                        model: root.mainToolsModel
                        delegate: ToolTile {
                            Layout.fillWidth: true
                            visible: modelData.visible

                            toolId: modelData.toolId
                            title: modelData.title
                            description: modelData.description
                            requiresDevice: modelData.requiresDevice
                            iconSource: modelData.iconSource

                            enabled: !requiresDevice || root.hasDevice

                            onClicked: {
                                root.toolClicked(toolId, requiresDevice, modelData.wirelessNotAllowed || false)
                            }
                        }
                    }
                }

                /* More Tools */
                Label {
                    text: qsTr("More Tools")
                    font.bold: true
                    font.pixelSize: 14
                    leftPadding: 10
                    topPadding: 6
                }

                GridLayout {
                    id: moreGrid
                    Layout.fillWidth: true
                    Layout.margins: 10
                    columns: 3
                    columnSpacing: 10
                    rowSpacing: 10

                    Repeater {
                        model: root.moreToolsModel
                        delegate: ToolTile {
                            Layout.fillWidth: true
                            visible: modelData.visible

                            toolId: modelData.toolId
                            title: modelData.title
                            description: modelData.description
                            requiresDevice: modelData.requiresDevice
                            iconSource: modelData.iconSource

                            enabled: !requiresDevice || root.hasDevice

                            onClicked: {
                                root.toolClicked(toolId, requiresDevice)
                            }
                        }
                    }
                }

                Item { Layout.fillHeight: true }
            }
        }
    }

    component ToolTile: Rectangle {
        id: tile

        property int toolId: -1
        property string title: ""
        property string description: ""
        property bool requiresDevice: false
        property url iconSource: ""

        signal clicked()

        radius: 8
        color: "transparent"

        implicitHeight: 92

        opacity: enabled ? 1.0 : 0.45

        MouseArea {
            id: mouse
            anchors.fill: parent
            hoverEnabled: true
            enabled: tile.enabled
            cursorShape: tile.enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
            onClicked: tile.clicked()
        }

        RowLayout {
            anchors.fill: parent
            anchors.margins: 12
            spacing: 12

            IconImage {
                id: icon
                source: tile.iconSource
                Layout.preferredHeight: 34
                Layout.preferredWidth: 34
                // FIXME: hardcoded accent color
                color: "#0078d7"
                opacity: tile.enabled ? 1.0 : 0.7
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 4

                Label {
                    text: tile.title
                    font.bold: true
                    elide: Text.ElideRight
                    Layout.fillWidth: true
                }

                Label {
                    text: tile.description
                    wrapMode: Text.WordWrap
                    elide: Text.ElideRight
                    maximumLineCount: 2
                    Layout.fillWidth: true
                    opacity: 0.85
                }
            }
        }
    }
}
