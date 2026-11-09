// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import QtQml.Models
import "../base"
import "../" as App

ToolWindow {
    id: root
    width: 980
    height: 650
    title: qsTr("Firmware Manager - iDescriptor")
    auto_close: false

    property var deviceChoices: []
    property var connectedDeviceChoices: []
    property var catalogDeviceChoices: []
    property var filteredCatalogChoices: []
    property var deviceFamilies: [
        {
            key: "",
            label: qsTr("All")
        },
        {
            key: "iPhone",
            label: qsTr("iPhone")
        },
        {
            key: "iPad",
            label: qsTr("iPad")
        },
        {
            key: "iPod",
            label: qsTr("iPod")
        },
        {
            key: "Apple TV",
            label: qsTr("Apple TV")
        },
        {
            key: "HomePod",
            label: qsTr("HomePod")
        },
        {
            key: "Watch",
            label: qsTr("Watch")
        },
        {
            key: "Mac",
            label: qsTr("Mac")
        },
        {
            key: "Vision",
            label: qsTr("Vision")
        },
        {
            key: "Other",
            label: qsTr("Other")
        }
    ]
    property string currentIdentifier: ""
    property string currentDeviceName: ""
    property string currentDeviceBadge: ""
    property string selectedFamily: ""
    property string downloadDirectory: ""
    property int currentTab: 0
    property bool signedOnly: true
    property var flashTargets: []

    function normalizedIdentifier(identifier) {
        return String(identifier || "").trim().toLowerCase();
    }

    function rebuildTargets() {
        const targets = [];
        for (let i = 0; i < App.DeviceContext.devices.count; ++i) {
            const device = App.DeviceContext.devices.get(i);
            const info = device.info || {};
            const identifier = root.normalizedIdentifier(info.ProductType);
            if (!identifier.length)
                continue;
            targets.push({
                identifier: identifier,
                product_identifier: info.ProductType,
                name: device.text,
                detail: info.is_wireless ? qsTr("Wi-Fi — connect via USB to flash") : qsTr("USB • %1").arg(device.udid),
                udid: device.udid || "",
                ecid: "",
                device: device,
                enabled: Boolean(device.udid)
            });
        }
        for (let i = 0; i < App.DeviceContext.recoveryDevices.count; ++i) {
            const device = App.DeviceContext.recoveryDevices.get(i);
            const info = device.info || {};
            const identifier = root.normalizedIdentifier(info.model_identifier);
            if (!identifier.length)
                continue;
            targets.push({
                identifier: identifier,
                product_identifier: info.model_identifier,
                name: device.text || info.marketing_name || info.model_identifier,
                detail: qsTr("%1 • ECID %2").arg(info.mode || qsTr("Recovery")).arg(info.ecid || qsTr("Unknown")),
                udid: "",
                ecid: info.ecid_decimal || "",
                device: device,
                enabled: Boolean(info.ecid_decimal)
            });
        }
        root.flashTargets = targets;
    }

    function targetsForFirmware(identifier) {
        const wanted = root.normalizedIdentifier(identifier);
        return root.flashTargets.filter(function (target) {
            return target.identifier === wanted;
        });
    }

    function openFlash(identifier, version, build, path, selectedTarget) {
        // Recheck the target: it may have disconnected or changed transport since the menu opened.
        root.rebuildTargets();
        const target = root.targetsForFirmware(identifier).find(function (candidate) {
            return candidate.enabled && (selectedTarget.udid.length ? candidate.udid === selectedTarget.udid : candidate.ecid === selectedTarget.ecid);
        });
        if (!target || FlashManager.busy) {
            App.Helpers.showError(root.contentItem, qsTr("This device is no longer available for flashing. Connect it via USB and try again."));
            return;
        }

        if (target.udid.length && target.device.info.is_wireless) {
            App.Helpers.showInfo(root.contentItem, qsTr("Wireless devices cannot be flashed. Connect this device via USB to continue."));
            return;
        }

        const component = Qt.createComponent("../actions/FlashAction.qml");
        if (component.status !== Component.Ready) {
            App.Helpers.showError(root.contentItem, qsTr("Could not load the flash action: %1").arg(component.errorString()));
            return;
        }

        const window = component.createObject(root, {
            firmwareIdentifier: identifier,
            firmwareVersion: version,
            firmwareBuild: build,
            firmwarePath: path,
            udid: target.udid,
            ecid: target.ecid,
            device: target.device,
            auto_close: false,
            width: 980,
            height: 650,
            title: qsTr("Flash %1 - iDescriptor").arg(target.name)
        });
        if (!window) {
            App.Helpers.showError(root.contentItem, qsTr("Could not create the flash action: %1").arg(component.errorString()));
            return;
        }
        window.closing.connect(function (close) {
            Qt.callLater(function () {
                if (!window.visible)
                    window.destroy();
            });
        });
        window.show();
        window.raise();
        window.requestActivate();
    }

    // Flash windows are parented to this window, so closing it must use the
    // same cancellation-and-wait flow before its children can be destroyed.
    onClosing: function (close) {
        App.ClosingHandler.handler("flash", close, root);
    }

    function deviceFamily(identifier) {
        if (identifier.startsWith("iPhone"))
            return "iPhone";
        if (identifier.startsWith("iPad"))
            return "iPad";
        if (identifier.startsWith("iPod"))
            return "iPod";
        if (identifier.startsWith("AppleTV"))
            return "Apple TV";
        if (identifier.startsWith("AudioAccessory"))
            return "HomePod";
        if (identifier.startsWith("Watch"))
            return "Watch";
        if (identifier.startsWith("Mac") || identifier.startsWith("iMac") || identifier.startsWith("VirtualMac"))
            return "Mac";
        if (identifier.startsWith("RealityDevice"))
            return "Vision";
        return "Other";
    }

    function rebuildDeviceChoices(preferredIdentifier) {
        const connectedChoices = [];
        const catalogChoices = [];
        const seen = Object.create(null);
        for (let i = 0; i < App.DeviceContext.devices.count; ++i) {
            const deviceEntry = App.DeviceContext.devices.get(i);
            const info = deviceEntry.info || {};
            const identifier = info.ProductType || "";
            if (identifier.length && !seen[identifier]) {
                const name = deviceEntry.text || info.product_type || identifier;
                connectedChoices.push({
                    name: name,
                    label: qsTr("%1 — %2").arg(name).arg(identifier),
                    identifier: identifier,
                    badge: qsTr("Connected"),
                    family: root.deviceFamily(identifier)
                });
                seen[identifier] = true;
            }
        }
        for (let i = 0; i < App.DeviceContext.recoveryDevices.count; ++i) {
            const recoveryEntry = App.DeviceContext.recoveryDevices.get(i);
            const info = recoveryEntry.info || {};
            const identifier = info.model_identifier || "";
            if (identifier.length && !seen[identifier]) {
                const name = recoveryEntry.text || info.marketing_name || identifier;
                connectedChoices.push({
                    name: name,
                    label: qsTr("%1 — %2").arg(name).arg(identifier),
                    identifier: identifier,
                    badge: info.mode || qsTr("Recovery"),
                    family: root.deviceFamily(identifier)
                });
                seen[identifier] = true;
            }
        }
        const apiDevices = firmwareManager.devices || [];
        for (let i = 0; i < apiDevices.length; ++i) {
            const apiDevice = apiDevices[i];
            const identifier = apiDevice.identifier || "";
            if (identifier.length && !seen[identifier]) {
                const name = apiDevice.name || identifier;
                catalogChoices.push({
                    name: name,
                    label: qsTr("%1 — %2").arg(name).arg(identifier),
                    identifier: identifier,
                    badge: "",
                    family: root.deviceFamily(identifier)
                });
                seen[identifier] = true;
            }
        }

        const choices = connectedChoices.concat(catalogChoices);
        root.connectedDeviceChoices = connectedChoices;
        root.catalogDeviceChoices = catalogChoices;
        root.deviceChoices = choices;
        let selected = -1;
        const wanted = preferredIdentifier || root.currentIdentifier;
        for (let i = 0; i < choices.length; ++i) {
            if (choices[i].identifier === wanted) {
                selected = i;
                break;
            }
        }
        if (selected < 0 && connectedChoices.length === 1)
            selected = 0;
        root.currentIdentifier = selected >= 0 ? choices[selected].identifier : "";
        root.currentDeviceName = selected >= 0 ? choices[selected].name : "";
        root.currentDeviceBadge = selected >= 0 ? choices[selected].badge : "";
        root.filterDeviceCatalog();
    }

    function filterDeviceCatalog() {
        const query = deviceSearch.text.trim().toLowerCase();
        const filtered = [];
        for (let i = 0; i < root.catalogDeviceChoices.length; ++i) {
            const choice = root.catalogDeviceChoices[i];
            if (root.selectedFamily.length && choice.family !== root.selectedFamily)
                continue;
            if (query.length && !choice.name.toLowerCase().includes(query) && !choice.identifier.toLowerCase().includes(query))
                continue;
            filtered.push(choice);
        }
        root.filteredCatalogChoices = filtered;
    }

    function selectDevice(choice) {
        if (!choice || !choice.identifier.length)
            return;
        root.currentIdentifier = choice.identifier;
        root.currentDeviceName = choice.name;
        root.currentDeviceBadge = choice.badge || "";
        devicePicker.close();
        root.loadCatalog(false);
    }

    function loadDeviceCatalog(refresh) {
        stateView.retryable = true;
        stateView.viewState = StateView.State.Loading;
        firmwareManager.load_devices(refresh);
    }

    function loadCatalog(refresh) {
        if (!root.currentIdentifier.length) {
            stateView.viewState = StateView.State.Content;
            return;
        }
        stateView.retryable = true;
        stateView.viewState = StateView.State.Loading;
        if (refresh)
            firmwareManager.refresh_firmwares(root.currentIdentifier, root.downloadDirectory);
        else
            firmwareManager.fetch_firmwares(root.currentIdentifier, root.downloadDirectory);
    }

    function formatBytes(bytes) {
        const value = Number(bytes || 0);
        if (value <= 0)
            return qsTr("Unknown");
        const units = [qsTr("B"), qsTr("KB"), qsTr("MB"), qsTr("GB"), qsTr("TB")];
        let amount = value;
        let unit = 0;
        while (amount >= 1024 && unit < units.length - 1) {
            amount /= 1024;
            ++unit;
        }
        return "%1 %2".arg(amount.toFixed(unit >= 3 ? 2 : 1)).arg(units[unit]);
    }

    function statusText(status, signed) {
        if (status === "downloading")
            return qsTr("Downloading");
        if (status === "verifying")
            return qsTr("Verifying");
        if (status === "downloaded")
            return qsTr("Downloaded and verified");
        if (status === "partial")
            return qsTr("Paused — ready to resume");
        if (status === "pausing")
            return qsTr("Pausing…");
        if (status === "cancelling")
            return qsTr("Cancelling…");
        if (status === "unverified")
            return qsTr("Needs verification");
        if (status === "invalid")
            return qsTr("Verification failed");
        if (status === "failed")
            return qsTr("Download failed");
        return signed ? qsTr("Available") : qsTr("Unsigned");
    }

    function downloadStatusText(status, signed, downloadSpeed) {
        const text = statusText(status, signed);
        if (status !== "downloading" || downloadSpeed <= 0)
            return text;
        return qsTr("%1 • %2/s").arg(text).arg(formatBytes(downloadSpeed));
    }

    function statusColor(status, signed) {
        if (status === "downloaded")
            return App.Theme.systemGreen;
        if (status === "invalid" || status === "failed" || !signed)
            return App.Theme.systemRed;
        if (status === "partial" || status === "unverified")
            return App.Theme.systemOrange;
        if (status === "downloading" || status === "verifying")
            return App.Theme.systemBlue;
        return App.Theme.textMuted;
    }

    function parentDirectory(path) {
        const normalized = String(path || "").replace(/\\/g, "/");
        const slash = normalized.lastIndexOf("/");
        return slash > 0 ? normalized.substring(0, slash) : normalized;
    }

    function showInFolder(path) {
        const directory = root.parentDirectory(path);
        if (directory.length)
            Qt.openUrlExternally(App.Helpers.toFileUrl(directory));
    }

    function preferredIdentifier() {
        if (App.DeviceContext.currentDestination === "recoveryDevice") {
            for (let i = 0; i < App.DeviceContext.recoveryDevices.count; ++i) {
                const entry = App.DeviceContext.recoveryDevices.get(i);
                if (entry.id === App.DeviceContext.currentDestinationId)
                    return entry.info.model_identifier || "";
            }
        }
        const deviceEntry = App.DeviceContext.getDevice(App.DeviceContext.currentDeviceUdid);
        return deviceEntry && deviceEntry.info ? deviceEntry.info.ProductType || "" : "";
    }

    //TODO: other devices are untested, and support is unknown
    function isFlashSupported(model_identifier) {
        return model_identifier.startsWith("iPhone") || model_identifier.startsWith("iPad") || model_identifier.startsWith("iPod");
    }

    Dialog {
        id: devicePicker
        anchors.centerIn: parent
        width: Math.min(620, root.width - 64)
        height: Math.min(590, root.height - 48)
        modal: true
        focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        padding: 0

        onOpened: {
            deviceSearch.clear();
            root.selectedFamily = "";
            root.filterDeviceCatalog();
            deviceSearch.forceActiveFocus();
        }

        background: Rectangle {
            radius: 12
            color: App.Theme.windowBackground
            border.color: App.Theme.separator
            border.width: 1
        }

        contentItem: ColumnLayout {
            spacing: 0

            RowLayout {
                Layout.fillWidth: true
                Layout.margins: 20
                spacing: 12

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 3

                    Label {
                        text: qsTr("Select a device")
                        color: App.Theme.text
                        font.pixelSize: 20
                        font.weight: Font.DemiBold
                    }

                    Label {
                        text: qsTr("Connected devices and models supported by IPSW.me")
                        color: App.Theme.textMuted
                        font.pixelSize: 11
                    }
                }

                Item {
                    Layout.fillWidth: true
                }

                Button {
                    text: qsTr("Close")
                    flat: true
                    onClicked: devicePicker.close()
                }
            }

            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 1
                color: App.Theme.separator
            }

            ColumnLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                Layout.margins: 20
                spacing: 12

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 7
                    visible: root.connectedDeviceChoices.length > 0

                    Label {
                        text: qsTr("CONNECTED")
                        color: App.Theme.textMuted
                        font.pixelSize: 10
                        font.weight: Font.DemiBold
                        font.letterSpacing: 0.8
                    }

                    ListView {
                        id: connectedDeviceList
                        Layout.fillWidth: true
                        Layout.preferredHeight: Math.min(contentHeight, 106)
                        clip: true
                        spacing: 6
                        model: root.connectedDeviceChoices
                        ScrollBar.vertical: ScrollBar {}

                        delegate: Button {
                            id: connectedDeviceButton
                            required property var modelData
                            width: connectedDeviceList.width - (connectedDeviceList.ScrollBar.vertical.visible ? 12 : 0)
                            height: 50
                            leftPadding: 14
                            rightPadding: 14
                            onClicked: root.selectDevice(modelData)

                            contentItem: RowLayout {
                                spacing: 10

                                Rectangle {
                                    Layout.preferredWidth: 8
                                    Layout.preferredHeight: 8
                                    radius: 4
                                    color: App.Theme.systemGreen
                                }

                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 1
                                    Label {
                                        Layout.fillWidth: true
                                        text: connectedDeviceButton.modelData.name
                                        color: App.Theme.text
                                        font.pixelSize: 13
                                        font.weight: Font.Medium
                                        elide: Text.ElideRight
                                    }
                                    Label {
                                        text: connectedDeviceButton.modelData.identifier
                                        color: App.Theme.textMuted
                                        font.pixelSize: 10
                                    }
                                }

                                Label {
                                    text: connectedDeviceButton.modelData.badge
                                    color: App.Theme.systemGreen
                                    font.pixelSize: 10
                                    font.weight: Font.Medium
                                }
                            }

                            background: Rectangle {
                                radius: 8
                                color: connectedDeviceButton.down ? App.Theme.pressed : connectedDeviceButton.hovered ? App.Theme.hover : App.Theme.rowSurface
                                border.color: root.currentIdentifier === connectedDeviceButton.modelData.identifier ? App.Theme.accent : App.Theme.separator
                                border.width: root.currentIdentifier === connectedDeviceButton.modelData.identifier ? 2 : 1
                            }
                        }
                    }
                }

                TextField {
                    id: deviceSearch
                    Layout.fillWidth: true
                    placeholderText: qsTr("Search by device name or identifier…")
                    selectByMouse: true
                    onTextChanged: root.filterDeviceCatalog()
                }

                Flickable {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 34
                    contentWidth: familyFilterRow.implicitWidth
                    contentHeight: height
                    clip: true
                    boundsBehavior: Flickable.StopAtBounds

                    Row {
                        id: familyFilterRow
                        spacing: 6

                        Repeater {
                            model: root.deviceFamilies

                            delegate: Button {
                                id: familyButton
                                required property var modelData
                                text: modelData.label
                                checkable: true
                                checked: root.selectedFamily === modelData.key
                                height: 32
                                leftPadding: 12
                                rightPadding: 12
                                font.pixelSize: 11
                                onClicked: {
                                    root.selectedFamily = modelData.key;
                                    root.filterDeviceCatalog();
                                }
                                background: Rectangle {
                                    radius: 8
                                    color: familyButton.checked ? App.Theme.accent : familyButton.down ? App.Theme.pressed : familyButton.hovered ? App.Theme.hover : App.Theme.elevatedSurface
                                    border.color: familyButton.checked ? App.Theme.accent : App.Theme.separator
                                    border.width: 1
                                }
                            }
                        }
                    }
                }

                Label {
                    text: qsTr("ALL DEVICES")
                    color: App.Theme.textMuted
                    font.pixelSize: 10
                    font.weight: Font.DemiBold
                    font.letterSpacing: 0.8
                }

                Item {
                    Layout.fillWidth: true
                    Layout.fillHeight: true

                    ListView {
                        id: deviceCatalogList
                        anchors.fill: parent
                        clip: true
                        spacing: 6
                        model: root.filteredCatalogChoices
                        ScrollBar.vertical: ScrollBar {}

                        delegate: Button {
                            id: catalogDeviceButton
                            required property var modelData
                            width: deviceCatalogList.width - (deviceCatalogList.ScrollBar.vertical.visible ? 12 : 0)
                            height: 50
                            leftPadding: 14
                            rightPadding: 14
                            onClicked: root.selectDevice(modelData)

                            contentItem: RowLayout {
                                spacing: 12
                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 1
                                    Label {
                                        Layout.fillWidth: true
                                        text: catalogDeviceButton.modelData.name
                                        color: App.Theme.text
                                        font.pixelSize: 13
                                        font.weight: Font.Medium
                                        elide: Text.ElideRight
                                    }
                                    Label {
                                        text: catalogDeviceButton.modelData.identifier
                                        color: App.Theme.textMuted
                                        font.pixelSize: 10
                                    }
                                }
                                Label {
                                    text: catalogDeviceButton.modelData.family
                                    color: App.Theme.textMuted
                                    font.pixelSize: 10
                                }
                            }

                            background: Rectangle {
                                radius: 8
                                color: catalogDeviceButton.down ? App.Theme.pressed : catalogDeviceButton.hovered ? App.Theme.hover : App.Theme.rowSurface
                                border.color: root.currentIdentifier === catalogDeviceButton.modelData.identifier ? App.Theme.accent : App.Theme.separator
                                border.width: root.currentIdentifier === catalogDeviceButton.modelData.identifier ? 2 : 1
                            }
                        }
                    }

                    Label {
                        anchors.centerIn: parent
                        visible: root.filteredCatalogChoices.length === 0
                        text: qsTr("No matching devices")
                        color: App.Theme.textMuted
                        font.pixelSize: 13
                    }
                }
            }
        }
    }

    FolderDialog {
        id: downloadDirectoryDialog
        title: qsTr("Select iOS Firmware Download Directory")
        currentFolder: App.Helpers.toFileUrl(root.downloadDirectory)
        onAccepted: {
            root.downloadDirectory = QmlUtils.url_to_path(selectedFolder);
            settingsManager.set_firmware_download_path(root.downloadDirectory);
            root.loadCatalog(false);
            firmwareManager.scan_local_files(root.downloadDirectory);
        }
    }

    Connections {
        target: firmwareManager

        function onLocalOperationFinished(operation, path, success, error) {
            if (!success)
                App.Helpers.showError(root.contentItem, qsTr("Could not complete the firmware action: %1").arg(error));
            else if (operation === "verify") {
                App.Helpers.showInfo(root.contentItem, qsTr("The firmware file passed verification."));
                root.loadCatalog(false);
            }
        }

        function onDeviceCatalogFinished(refreshed, success, error) {
            const preferred = root.currentIdentifier || root.preferredIdentifier();
            root.rebuildDeviceChoices(preferred);
            if (!success && !root.deviceChoices.length) {
                stateView.errorText = error || qsTr("The firmware device catalog could not be loaded.");
                stateView.viewState = StateView.State.Error;
                return;
            }
            if (!success)
                App.Helpers.showError(root.contentItem, qsTr("The device catalog could not be refreshed: %1").arg(error || qsTr("Unknown error.")));
            root.loadCatalog(refreshed);
        }

        function onFirmwareListFinished(identifier, refreshed, success, error) {
            firmwareManager.scan_local_files(root.downloadDirectory);
            if (identifier !== root.currentIdentifier)
                return;
            if (success) {
                stateView.viewState = StateView.State.Content;
            } else {
                stateView.errorText = error || qsTr("The firmware catalog could not be loaded.");
                stateView.viewState = StateView.State.Error;
            }
        }

        function onDownloadFinished(identifier, buildId, success, path, error) {
            firmwareManager.scan_local_files(root.downloadDirectory);
            if (success) {
                App.Helpers.showInfo(root.contentItem, qsTr("Firmware build %1 was downloaded and verified.").arg(buildId));
            } else {
                App.Helpers.showError(root.contentItem, qsTr("Could not download firmware build %1: %2").arg(buildId).arg(error || qsTr("Unknown download error.")));
            }
        }

        function onDownloadPaused(identifier, buildId) {
            firmwareManager.scan_local_files(root.downloadDirectory);
            App.Helpers.showInfo(root.contentItem, qsTr("The download for firmware build %1 was paused and can be resumed later.").arg(buildId));
        }

        function onDownloadCancelled(identifier, buildId) {
            firmwareManager.scan_local_files(root.downloadDirectory);
            App.Helpers.showInfo(root.contentItem, qsTr("The download for firmware build %1 was cancelled and its files were deleted.").arg(buildId));
        }

        function onDownloadStopFailed(identifier, buildId, error) {
            firmwareManager.scan_local_files(root.downloadDirectory);
            App.Helpers.showError(root.contentItem, qsTr("Could not stop or delete firmware build %1: %2").arg(buildId).arg(error));
        }

        function onVerificationFinished(identifier, buildId, success, error) {
            firmwareManager.scan_local_files(root.downloadDirectory);
            if (success)
                App.Helpers.showInfo(root.contentItem, qsTr("Firmware build %1 passed verification.").arg(buildId));
            else
                App.Helpers.showError(root.contentItem, qsTr("Firmware build %1 failed verification: %2").arg(buildId).arg(error || qsTr("Unknown verification error.")));
        }
    }

    Connections {
        target: App.DeviceContext
        function onDeviceAdded(udid, mac) {
            const oldIdentifier = root.currentIdentifier;
            root.rebuildDeviceChoices(oldIdentifier);
            if (!oldIdentifier.length && root.currentIdentifier.length)
                root.loadCatalog(false);
        }
        function onDeviceRemoved(udid) {
            const oldIdentifier = root.currentIdentifier;
            root.rebuildDeviceChoices(oldIdentifier);
            if (root.currentIdentifier !== oldIdentifier)
                root.loadCatalog(false);
        }
    }

    Connections {
        target: App.DeviceContext.devices
        function onCountChanged() {
            root.rebuildTargets();
        }
        function onDataChanged() {
            root.rebuildTargets();
        }
    }

    Connections {
        target: App.DeviceContext.recoveryDevices
        function onDataChanged() {
            root.rebuildTargets();
        }
        function onCountChanged() {
            root.rebuildTargets();
            const oldIdentifier = root.currentIdentifier;
            root.rebuildDeviceChoices(oldIdentifier);
            if (root.currentIdentifier !== oldIdentifier)
                root.loadCatalog(false);
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 8

        SegmentedTabs {
            id: libraryTabs
            Layout.fillWidth: true
            Layout.leftMargin: 24
            Layout.rightMargin: 24
            Layout.topMargin: 24
            tabs: [
                {
                    title: qsTr("Firmware Catalog")
                },
                {
                    title: qsTr("Downloaded")
                }
            ]
            currentIndex: root.currentTab
            onTabRequested: index => root.currentTab = index
            onCurrentIndexChanged: {
                if (currentIndex === 1 && root.downloadDirectory.length)
                    firmwareManager.scan_local_files(root.downloadDirectory);
            }
        }

        StackLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            currentIndex: libraryTabs.currentIndex

            StateView {
                id: stateView
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                autoSwitchContent: false
                viewState: StateView.State.Loading
                onRetryRequested: root.loadDeviceCatalog(true)

                contentItem: ColumnLayout {
                    anchors.fill: parent
                    spacing: 16

                    Rectangle {
                        Layout.fillWidth: true
                        Layout.leftMargin: 24
                        Layout.rightMargin: 24
                        Layout.topMargin: 24
                        implicitHeight: headerLayout.implicitHeight + 32
                        radius: 10
                        color: App.Theme.groupedBackground
                        border.color: App.Theme.separator
                        border.width: 1

                        ColumnLayout {
                            id: headerLayout
                            anchors.fill: parent
                            anchors.margins: 16
                            spacing: 16

                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 16

                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 10

                                    Label {
                                        Layout.fillWidth: true
                                        text: qsTr("Firmware")
                                        color: App.Theme.text
                                        font.pixelSize: 24
                                        font.weight: Font.Light
                                    }

                                    Label {
                                        Layout.fillWidth: true
                                        text: qsTr("Download official IPSW firmware files directly from Apple's servers.")
                                        color: App.Theme.textMuted
                                        wrapMode: Text.WordWrap
                                        font.pixelSize: 10
                                    }
                                }

                                IconToolButton {
                                    icon.source: "qrc:/resources/icons/ic_outline-refresh.svg"
                                    toolTipText: qsTr("Refresh")
                                    Accessible.name: qsTr("Refresh")
                                    onClicked: root.loadDeviceCatalog(true)
                                }
                            }

                            RowLayout {
                                Layout.fillWidth: true
                                // Layout.alignment: Qt.AlignHCenter
                                spacing: 12

                                Button {
                                    id: devicePickerButton
                                    Layout.preferredWidth: 330
                                    Layout.preferredHeight: 42
                                    leftPadding: 14
                                    rightPadding: 14
                                    onClicked: devicePicker.open()

                                    contentItem: RowLayout {
                                        spacing: 10

                                        Rectangle {
                                            visible: root.currentDeviceBadge.length > 0
                                            Layout.preferredWidth: 8
                                            Layout.preferredHeight: 8
                                            radius: 4
                                            color: App.Theme.systemGreen
                                        }

                                        ColumnLayout {
                                            Layout.fillWidth: true
                                            spacing: 0
                                            Label {
                                                Layout.fillWidth: true
                                                text: root.currentDeviceName.length ? root.currentDeviceName : qsTr("Choose a device")
                                                color: App.Theme.text
                                                font.pixelSize: 12
                                                font.weight: Font.Medium
                                                elide: Text.ElideRight
                                            }
                                            Label {
                                                Layout.fillWidth: true
                                                visible: root.currentIdentifier.length > 0
                                                text: root.currentIdentifier
                                                color: App.Theme.textMuted
                                                font.pixelSize: 9
                                            }
                                        }

                                        Label {
                                            text: qsTr("⌄")
                                            color: App.Theme.textMuted
                                            font.pixelSize: 16
                                        }
                                    }

                                    background: Rectangle {
                                        radius: 8
                                        color: devicePickerButton.down ? App.Theme.pressed : devicePickerButton.hovered ? App.Theme.hover : App.Theme.elevatedSurface
                                        border.color: App.Theme.separator
                                        border.width: 1
                                    }
                                }

                                CheckBox {
                                    text: qsTr("Signed only")
                                    checked: root.signedOnly
                                    onToggled: root.signedOnly = checked
                                }

                                // LocationSelector {
                                //     Layout.fillWidth: true
                                //     labelText: qsTr("Download Location")
                                //     location: root.downloadDirectory
                                //     onChangeRequested: downloadDirectoryDialog.open()
                                // }
                            }

                            Label {
                                Layout.fillWidth: true
                                text: qsTr("You need to download a signed & compatible firmware before you can flash")
                                color: App.Theme.text
                                wrapMode: Text.WordWrap
                                font.pixelSize: 10
                            }
                        }
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        Layout.leftMargin: 40
                        Layout.rightMargin: 40
                        spacing: 12

                        Label {
                            text: qsTr("Version")
                            Layout.preferredWidth: 100
                            color: App.Theme.textMuted
                            font.pixelSize: 11
                        }
                        Label {
                            text: qsTr("Build")
                            Layout.preferredWidth: 95
                            color: App.Theme.textMuted
                            font.pixelSize: 11
                        }
                        Label {
                            text: qsTr("Released")
                            Layout.preferredWidth: 145
                            color: App.Theme.textMuted
                            font.pixelSize: 11
                        }
                        Label {
                            text: qsTr("Size")
                            Layout.preferredWidth: 80
                            color: App.Theme.textMuted
                            font.pixelSize: 11
                        }
                        Label {
                            text: qsTr("Status")
                            Layout.fillWidth: true
                            color: App.Theme.textMuted
                            font.pixelSize: 11
                        }
                        Item {
                            Layout.preferredWidth: 376
                        }
                    }

                    ListView {
                        id: firmwareList
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        leftMargin: 24
                        rightMargin: 24
                        bottomMargin: 24
                        spacing: 8
                        clip: true
                        model: root.currentIdentifier.length ? firmwareManager.firmware_model : null
                        ScrollBar.vertical: ScrollBar {}

                        Label {
                            anchors.centerIn: parent
                            visible: !root.currentIdentifier.length
                            text: qsTr("Choose a device to view available firmware.")
                            color: App.Theme.textMuted
                            font.pixelSize: 13
                        }

                        delegate: Pane {
                            id: rowItem
                            required property var model
                            required property int index
                            width: firmwareList.width - firmwareList.leftMargin - firmwareList.rightMargin
                            height: visible ? implicitHeight : 0
                            visible: !root.signedOnly || model.signed
                            topPadding: 10
                            bottomPadding: 10
                            leftPadding: 20
                            rightPadding: 20
                            hoverEnabled: true

                            background: Rectangle {
                                radius: 10
                                color: rowItem.hovered ? App.Theme.selectionHover : App.Theme.rowSurface
                                border.color: App.Theme.separator
                                border.width: 1
                            }

                            contentItem: RowLayout {
                                id: rowContent
                                spacing: 3

                                Label {
                                    text: model.version
                                    Layout.preferredWidth: 100
                                    color: App.Theme.text
                                    font.pixelSize: 15
                                    font.weight: Font.DemiBold
                                    elide: Text.ElideRight
                                }

                                Label {
                                    text: model.build_id
                                    Layout.preferredWidth: 95
                                    color: App.Theme.text
                                    font.pixelSize: 13
                                    elide: Text.ElideRight
                                }

                                Label {
                                    text: model.release_date.length ? model.release_date : qsTr("Unknown")
                                    Layout.preferredWidth: 145
                                    color: App.Theme.textMuted
                                    font.pixelSize: 12
                                    elide: Text.ElideRight
                                }

                                Label {
                                    text: root.formatBytes(model.file_size)
                                    Layout.preferredWidth: 80
                                    color: App.Theme.textMuted
                                    font.pixelSize: 12
                                }

                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 5

                                    Label {
                                        Layout.fillWidth: true
                                        text: root.downloadStatusText(model.status, model.signed, model.download_speed)
                                        color: root.statusColor(model.status, model.signed)
                                        font.pixelSize: 12
                                        elide: Text.ElideRight
                                    }

                                    ProgressBar {
                                        visible: model.is_downloading || model.has_partial
                                        from: 0
                                        to: 100
                                        value: model.progress
                                        Layout.fillWidth: true
                                        Layout.maximumWidth: 170
                                        Layout.preferredHeight: 7
                                        background: Rectangle {
                                            implicitHeight: 7
                                            radius: 4
                                            color: App.Theme.softBg
                                            border.color: App.Theme.separator
                                            border.width: 1
                                        }
                                        contentItem: Item {
                                            Rectangle {
                                                width: parent.width * Math.min(1, Math.max(0, model.progress / 100))
                                                height: parent.height
                                                radius: 4
                                                color: App.Theme.systemBlue
                                                Behavior on width {
                                                    NumberAnimation {
                                                        duration: 180
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }

                                Button {
                                    id: flashButton
                                    readonly property var targets: root.targetsForFirmware(rowItem.model.identifier)
                                    visible: model.is_downloaded
                                    enabled: !FlashManager.busy && model.signed && flashButton.targets.length > 0 && root.isFlashSupported(model.identifier)
                                    text: qsTr("Flash ▾")
                                    Layout.preferredWidth: 88
                                    Layout.preferredHeight: 34
                                    contentItem: Label {
                                        text: flashButton.text
                                        horizontalAlignment: Text.AlignHCenter
                                        verticalAlignment: Text.AlignVCenter
                                        color: flashButton.enabled ? App.Theme.text : App.Theme.textMuted
                                        opacity: flashButton.enabled ? 1 : 0.55
                                    }
                                    background: Rectangle {
                                        radius: 8
                                        color: !flashButton.enabled ? App.Theme.rowSurface : flashButton.down ? App.Theme.pressed : flashButton.hovered ? App.Theme.hover : App.Theme.elevatedSurface
                                        border.color: flashButton.enabled ? App.Theme.systemBlue : App.Theme.separator
                                        border.width: 1
                                    }
                                    onClicked: flashTargetMenu.popup()

                                    Menu {
                                        id: flashTargetMenu
                                        y: flashButton.height

                                        Instantiator {
                                            model: flashButton.targets
                                            delegate: MenuItem {
                                                required property var modelData
                                                text: qsTr("%1 — %2").arg(modelData.name).arg(modelData.detail)
                                                enabled: modelData.enabled
                                                onTriggered: root.openFlash(rowItem.model.identifier, rowItem.model.version, rowItem.model.build_id, rowItem.model.local_path, modelData)
                                            }
                                            onObjectAdded: function (index, object) {
                                                flashTargetMenu.insertItem(index, object);
                                            }
                                            onObjectRemoved: function (index, object) {
                                                flashTargetMenu.removeItem(object);
                                            }
                                        }
                                    }
                                }

                                Button {
                                    id: verifyButton
                                    visible: model.local_path.length > 0 && (model.is_downloaded || model.status === "unverified" || model.status === "invalid")
                                    enabled: !model.is_downloading && !model.is_verifying
                                    text: model.is_verifying ? qsTr("Verifying…") : qsTr("Verify")
                                    Layout.preferredWidth: 78
                                    Layout.preferredHeight: 34
                                    background: Rectangle {
                                        radius: 8
                                        color: verifyButton.down ? App.Theme.pressed : verifyButton.hovered ? App.Theme.hover : App.Theme.elevatedSurface
                                        border.color: App.Theme.separator
                                        border.width: 1
                                    }
                                    onClicked: firmwareManager.verify_firmware(model.identifier, model.build_id, root.downloadDirectory)
                                }

                                Button {
                                    id: showButton
                                    visible: model.is_downloaded || model.status === "unverified"
                                    text: qsTr("Open Folder")
                                    Layout.preferredWidth: 98
                                    Layout.preferredHeight: 34
                                    background: Rectangle {
                                        radius: 8
                                        color: showButton.down ? App.Theme.pressed : showButton.hovered ? App.Theme.hover : App.Theme.elevatedSurface
                                        border.color: App.Theme.separator
                                        border.width: 1
                                    }
                                    onClicked: root.showInFolder(model.local_path)
                                }

                                Button {
                                    id: cancelDownloadButton
                                    text: qsTr("Cancel")
                                    visible: model.is_downloading || (model.has_partial && !model.is_downloaded)
                                    enabled: !model.is_verifying
                                    Layout.preferredWidth: 78
                                    Layout.preferredHeight: 34
                                    background: Rectangle {
                                        radius: 8
                                        color: cancelDownloadButton.down ? App.Theme.pressed : cancelDownloadButton.hovered ? App.Theme.hover : App.Theme.elevatedSurface
                                        border.color: App.Theme.systemRed
                                        border.width: 1
                                    }
                                    onClicked: {
                                        const identifier = model.identifier;
                                        const buildId = model.build_id;
                                        App.Helpers.messageBox(root.contentItem, qsTr("Cancel Download"), qsTr("Are you sure you want to cancel the download for firmware build %1? Its downloaded files will be deleted and you will need to start over.").arg(buildId), MessageDialog.Yes | MessageDialog.No, function (button) {
                                            if (button === MessageDialog.Yes && !firmwareManager.cancel_download(identifier, buildId))
                                                App.Helpers.showError(root.contentItem, qsTr("Could not cancel this firmware download. It may have already finished."));
                                        });
                                    }
                                }

                                Button {
                                    id: downloadButton
                                    text: model.is_downloading ? qsTr("Pause") : model.is_downloaded ? qsTr("Re-download") : model.has_partial ? qsTr("Resume") : qsTr("Download")
                                    enabled: !model.is_verifying
                                    Layout.preferredWidth: 100
                                    Layout.preferredHeight: 34
                                    contentItem: Label {
                                        text: downloadButton.text
                                        horizontalAlignment: Text.AlignHCenter
                                        verticalAlignment: Text.AlignVCenter
                                        color: App.Theme.text
                                        font.pixelSize: 13
                                        font.weight: Font.Medium
                                    }
                                    background: Rectangle {
                                        radius: 8
                                        color: downloadButton.down ? App.Theme.pressed : downloadButton.hovered ? App.Theme.hover : App.Theme.elevatedSurface
                                        border.color: App.Theme.separator
                                        border.width: 1
                                    }
                                    onClicked: {
                                        if (model.is_downloading) {
                                            if (!firmwareManager.pause_download(model.identifier, model.build_id))
                                                App.Helpers.showError(root.contentItem, qsTr("Could not pause this firmware download."));
                                            return;
                                        }
                                        if (!firmwareManager.download_firmware(model.identifier, model.build_id, root.downloadDirectory, model.is_downloaded))
                                            App.Helpers.showError(root.contentItem, qsTr("Could not start this firmware download."));
                                    }
                                }
                            }
                        }
                    }
                }
            }

            DownloadedFirmwares {
                directory: root.downloadDirectory
                availableTargets: root.flashTargets.filter(function (target) {
                    return root.isFlashSupported(target.product_identifier);
                })
                onChangeDirectoryRequested: downloadDirectoryDialog.open()
                onBrowseRequested: root.currentTab = 0
                onRevealRequested: function (path) {
                    root.showInFolder(path);
                }
                onFlashRequested: function (identifier, version, build, path, target) {
                    root.openFlash(identifier, version, build, path, target);
                }
            }
        }
    }

    Component.onCompleted: {
        root.rebuildTargets();
        root.downloadDirectory = settingsManager.firmware_download_path();
        firmwareManager.scan_local_files(root.downloadDirectory);
        root.loadDeviceCatalog(false);
    }

    component DownloadedFirmwares: Item {
        id: downloadedRoot
        required property string directory
        required property var availableTargets

        signal changeDirectoryRequested
        signal browseRequested
        signal revealRequested(string path)
        signal flashRequested(string identifier, string version, string build, string path, var target)

        function refresh() {
            firmwareManager.scan_local_files(downloadedRoot.directory);
        }

        function matchingTargets(identifiers) {
            return downloadedRoot.availableTargets.filter(function (target) {
                for (let i = 0; i < identifiers.length; ++i) {
                    if (String(identifiers[i]).toLowerCase() === target.identifier)
                        return true;
                }
                return false;
            });
        }

        function statusText(item) {
            if (item.status === "verifying")
                return qsTr("Checking firmware…");
            if (item.status === "stopping")
                return qsTr("Stopping download…");
            if (item.is_downloading)
                return qsTr("Downloading");
            if (item.status === "invalid")
                return qsTr("Invalid IPSW");
            if (item.status === "failed")
                return qsTr("Action failed");
            if (item.has_partial)
                return qsTr("Partial download — ready to resume");
            if (item.is_downloaded)
                return item.signed ? qsTr("Verified") : qsTr("Verified — no signed catalog match; verify to refresh");
            return qsTr("Not verified");
        }

        function cancelPartial(path) {
            App.Helpers.messageBox(downloadedRoot, qsTr("Cancel Download"), qsTr("Cancel this download and delete its partial file? You will need to start over."), MessageDialog.Yes | MessageDialog.No, function (button) {
                if (button !== MessageDialog.Yes)
                    return;
                if (!firmwareManager.cancel_local_download(path))
                    App.Helpers.showError(downloadedRoot, qsTr("Could not cancel this download. Refresh the list and try again."));
                else
                    downloadedRoot.refresh();
            });
        }

        // Refresh partial-file sizes while a download or local operation is active.
        Timer {
            interval: 2000
            repeat: true
            running: downloadedRoot.visible && firmwareManager.local_active
            onTriggered: {
                if (!firmwareManager.local_loading)
                    downloadedRoot.refresh();
            }
        }

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 24
            spacing: 12

            RowLayout {
                Layout.fillWidth: true
                Label {
                    Layout.fillWidth: true
                    text: qsTr("Firmware in this folder")
                    font.pixelSize: 18
                    font.bold: true
                    color: App.Theme.text
                }
                IconToolButton {
                    icon.source: "qrc:/resources/icons/ic_outline-refresh.svg"
                    toolTipText: qsTr("Refresh")
                    Accessible.name: qsTr("Refresh")
                    enabled: !firmwareManager.local_loading
                    onClicked: downloadedRoot.refresh()
                }
            }

            LocationSelector {
                Layout.fillWidth: true
                labelText: qsTr("Download Location")
                location: downloadedRoot.directory
                onChangeRequested: downloadedRoot.changeDirectoryRequested()
            }

            StateView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                autoSwitchContent: false
                viewState: firmwareManager.local_loading ? StateView.State.Loading : firmwareManager.local_error.length ? StateView.State.Error : StateView.State.Content
                errorText: firmwareManager.local_error
                onRetryRequested: downloadedRoot.refresh()

                contentItem: ListView {
                    id: downloadedLocalList
                    anchors.fill: parent
                    model: firmwareManager.local_model
                    clip: true
                    spacing: 10
                    section.property: "has_partial"
                    section.delegate: Label {
                        required property string section
                        text: section === "true" ? qsTr("Partial Downloads") : qsTr("Firmware Files")
                        color: App.Theme.textMuted
                        font.bold: true
                        topPadding: 8
                        bottomPadding: 8
                    }
                    ScrollBar.vertical: ScrollBar {}

                    delegate: Rectangle {
                        id: downloadedRow
                        required property var model
                        width: downloadedLocalList.width
                        implicitHeight: downloadedRowContent.implicitHeight + 24
                        radius: 9
                        color: App.Theme.rowSurface
                        border.color: App.Theme.separator

                        ColumnLayout {
                            id: downloadedRowContent
                            anchors.left: parent.left
                            anchors.right: parent.right
                            anchors.top: parent.top
                            anchors.margins: 12
                            spacing: 6

                            Label {
                                Layout.fillWidth: true
                                text: downloadedRow.model.version.length ? qsTr("iOS %1 — %2").arg(downloadedRow.model.version).arg(downloadedRow.model.build_id) : downloadedRow.model.file_name
                                font.bold: true
                                font.pixelSize: 14
                                color: App.Theme.text
                                elide: Text.ElideRight
                            }
                            Label {
                                Layout.fillWidth: true
                                text: downloadedRow.model.supported_identifiers.length ? downloadedRow.model.supported_identifiers.join(", ") : qsTr("Device model could not be identified")
                                color: App.Theme.textMuted
                                font.pixelSize: 11
                                wrapMode: Text.WordWrap
                            }
                            Label {
                                Layout.fillWidth: true
                                text: downloadedRow.model.local_path
                                color: App.Theme.textMuted
                                font.pixelSize: 11
                                elide: Text.ElideMiddle
                            }
                            Label {
                                Layout.fillWidth: true
                                visible: downloadedRow.model.error.length > 0
                                text: downloadedRow.model.error
                                color: App.Theme.systemRed
                                font.pixelSize: 11
                                wrapMode: Text.WordWrap
                            }
                            ProgressBar {
                                Layout.fillWidth: true
                                visible: downloadedRow.model.has_partial && downloadedRow.model.file_size > 0
                                from: 0
                                to: 100
                                value: downloadedRow.model.progress
                            }
                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 8
                                Label {
                                    Layout.fillWidth: true
                                    text: qsTr("%1 • %2").arg(App.Helpers.formatSize(downloadedRow.model.downloaded_bytes)).arg(downloadedRoot.statusText(downloadedRow.model))
                                    color: App.Theme.textMuted
                                    font.pixelSize: 11
                                    wrapMode: Text.WordWrap
                                }
                                Button {
                                    text: qsTr("Open Folder")
                                    onClicked: downloadedRoot.revealRequested(downloadedRow.model.local_path)
                                }
                                Button {
                                    visible: !downloadedRow.model.has_partial
                                    text: downloadedRow.model.is_verifying ? qsTr("Verifying…") : qsTr("Verify")
                                    enabled: !downloadedRow.model.is_verifying && !downloadedRow.model.is_downloading && downloadedRow.model.supported_identifiers.length > 0
                                    onClicked: {
                                        if (!firmwareManager.verify_local_file(downloadedRow.model.local_path))
                                            App.Helpers.showError(downloadedRoot, qsTr("Could not verify this firmware. Refresh the list and try again."));
                                    }
                                }
                                Button {
                                    visible: downloadedRow.model.has_partial
                                    text: downloadedRow.model.is_downloading ? qsTr("Pause") : qsTr("Resume")
                                    enabled: !downloadedRow.model.is_verifying && downloadedRow.model.identifier.length > 0
                                    onClicked: {
                                        const success = downloadedRow.model.is_downloading ? firmwareManager.pause_download(downloadedRow.model.identifier, downloadedRow.model.build_id) : firmwareManager.resume_local_download(downloadedRow.model.local_path);
                                        if (!success)
                                            App.Helpers.showError(downloadedRoot, qsTr("Could not change this download. Refresh the list and try again."));
                                        else
                                            downloadedRoot.refresh();
                                    }
                                }
                                Button {
                                    visible: downloadedRow.model.has_partial
                                    text: qsTr("Cancel")
                                    enabled: !downloadedRow.model.is_verifying
                                    onClicked: downloadedRoot.cancelPartial(downloadedRow.model.local_path)
                                }
                                Button {
                                    id: downloadedFlashButton
                                    visible: !downloadedRow.model.has_partial
                                    text: qsTr("Flash ▾")
                                    readonly property var targets: downloadedRoot.matchingTargets(downloadedRow.model.flash_identifiers)
                                    enabled: downloadedRow.model.is_downloaded && !downloadedRow.model.is_verifying && !FlashManager.busy && downloadedFlashButton.targets.length > 0
                                    contentItem: Label {
                                        text: downloadedFlashButton.text
                                        horizontalAlignment: Text.AlignHCenter
                                        verticalAlignment: Text.AlignVCenter
                                        color: downloadedFlashButton.enabled ? App.Theme.text : App.Theme.textMuted
                                        opacity: downloadedFlashButton.enabled ? 1 : 0.55
                                    }
                                    background: Rectangle {
                                        radius: 8
                                        color: !downloadedFlashButton.enabled ? App.Theme.rowSurface : downloadedFlashButton.down ? App.Theme.pressed : downloadedFlashButton.hovered ? App.Theme.hover : App.Theme.elevatedSurface
                                        border.color: downloadedFlashButton.enabled ? App.Theme.systemBlue : App.Theme.separator
                                        border.width: 1
                                    }
                                    onClicked: downloadedTargetMenu.popup()
                                    Menu {
                                        id: downloadedTargetMenu
                                        y: downloadedFlashButton.height
                                        Instantiator {
                                            model: downloadedFlashButton.targets
                                            delegate: MenuItem {
                                                required property var modelData
                                                text: qsTr("%1 — %2").arg(modelData.name).arg(modelData.detail)
                                                enabled: modelData.enabled
                                                onTriggered: downloadedRoot.flashRequested(modelData.product_identifier, downloadedRow.model.version, downloadedRow.model.build_id, downloadedRow.model.local_path, modelData)
                                            }
                                            onObjectAdded: function (index, object) {
                                                downloadedTargetMenu.insertItem(index, object);
                                            }
                                            onObjectRemoved: function (index, object) {
                                                downloadedTargetMenu.removeItem(object);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    ColumnLayout {
                        anchors.centerIn: parent
                        width: Math.min(parent.width - 32, 420)
                        visible: downloadedLocalList.count === 0 && !firmwareManager.local_loading && !firmwareManager.local_error.length
                        Label {
                            Layout.fillWidth: true
                            text: qsTr("No firmware downloaded to this folder")
                            color: App.Theme.textMuted
                            horizontalAlignment: Text.AlignHCenter
                            wrapMode: Text.WordWrap
                        }
                        Button {
                            Layout.alignment: Qt.AlignHCenter
                            text: qsTr("Browse Firmware")
                            onClicked: downloadedRoot.browseRequested()
                        }
                    }
                }
            }
        }
    }
}
