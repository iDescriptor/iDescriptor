// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "./app-store/"
import "./base"
import "." as App

Item {
    id: root
    anchors.fill: parent
    clip:true

    readonly property bool isMacOS: Qt.platform.os === "osx" || Qt.platform.os === "darwin"

    property bool loading: true
    property string error: ""
    property bool keychainPromptPending: false
    property string email: ""
    property bool isLoggedIn: email.length > 0
    property bool sponsorsFetched: false

    property string searchTerm: ""
    property bool searchLoading: false
    property bool searchError: false
    property string searchErrorText: ""

    property string bundleId: ""
    property string appName: ""
    property string appDescription: ""


    ListModel { id: searchResultsModel }
    ListModel { id: sponsorModel }
    ListModel { id: appModel }

    SponsorUsDialog {
        id: sponsorUsDialog
        anchors.centerIn: parent
    }

    InstallAppPopup {
        id: installPopup
        bundleId: root.bundleId
        appName: root.appName
        appDescription: root.appDescription
        anchors.centerIn: parent
    }

    GetIpaPopup {
        id: getIpaPopup
        bundleId: root.bundleId
        appName: root.appName
        appDescription: root.appDescription
        anchors.centerIn: parent
    }

    LoginDialog {
        id: loginDialog
    }

    Loader {
        id: keyringPassphraseDialogLoader
        active: Qt.platform.os === "linux" || Qt.platform.os === "windows"
        source: active ? "KeyringPassphraseDialog.qml" : ""
    }

    Loader {
        id: keychainDialogLoader
        anchors.fill: parent
        active: root.isMacOS
        source: active ? "KeychainDialog.qml" : ""
        onLoaded: Qt.callLater(root.openKeychainDialogWhenReady)
        onStatusChanged: {
            if (status === Loader.Error)
                Qt.callLater(root.openKeychainDialogWhenReady)
        }
    }

    function openKeychainDialogWhenReady() {
        if (!root.keychainPromptPending)
            return
        if (keychainDialogLoader.status === Loader.Ready && keychainDialogLoader.item) {
            root.keychainPromptPending = false
            keychainDialogLoader.item.open()
        } else if (keychainDialogLoader.status === Loader.Error) {
            root.keychainPromptPending = false
            console.warn("Could not load the Keychain dialog; initializing without the saved account")
            apps.init(false)
        }
    }

    Connections {
        target: apps

        function onKeyringPassphraseRequested() {
            if (keyringPassphraseDialogLoader.active)
                keyringPassphraseDialogLoader.item.open()
        }
    }

    Connections {
        target: keychainDialogLoader.item

        function onContinueRequested() {
            apps.init(true)
        }

        function onSkipRequested() {
            apps.init(false)
        }
    }

    function openInstallPopup(bundleId, appName, description) {
        const isLoggedIn = apps.state.email.length > 0
        if (!isLoggedIn) {
            if (Qt.platform.os === "windows") {
                // showWarning from FluWindow
                showWarning(qsTr("You must be signed in to install apps."),3000)
            } else {
                loginDialog.open()
            }
            return
        }

        root.bundleId = bundleId
        root.appName = appName
        root.appDescription = description || ""
        Qt.callLater(() => {
          installPopup.open()
        })
    }

    function openGetIpaPopup(bundleId, appName, description) {
        const isLoggedIn = apps.state.email.length > 0
        if (!isLoggedIn) {
            if (Qt.platform.os === "windows") {
                // showWarning from FluWindow
                showWarning(qsTr("You must be signed in to download IPA files."),3000)
            } else {
                loginDialog.open()
            }
            return
        }
        root.bundleId = bundleId
        root.appName = appName
        root.appDescription = description || ""
        Qt.callLater(() => {
          getIpaPopup.open()
        })
    }

    function clearCatalog() {
        sponsorModel.clear()
        appModel.clear()
    }

    function addApp(obj) { appModel.append(obj) }

    function addSponsor(obj) { sponsorModel.append(obj) }

    function addSponsors(tierObj, label, color) {
        if (!tierObj || !tierObj.members) return
        for (var i = 0; i < tierObj.members.length; ++i) {
            var m = tierObj.members[i]
            addSponsor({
                name: m.name || "",
                logo: m.logo,
                website: m.website || "",
                tierLabel: label,
                tierColor: color,
                bundleId: m.bundleId,
                description: m.description,
                useBundleIdForIcon: m.useBundleIdForIcon || false
            })
        }
    }

    function addDefaultApps() {
        addApp({ name: "Instagram", bundleId: "com.burbn.instagram", description: qsTr("Photo & Video sharing social network"), logoUrl: "" })
        addApp({ name: "Spotify", bundleId: "com.spotify.client", description: qsTr("Music streaming and podcast platform"), logoUrl: "" })
        addApp({ name: "YouTube", bundleId: "com.google.ios.youtube", description: qsTr("Video sharing and streaming platform"), logoUrl: "" })
        addApp({ name: "X", bundleId: "com.atebits.Tweetie2", description: qsTr("Social media and microblogging"), logoUrl: "" })
        addApp({ name: "TikTok", bundleId: "com.zhiliaoapp.musically", description: qsTr("Short-form video hosting service"), logoUrl: "" })
        addApp({ name: "Twitch", bundleId: "tv.twitch", description: qsTr("Live streaming platform"), logoUrl: "" })
        addApp({ name: "Telegram", bundleId: "ph.telegra.Telegraph", description: qsTr("Cloud-based instant messaging"), logoUrl: "" })
        addApp({ name: "Reddit", bundleId: "com.reddit.Reddit", description: qsTr("Social news aggregation platform"), logoUrl: "" })
    }

    function fetchSponsors() {
        loading = true
        error = ""
        clearCatalog()

        var xhr = new XMLHttpRequest()
        xhr.open("GET", App.Constants.sponsorsUrl)
        xhr.onreadystatechange = function() {
            if (xhr.readyState !== XMLHttpRequest.DONE) return
            if (xhr.status === 200) {
                try {
                    var rootObj = JSON.parse(xhr.responseText)
                    var key = App.Helpers.pickMatchingVersionKey(rootObj)
                    var versioned = key ? rootObj[key] : null
                    var sponsors = versioned && versioned.sponsors ? versioned.sponsors : null

                    if (sponsors) {
                        addSponsors(sponsors.platinum, qsTr("Platinum"), "#E5E4E2")
                        addSponsors(sponsors.gold, qsTr("Gold"), "#D4AF37")
                        addSponsors(sponsors.silver, qsTr("Silver"), "#C0C0C0")
                        addSponsors(sponsors.bronze, qsTr("Bronze"), "#CD7F32")
                    }
                } catch (e) {
                    error = qsTr("Failed to parse sponsors JSON.")
                }
            } else {
                error = qsTr("Failed to fetch sponsors.")
            }

            addDefaultApps()
            loading = false
        }
        xhr.send()
    }

    function openDetails(app) {
        if (!app || !app.bundleId) return
        nav.push(detailsHostComponent, { app: app })
    }

    Component.onCompleted: {
        if (root.isMacOS && settingsManager.show_keychain_dialog()) {
            root.keychainPromptPending = true
            root.openKeychainDialogWhenReady()
            return
        }

        apps.init(true)
    }

    Connections {
        target: apps

        function onStateChanged() {
            var s = apps.state
            if (!s) return
            email = s.email || ""
            if (s.error && !loginDialog.visible) error = s.error
            if (s.init && !root.sponsorsFetched) {
                root.sponsorsFetched = true
                fetchSponsors()
            }
        }

        function onSearch_ready(searchTerm, success, res) {
            if (searchTerm !== root.searchTerm.trim()) return

            root.searchLoading = false
            searchResultsModel.clear()

            if (!success) {
                console.error("Search failed for term:", searchTerm)
                searchError = true
                searchErrorText = qsTr("Search failed.")
                return
            }

            try {
                const parsedRes = JSON.parse(res)
                if (!parsedRes || !Array.isArray(parsedRes)) {
                    console.error("Invalid search results format: 'results' array not found or not an array.", res)
                    searchError = true
                    searchErrorText = qsTr("Search returned an invalid response.")
                    return
                }

                searchError = false
                searchErrorText = ""

                for (var i = 0; i < parsedRes.length; ++i) {
                    const item = parsedRes[i]
                    searchResultsModel.append({
                        id: item.trackId || 0,
                        bundleId: item.bundleId || "",
                        name: item.trackName || "",
                        price: item.price || 0,
                        description: item.description || ""
                    })
                }
            } catch (e) {
                searchError = true
                searchErrorText = qsTr("Failed to parse search results.")
            }
        }
    }

    StackView {
        id: nav
        anchors.fill: parent
        initialItem: mainPageComponent
        clip: true

        pushEnter: Transition {
            PropertyAnimation { property: "x"; from: root.width; to: 0; duration: 320; easing.type: Easing.OutCubic }
        }
        pushExit: Transition {
            PropertyAnimation { property: "x"; from: 0; to: -root.width; duration: 320; easing.type: Easing.OutCubic }
            PropertyAnimation { property: "opacity"; from: 1; to: 0.55; duration: 320; easing.type: Easing.OutCubic }
        }
        popEnter: Transition {
            PropertyAnimation { property: "x"; from: -root.width; to: 0; duration: 280; easing.type: Easing.OutCubic }
            PropertyAnimation { property: "opacity"; from: 0.55; to: 1; duration: 280; easing.type: Easing.OutCubic }
        }
        popExit: Transition {
            PropertyAnimation { property: "x"; from: 0; to: nav.width; duration: 280; easing.type: Easing.OutCubic }
        }
    }

    Component {
        id: mainPageComponent

        ColumnLayout {
            spacing: 0

            function showCatalog() {
                root.searchLoading = false
                root.searchError = false
                root.searchErrorText = ""
                searchResultsModel.clear()
                if (!resultStack.currentItem || resultStack.currentItem.objectName !== "catalogPage")
                    resultStack.replace(catalogPageComponent)
            }

            function showSearch() {
                if (!resultStack.currentItem || resultStack.currentItem.objectName !== "searchPage")
                    resultStack.replace(searchPageComponent)
            }

            Component.onCompleted: resultStack.push(catalogPageComponent)

            Timer {
                id: searchTimer
                interval: 260
                repeat: false
                onTriggered: {
                    var term = root.searchTerm.trim()
                    if (!term.length) {
                        showCatalog()
                        return
                    }

                    showSearch()

                    root.searchLoading = true
                    root.searchError = false
                    root.searchErrorText = ""
                    searchResultsModel.clear()
                    apps.search(term)
                }
            }

            RowLayout {
                Layout.fillWidth: true
                Layout.margins: 18
                spacing: 12

                TextField {
                    id: searchField
                    Layout.preferredWidth: 200
                    placeholderText: qsTr("Search for apps...")
                    onTextChanged: {
                        root.searchTerm = text
                        searchTimer.restart()
                    }
                }

                Item { Layout.fillWidth: true }

                Label {
                    text: isLoggedIn ? qsTr("Signed in as %1").arg(email) : qsTr("Not signed in")
                    color: App.Theme.textMuted
                }

                Button {
                    text: isLoggedIn ? qsTr("Sign Out") : qsTr("Sign In")
                    onClicked: {
                        if (isLoggedIn)
                            apps.sign_out()
                        else
                            loginDialog.open()
                    }
                }
            }

            StackView {
                id: resultStack
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                replaceEnter: Transition {
                    PropertyAnimation { property: "opacity"; from: 0; to: 1; duration: 180 }
                }
                replaceExit: Transition {
                    PropertyAnimation { property: "opacity"; from: 1; to: 0; duration: 120 }
                }
            }
        }
    }

    Component {
        id: catalogPageComponent

        ScrollView {
            id: catalogScroll
            anchors.margins: 10
            anchors.fill: parent
            clip: true

            Flow {
                id: catalogGrid
                width: catalogScroll.availableWidth
                spacing: 10

                readonly property real itemWidth: Math.max(
                    300,
                    (width - (2 * spacing)) / 3
                )
                readonly property real itemHeight: 110

                SponsorUs {
                  width: catalogGrid.itemWidth
                  height: catalogGrid.itemHeight
                  visible: sponsorModel.count === 0 && root.error.length === 0 && !root.loading
                  onSponsorshipRequested: sponsorUsDialog.open()
                }

                Repeater {
                    model: sponsorModel
                    delegate: Item {
                        width: catalogGrid.itemWidth
                        height: catalogGrid.itemHeight
                        SponsorItem {
                            anchors.fill: parent
                            name: model.name
                            bundleId: model.bundleId
                            logo: model.logo
                            website:model.website
                            tierLabel: model.tierLabel
                            tierColor: model.tierColor
                            description: model.description
                            useBundleIdForIcon: model.useBundleIdForIcon
                            onInstallRequested: function(bundleId, appName) { root.openInstallPopup(bundleId, appName, description) }
                        }
                    }
                }

                Repeater {
                    model: appModel
                    delegate: Item {
                        width: catalogGrid.itemWidth
                        height: catalogGrid.itemHeight
                        AppItem {
                            anchors.fill: parent
                            name: model.name
                            bundleId: model.bundleId
                            description: model.description
                            onSelected: function(app) { root.openDetails(app) }
                            onInstallRequested: function(bundleId, appName) { root.openInstallPopup(bundleId, appName, description) }
                            onGetIpaRequested: function(bundleId, appName) { root.openGetIpaPopup(bundleId, appName, description) }
                        }
                    }
                }
            }
        }
    }

    Component {
        id: searchPageComponent

        StateView {
            objectName: "searchPage"
            autoSwitchContent: false
            retryable: false
            viewState: root.searchLoading
                       ? StateView.State.Loading
                       : (root.searchError ? StateView.State.Error : StateView.State.Content)
            errorText: root.searchErrorText

            contentItem: Item {
                anchors.fill: parent

                ScrollView {
                    anchors.fill: parent
                    clip: true

                    ListView {
                        id: searchList
                        anchors.fill: parent
                        anchors.margins: 18
                        spacing: 10
                        model: searchResultsModel

                        delegate: Item {
                            width: searchList.width
                            height: 112
                            AppItem {
                                anchors.fill: parent
                                name: model.name
                                bundleId: model.bundleId
                                description: model.description
                                onSelected: function(app) { root.openDetails(app) }
                                onInstallRequested: function(bundleId, appName) { root.openInstallPopup(bundleId, appName, description) }
                                onGetIpaRequested: function(bundleId, appName) { root.openGetIpaPopup(bundleId, appName, description) }
                            }
                        }
                    }
                }

                Label {
                    anchors.centerIn: parent
                    text: qsTr("No results")
                    color: "#6e6e73"
                    visible: !root.searchLoading && !root.searchError && searchResultsModel.count === 0
                }
            }
        }
    }

    Component {
        id: detailsHostComponent

        Item {
            required property var app

            Loader {
                id: detailsLoader
                anchors.fill: parent
                property var detailsApp: app
                active: true
                sourceComponent: AppDetails {
                    app: detailsLoader.detailsApp
                    onBackRequested: nav.pop()
                }
            }
        }
    }
}
