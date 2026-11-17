// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "./base"

Item {
    id: root

    property var diagnoseState: DiagnoseImpl.state
    property bool autoCheck: true
    property string focusedDependencyId: ""
    property string message: ""

    function openDiagnostics() {
        diagnosticsDialog.open()
    }

    function openDiagnosticsFor(dependencyId, message) {
        root.focusedDependencyId = dependencyId
        root.message = message || ""
        diagnosticsDialog.open()
        root.scheduleCheck()
    }

    function focusSelectedDependency() {
        if (root.focusedDependencyId.length === 0 || !root.diagnoseState.items)
            return

        for (let index = 0; index < root.diagnoseState.items.length; ++index) {
            if (root.diagnoseState.items[index].id === root.focusedDependencyId) {
                dependencyList.currentIndex = index
                dependencyList.positionViewAtIndex(index, ListView.Center)
                return
            }
        }
    }

    function colorForKind(kind) {
        if (kind === "ok")
            return "#16a34a"
        if (kind === "warning")
            return "#d97706"
        if (kind === "error")
            return "#dc2626"
        return "#6b7280"
    }

    function statusText(modelData) {
        if (modelData.availability === 0)
            return qsTr("Installed")
        if (modelData.availability === 1)
            return qsTr("Installed, not running")
        if (modelData.availability === 2)
            return qsTr("Missing")
        return qsTr("Unable to check")
    }

    function performDependencyAction(dependency) {
        if (dependency.actionMode === "instructions")
            Qt.openUrlExternally(dependency.documentationUrl)
        else
            DiagnoseImpl.install(dependency.id)
    }

    // Shared presentation only. Each dependency below owns its copy and action.
    component DependencyRow: Rectangle {
        id: row
        property var dependency: ({})
        property string dependencyName: ""
        property string description: ""
        property string actionLabel: ""
        signal actionRequested()

        implicitHeight: Math.max(78, rowContents.implicitHeight + 20)
        radius: 7
        color: "transparent"
        border.color: dependency.id === root.focusedDependencyId
                      ? diagnosticsDialog.palette.highlight : "transparent"
        border.width: dependency.id === root.focusedDependencyId ? 2 : 1

        RowLayout {
            id: rowContents
            anchors.fill: parent
            anchors.margins: 10
            spacing: 10

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 3

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 6
                    Label {
                        Layout.fillWidth: true
                        text: row.dependencyName
                        font.pixelSize: 13
                        font.weight: Font.DemiBold
                        elide: Text.ElideRight
                    }
                    Label {
                        visible: !!row.dependency.optional
                        text: qsTr("Optional")
                        color: "#94a3b8"
                        font.pixelSize: 11
                    }
                }
                Label {
                    Layout.fillWidth: true
                    text: row.description
                    color: "#94a3b8"
                    font.pixelSize: 11
                    wrapMode: Text.WordWrap
                }
                Label {
                    Layout.fillWidth: true
                    text: root.statusText(row.dependency)
                    color: root.colorForKind(row.dependency.statusKind)
                    font.pixelSize: 11
                    font.weight: Font.DemiBold
                }
            }
            BusyIndicator {
                Layout.preferredWidth: 24
                Layout.preferredHeight: 24
                visible: root.diagnoseState.installingId === row.dependency.id
                running: visible
            }
            Button {
                visible: !!row.dependency.actionVisible
                         && root.diagnoseState.installingId !== row.dependency.id
                enabled: !root.diagnoseState.checking
                         && !root.diagnoseState.installingId
                text: row.actionLabel
                onClicked: row.actionRequested()
            }
        }
    }

    component AvahiDependency: DependencyRow {
        dependencyName: qsTr("Avahi Daemon")
        description: qsTr("Required for AirPlay, optional backend for wireless device discovery.")
        actionLabel: dependency.availability === 1 ? qsTr("Start") : qsTr("Install")
        onActionRequested: DiagnoseImpl.install(dependency.id)
    }

    component UdevDependency: DependencyRow {
        dependencyName: qsTr("UDEV rules")
        description: qsTr("Optional USB permissions for devices in recovery mode.")
        actionLabel: qsTr("View Instructions")
        onActionRequested: udevInstructions.open()
    }

    // The backend retains the Windows Store instructions policy and URLs.
    component WindowsDependency: DependencyRow {
        actionLabel: dependency.actionMode === "instructions" ? qsTr("View Instructions")
                     : dependency.availability === 1 ? qsTr("Start") : qsTr("Install")
        onActionRequested: root.performDependencyAction(dependency)
    }

    component AppleMobileDependency: WindowsDependency {
        dependencyName: qsTr("Apple Mobile Device Support")
        description: qsTr("Required for iOS device communication.")
    }

    component BonjourDependency: WindowsDependency {
        dependencyName: qsTr("Bonjour Service")
        description: qsTr("Required for AirPlay, optional backend for wireless device discovery.")
    }

    component WinFspDependency: WindowsDependency {
        dependencyName: qsTr("WinFsp")
        description: qsTr("Optional. Required for mounting the device as a drive.")
    }

    Component { id: avahiComponent; AvahiDependency {} }
    Component { id: udevComponent; UdevDependency {} }
    Component { id: appleMobileComponent; AppleMobileDependency {} }
    Component { id: bonjourComponent; BonjourDependency {} }
    Component { id: winFspComponent; WinFspDependency {} }

    Dialog {
        id: udevInstructions
        modal: true
        anchors.centerIn: Overlay.overlay
        width: 380
        title: qsTr("UDEV Rules")
        standardButtons: Dialog.Ok
        contentItem: ColumnLayout {
            spacing: 12
            Label {
                Layout.fillWidth: true
                text: qsTr("You can read the UDEV.md guide for manual configuration.")
                wrapMode: Text.WordWrap
            }
            Label {
                Layout.fillWidth: true
                textFormat: Text.RichText
                text: '<a href="https://github.com/iDescriptor/iDescriptor/blob/main/UDEV.md">'
                      + qsTr("Read the UDEV.md guide") + '</a>'
                wrapMode: Text.WordWrap
                onLinkActivated: function(link) { Qt.openUrlExternally(link) }
            }
        }
    }

    function updateStateView() {
        if (diagnoseState.error && diagnoseState.error.length > 0)
            diagnosticsState.viewState = StateView.State.Error
        else if (diagnoseState.checking)
            diagnosticsState.viewState = StateView.State.Loading
        else
            diagnosticsState.viewState = StateView.State.Content
    }

    function scheduleCheck() {
        diagnosticsState.viewState = StateView.State.Loading
        delayedCheckTimer.restart()
    }

    onDiagnoseStateChanged: {
        updateStateView()

        if (diagnoseState.notice && diagnoseState.notice.length > 0)
            noticeDialog.open()

        if (!diagnoseState.checking)
            Qt.callLater(root.focusSelectedDependency)
    }

    Component.onCompleted: {
        if (root.autoCheck)
            scheduleCheck()
    }

    Timer {
        id: delayedCheckTimer
        interval: 350
        repeat: false
        onTriggered: DiagnoseImpl.check()
    }

    Dialog {
        id: noticeDialog
        modal: true
        anchors.centerIn: Overlay.overlay
        width: 340
        title: qsTr("Dependency Check")
        standardButtons: Dialog.Ok
        onAccepted: Qt.callLater(DiagnoseImpl.clear_notice)
        onRejected: Qt.callLater(DiagnoseImpl.clear_notice)

        TextEdit {
            readOnly: true
            selectByMouse: true
            persistentSelection: true
            color: palette.text
            width: 300
            textFormat: TextEdit.PlainText
            text: root.diagnoseState.notice || ""
            wrapMode: Text.WordWrap
        }
    }

    AnimatedDialog {
        id: diagnosticsDialog
        modal: true
        anchors.centerIn: Overlay.overlay
        title: qsTr("Diagnostics")
        width: 620
        height: 500
        standardButtons: Dialog.Close
        onOpened: {
            root.updateStateView()
            Qt.callLater(root.focusSelectedDependency)
        }

        contentItem: StateView {
            id: diagnosticsState
            implicitWidth: 580
            implicitHeight: 390
            autoSwitchContent: false
            retryable: true
            errorText: root.diagnoseState.error || qsTr("Unable to check system dependencies.")
            onRetryRequested: root.scheduleCheck()

            contentItem: SectionBox {
                anchors.fill: parent
                title: qsTr("Dependency Check")

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 12

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 12

                        Label {
                            Layout.fillWidth: true
                            text: root.diagnoseState.summary || qsTr("Checking system dependencies...")
                            color: root.colorForKind(root.diagnoseState.summaryKind)
                            font.pixelSize: 12
                            elide: Text.ElideRight
                        }

                        IconToolButton {
                            icon.source: "qrc:/resources/icons/ic_outline-refresh.svg"
                            toolTipText: qsTr("Refresh")
                            Accessible.name: qsTr("Refresh")
                            enabled: !root.diagnoseState.checking
                            onClicked: root.scheduleCheck()
                        }
                    }

                    ListView {
                        id: dependencyList
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        clip: true
                        interactive: contentHeight > height
                        model: root.diagnoseState.items || []
                        spacing: 8

                        // Rust supplies the platform-specific list and live status.
                        // Dispatch only presentation here; do not duplicate OS filtering.
                        delegate: Loader {
                            id: dependencyLoader
                            required property var modelData
                            width: ListView.view.width
                            height: item ? (item as DependencyRow).implicitHeight : 0
                            sourceComponent: {
                                switch (modelData.id) {
                                case "avahi": return avahiComponent
                                case "udev_rules": return udevComponent
                                case "apple_mobile_device_support": return appleMobileComponent
                                case "bonjour": return bonjourComponent
                                case "winfsp": return winFspComponent
                                default: return null
                                }
                            }
                            onLoaded: {
                                const row = item as DependencyRow
                                row.dependency = Qt.binding(function() {
                                    return dependencyLoader.modelData
                                })
                            }
                        }
                    }

                    Label {
                        Layout.fillWidth: true
                        visible: root.message.length > 0
                        text: root.message
                        color: "#dc2626"
                        font.weight: Font.DemiBold
                        wrapMode: Text.WordWrap
                    }
                }
            }
        }
    }
}
