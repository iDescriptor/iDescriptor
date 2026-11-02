// SPDX-FileCopyrightText: 2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later
import QtQuick
import QtQuick.Window
import QtQuick.Controls
import QtQuick.Layouts
import Qt5Compat.GraphicalEffects
import iDescriptor
import org.freedesktop.gstreamer.Qt6GLVideoItem
import ".." as App
import "../base"

ToolWindow {
    id: root
    width: 380
    height: 780
    minimumWidth: 260
    minimumHeight: 260
    visible: true
    title: qsTr("Screen Mirroring - iDescriptor")
    color: "transparent"
    flags: Qt.Window | Qt.FramelessWindowHint | Qt.CustomizeWindowHint
         | Qt.WindowMinimizeButtonHint | Qt.WindowCloseButtonHint
    showMaximize: false
    showMinimize: false
    showClose: false
    customChrome: true
    fitsAppBarWindows: true
    autoDestroy: false
    _effect: "normal"

    property bool muted: false
    property bool phoneSilenced: true
    property bool restorePhoneAudio: true
    property bool fittingWindow: false
    property bool preparing: false
    property bool closingRequested: false
    property bool toolbarHeld: false
    property real lastAspect: 0
    readonly property bool busy: preparing || backend.state === 1 || backend.state === 3
    readonly property bool running: backend.state === 1 || backend.state === 2 || backend.state === 3
    readonly property bool toolbarVisible: toolbarHeld || backend.state !== 2 || contextMenu.visible || audioPopup.visible
    readonly property bool macChrome: Qt.platform.os === "osx" || Qt.platform.os === "macos"
    property real toolbarReveal: toolbarVisible ? 52 : 0
    Behavior on toolbarReveal { NumberAnimation { duration: 180; easing.type: Easing.OutCubic } }
    readonly property real cornerRadius: Math.min(width, height) * (0.095 - toolbarReveal / 52 * 0.04)
    // Reserve a separate hover strip above the video even when the header is hidden.
    readonly property real headerExtent: Math.max(8, toolbarReveal)
    readonly property real frameInset: toolbarReveal / 52 * 8
    readonly property real videoAspect: backend.video_width > 0 && backend.video_height > 0
        ? (sideways ? backend.video_height / backend.video_width : backend.video_width / backend.video_height) : 0
    readonly property bool sideways: backend.display_rotation === 90 || backend.display_rotation === 270
    property string preparationError: ""
    // property string diagnosticStatus: ""

    function startMirroring() {
        root.preparationError = ""
        if (root.device.info.ios_version_major < 27) {
            root.preparationError = qsTr("Screen Mirroring requires iOS 27 or later. Use Live Screen for this device.")
            return
        }
        root.preparing = true
        developerHelper.start()
    }
//     function captureDiagnostics() {
//         root.diagnosticStatus = qsTr("Waiting for the next video frame…")
//         backend.capture_diagnostics(JSON.stringify({
//             windowWidth: root.width, windowHeight: root.height,
//             devicePixelRatio: Screen.devicePixelRatio,
//             viewportX: viewport.x, viewportY: viewport.y,
//             viewportWidth: viewport.width, viewportHeight: viewport.height,
//             videoX: video.x, videoY: video.y,
//             videoWidth: video.width, videoHeight: video.height,
//             rotation: backend.display_rotation,
//             decodedWidth: backend.video_width, decodedHeight: backend.video_height,
//             toolbarReveal: root.toolbarReveal, frameInset: root.frameInset,
//             cornerRadius: root.cornerRadius
//         }))
//     }
    function applyVolume() { backend.set_volume(root.muted ? 0 : volumeSlider.value) }
    function revealToolbar() {
        if (pointer.touching) return
        hideToolbar.stop()
        root.toolbarHeld = true
    }

    function fitWindow() {
        if (videoAspect <= 0 || fittingWindow) return
        fittingWindow = true
        const bar = headerExtent + frameInset * 2
        let targetHeight = Math.round((width - frameInset * 2) / videoAspect) + bar
        if (targetHeight > Screen.desktopAvailableHeight - 48) {
            targetHeight = Screen.desktopAvailableHeight - 48
            width = Math.round((targetHeight - bar) * videoAspect + frameInset * 2)
        }
        height = Math.max(minimumHeight, targetHeight)
        fittingWindow = false
    }
    onWidthChanged: { fitTimer.restart(); shapeTimer.restart() }
    onCornerRadiusChanged: shapeTimer.restart()
    Timer { id: shapeTimer; interval: 0; onTriggered: backend.shape_window(root.contentItem, root.cornerRadius) }
    onHeightChanged: {
        shapeTimer.restart()
        if (!fittingWindow && videoAspect > 0) {
            fittingWindow = true
            width = Math.max(minimumWidth, Math.round((height - headerExtent - frameInset * 2) * videoAspect + frameInset * 2))
            fittingWindow = false
        }
    }
    onHeaderExtentChanged: fitWindow()
    onVideoAspectChanged: {
        if (videoAspect <= 0) return
        pointer.touching = false
        backend.release_input()
        if (lastAspect > 0 && (videoAspect > 1) !== (lastAspect > 1)) {
            fittingWindow = true
            width = Math.max(minimumWidth, Math.min(Screen.desktopAvailableWidth - 48,
                Math.round((height - headerExtent))))
            fittingWindow = false
        }
        lastAspect = videoAspect
        fitWindow()
    }
    Timer { id: fitTimer; interval: 0; onTriggered: root.fitWindow() }
    onActiveChanged: { if (!active) backend.release_input() }
    onClosing: {
        root.closingRequested = true
        backend.stop()
    }
    Component.onCompleted: {
        // TODO:
        // ToolWindow uses FluentUI on Windows. Its stock title bar and surface
        // must yield to this tool's own chrome; other tools keep their defaults.
        if (Qt.platform.os === "windows") {
            // Qt.callLater(function() {
            //     root.appBar.visible = false
            //     root.appBar.height = 0
            //     root.backgroundColor = "transparent"
            //     root.resizeBorderWidth = 0
            //     root.setHitTestVisible(surface)
            // })
        }
        Qt.callLater(function() {
            if (!root.closingRequested && !root.busy && !root.running)
                root.startMirroring()
        })
    }

    Timer {
        id: hideToolbar
        interval: 250
        onTriggered: {
            if (pointer.touching) restart()
            else if (!topHover.hovered && !toolbarHover.hovered) root.toolbarHeld = false
        }
    }
    ScreenMirroringBackend {
        id: backend
//         onDiagnosticsReady: function(success, detail) {
//             root.diagnosticStatus = success ? qsTr("Saved diagnostics: %1").arg(detail) : detail
//             if (success) {
//                 surface.grabToImage(function(result) {
//                     if (!result.saveToFile(detail + "/rendered-window.png"))
//                         console.warn("Could not save rendered mirroring comparison")
//                 })
//             }
//         }
        onInputReadyChanged: {
            if (input_ready && root.phoneSilenced) backend.silence_phone(true)
        }
        onDisplayRotationChanged: {
            pointer.touching = false
            backend.release_input()
        }
    }
    App.DevModeHelper {
        id: developerHelper
        device: root.device
        parent: root.contentItem
        onHandled: function(success, forced) {
            root.preparing = false
            if (root.closingRequested)
                return
            if (success) {
                root.applyVolume()
                backend.set_restore_phone_audio(root.restorePhoneAudio)
                backend.start(root.udid, video)
            } else {
                root.preparationError = qsTr("Prepare the developer image and enable Developer Mode, then try again.")
            }
        }
    }

    Shortcut { sequence: "Ctrl+H"; enabled: root.active && backend.input_ready; onActivated: backend.home() }
    Shortcut { sequence: "Ctrl+Tab"; enabled: root.active && backend.input_ready; onActivated: backend.app_switcher() }

    Item {
        id: surface
        anchors.fill: parent
        layer.enabled: true
        layer.effect: OpacityMask {
            maskSource: Rectangle {
                width: surface.width
                height: surface.height
                radius: root.cornerRadius
                color: "white"
            }
        }
        Rectangle { anchors.fill: parent; color: root.toolbarVisible ? App.Theme.controlFill : "transparent"; radius: root.cornerRadius }

        Item {
            id: viewport
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.top: parent.top
            anchors.bottom: parent.bottom
            anchors.topMargin: root.headerExtent + root.frameInset
            anchors.leftMargin: root.frameInset
            anchors.rightMargin: root.frameInset
            anchors.bottomMargin: root.frameInset
            clip: true
            // Round the phone display independently from the outer window.
            // Keep the mask in viewport space so landscape rotation does not
            // rotate the frame or change the pointer coordinate system.
            layer.enabled: true
            layer.effect: OpacityMask {
                maskSource: Rectangle {
                    width: viewport.width
                    height: viewport.height
                    // With no toolbar/inset, video and window share one edge.
                    // Reveal the larger inner phone radius with the frame.
                    radius: root.cornerRadius * (1 - root.toolbarReveal / 52)
                          + Math.min(width, height) * 0.14 * (root.toolbarReveal / 52)
                    color: "white"
                }
            }

            GstGLQt6VideoItem {
                id: video
                anchors.centerIn: parent
                readonly property real fitScale: backend.video_width > 0 && backend.video_height > 0
                    ? Math.min(parent.width / (root.sideways ? backend.video_height : backend.video_width),
                               parent.height / (root.sideways ? backend.video_width : backend.video_height)) : 1
                width: backend.video_width > 0 ? backend.video_width * fitScale : parent.width
                height: backend.video_height > 0 ? backend.video_height * fitScale : parent.height
                rotation: backend.display_rotation
            }
            MouseArea {
                id: pointer
                anchors.fill: parent
                acceptedButtons: Qt.LeftButton | Qt.RightButton
                enabled: backend.state === 2
                preventStealing: true
                property bool touching: false
                onPressed: function(mouse) {
                    if (mouse.button === Qt.RightButton) {
                        backend.release_input()
                        contextMenu.popup()
                        return
                    }
                    touching = backend.touch(0, mouse.x, mouse.y, width, height)
                    mouse.accepted = touching
                }
                onPositionChanged: function(mouse) {
                    if (touching)
                        backend.touch(1, mouse.x, mouse.y, width, height)
                }
                onReleased: function(mouse) {
                    if (touching)
                        backend.touch(2, mouse.x, mouse.y, width, height)
                    touching = false
                    if (topHover.hovered) root.revealToolbar()
                    else hideToolbar.restart()
                }
                onCanceled: { touching = false; backend.release_input() }
                onWheel: function(wheel) {
                    if (!backend.input_ready || touching) {
                        wheel.accepted = false
                        return
                    }
                    const dx = wheel.pixelDelta.x !== 0 ? wheel.pixelDelta.x : wheel.angleDelta.x / 120 * width * 0.16
                    const dy = wheel.pixelDelta.y !== 0 ? wheel.pixelDelta.y : wheel.angleDelta.y / 120 * height * 0.16
                    backend.scroll(wheel.x, wheel.y, width, height, dx, dy)
                    wheel.accepted = true
                }
            }
            Rectangle { anchors.fill: parent; color: "#090909"; visible: backend.state !== 2 }
            ColumnLayout {
                anchors.centerIn: parent
                width: Math.max(100, parent.width - 40)
                visible: backend.state !== 2
                spacing: 18
                BusyIndicator { Layout.alignment: Qt.AlignHCenter; running: root.busy; visible: running }
                Label {
                    Layout.fillWidth: true
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.WordWrap
                    color: "#eeeeee"
                    font.pixelSize: 17
                    text: root.preparing ? qsTr("Preparing your iPhone…")
                        : backend.state === 1 ? qsTr("Connecting to your iPhone…")
                        : backend.state === 3 ? qsTr("Stopping mirroring…")
                        : backend.state === 4 ? qsTr("Could not mirror this device")
                        : qsTr("Your iPhone, right here")
                }
                Label {
                    Layout.fillWidth: true
                    text: root.preparationError.length > 0 ? root.preparationError : backend.error
                    visible: text.length > 0
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.Wrap
                    color: "#cccccc"
                }
                Button {
                    Layout.alignment: Qt.AlignHCenter
                    text: qsTr("Start Mirroring")
                    enabled: !root.busy && !root.closingRequested
                    onClicked: root.startMirroring()
                }
            }
        }

        Item {
            id: topHoverZone
            anchors.top: parent.top
            anchors.left: parent.left
            anchors.right: parent.right
            // This transparent strip never overlaps the video input area.
            height: 8
            HoverHandler {
                id: topHover
                onHoveredChanged: {
                    if (hovered) root.revealToolbar()
                    else hideToolbar.restart()
                }
            }
        }
        Rectangle {
            id: toolbar
            anchors.top: parent.top
            anchors.left: parent.left
            anchors.right: parent.right
            height: root.toolbarReveal
            clip: true
            visible: height > 0
            opacity: root.toolbarReveal / 52
            color: App.Theme.darkMode ? "#242426" : "#f1f1f3"
            HoverHandler {
                id: toolbarHover
                onHoveredChanged: {
                    if (hovered) root.revealToolbar()
                    else hideToolbar.restart()
                }
            }
            MouseArea {
                anchors.fill: parent
                acceptedButtons: Qt.LeftButton
                onPressed: root.startSystemMove()
                onDoubleClicked: function(mouse) { mouse.accepted = true }
            }
            Row {
                visible: root.macChrome
                anchors.left: parent.left
                anchors.leftMargin: 14
                anchors.verticalCenter: parent.verticalCenter
                spacing: 10
                Repeater {
                    model: [qsTr("Close"), qsTr("Minimize")]
                    delegate: ToolButton {
                        required property int index
                        required property string modelData
                        width: 28
                        height: 32
                        Accessible.name: modelData
                        ToolTip.visible: hovered
                        ToolTip.text: modelData
                        background: Rectangle {
                            anchors.centerIn: parent
                            width: 17
                            height: 17
                            radius: 9
                            color: index === 0 ? "#ff6059" : "#ffbd2e"
                            border.width: 1
                            border.color: index === 0 ? "#e54842" : "#dfa323"
                        }
                        contentItem: Text {
                            text: parent.hovered ? (index === 0 ? "×" : "−") : ""
                            color: "#663c22"
                            font.pixelSize: 16
                            horizontalAlignment: Text.AlignHCenter
                            verticalAlignment: Text.AlignVCenter
                        }
                        onClicked: { if (index === 0) root.close(); else root.showMinimized() }
                    }
                }
            }
            Row {
                anchors.right: root.macChrome ? parent.right : undefined
                anchors.left: root.macChrome ? undefined : parent.left
                anchors.leftMargin: 12
                anchors.rightMargin: 12
                anchors.verticalCenter: parent.verticalCenter
                spacing: 10
                ToolButton {
                    text: "⋯"
                    font.pixelSize: 24
                    Accessible.name: qsTr("Audio settings")
                    ToolTip.visible: hovered
                    ToolTip.text: qsTr("Audio settings")
                    onClicked: audioPopup.open()
                }
                ToolButton {
                    icon.source: "qrc:/resources/icons/idescriptor-mirroring-home.svg"
                    icon.width: 23
                    icon.height: 23
                    icon.color: App.Theme.textMuted
                    enabled: backend.input_ready
                    Accessible.name: qsTr("Home")
                    ToolTip.visible: hovered
                    ToolTip.text: qsTr("Home · Ctrl+H")
                    onClicked: backend.home()
                }
                ToolButton {
                    icon.source: "qrc:/resources/icons/idescriptor-mirroring-switcher.svg"
                    icon.width: 26
                    icon.height: 26
                    icon.color: App.Theme.textMuted
                    enabled: backend.input_ready
                    Accessible.name: qsTr("App Switcher")
                    ToolTip.visible: hovered
                    ToolTip.text: qsTr("App Switcher · Ctrl+Tab")
                    onClicked: backend.app_switcher()
                }
            }
            Row {
                visible: !root.macChrome
                anchors.right: parent.right
                anchors.rightMargin: 8
                anchors.verticalCenter: parent.verticalCenter
                spacing: 4
                ToolButton {
                    width: 36; height: 36
                    text: "−"
                    font.pixelSize: 22
                    Accessible.name: qsTr("Minimize")
                    ToolTip.visible: hovered
                    ToolTip.text: qsTr("Minimize")
                    onClicked: root.showMinimized()
                }
                ToolButton {
                    width: 36; height: 36
                    text: "×"
                    font.pixelSize: 22
                    Accessible.name: qsTr("Close")
                    ToolTip.visible: hovered
                    ToolTip.text: qsTr("Close")
                    onClicked: root.close()
                }
            }
        }

        // Corner-only resizing leaves the bottom-center navigation gesture free.
        Repeater {
            model: [Qt.TopEdge | Qt.LeftEdge, Qt.TopEdge | Qt.RightEdge,
                    Qt.BottomEdge | Qt.LeftEdge, Qt.BottomEdge | Qt.RightEdge]
            delegate: MouseArea {
                required property int modelData
                readonly property bool leftEdge: (modelData & Qt.LeftEdge) !== 0
                readonly property bool topEdge: (modelData & Qt.TopEdge) !== 0
                width: 18
                height: 18
                x: leftEdge ? 0 : surface.width - width
                y: topEdge ? 0 : surface.height - height
                cursorShape: leftEdge === topEdge ? Qt.SizeFDiagCursor : Qt.SizeBDiagCursor
                onPressed: { backend.release_input(); root.startSystemResize(modelData) }
            }
        }
    }
    Popup {
        id: audioPopup
        parent: root.contentItem
        x: Math.max(8, root.width - width - 12)
        y: 56
        width: Math.min(310, root.width - 16)
        padding: 18
        modal: false
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        background: Rectangle {
            color: App.Theme.controlFill
            radius: 14
            border.width: 1
            border.color: App.Theme.controlStroke
        }
        contentItem: ColumnLayout {
            spacing: 12
            Label { text: qsTr("Audio"); font.bold: true; font.pixelSize: 17 }
            Switch {
                text: qsTr("Mute PC audio")
                checked: root.muted
                onToggled: { root.muted = checked; root.applyVolume() }
            }
            Slider {
                id: volumeSlider
                Layout.fillWidth: true
                from: 0; to: 1; value: 0.8
                Accessible.name: qsTr("PC playback volume")
                onMoved: root.applyVolume()
            }
            Switch {
                text: qsTr("Silence iPhone")
                enabled: backend.input_ready
                checked: root.phoneSilenced
                onToggled: {
                    root.phoneSilenced = checked
                    backend.silence_phone(checked)
                }
            }
            Switch {
                Layout.fillWidth: true
                text: qsTr("Enable iPhone audio after disconnect")
                checked: root.restorePhoneAudio
                onToggled: {
                    root.restorePhoneAudio = checked
                    backend.set_restore_phone_audio(checked)
                }
            }
            Label {
                Layout.fillWidth: true
                text: qsTr("Raises iPhone volume to maximum when mirroring ends, if the device is still reachable.")
                wrapMode: Text.WordWrap
                opacity: 0.65
            }
            Label {
                Layout.fillWidth: true
                text: qsTr("Turning this off raises iPhone volume to maximum. PC volume stays independent.")
                wrapMode: Text.WordWrap
                opacity: 0.65
            }
//             Button {
//                 text: qsTr("Capture frame diagnostics")
//                 enabled: backend.state === 2
//                 onClicked: root.captureDiagnostics()
//             }
//             Label {
//                 Layout.fillWidth: true
//                 text: root.diagnosticStatus
//                 visible: text.length > 0
//                 wrapMode: Text.WrapAnywhere
//             }
            Label {
                Layout.fillWidth: true
                visible: text.length > 0
                text: backend.input_error.length > 0 ? qsTr("Controls unavailable: %1").arg(backend.input_error)
                    : backend.audio_error.length > 0 ? qsTr("Audio unavailable: %1").arg(backend.audio_error) : ""
                wrapMode: Text.Wrap
            }
        }
    }
    Menu {
        id: contextMenu
        MenuItem { text: qsTr("Home"); enabled: backend.input_ready; onTriggered: backend.home() }
        MenuItem { text: qsTr("App Switcher"); enabled: backend.input_ready; onTriggered: backend.app_switcher() }
        MenuSeparator {}
        MenuItem { text: qsTr("Audio settings…"); onTriggered: audioPopup.open() }
        // MenuItem { text: qsTr("Capture frame diagnostics"); enabled: backend.state === 2; onTriggered: root.captureDiagnostics() }
        MenuItem { text: qsTr("Minimize"); onTriggered: root.showMinimized() }
        MenuItem { text: qsTr("Close"); onTriggered: root.close() }
    }
}
