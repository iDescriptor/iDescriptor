// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

pragma Singleton

import QtQuick

QtObject {
    id: root
    property var size_units : ["B","KB","MB","GB","TB"]

    function appDescriptionPreview(description) {
        const text = String(description || "").replace(/\s+/g, " ").trim()
        const characters = Array.from(text)
        return characters.length > 160
                ? characters.slice(0, 159).join("").trim() + "…"
                : text
    }

    function fetchAppIconFromApple(bundleId, cb) {
        if (!bundleId) { cb(""); return; }
        fetch_app(bundleId, function(app, error) {
            if (error || !app) { cb(""); return; }
            cb(app.artworkUrl100 || app.artworkUrl512 || app.artworkUrl60 || "");
        });
    }

    function fetch_app_name(bundleId,cb) {
        if (!bundleId) { cb(""); return; }
        fetch_app(bundleId, function(app, error) {
            if (error || !app) { cb(""); return; }
            cb(app.trackName || "");
        });
    }

    function fetch_app(bundleId, cb) {
        if (!bundleId) {
            cb(null, qsTr("Missing bundle ID."));
            return;
        }

        var xhr = new XMLHttpRequest();
        xhr.open("GET", "https://itunes.apple.com/lookup?bundleId=" + encodeURIComponent(bundleId));
        xhr.onreadystatechange = function() {
            if (xhr.readyState !== XMLHttpRequest.DONE) return;

            if (xhr.status !== 200) {
                cb(null, qsTr("Failed to fetch app details."));
                return;
            }

            try {
                var obj = JSON.parse(xhr.responseText);
                var results = obj && obj.results ? obj.results : [];
                if (!results.length) {
                    cb(null, qsTr("No App Store details found for this bundle ID."));
                    return;
                }
                cb(results[0], "");
            } catch (e) {
                cb(null, qsTr("Failed to parse App Store details."));
            }
        };
        xhr.send();
    }

    function is_video_file(name) {
        var lower = (name || "").toLowerCase()
        return lower.endsWith(".mp4") || lower.endsWith(".m4v") || lower.endsWith(".mov") ||
               lower.endsWith(".avi") || lower.endsWith(".mkv")
    }

    function is_image_file(name) {
        var lower = (name || "").toLowerCase()
        return lower.endsWith(".png") || lower.endsWith(".jpg") || lower.endsWith(".jpeg") ||
               lower.endsWith(".heic") || lower.endsWith(".heif") || lower.endsWith(".gif") ||
               lower.endsWith(".bmp") || lower.endsWith(".webp") || lower.endsWith(".tif") ||
               lower.endsWith(".tiff")
    }

    function is_previewable(name) {
        return is_video_file(name) || is_image_file(name)
    }


    function formatSize(_size) {
        let unit_index = 0;

        let size = _size;

        while(size >= 1024 && unit_index < 4) {
            size /= 1024;
            unit_index++;
        }

        return `${size.toFixed(2)} ${root.size_units[unit_index]}`;
    }

    function parseSemanticVersion(version) {
        const match = String(version || "").match(/^v?(\d+)\.(\d+)\.(\d+)/)
        if (!match)
            return null

        return [Number(match[1]), Number(match[2]), Number(match[3])]
    }

    function compareSemanticVersions(left, right) {
        for (let index = 0; index < 3; ++index) {
            if (left[index] !== right[index])
                return left[index] < right[index] ? -1 : 1
        }
        return 0
    }

    function versionMatchesConstraint(version, constraint) {
        const match = String(constraint || "").match(/^(>=|<=|>|<|=)?(\d+)\.(\d+)\.(\d+)$/)
        if (!match)
            return false

        const requiredVersion = [
            Number(match[2]),
            Number(match[3]),
            Number(match[4])
        ]
        const comparison = compareSemanticVersions(version, requiredVersion)

        switch (match[1] || "=") {
        case ">":
            return comparison > 0
        case ">=":
            return comparison >= 0
        case "<":
            return comparison < 0
        case "<=":
            return comparison <= 0
        default:
            return comparison === 0
        }
    }

    function pickMatchingVersionKey(obj) {
        const version = parseSemanticVersion(settingsManager.current_version())
        if (!version)
            return ""

        const keys = Object.keys(obj || {})
        for (let index = 0; index < keys.length; ++index) {
            if (versionMatchesConstraint(version, keys[index]))
                return keys[index]
        }
        return ""
    }

    // single-shot connection helper
    // since this does not exist in QML
    function connectOnce(signal, handler) {
        var wrapper = function() {
            handler.apply(this, arguments)
            signal.disconnect(wrapper)
        }
        signal.connect(wrapper)
    }


    function setTimeout(callback, delay) {
        var timer = Qt.createQmlObject(
            "import QtQuick; Timer { interval: " + delay + "; repeat: false; running: true; }",
            root,
            "dynamicTimer"
        );

        timer.triggered.connect(function() {
            callback();
            timer.destroy(); // Clean up memory after execution
        });
    }

    function messageBox(parent, title, message, buttons, onButtonClicked) {
        var dialog = Qt.createQmlObject(
            "import QtQuick; import QtQuick.Dialogs; MessageDialog { buttons: MessageDialog.Ok }",
            parent || root,
            "dynamicMessageBox"
        );

        dialog.title = title;
        dialog.text = message;
        if (buttons !== undefined)
            dialog.buttons = buttons;
        dialog.buttonClicked.connect(function(button, role) {
            if (onButtonClicked)
                onButtonClicked(button, role);
            dialog.destroy();
        });
        dialog.open();
        return dialog;
    }

    function showError(parent, message) {
        return messageBox(parent, qsTr("Error"), message);
    }

    function showWarning(parent, message) {
        return messageBox(parent, qsTr("Warning"), message);
    }

    function showInfo(parent, message) {
        return messageBox(parent, qsTr("Information"), message);
    }

    function showFileInfo(parentWindow, afcClient, filePath, displayName) {
        var component = Qt.createComponent("FileInfoDialog.qml")
        if (component.status !== Component.Ready) {
            console.error("Failed to create file information dialog:", component.errorString())
            return null
        }

        var dialog = component.createObject(parentWindow.contentItem, {
            "afcClient": afcClient,
            "filePath": filePath,
            "displayName": displayName
        })
        if (dialog === null) {
            console.error("Failed to instantiate file information dialog:", component.errorString())
            return null
        }

        dialog.closed.connect(function() {
            dialog.destroy()
        })
        dialog.open()
        dialog.load()
        return dialog
    }

    function showDeleteConfirmation(parentWindow, fileCount, folderCount, onConfirmed) {
        var component = Qt.createComponent("DeleteConfirmationDialog.qml")
        if (component.status !== Component.Ready) {
            console.error("Failed to create deletion confirmation dialog:", component.errorString())
            return null
        }

        var dialog = component.createObject(parentWindow.contentItem, {
            "fileCount": fileCount,
            "folderCount": folderCount
        })
        if (dialog === null) {
            console.error("Failed to instantiate deletion confirmation dialog:", component.errorString())
            return null
        }

        dialog.confirmed.connect(function() {
            if (onConfirmed)
                onConfirmed()
        })
        dialog.closed.connect(function() {
            dialog.destroy()
        })
        dialog.open()
        return dialog
    }


    function toFileUrl(path) {
        if (Qt.platform.os === "windows")
            return "file:///" + path.replace(/\\/g, "/")
        return "file://" + path
    }
}
