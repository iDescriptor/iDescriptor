// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl
import QtQuick.Layouts
import ".." as App

Item {
    id: root

    required property var tabs
    property int currentIndex: 0

    signal tabRequested(int index)

    implicitWidth: segmentedBackground.implicitWidth
    implicitHeight: 30

    function focusTab(sectionIndex) {
        const button = sectionRepeater.itemAt(sectionIndex)
        if (button)
            button.forceActiveFocus()
    }

    Rectangle {
        id: segmentedBackground

        anchors.centerIn: parent
        implicitWidth: sectionTrack.implicitWidth + 4
        width: implicitWidth
        height: 30
        radius: 7
        color: App.Theme.darkMode ? Qt.rgba(1, 1, 1, 0.055)
                                  : Qt.rgba(0, 0, 0, 0.045)
        border.color: App.Theme.controlStroke
        border.width: 1

        Item {
            id: sectionTrack

            anchors.centerIn: parent
            implicitWidth: sectionRow.implicitWidth
            implicitHeight: 26

            readonly property Item selectedButton:
                root.currentIndex >= 0
                && root.currentIndex < sectionRepeater.count
                    ? sectionRepeater.itemAt(root.currentIndex) : null

            onSelectedButtonChanged: {
                Qt.callLater(function() {
                    selectionIndicator.syncToCurrentButton(true)
                })
            }

            Rectangle {
                id: selectionIndicator

                visible: sectionTrack.selectedButton !== null
                x: 0
                anchors.verticalCenter: parent.verticalCenter
                width: 0
                height: 24
                radius: 5
                color: App.Theme.darkMode
                       ? Qt.rgba(1, 1, 1, 0.14)
                       : Qt.rgba(1, 1, 1, 0.96)
                border.color: App.Theme.controlStroke
                border.width: 1

                function syncToCurrentButton(animated) {
                    const button = sectionTrack.selectedButton
                    if (!button) {
                        slideAnimation.stop()
                        selectionIndicator.width = 0
                        return
                    }

                    if (!animated || selectionIndicator.width === 0) {
                        slideAnimation.stop()
                        selectionIndicator.x = button.x
                        selectionIndicator.width = button.width
                        return
                    }

                    xAnimation.to = button.x
                    widthAnimation.to = button.width
                    slideAnimation.restart()
                }

                Component.onCompleted: {
                    Qt.callLater(function() {
                        selectionIndicator.syncToCurrentButton(false)
                    })
                }

                ParallelAnimation {
                    id: slideAnimation

                    NumberAnimation {
                        id: xAnimation
                        target: selectionIndicator
                        property: "x"
                        duration: App.Theme.mediumAnimation
                        easing.type: Easing.OutCubic
                    }

                    NumberAnimation {
                        id: widthAnimation
                        target: selectionIndicator
                        property: "width"
                        duration: App.Theme.mediumAnimation
                        easing.type: Easing.OutCubic
                    }
                }

                Behavior on color {
                    ColorAnimation { duration: App.Theme.fastAnimation }
                }
            }

            Row {
                id: sectionRow

                anchors.fill: parent
                spacing: 0

                Repeater {
                    id: sectionRepeater
                    model: root.tabs

                    delegate: Button {
                        id: sectionButton

                        required property var modelData
                        required property int index

                        readonly property bool selected:
                            root.currentIndex === sectionButton.index

                        width: Math.max(72, contentRow.implicitWidth + 20)
                        onWidthChanged: Qt.callLater(function() { selectionIndicator.syncToCurrentButton(false) })
                        onXChanged: Qt.callLater(function() { selectionIndicator.syncToCurrentButton(false) })
                        height: 26
                        hoverEnabled: true
                        focusPolicy: Qt.StrongFocus
                        leftPadding: 10
                        rightPadding: 10
                        topPadding: 0
                        bottomPadding: 0
                        onClicked:
                            root.tabRequested(sectionButton.index)

                        Keys.onLeftPressed: function(event) {
                            const nextSection = (sectionButton.index
                                                 + root.tabs.length - 1)
                                                % root.tabs.length
                            root.tabRequested(nextSection)
                            root.focusTab(nextSection)
                            event.accepted = true
                        }

                        Keys.onRightPressed: function(event) {
                            const nextSection = (sectionButton.index + 1)
                                                % root.tabs.length
                            root.tabRequested(nextSection)
                            root.focusTab(nextSection)
                            event.accepted = true
                        }

                        background: Rectangle {
                            radius: 5
                            color: sectionButton.selected
                                   ? "transparent"
                                   : sectionButton.down
                                       ? App.Theme.pressed
                                       : sectionButton.hovered
                                           ? App.Theme.hover
                                           : "transparent"
                            border.color: sectionButton.activeFocus
                                          ? App.Theme.focus
                                          : "transparent"
                            border.width: 1

                            Behavior on color {
                                ColorAnimation {
                                    duration: App.Theme.fastAnimation
                                }
                            }

                            Behavior on border.color {
                                ColorAnimation {
                                    duration: App.Theme.fastAnimation
                                }
                            }
                        }

                        contentItem: RowLayout {
                            id: contentRow

                            spacing: 5

                            IconImage {
                                visible: source.toString().length > 0
                                source: sectionButton.modelData.iconSource || ""
                                color: sectionButton.selected
                                       ? App.Theme.text : App.Theme.icon
                                sourceSize.width: 15
                                sourceSize.height: 15
                                Layout.preferredWidth: 15
                                Layout.preferredHeight: 15

                                Behavior on color {
                                    ColorAnimation {
                                        duration: App.Theme.fastAnimation
                                    }
                                }
                            }

                            Label {
                                text: sectionButton.modelData.title
                                color: sectionButton.selected
                                       ? App.Theme.text
                                       : App.Theme.textMuted
                                font.pixelSize: 12
                                font.weight: sectionButton.selected
                                             ? Font.DemiBold : Font.Normal
                                verticalAlignment: Text.AlignVCenter

                                Behavior on color {
                                    ColorAnimation {
                                        duration: App.Theme.fastAnimation
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
