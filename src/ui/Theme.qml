// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

pragma Singleton

import QtQuick

QtObject {
    id: theme

    property string colorScheme: normalizeColorScheme(settingsManager.theme())
    readonly property bool darkMode: colorScheme === "dark"
                                     || (colorScheme === "system" && SystemAppearance.dark_mode)
    property string windowEffect: settingsManager.window_effect()

    function normalizeColorScheme(value) {
        const normalized = String(value || "system").trim().toLowerCase()
        if (normalized === "dark")
            return "dark"
        if (normalized === "light")
            return "light"
        return "system"
    }


    readonly property color accent: "#0a84ff"
    readonly property color accentPressed: "#006edb"
    readonly property color accentHover: "#006edb"
    readonly property color systemBlue: "#0a84ff"
    readonly property color systemGreen: darkMode ? "#30d158" : "#34c759"
    readonly property color systemOrange: darkMode ? "#ff9f0a" : "#ff9500"
    readonly property color systemRed: darkMode ? "#ff453a" : "#ff3b30"
    readonly property color text: darkMode ? "#f5f5f7" : "#1d1d1f"
    readonly property color textMuted: darkMode ? "#a1a1a6" : "#6e6e73"
    readonly property color textSelected: "#ffffff"
    readonly property color dangerText: darkMode ? "#ff6961" : "#d70015"
    readonly property color icon: darkMode ? "#d1d1d6" : "#3a3a3c"
    readonly property color iconSelected: "#ffffff"
    readonly property color selection: accent
    readonly property color hover: darkMode ? Qt.rgba(1, 1, 1, 0.08) : Qt.rgba(0, 0, 0, 0.055)
    readonly property color pressed: darkMode ? Qt.rgba(1, 1, 1, 0.12) : Qt.rgba(0, 0, 0, 0.085)
    readonly property color focus: Qt.rgba(10 / 255, 132 / 255, 255 / 255, darkMode ? 0.55 : 0.42)
    readonly property color controlFill: darkMode ? "#2c2c2e" : "#ffffff"
    readonly property color controlStroke: darkMode ? Qt.rgba(1, 1, 1, 0.13) : Qt.rgba(0, 0, 0, 0.1)
    readonly property color softBg: darkMode ? Qt.rgba(1, 1, 1, 0.04) : Qt.rgba(0, 0, 0, 0.04)
    readonly property color acrylicSurface: darkMode ? Qt.rgba(31 / 255, 31 / 255, 34 / 255, 0.72) : Qt.rgba(1, 1, 1, 0.72)
    readonly property color acrylicTabTextActive: Qt.rgba(1, 1, 1, 1)
    readonly property color acrylicTabTextInactive: Qt.rgba(1, 1, 1, 0.72)
    readonly property color softBgBorder: darkMode ? Qt.rgba(1, 1, 1, 0.15) : Qt.rgba(0, 0, 0, 0.15)
    readonly property color windowBackground: darkMode ? "#1f1f22" : "#f5f5f7"
    readonly property color windowBackgroundMacOS: darkMode ? Qt.rgba(0.122, 0.122, 0.133, 0.85) : Qt.rgba(0.961, 0.961, 0.969, 0.85)
    readonly property color windowBackgroundWindows: theme.windowEffect === "acrylic" ? theme.windowBackgroundMacOS : theme.windowBackground

    readonly property color groupedBackground: darkMode ? Qt.rgba(1, 1, 1, 0.06) : Qt.rgba(1, 1, 1, 0.74)
    readonly property color elevatedSurface: darkMode ? Qt.rgba(1, 1, 1, 0.08) : Qt.rgba(1, 1, 1, 0.88)
    readonly property color rowSurface: darkMode ? Qt.rgba(1, 1, 1, 0.035) : Qt.rgba(1, 1, 1, 0.62)
    readonly property color separator: darkMode ? Qt.rgba(1, 1, 1, 0.10) : Qt.rgba(0, 0, 0, 0.08)
    readonly property color selectionSoft: Qt.rgba(10 / 255, 132 / 255, 255 / 255, darkMode ? 0.22 : 0.16)
    readonly property color selectionHover: Qt.rgba(10 / 255, 132 / 255, 255 / 255, darkMode ? 0.14 : 0.10)
    readonly property color selectionStroke: Qt.rgba(10 / 255, 132 / 255, 255 / 255, darkMode ? 0.34 : 0.28)
    readonly property color sidebarSelection: {
        switch (Qt.platform.os) {
            case "osx":
                return theme.selectionSoft;
            case "windows":
                return theme.windowEffect === "normal" ? theme.selectionSoft : theme.pressed;
            default:
                return theme.selectionSoft;
        }
    }


    readonly property color sidebarBackground: darkMode ? "#1c1c1e" : "#f5f5f7"
    readonly property color sidebarBackgroundWindows: theme.windowEffect === "acrylic" ?  "transparent" : theme.sidebarBackground
    readonly property color sidebarDivider: darkMode ? Qt.rgba(1, 1, 1, 0.08) : Qt.rgba(0, 0, 0, 0.08)

    readonly property color diskUsageSystem: darkMode ? "#8e8e93" : "#636366"
    readonly property color diskUsageApps: accent
    readonly property color diskUsageMedia: darkMode ? "#5e5ce6" : "#5856d6"
    readonly property color diskUsageGallery: darkMode ? "#ff375f" : "#ff2d55"
    readonly property color diskUsageOthers: systemGreen
    readonly property color diskUsageFree: darkMode ? "#2c2c2e" : "#e5e5ea"

    readonly property Palette palette: Palette {
        window: theme.windowBackground
        windowText: theme.text
        base: theme.controlFill
        alternateBase: theme.groupedBackground
        text: theme.text
        button: theme.controlFill
        buttonText: theme.text
        brightText: theme.textSelected
        highlight: theme.selection
        highlightedText: theme.textSelected
        placeholderText: theme.textMuted
        accent: theme.accent
        link: theme.systemBlue
        linkVisited: theme.accentPressed
        toolTipBase: theme.elevatedSurface
        toolTipText: theme.text
        light: theme.darkMode ? theme.controlStroke : "#ffffff"
        midlight: theme.softBgBorder
        mid: theme.separator
        dark: theme.darkMode ? "#111113" : "#8e8e93"
        shadow: theme.darkMode ? "#000000" : Qt.rgba(0, 0, 0, 0.35)
    }

    readonly property int sidebarCornerRadius: 8
    readonly property int sidebarRowHeight: 36
    readonly property int sidebarHorizontalPadding: 12
    readonly property int sidebarIconSize: 18 * Screen.devicePixelRatio
    readonly property int fastAnimation: 160
    readonly property int mediumAnimation: 220
    readonly property int radius: 10
}
