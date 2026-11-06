// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

use qmetaobject::qrc;

qrc!(pub qml,
    "/" {
        "src/ui/AlbumContents.qml",
        "src/ui/AppsTab.qml",
        "src/ui/AppSidebar.qml",
        "src/ui/BackupAction.qml",
        "src/ui/BackupEncryptionDialog.qml",
        "src/ui/BackupDetails.qml",
        "src/ui/BackupDetailsWithoutDevice.qml",
        "src/ui/ClosingHandler.qml",
        "src/ui/Constants.qml",
        "src/ui/Community.qml",
        "src/ui/BatteryIndicator.qml",
        "src/ui/CustomPairingDialog.qml",
        "src/ui/Device.qml",
        "src/ui/DeviceContext.qml",
        "src/ui/DeviceGallery.qml",
        "src/ui/DeviceImage.qml",
        "src/ui/DeviceInfo.qml",
        "src/ui/DeviceSidebarButton.qml",
        "src/ui/DeviceSectionTabs.qml",
        "src/ui/DeviceTab.qml",
        "src/ui/Diagnose.qml",
        "src/ui/Donate.qml",
        "src/ui/DonateActions.qml",
        "src/ui/DiskUsage.qml",
        "src/ui/DeleteConfirmationDialog.qml",
        "src/ui/FileExplorer.qml",
        "src/ui/FileExplorerSelection.qml",
        "src/ui/FileInfoDialog.qml",
        "src/ui/FilesSection.qml",
        "src/ui/Helpers.qml",
        "src/ui/HowToConnect.qml",
        "src/ui/IconLoader.qml",
        "src/ui/Jailbroken.qml",
        "src/ui/LoginDialog.qml",
        "src/ui/LocalNetworkPermissionDialog.qml",
        "src/ui/Main.qml",
        "src/ui/MainWorkspace.qml",
        "src/ui/NetworkDevicesToConnect.qml",
        "src/ui/PendingDevice.qml",
        "src/ui/PendingDeviceSidebar.qml",
        "src/ui/PreviewWindow.qml",
        "src/ui/RecoveryDeviceInfo.qml",
        "src/ui/RecoveryDeviceSidebar.qml",
        "src/ui/RestoreDialog.qml",
        "src/ui/RubberBandSelection.qml",
        "src/ui/Theme.qml",
        "src/ui/Settings.qml",
        "src/ui/Updater.qml",
        "src/ui/SidebarCollapsibleContent.qml",
        "src/ui/SidebarDestinationButton.qml",
        "src/ui/SidebarFooter.qml",
        "src/ui/SidebarSectionHeader.qml",
        "src/ui/SidebarToggleButton.qml",
        "src/ui/SponsorUsDialog.qml",
        "src/ui/StatusWindow.qml",
        "src/ui/StatusWindowProcess.qml",
        "src/ui/Toolbox.qml",
        "src/ui/WorkspaceStack.qml",
        "src/ui/Welcome.qml",
        "src/ui/WhatsNew.qml",
        "src/ui/DevModeHelper.qml",
        "src/ui/EraseDialog.qml",
        "src/ui/qmldir",

        // styles/IDescriptorStyle/
        "src/ui/styles/IDescriptorStyle/qmldir",
        "src/ui/styles/IDescriptorStyle/ScrollBar.qml",
        "src/ui/styles/IDescriptorStyle/ScrollView.qml",
        "src/ui/styles/IDescriptorStyle/Switch.qml",

        // base/
        "src/ui/base/AnimatedTab.qml",
        "src/ui/base/LocationSelector.qml",
        "src/ui/base/Spinner.qml",
        "src/ui/base/StateView.qml",
        "src/ui/base/ThemedSplitView.qml",
        "src/ui/base/ToolWindow.qml",
        "src/ui/base/DefaultWindow.qml",
        "src/ui/base/CopyableText.qml",
        "src/ui/base/IconToolButton.qml",
        "src/ui/base/PrivateText.qml",
        "src/ui/base/SectionBox.qml",
        "src/ui/base/AnimatedDialog.qml",

        // tools/
        "src/ui/tools/Airplay.qml",
        "src/ui/tools/CableInfo.qml",
        "src/ui/tools/QueryMobileGestalt.qml",
        "src/ui/tools/SimulateLocation.qml",
        "src/ui/tools/DevDiskImages.qml",
        "src/ui/tools/WirelessGalleryImport.qml",
        "src/ui/tools/WiredGalleryImport.qml",
        "src/ui/tools/IFuse.qml",
        "src/ui/tools/LiveScreen.qml",
        "src/ui/tools/ScreenMirroring.qml",
        "src/ui/tools/NetworkDevices.qml",
        "src/ui/tools/SSHTerminalTool.qml",
        "src/ui/tools/SSHProcessWindow.qml",
        "src/ui/tools/BackupManager.qml",
        "src/ui/tools/BatteryInfo.qml",
        "src/ui/tools/DisplayInfo.qml",
        "src/ui/tools/TransferSpeedTest.qml",

        // app-store/
        "src/ui/app-store/AppDetails.qml",
        "src/ui/app-store/GetIpaPopup.qml",
        "src/ui/app-store/InstallAppPopup.qml",
        "src/ui/app-store/AppItem.qml",
        "src/ui/app-store/SponsorItem.qml",
        "src/ui/app-store/SponsorUs.qml",

        // installed-apps/
        "src/ui/installed-apps/AppTab.qml",
        "src/ui/installed-apps/InstalledApps.qml",
    }
);

#[cfg(target_os = "linux")]
qrc!(pub linux_qml,
    "/" {
        "src/ui/ZLinuxWindow.qml",
        "src/ui/KeyringPassphraseDialog.qml",
    }
);

#[cfg(target_os = "windows")]
qrc!(pub windows_qml,
    "/" {
        "src/ui/platform/windows/Index.qml",
        "src/ui/platform/windows/Main.qml",
        "src/ui/platform/windows/WindowEffectPanel.qml",
        // "src/ui/+windows/DeviceSidebarButton.qml",
        "src/ui/base/+windows/DefaultWindow.qml",
        "src/ui/base/+windows/ToolWindow.qml",
        "src/ui/base/+windows/Spinner.qml",
        "src/ui/KeyringPassphraseDialog.qml",
    }
);

#[cfg(target_os = "macos")]
qrc!(pub macos_qml,
    "/" {
        "src/ui/KeychainDialog.qml",
        "src/ui/platform/macos/Main.qml"
    }
);
