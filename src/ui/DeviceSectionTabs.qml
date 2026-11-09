// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

pragma ComponentBehavior: Bound

import "base"

SegmentedTabs {
    id: root

    required property int currentSection
    currentIndex: currentSection
    signal sectionRequested(int sectionIndex)
    onTabRequested: index => root.sectionRequested(index)

    tabs: [
        {
            title: qsTr("Info"),
            iconSource: "qrc:/resources/icons/sidebar_info.svg"
        },
        {
            title: qsTr("Apps"),
            iconSource: "qrc:/resources/icons/sidebar_apps.svg"
        },
        {
            title: qsTr("Gallery"),
            iconSource: "qrc:/resources/icons/sidebar_gallery.svg"
        },
        {
            title: qsTr("Files"),
            iconSource: "qrc:/resources/icons/sidebar_files.svg"
        }
    ]

    function focusSection(sectionIndex) {
        focusTab(sectionIndex)
    }
}
