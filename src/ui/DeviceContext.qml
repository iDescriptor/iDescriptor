// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

pragma Singleton
import QtQml
import QtQml.Models
import QtQuick
import iDescriptor

QtObject {
    id: root
    property ListModel devices: ListModel {}
    property ListModel recoveryDevices: ListModel {}
    property ListModel pendingDevices: ListModel {}
    property string currentDeviceUdid : ""
    property string currentDestination: "welcome"
    property string currentDestinationId: ""
    // Record<normalized MAC, { ip, selectOnSuccess }>
    property var wirelessConnectionAttempts: ({})
    // Record<UDID, attempt token>
    property var wifiEnableAttempts: ({})
    property int wifiEnableAttemptToken: 0
    property bool gcScheduled: false
    property bool sidebarVisible: true

    signal deviceRemoved(string udid)
    signal recoveryDeviceRemoved(string ecid)
    signal deviceAdded(string udid, string mac)
    signal deviceAlreadyExistsMAC(string mac)
    signal initStarted(string mac)
    signal noPairingFileForWirelessDevice(string mac)
    signal pairingFailed(string udid)

    /*
       schedule garbage collection otherwise
       things get cleaned up after a long time
    */
    function scheduleGc() {
        if (root.gcScheduled)
            return

        root.gcScheduled = true
        Qt.callLater(function() {
            root.gcScheduled = false
            gc()
        })
    }

    function init() {
        /* core is a global obj set from rust side*/
        core.init()
    }

    function getDevice(udid) {
        for (let i = 0; i < devices.count; i++) {
            const device = devices.get(i)
            if (device.udid === udid) {
                return device
            }
        }
        return null
    }

    function getDeviceCount() {
        return devices.count + recoveryDevices.count
    }

    function getVisibleDeviceCount() {
        return devices.count + recoveryDevices.count + pendingDevices.count
    }

    function selectConnectedDevice(udid) {
        if (!root.getDevice(udid))
            return

        root.currentDeviceUdid = udid
        root.currentDestinationId = udid
        root.currentDestination = "device"
    }

    function setCurrentDevice(udid) {
        if (root.getDevice(udid))
            root.currentDeviceUdid = udid
    }

    function selectRecoveryDevice(id) {
        if (!root.getRecoveryDevice(id))
            return

        root.currentDestinationId = id
        root.currentDestination = "recoveryDevice"
    }

    function selectPendingDevice(udid) {
        if (!root.getPendingDevice(udid))
            return

        root.currentDestinationId = udid
        root.currentDestination = "pendingDevice"
    }

    function selectWelcomePage() {
        root.currentDestinationId = ""
        root.currentDestination = "welcome"
    }

    function selectAppsPage() {
        root.currentDestinationId = ""
        root.currentDestination = "apps"
    }

    function selectToolboxPage() {
        root.currentDestinationId = ""
        root.currentDestination = "toolbox"
    }

    function selectJailbrokenPage() {
        root.currentDestinationId = ""
        root.currentDestination = "jailbroken"
    }

    function selectCommunityPage() {
        root.currentDestinationId = ""
        root.currentDestination = "community"
    }

    function selectDonatePage() {
        root.currentDestinationId = ""
        root.currentDestination = "donate"
    }

    function selectDeviceSection(sectionIndex) {
        if (root.currentDestination !== "device")
            return

        const device = root.getDevice(root.currentDestinationId)
        if (!device || sectionIndex < 0 || sectionIndex > 3)
            return

        if (device.currentSection !== sectionIndex)
            device.currentSection = sectionIndex
    }

    function getRecoveryDevice(id) {
        for (let i = 0; i < recoveryDevices.count; i++) {
            const device = recoveryDevices.get(i)
            if (device.id === id) {
                return device
            }
        }
        return null
    }

    function getPendingDevice(udid) {
        for (let i = 0; i < pendingDevices.count; i++) {
            const device = pendingDevices.get(i)
            if (device.udid === udid)
                return device
        }
        return null
    }

    function removePendingDevice(udid, updateSelection) {
        for (let i = 0; i < pendingDevices.count; i++) {
            if (pendingDevices.get(i).udid === udid) {
                pendingDevices.remove(i)
                break
            }
        }

        const removedDestination = root.currentDestination === "pendingDevice"
                                   && root.currentDestinationId === udid
        if (updateSelection !== false && removedDestination)
            root.selectFallbackDevice()
    }

    function selectFallbackDevice() {
        if (root.currentDestination === "device"
                && root.currentDestinationId
                && root.getDevice(root.currentDestinationId)) {
            root.selectConnectedDevice(root.currentDestinationId)
            return
        }

        if (root.currentDestination === "recoveryDevice"
                && root.currentDestinationId
                && root.getRecoveryDevice(root.currentDestinationId)) {
            root.selectRecoveryDevice(root.currentDestinationId)
            return
        }

        if (root.currentDestination === "pendingDevice"
                && root.currentDestinationId
                && root.getPendingDevice(root.currentDestinationId)) {
            root.selectPendingDevice(root.currentDestinationId)
            return
        }

        if (devices.count > 0) {
            root.selectConnectedDevice(devices.get(0).udid)
        } else if (recoveryDevices.count > 0) {
            root.selectRecoveryDevice(recoveryDevices.get(0).id)
        } else if (pendingDevices.count > 0) {
            root.selectPendingDevice(pendingDevices.get(0).udid)
        } else {
            root.selectWelcomePage()
        }
    }

    function getDeviceByMacAddress(mac) {
        const normalizedMac = root.normalizeMacAddress(mac)
        if (normalizedMac.length !== 12)
            return null
        for (let i = 0; i < devices.count; i++) {
            const device = devices.get(i)
            if (root.normalizeMacAddress(device.info["WiFiAddress"]) === normalizedMac) {
                return device
            }
        }
        return null
    }

    function normalizeMacAddress(mac) {
        return String(mac || "").replace(/[^0-9a-f]/gi, "").toLowerCase()
    }

    function enableWifiConnections(device, dialogParent) {
        if (!device || !device.udid || !device.service_manager) {
            console.log("WTF")
            return false
        }

        const udid = String(device.udid)
        if (root.wifiEnableAttempts[udid])
            return false

        const serviceManager = device.service_manager
        const deviceName = device.info && device.info.marketing_name
                ? device.info.marketing_name : qsTr("this device")
        const attemptToken = ++root.wifiEnableAttemptToken
        root.wifiEnableAttempts[udid] = attemptToken

        Helpers.connectOnce(serviceManager.enableWifiConnectionsResult, function(success) {
            if (root.wifiEnableAttempts[udid] !== attemptToken)
                return

            delete root.wifiEnableAttempts[udid]

            if (success) {
                settingsManager.set_has_seen_device(udid, true)
                Helpers.messageBox(
                    dialogParent,
                    qsTr("Wi-Fi Connections Enabled"),
                    qsTr("Wi-Fi connections are now enabled for %1. You can disconnect the cable and use this device wirelessly.").arg(deviceName)
                )
            } else {
                Helpers.messageBox(
                    dialogParent,
                    qsTr("Unable to Enable Wi-Fi Connections"),
                    qsTr("Wi-Fi connections could not be enabled for %1. Keep the device connected, unlocked, and trusted, then try again.").arg(deviceName)
                )
            }
        })

        serviceManager.enable_wifi_connections()
        return true
    }

    function networkDeviceForMac(mac) {
        const normalizedMac = root.normalizeMacAddress(mac)
        if (normalizedMac.length !== 12)
            return null

        const networkDevices = NetworkDeviceProvider.getNetworkDevices()
        const keys = Object.keys(networkDevices)
        for (let i = 0; i < keys.length; ++i) {
            const key = keys[i]
            const networkDevice = networkDevices[key]
            const candidateMac = networkDevice.macAddress || networkDevice.mac || key
            if (root.normalizeMacAddress(candidateMac) === normalizedMac)
                return networkDevice
        }

        return null
    }

    function upgradeWiredDeviceToWireless(mac, selectOnSuccess) {
        if (!settingsManager.upgrade_to_wireless_on_disconnect())
            return

        const networkDevice = root.networkDeviceForMac(mac)
        if (!networkDevice)
            return

        const ip = networkDevice.address || networkDevice.ip || ""
        if (!ip)
            return

        root.tryToConnectToNetworkDevice(mac, ip, selectOnSuccess)
    }

    function finishWirelessConnectionAttempt(mac) {
        const key = root.normalizeMacAddress(mac)
        const attempt = root.wirelessConnectionAttempts[key]
        if (attempt)
            delete root.wirelessConnectionAttempts[key]
        return attempt || null
    }

    // receives path of pairing file
    function tryToConnectToNetworkDeviceCustom(ip, path) {
        console.log("QML: Trying to connect to network device with IP:", ip, "and pairing file path:", path);
        core.init_wireless_device_custom(ip, path)
    }

    function tryToConnectToNetworkDevice(mac, ip, select_on_success) {
        console.log("QML: Trying to connect to network device with MAC:", mac, "IP:", ip);

        const existingDevice = root.getDeviceByMacAddress(mac);

        if (existingDevice) {
            console.log("Device with MAC:", mac, "already exists. Emitting deviceAlreadyExistsMAC event");
            root.deviceAlreadyExistsMAC(mac);
            if (select_on_success) {
                console.log("Setting existing device as current selection");
                root.selectConnectedDevice(existingDevice.udid);
            }
            return;
        }

        const key = root.normalizeMacAddress(mac)
        const existingAttempt = root.wirelessConnectionAttempts[key]
        if (existingAttempt) {
            // A manual request upgrades an automatic attempt without starting another connection.
            if (select_on_success)
                existingAttempt.selectOnSuccess = true
            return;
        }

        root.wirelessConnectionAttempts[key] = {
            ip: ip,
            selectOnSuccess: !!select_on_success
        }
        core.init_wireless_device(ip, mac);
        root.initStarted(mac);
    }

    function removeDevice(udid) {
        const removedDeviceWasVisible = root.currentDestination === "device"
                                        && root.currentDestinationId === udid
        for (let i = 0; i < devices.count; i++) {
            const device = devices.get(i)
            if (device.udid === udid) {
                const connectionId = device.connectionId
                devices.remove(i)
                if (root.currentDeviceUdid === udid) {
                    root.currentDeviceUdid = devices.count > 0
                            ? devices.get(0).udid : ""
                }
                core.remove_device(udid, connectionId)
                if (removedDeviceWasVisible)
                    root.selectFallbackDevice()
                root.scheduleGc()
                break
            }
        }
    }



    // workaround to use connections inside a QtObject
    property var _connections : Connections {
        target: core

        function onDeviceEvent(eventType, udid, info) {
            console.log("Device event:", eventType, udid, JSON.stringify(info))

            switch (eventType) {
                /* Device added */
                case 1:
                    const addedConnectionId = Number(info.connection_id || 0)
                    if (!Number.isInteger(addedConnectionId) || addedConnectionId <= 0) {
                        console.warn("Ignoring connected event without a connection ID:", udid)
                        break
                    }

                    const service_manager = serviceFactory.create_service_manager(
                        udid, addedConnectionId, info.ios_version_major)
                    const afcClient = serviceFactory.create_afc_client(
                        udid, addedConnectionId, false)
                    if (!service_manager || !afcClient) {
                        console.warn("Ignoring stale connected event:", udid, addedConnectionId)
                        break
                    }
                    const sb_client = serviceFactory.create_springboard_services_client(
                        udid, addedConnectionId)

                    const currentDevice = root.getDevice(udid)
                    if (currentDevice && currentDevice.connectionId === addedConnectionId)
                        break

                    let skipWirelessReplacement = false
                    for (let i = 0; i < devices.count; ++i) {
                        const existingDevice = devices.get(i)
                        if (existingDevice.udid === udid) {
                            if (!existingDevice.info.is_wireless && info.is_wireless) {
                                console.log("Device is already connected via USB. Ignoring wireless connection event for:", udid)
                                skipWirelessReplacement = true
                                break
                            }
                            devices.remove(i)
                            root.scheduleGc()
                            break
                        }
                    }
                    if (skipWirelessReplacement) {
                        root.selectConnectedDevice(udid)
                        break
                    }

                    const pendingDeviceWasVisible =
                        root.currentDestination === "pendingDevice"
                        && root.currentDestinationId === udid
                    root.removePendingDevice(udid, false)
                    const text = `${info.marketing_name} / ${udid.slice(0,10)}...`
                    const mac = info["WiFiAddress"]
                    const wirelessAttempt = info.is_wireless
                            ? root.finishWirelessConnectionAttempt(mac) : null
                    devices.append(
                        {
                            udid: udid,
                            connectionId: addedConnectionId,
                            info: info,
                            text,
                            service_manager,
                            sb_client,
                            // default to info section
                            currentSection: 0,
                            afcClient
                        }
                    )
                    root.deviceAdded(udid, mac)
                    if (pendingDeviceWasVisible
                            || root.currentDestination === "welcome")
                        root.selectConnectedDevice(udid)
                    else if (wirelessAttempt && wirelessAttempt.selectOnSuccess)
                        root.selectConnectedDevice(udid)
                    else
                        root.setCurrentDevice(udid)
                    break;
                /* Device removed */
                case 2:
                    const removedConnectionId = Number(info.connection_id || 0)
                    if (!Number.isInteger(removedConnectionId) || removedConnectionId <= 0) {
                        console.warn("Ignoring disconnected event without a connection ID:", udid)
                        break
                    }
                    let removedMac = ""
                    let removedDeviceWasWireless = false
                    let removedMatchingDevice = false
                    const removedDeviceWasSelected = root.currentDeviceUdid === udid
                    const removedDeviceWasVisible = root.currentDestination === "device"
                                                    && root.currentDestinationId === udid

                    // FIXME: find an O(1) solution
                    for (let i = 0; i < devices.count; i++) {
                        const device = devices.get(i)
                        if (device.udid === udid) {
                            if (device.connectionId !== removedConnectionId) {
                                console.log("Ignoring stale disconnected event:", udid,
                                            removedConnectionId, "current:", device.connectionId)
                                break
                            }
                            removedMac = device.info["WiFiAddress"] || ""
                            removedDeviceWasWireless = !!device.info.is_wireless
                            devices.remove(i)
                            removedMatchingDevice = true
                            break
                        }
                    }
                    if (!removedMatchingDevice)
                        break
                    delete root.wifiEnableAttempts[udid]
                    if (removedDeviceWasSelected) {
                        root.currentDeviceUdid = devices.count > 0
                                ? devices.get(0).udid : ""
                    }
                    root.removePendingDevice(udid, false)
                    root.deviceRemoved(udid)
                    if (removedDeviceWasVisible)
                        root.selectFallbackDevice()
                    if (!removedDeviceWasWireless && removedMac) {
                        Qt.callLater(function() {
                            root.upgradeWiredDeviceToWireless(
                                removedMac,
                                removedDeviceWasSelected
                            )
                        })
                    }
                    root.scheduleGc()
                    break;
                /* Pending device added */
                case 3:
                    if (root.getDevice(udid) || root.getPendingDevice(udid))
                        break

                    pendingDevices.append({
                        udid: udid,
                        text: qsTr("Pairing…")
                    })
                    if (root.currentDestination === "welcome")
                        root.selectPendingDevice(udid)
                    break;
                /* Pending device removed */
                case 4:
                    if (root.getPendingDevice(udid)) {
                        root.removePendingDevice(udid, true)
                        root.pairingFailed(udid)
                    }
                    break;
                default:

            }
        }


        function onRecoveryDeviceEvent(eventType, id, info) {
            console.log("Recovery device event:", eventType, id, JSON.stringify(info))

            switch (eventType) {
                /* Recovery device added */
                case 1:

                    const name = info.marketing_name;
                    const entry = { id, info: info, text: name }
                    recoveryDevices.append(entry)

                    if (root.currentDestination === "welcome")
                        root.selectRecoveryDevice(id)
                    break;
                /* Recovery device removed */
                case 2:
                    for (let i = 0; i < recoveryDevices.count; i++) {
                        const device = recoveryDevices.get(i)
                        if (device.id === id) {
                            const removedEcid = device.info.ecid_decimal || ""
                            recoveryDevices.remove(i)
                            root.recoveryDeviceRemoved(removedEcid)
                            break
                        }
                    }
                    if (root.currentDestination === "recoveryDevice"
                            && root.currentDestinationId === id)
                        root.selectFallbackDevice()
                    root.scheduleGc()
                    break;
            }
        }

        function onInitFailed(macAddress) {
            root.finishWirelessConnectionAttempt(macAddress)
        }

        function onNoPairingFile(macAddress) {
            root.finishWirelessConnectionAttempt(macAddress)
            root.noPairingFileForWirelessDevice(macAddress)
        }
    }

}
