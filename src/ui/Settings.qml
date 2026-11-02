// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

pragma Singleton

import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import QtQuick.Window
import "." as App
import "./base"

DefaultWindow {
    id: root
    width: 500
    height: 500
    minimumWidth: 500
    minimumHeight: 500
    title: qsTr("Settings - iDescriptor")
    visible: false
    modality: Qt.ApplicationModal
    autoDestroy: false
    autoVisible: false

    /* Windows only*/
    showMaximize: false
    showMinimize: false
    /*----------*/
    property bool dirty: false
    property bool restartRequired: false
    readonly property var backend: typeof settingsManager !== "undefined" ? settingsManager : null

    property string developerImageRevision: ""
    property string pendingImageRevision: ""
    property var imageCommits: []
    property var imageRepositoryRequest: null
    property int imageRepositoryRequestId: 0
    property bool imageRepositoryBusy: false

    function cancelImageRepositoryRequest() {
        ++imageRepositoryRequestId
        if (imageRepositoryRequest)
            imageRepositoryRequest.abort()
        imageRepositoryRequest = null
        imageRepositoryBusy = false
    }

    function repositoryRequest(path, callback) {
        cancelImageRepositoryRequest()
        const requestId = imageRepositoryRequestId
        const xhr = new XMLHttpRequest()
        imageRepositoryRequest = xhr
        imageRepositoryBusy = true
        xhr.open("GET", "https://api.github.com/repos/doronz88/DeveloperDiskImage/" + path)
        xhr.setRequestHeader("Accept", "application/vnd.github+json")
        xhr.onreadystatechange = function() {
            if (requestId !== root.imageRepositoryRequestId
                    || xhr.readyState !== XMLHttpRequest.DONE)
                return
            root.imageRepositoryRequest = null
            root.imageRepositoryBusy = false
            if (xhr.status !== 200) {
                let message
                if (xhr.status === 403 || xhr.status === 429)
                    message = qsTr("GitHub rejected the request. Its API rate limit may have been reached.")
                else if (xhr.status === 404)
                    message = qsTr("The commit was not found in the developer image repository.")
                else if (xhr.status === 0)
                    message = qsTr("Could not connect to GitHub. Check your network connection.")
                else
                    message = qsTr("Could not fetch repository information (HTTP %1).").arg(xhr.status)
                callback(null, message)
                return
            }
            let result
            try {
                result = JSON.parse(xhr.responseText)
            } catch (error) {
                callback(null, qsTr("GitHub returned an invalid response."))
                return
            }
            callback(result, "")
        }
        xhr.send()
    }

    function fetchImageCommits(useLatest) {
        repositoryRequest("commits?per_page=10", function(commits, error) {
            if (error) {
                App.Helpers.showError(root, error)
                return
            }
            if (!Array.isArray(commits) || commits.length === 0) {
                App.Helpers.showInfo(root, qsTr("No repository commits were found."))
                return
            }
            if (!commits.every(function(entry) {
                return /^[0-9a-f]{40}$/.test(entry.sha) && entry.commit
                    && typeof entry.commit.message === "string"
                    && entry.commit.committer
                    && typeof entry.commit.committer.date === "string"
            })) {
                App.Helpers.showError(root, qsTr("GitHub returned an invalid commit list."))
                return
            }
            root.imageCommits = commits
            if (useLatest)
                root.selectImageRevision(commits[0].sha)
        })
    }

    function selectImageRevision(revision) {
        pendingImageRevision = revision
        if (pendingImageRevision !== developerImageRevision)
            markDirty(false)
    }

    function selectCustomImageRevision(value) {
        const revision = value.trim().toLowerCase()
        if (!/^[0-9a-f]{40}$/.test(revision)) {
            App.Helpers.showError(root, qsTr("Enter a full 40-character commit hash."))
            return
        }
        repositoryRequest("commits/" + revision, function(commit, error) {
            if (error) {
                App.Helpers.showError(root, error)
                return
            }
            if (!commit || commit.sha !== revision) {
                App.Helpers.showError(root, qsTr("GitHub returned an invalid commit."))
                return
            }
            root.selectImageRevision(commit.sha)
        })
    }

    property string downloadPath: ""
    property string ipaDownloadPath: ""
    property string backupRootPath: ""
    property int wireless_file_server_port: 8080
    property bool unmount_ifuse_on_exit: false
    property bool auto_check_updates: true
    property bool z_linux_window: false
    property bool auto_enable_wifi_connections: true
    property string theme: "system"
    property string language: "en"
    property bool auto_raise_window: true
    property bool switch_to_new_device: true
    property bool auto_connect_wireless_devices: true
    property bool upgrade_to_wireless_on_disconnect: true
    property int connection_timeout: 30
    property int gallery_backend: 1
    property string network_discovery_backend: "auto"
    property string window_effect: "normal"
    property string default_jailbroken_root_password: "alpine"
    property int airplay_fps: 60
    property int airplay_width: 1920
    property int airplay_height: 1080
    property int airplay_refresh_rate: 60
    property bool airplay_h265: false
    property bool airplay_overscanned: false
    property bool airplay_audio_start_on_launch: false
    property bool airplay_no_hold: true
    property bool airplay_use_legacy_ports: true
    property bool show_v4l2: false

    Connections {
        target: App.Theme
        function onWindowEffectChanged() {
            root.applyEffect(App.Theme.windowEffect)
        }
    }

    function open() {
        loadSettings()
        fetchImageCommits(false)
        show()
        raise()
        requestActivate()
    }

    function markDirty(restart) {
        dirty = true
        if (restart)
            restartRequired = true
    }

    function backendValue(name, fallback) {
        if (!backend || typeof backend[name] !== "function")
            return fallback
        return backend[name]()
    }

    function callBackend(name) {
        if (!backend || typeof backend[name] !== "function")
            return

        const args = Array.prototype.slice.call(arguments, 1)
        backend[name].apply(backend, args)
    }

    function normalizeLanguage(value) {
        const normalized = String(value || "en").trim().toLowerCase()
        if (normalized === "german" || normalized.indexOf("de") === 0)
            return "de"
        if (normalized === "traditional chinese" || normalized === "zh-tw"
                || normalized === "zh_tw" || normalized === "zh-hant"
                || normalized === "zh_hant" || normalized === "zh-hk"
                || normalized === "zh_hk" || normalized === "zh-mo"
                || normalized === "zh_mo")
            return "zh_TW"
        if (normalized === "chinese" || normalized === "simplified chinese"
                || normalized === "zh" || normalized === "zh-cn" || normalized === "zh_hans"
                || normalized === "zh-hans" || normalized.indexOf("zh_cn") === 0)
            return "zh_CN"

        const languageCode = normalized.replace("-", "_").split("_")[0]
        const supportedLanguages = [
            "af", "ar", "ca", "cs", "da", "de", "el", "en", "es",
            "fi", "fr", "he", "hu", "it", "ja", "ko", "nl", "no",
            "pl", "pt", "ro", "ru", "sr", "sv", "tr", "uk", "vi"
        ]
        if (supportedLanguages.indexOf(languageCode) !== -1)
            return languageCode
        return "en"
    }

    function normalizeGalleryBackend(value) {
        const parsedBackend = Number(value)
        if (parsedBackend === 0 || parsedBackend === 2)
            return parsedBackend
        return 1
    }

    function normalizeTheme(value) {
        return App.Theme.normalizeColorScheme(value)
    }

    function applyLanguage() {
        if (typeof QmlUtils !== "undefined" && QmlUtils && typeof QmlUtils.set_language === "function")
            QmlUtils.set_language(language)
    }

    function loadSettings() {
        cancelImageRepositoryRequest()
        developerImageRevision = settingsManager.developer_image_revision()
        pendingImageRevision = developerImageRevision
        downloadPath = backendValue("dev_disk_img_path", "")
        ipaDownloadPath = backendValue("ipa_download_path", "")
        backupRootPath = backendValue("backup_root_path", "")
        wireless_file_server_port = backendValue("wireless_file_server_port", 8080)
        unmount_ifuse_on_exit = backendValue("unmount_ifuse_on_exit", false)
        auto_check_updates = backendValue("auto_check_updates", true)
        z_linux_window = backendValue("z_linux_window", false)
        auto_enable_wifi_connections = backendValue("auto_enable_wifi_connections", true)
        theme = normalizeTheme(backendValue("theme", "system"))
        language = normalizeLanguage(backendValue("language", "en"))
        auto_raise_window = backendValue("auto_raise_window", true)
        switch_to_new_device = backendValue("switch_to_new_device", true)
        auto_connect_wireless_devices = backendValue("auto_connect_wireless_devices", true)
        upgrade_to_wireless_on_disconnect = backendValue("upgrade_to_wireless_on_disconnect", true)
        connection_timeout = backendValue("connection_timeout", 30)
        gallery_backend = normalizeGalleryBackend(backendValue("gallery_backend", 1))
        network_discovery_backend = backendValue("network_discovery_backend", "auto")
        window_effect = backendValue("window_effect", "normal")
        default_jailbroken_root_password = backendValue("default_jailbroken_root_password", "alpine")
        airplay_fps = backendValue("airplay_fps", 60)
        airplay_width = backendValue("airplay_width", 1920)
        airplay_height = backendValue("airplay_height", 1080)
        airplay_refresh_rate = backendValue("airplay_refresh_rate", 60)
        airplay_h265 = backendValue("airplay_h265", false)
        airplay_overscanned = backendValue("airplay_overscanned", false)
        airplay_audio_start_on_launch = backendValue("airplay_audio_start_on_launch", false)
        airplay_no_hold = backendValue("airplay_no_hold", true)
        airplay_use_legacy_ports = backendValue("airplay_use_legacy_ports", true)
        show_v4l2 = backendValue("show_v4l2", false)
        App.Theme.colorScheme = theme
        dirty = false
        restartRequired = false
        applyLanguage()
    }

    function applySettings() {
        settingsManager.set_developer_image_revision(pendingImageRevision)
        developerImageRevision = settingsManager.developer_image_revision()
        callBackend("set_dev_disk_img_path", downloadPath)
        callBackend("set_ipa_download_path", ipaDownloadPath)
        callBackend("set_backup_root_path", backupRootPath)
        callBackend("set_wireless_file_server_port", wireless_file_server_port)
        callBackend("set_unmount_ifuse_on_exit", unmount_ifuse_on_exit)
        callBackend("set_auto_check_updates", auto_check_updates)
        callBackend("set_z_linux_window", z_linux_window)
        callBackend("set_auto_enable_wifi_connections", auto_enable_wifi_connections)
        callBackend("set_theme", theme)
        App.Theme.colorScheme = theme
        callBackend("set_language", language)
        applyLanguage()
        callBackend("set_auto_raise_window", auto_raise_window)
        callBackend("set_switch_to_new_device", switch_to_new_device)
        callBackend("set_auto_connect_wireless_devices", auto_connect_wireless_devices)
        callBackend("set_upgrade_to_wireless_on_disconnect", upgrade_to_wireless_on_disconnect)
        callBackend("set_connection_timeout", connection_timeout)
        callBackend("set_gallery_backend", gallery_backend)
        NetworkDeviceProvider.set_backend(network_discovery_backend)
        callBackend("set_window_effect", window_effect)
        callBackend("set_default_jailbroken_root_password", default_jailbroken_root_password)
        callBackend("set_airplay_fps", airplay_fps)
        callBackend("set_airplay_width", airplay_width)
        callBackend("set_airplay_height", airplay_height)
        callBackend("set_airplay_refresh_rate", airplay_refresh_rate)
        callBackend("set_airplay_h265", airplay_h265)
        callBackend("set_airplay_overscanned", airplay_overscanned)
        callBackend("set_airplay_audio_start_on_launch", airplay_audio_start_on_launch)
        callBackend("set_airplay_no_hold", airplay_no_hold)
        callBackend("set_airplay_use_legacy_ports", airplay_use_legacy_ports)
        callBackend("set_show_v4l2", show_v4l2)

        dirty = false
        appliedDialog.text = restartRequired
                ? qsTr("Settings applied. Please restart the application for changes to take effect.")
                : qsTr("Settings applied.")
        restartRequired = false
        appliedDialog.open()
    }

    function reset_to_defaults() {
        callBackend("reset_to_defaults")
        loadSettings()
        dirty = true
    }

    Component.onCompleted: loadSettings()

    onVisibleChanged: {
        if (!visible) cancelImageRepositoryRequest()
        //restore the window effect when the settings window is closed, in case it was changed
        if (Qt.platform.os === "windows") {
            const effect = settingsManager.window_effect()
            root.window_effect = effect
            App.Theme.windowEffect = effect
        }
    }

    FolderDialog {
        id: downloadPathDialog
        title: qsTr("Select Download Directory")
        onAccepted: {
            root.downloadPath = QmlUtils.url_to_path(selectedFolder)
            root.markDirty(false)
        }
    }

    FolderDialog {
        id: backupRootPathDialog
        title: qsTr("Select Backup Directory")
        onAccepted: {
            root.backupRootPath = QmlUtils.url_to_path(selectedFolder)
            root.markDirty(false)
        }
    }

    FolderDialog {
        id: ipaDownloadPathDialog
        title: qsTr("Select IPA Download Directory")
        onAccepted: {
            root.ipaDownloadPath = QmlUtils.url_to_path(selectedFolder)
            root.markDirty(false)
        }
    }

    MessageDialog {
        id: appliedDialog
        title: qsTr("Settings")
    }

    MessageDialog {
        id: resetDialog
        title: qsTr("Reset Settings")
        text: qsTr("Are you sure you want to reset all settings to their default values?")
        buttons: MessageDialog.Yes | MessageDialog.No
        onButtonClicked: function(button, role) {
            if (button === MessageDialog.Yes)
                root.reset_to_defaults()
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        ScrollView {
            id: settingsScroll
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            contentWidth: availableWidth
            ScrollBar.horizontal.policy: ScrollBar.AlwaysOff

            ColumnLayout {
                width: Math.min(560, Math.max(0, settingsScroll.availableWidth - 32))
                anchors.horizontalCenter: parent.horizontalCenter
                spacing: 22

                Item { Layout.preferredHeight: 16 }

                SettingsSection {
                    title: qsTr("General")

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 10

                        Label {
                            text: qsTr("Developer Disk Image Path")
                            Layout.preferredWidth: 175
                        }

                        TextField {
                            Layout.fillWidth: true
                            text: root.downloadPath
                            readOnly: true
                            selectByMouse: true
                        }

                        Button {
                            text: qsTr("Browse")
                            onClicked: downloadPathDialog.open()
                        }
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 10

                        Label {
                            text: qsTr("Network discovery")
                            Layout.preferredWidth: 175
                        }

                        ComboBox {
                            Layout.fillWidth: true
                            textRole: "label"
                            valueRole: "value"
                            model: {
                                const labels = {
                                    "auto": qsTr("Automatic"),
                                    "pure_rust": qsTr("Built-in"),
                                    "avahi": qsTr("Avahi"),
                                    "bonjour": qsTr("Bonjour")
                                }
                                const choices = []
                                for (const value of NetworkDeviceProvider.available_backends)
                                    choices.push({ value: value, label: labels[value] })
                                return choices
                            }
                            currentIndex: Math.max(0, indexOfValue(root.network_discovery_backend))
                            ToolTip.visible: hovered
                            ToolTip.text: qsTr("Choose how devices are discovered on the local network.")
                            onActivated: {
                                root.network_discovery_backend = currentValue
                                root.markDirty(false)
                            }
                        }
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 10

                        Label {
                            text: qsTr("IPA Download Path")
                            Layout.preferredWidth: 175
                        }

                        TextField {
                            Layout.fillWidth: true
                            text: root.ipaDownloadPath
                            readOnly: true
                            selectByMouse: true
                        }

                        Button {
                            text: qsTr("Browse")
                            onClicked: ipaDownloadPathDialog.open()
                        }
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 10

                        Label {
                            text: qsTr("Backup Path")
                            Layout.preferredWidth: 175
                        }

                        TextField {
                            Layout.fillWidth: true
                            text: root.backupRootPath
                            readOnly: true
                            selectByMouse: true
                        }

                        Button {
                            text: qsTr("Browse")
                            onClicked: backupRootPathDialog.open()
                        }
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 10

                        Label {
                            text: qsTr("Wireless File Server Port")
                            Layout.preferredWidth: 175
                        }

                        TextField {
                            Layout.preferredWidth: 110
                            text: String(root.wireless_file_server_port)
                            selectByMouse: true
                            inputMethodHints: Qt.ImhDigitsOnly
                            validator: IntValidator {
                                bottom: 1024
                                top: 65535
                            }
                            ToolTip.visible: hovered
                            ToolTip.text: qsTr("The starting port for the wireless file server. If this port is unavailable, it will try the next 10 ports.")
                            onEditingFinished: {
                                const port = Number(text)
                                if (acceptableInput && Number.isInteger(port)) {
                                    if (root.wireless_file_server_port !== port) {
                                        root.wireless_file_server_port = port
                                        root.markDirty(false)
                                    }
                                } else {
                                    text = String(root.wireless_file_server_port)
                                }
                            }
                        }

                        Item { Layout.fillWidth: true }
                    }

                    Switch {
                        Layout.fillWidth: true
                        visible: Qt.platform.os !== "osx" && Qt.platform.os !== "darwin"
                        text: qsTr("Unmount iFuse drives on exit")
                        checked: root.unmount_ifuse_on_exit
                        onToggled: {
                            root.unmount_ifuse_on_exit = checked
                            root.markDirty(false)
                        }
                    }

                    Switch {
                        Layout.fillWidth: true
                        text: qsTr("Automatically check for updates")
                        checked: root.auto_check_updates
                        onToggled: {
                            root.auto_check_updates = checked
                            root.markDirty(false)
                        }
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 10

                        Label {
                            text: qsTr("Gallery backend")
                            Layout.preferredWidth: 175
                        }

                        ComboBox {
                            Layout.fillWidth: true
                            model: [
                                qsTr("Filesystem (AFC)"),
                                qsTr("SQLite"),
                                qsTr("SQLite through VFS")
                            ]
                            currentIndex: root.normalizeGalleryBackend(root.gallery_backend)
                            ToolTip.visible: hovered
                            ToolTip.text: qsTr("Choose how gallery albums are loaded from the device.")
                            onActivated: function(index) {
                                root.gallery_backend = root.normalizeGalleryBackend(index)
                                root.markDirty(false)
                            }
                        }
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 10

                        Label {
                            text: qsTr("Language")
                            Layout.preferredWidth: 175
                        }

                        ComboBox {
                            textRole: "label"
                            valueRole: "value"
                            model: [
                                { value: "en", label: qsTr("English") },
                                { value: "af", label: qsTr("Afrikaans") },
                                { value: "ar", label: qsTr("Arabic") },
                                { value: "ca", label: qsTr("Catalan") },
                                { value: "zh_CN", label: qsTr("Chinese (Simplified)") },
                                { value: "zh_TW", label: qsTr("Chinese (Traditional)") },
                                { value: "cs", label: qsTr("Czech") },
                                { value: "da", label: qsTr("Danish") },
                                { value: "nl", label: qsTr("Dutch") },
                                { value: "fi", label: qsTr("Finnish") },
                                { value: "fr", label: qsTr("French") },
                                { value: "de", label: qsTr("German") },
                                { value: "el", label: qsTr("Greek") },
                                { value: "he", label: qsTr("Hebrew") },
                                { value: "hu", label: qsTr("Hungarian") },
                                { value: "it", label: qsTr("Italian") },
                                { value: "ja", label: qsTr("Japanese") },
                                { value: "ko", label: qsTr("Korean") },
                                { value: "no", label: qsTr("Norwegian") },
                                { value: "pl", label: qsTr("Polish") },
                                { value: "pt", label: qsTr("Portuguese (Brazil)") },
                                { value: "ro", label: qsTr("Romanian") },
                                { value: "ru", label: qsTr("Russian") },
                                { value: "sr", label: qsTr("Serbian") },
                                { value: "es", label: qsTr("Spanish") },
                                { value: "sv", label: qsTr("Swedish") },
                                { value: "tr", label: qsTr("Turkish") },
                                { value: "uk", label: qsTr("Ukrainian") },
                                { value: "vi", label: qsTr("Vietnamese") }
                            ]
                            currentIndex: Math.max(0, indexOfValue(root.language))
                            onActivated: {
                                root.language = currentValue
                                root.markDirty(true)
                            }
                        }

                        Item { Layout.fillWidth: true }
                    }
                }

                SettingsSection {
                    title: qsTr("Appearance")

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 10

                        Label {
                            text: qsTr("Theme")
                            Layout.preferredWidth: 175
                        }

                        ComboBox {
                            textRole: "label"
                            valueRole: "value"
                            model: [
                                { value: "system", label: qsTr("System Default") },
                                { value: "light", label: qsTr("Light") },
                                { value: "dark", label: qsTr("Dark") }
                            ]
                            currentIndex: Math.max(0, indexOfValue(root.theme))
                            onActivated: {
                                root.theme = currentValue
                                root.markDirty(false)
                            }
                        }

                        Item { Layout.fillWidth: true }
                    }

                    Switch {
                        Layout.fillWidth: true
                        visible: Qt.platform.os === "linux"
                        text: qsTr("Use custom window frame")
                        checked: root.z_linux_window
                        ToolTip.visible: hovered
                        ToolTip.text: qsTr("Use a custom Linux window frame instead of default.")
                        onToggled: {
                            root.z_linux_window = checked
                            root.markDirty(true)
                        }
                    }

                    RowLayout {
                        visible: Qt.platform.os === "windows"
                        Layout.fillWidth: true
                        spacing: 10

                        Label {
                            text: qsTr("Window Effect")
                            Layout.preferredWidth: 175
                        }

                        ComboBox {
                            textRole: "label"
                            valueRole: "value"
                            model: [
                                { value: "normal", label: qsTr("Normal") },
                                { value: "acrylic", label: qsTr("Acrylic") }
                            ]
                            currentIndex: Math.max(0, indexOfValue(root.window_effect))
                            onActivated: {
                                root.window_effect = currentValue
                                root.markDirty(false)
                                App.Theme.windowEffect = root.window_effect
                            }
                        }

                        Item { Layout.fillWidth: true }
                    }
                }

                SettingsSection {
                    title: qsTr("Device Connection")

                    Switch {
                        Layout.fillWidth: true
                        text: qsTr("Auto-raise main window on device connection")
                        checked: root.auto_raise_window
                        onToggled: {
                            root.auto_raise_window = checked
                            root.markDirty(false)
                        }
                    }

                    Switch {
                        Layout.fillWidth: true
                        text: qsTr("Switch to newly connected device")
                        checked: root.switch_to_new_device
                        onToggled: {
                            root.switch_to_new_device = checked
                            root.markDirty(false)
                        }
                    }

                    Switch {
                        Layout.fillWidth: true
                        text: qsTr("Automatically enable Wi-Fi connections")
                        checked: root.auto_enable_wifi_connections
                        onToggled: {
                            root.auto_enable_wifi_connections = checked
                            root.markDirty(false)
                        }
                    }

                    Switch {
                        Layout.fillWidth: true
                        text: qsTr("Automatically connect to wireless devices")
                        checked: root.auto_connect_wireless_devices
                        onToggled: {
                            root.auto_connect_wireless_devices = checked
                            root.markDirty(false)
                        }
                    }

                    Switch {
                        Layout.fillWidth: true
                        text: qsTr("Upgrade to wireless on disconnect")
                        checked: root.upgrade_to_wireless_on_disconnect
                        ToolTip.visible: hovered
                        ToolTip.text: qsTr("When a USB-connected device disconnects, reconnect to it over Wi-Fi when it is available.")
                        onToggled: {
                            root.upgrade_to_wireless_on_disconnect = checked
                            root.markDirty(false)
                        }
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 10

                        Label {
                            text: qsTr("Connection Timeout")
                            Layout.preferredWidth: 175
                        }

                        SpinBox {
                            from: 5
                            to: 60
                            value: root.connection_timeout
                            textFromValue: function(value, locale) { return value + qsTr(" seconds") }
                            valueFromText: function(text, locale) { return parseInt(text) || 5 }
                            onValueModified: {
                                root.connection_timeout = value
                                root.markDirty(false)
                            }
                        }

                        Item { Layout.fillWidth: true }
                    }
                }

                SettingsSection {
                    title: qsTr("Jailbroken")

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 10

                        Label {
                            text: qsTr("Default Root Password")
                            Layout.preferredWidth: 175
                        }

                        TextField {
                            Layout.preferredWidth: 100
                            text: root.default_jailbroken_root_password
                            echoMode: TextInput.PasswordEchoOnEdit
                            ToolTip.visible: hovered
                            ToolTip.text: qsTr("Default password used for SSH root authentication on jailbroken devices. Default is 'alpine'.")
                            onTextEdited: {
                                root.default_jailbroken_root_password = text
                                root.markDirty(false)
                            }
                        }

                        Item { Layout.fillWidth: true }
                    }
                }

                SettingsSection {
                    title: qsTr("AirPlay")

                    Switch {
                        Layout.fillWidth: true
                        text: qsTr("Start AirPlay audio receiver when iDescriptor starts")
                        checked: root.airplay_audio_start_on_launch
                        onToggled: {
                            root.airplay_audio_start_on_launch = checked
                            root.markDirty(false)
                        }
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 10

                        Label {
                            text: qsTr("Resolution")
                            Layout.preferredWidth: 175
                        }

                        ComboBox {
                            readonly property var resolutions: [
                                { label: "1280 × 720", width: 1280, height: 720 },
                                { label: "1920 × 1080", width: 1920, height: 1080 },
                                { label: "2560 × 1440", width: 2560, height: 1440 },
                                { label: "3840 × 2160", width: 3840, height: 2160 }
                            ]
                            model: resolutions.map(function(item) { return item.label })
                            currentIndex: {
                                for (let i = 0; i < resolutions.length; ++i) {
                                    if (resolutions[i].width === root.airplay_width
                                            && resolutions[i].height === root.airplay_height)
                                        return i
                                }
                                return 1
                            }
                            ToolTip.visible: hovered
                            ToolTip.text: qsTr("Request this maximum display resolution from the AirPlay sender.")
                            onActivated: {
                                root.airplay_width = resolutions[currentIndex].width
                                root.airplay_height = resolutions[currentIndex].height
                                root.markDirty(false)
                            }
                        }

                        Item { Layout.fillWidth: true }
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 10

                        Label {
                            text: qsTr("Refresh rate")
                            Layout.preferredWidth: 175
                        }

                        ComboBox {
                            model: ["24", "30", "60", "120"]
                            currentIndex: Math.max(0, model.indexOf(String(root.airplay_refresh_rate)))
                            onActivated: {
                                root.airplay_refresh_rate = Number(currentText)
                                root.markDirty(false)
                            }
                        }

                        Item { Layout.fillWidth: true }
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 10

                        Label {
                            text: qsTr("Fps")
                            Layout.preferredWidth: 175
                        }

                        ComboBox {
                            model: ["24", "30", "60", "120"]
                            currentIndex: Math.max(0, model.indexOf(String(root.airplay_fps)))
                            ToolTip.visible: hovered
                            ToolTip.text: qsTr("Set the fps for AirPlay. Go with 30 fps if you have an older device.")
                            onActivated: {
                                root.airplay_fps = Number(currentText)
                                root.markDirty(false)
                            }
                        }

                        Item { Layout.fillWidth: true }
                    }

                    Switch {
                        Layout.fillWidth: true
                        text: qsTr("Enable H.265 / HEVC")
                        checked: root.airplay_h265
                        ToolTip.visible: hovered
                        ToolTip.text: qsTr("Allow compatible devices to use HEVC video, including resolutions up to 4K.")
                        onToggled: {
                            root.airplay_h265 = checked
                            root.markDirty(false)
                        }
                    }

                    Switch {
                        Layout.fillWidth: true
                        text: qsTr("Overscanned display")
                        checked: root.airplay_overscanned
                        ToolTip.visible: hovered
                        ToolTip.text: qsTr("Tell the AirPlay sender that the display may crop content around its edges.")
                        onToggled: {
                            root.airplay_overscanned = checked
                            root.markDirty(false)
                        }
                    }

                    Switch {
                        Layout.fillWidth: true
                        text: qsTr("Allow New Connections to Take Over")
                        checked: root.airplay_no_hold
                        onToggled: {
                            root.airplay_no_hold = checked
                            root.markDirty(false)
                        }
                    }

                    Switch {
                        Layout.fillWidth: true
                        visible: Qt.platform.os === "linux"
                        text: qsTr("Use legacy ports")
                        checked: root.airplay_use_legacy_ports
                        ToolTip.visible: hovered
                        ToolTip.text: qsTr("Use legacy ports, refer to AIRPLAY.md for more information.")
                        onToggled: {
                            root.airplay_use_legacy_ports = checked
                            root.markDirty(false)
                        }
                    }

                    Switch {
                        Layout.fillWidth: true
                        visible: Qt.platform.os === "linux"
                        text: qsTr("Show V4L2 Button on AirPlay Widget")
                        checked: root.show_v4l2
                        onToggled: {
                            root.show_v4l2 = checked
                            root.markDirty(false)
                        }
                    }
                }

                SettingsSection {
                    title: qsTr("Developer image repository")

                    Label {
                        Layout.fillWidth: true
                        text: qsTr("Current commit: %1").arg(root.developerImageRevision)
                        wrapMode: Text.WrapAnywhere
                    }
                    Label {
                        Layout.fillWidth: true
                        visible: root.pendingImageRevision !== root.developerImageRevision
                        text: qsTr("Selected commit: %1 (apply to save)").arg(root.pendingImageRevision)
                        wrapMode: Text.WrapAnywhere
                    }
                    ComboBox {
                        id: imageCommitSelector
                        Layout.fillWidth: true
                        enabled: !root.imageRepositoryBusy && count > 0
                        model: root.imageCommits.map(function(entry) {
                            return entry.sha.slice(0, 12) + " — "
                                + entry.commit.message.split("\n")[0]
                                + " — " + entry.commit.committer.date.slice(0, 10)
                        })
                        ToolTip.visible: hovered && currentIndex >= 0
                        ToolTip.text: currentIndex >= 0 ? root.imageCommits[currentIndex].sha : ""
                    }
                    RowLayout {
                        Layout.fillWidth: true
                        Button {
                            text: qsTr("Refresh")
                            enabled: !root.imageRepositoryBusy
                            onClicked: root.fetchImageCommits(false)
                        }
                        Button {
                            text: qsTr("Use latest")
                            enabled: !root.imageRepositoryBusy
                            onClicked: root.fetchImageCommits(true)
                        }
                        Button {
                            text: qsTr("Use selected")
                            enabled: !root.imageRepositoryBusy && imageCommitSelector.currentIndex >= 0
                            onClicked: root.selectImageRevision(root.imageCommits[imageCommitSelector.currentIndex].sha)
                        }
                        BusyIndicator {
                            Layout.preferredWidth: 24
                            Layout.preferredHeight: 24
                            running: root.imageRepositoryBusy
                            visible: running
                        }
                    }
                    RowLayout {
                        Layout.fillWidth: true
                        TextField {
                            id: customImageCommit
                            Layout.fillWidth: true
                            placeholderText: qsTr("Full commit hash")
                            selectByMouse: true
                            enabled: !root.imageRepositoryBusy
                            onAccepted: root.selectCustomImageRevision(text)
                        }
                        Button {
                            text: qsTr("Use commit")
                            enabled: !root.imageRepositoryBusy && customImageCommit.text.trim().length > 0
                            onClicked: root.selectCustomImageRevision(customImageCommit.text)
                        }
                    }
                    Label {
                        Layout.fillWidth: true
                        text: qsTr("Applies when preparing developer images. Existing mounted images are unchanged. Repository commits may not contain compatible images.")
                        wrapMode: Text.WordWrap
                        opacity: 0.65
                    }
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 2

                    Label {
                        Layout.fillWidth: true
                        horizontalAlignment: Text.AlignHCenter
                        text: qsTr("iDescriptor")
                        color: "#8a8a8e"
                        font.pixelSize: 11
                    }

                    CopyableText {
                        Layout.alignment: Qt.AlignHCenter
                        text: qsTr("Version %1 · %2")
                                .arg(settingsManager.current_version())
                                .arg(settingsManager.build_description())
                        color: "#8a8a8e"
                        font.pixelSize: 11
                    }

                    Label {
                        Layout.fillWidth: true
                        Layout.topMargin: 10
                        horizontalAlignment: Text.AlignHCenter
                        text: qsTr("A free, open-source, cross-platform iDevice management tool.\n\n© 2026 Uncore <https://github.com/uncor3> and iDescriptor contributors")
                        color: "#8a8a8e"
                        font.pixelSize: 11
                        wrapMode: Text.WordWrap
                    }
                }

                Item { Layout.preferredHeight: 18 }
            }
        }

        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: 58

            RowLayout {
                anchors.fill: parent
                anchors.margins: 10
                spacing: 10

                Button {
                    text: qsTr("Check for Updates")
                    onClicked: App.Updater.checkForUpdates(true)
                }

                Button {
                    text: qsTr("Reset Settings")
                    onClicked: resetDialog.open()
                }

                Item { Layout.fillWidth: true }

                Button {
                    text: qsTr("Apply")
                    enabled: root.dirty && !root.imageRepositoryBusy
                    onClicked: root.applySettings()
                }
            }
        }
    }

    component SettingsSection: SectionBox {
        Layout.fillWidth: true
    }
}
