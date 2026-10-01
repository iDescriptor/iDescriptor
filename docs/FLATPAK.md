# Known Issues with the Flatpak Release

## No iFuse Support

The Flatpak version does not support iFuse for now. It's technically possible, but it requires escaping the sandbox to call `fusermount3` (see [fusermount3-wrapper](../packaging/linux/flatpak/idescriptor-fusermount3)).

Our Flatpak submission was denied because we used the `flatpak-spawn --host` option to escape the sandbox.

iFuse is a feature that lets you mount your device's filesystem. Without it, you can still use iDescriptor, but you won't be able to mount your device's filesystem.

If you need this feature, install iDescriptor from the AUR (Arch User Repository) or use the AppImage build instead.

## Device Is Not Detected or Hot Plug Not Working

If your device is not detected, or hot plug isn't working, follow these steps:

1. **Check that `usbmuxd` is installed.** iDescriptor needs it to listen for device events.

   This depends on your distribution — most have a package for `usbmuxd`, installable via your package manager. Some distributions (e.g. Arch Linux, Ubuntu, Debian) ship it by default.

2. **If `usbmuxd` is installed**, the issue is most likely that its socket gets shut down after the last device disconnects.

   You can patch the udev rule to prevent this (we plan to open a PR to fix this upstream in libimobiledevice).

   Locate your `39-usbmuxd.rules` file (usually in `/usr/lib/udev/rules.d/` or `/lib/udev/rules.d/`).

   **Example for Arch Linux:**
   ```bash
   sudo cp /usr/lib/udev/rules.d/39-usbmuxd.rules /usr/lib/udev/rules.d/39-usbmuxd.rules.bak
   sudo nano /usr/lib/udev/rules.d/39-usbmuxd.rules
   ```

   Comment out the last line:
   ```
   # Exit usbmuxd when the last device is removed
   #SUBSYSTEM=="usb", ENV{DEVTYPE}=="usb_device", ENV{PRODUCT}=="5ac/12[9a][0-9a-f]/*|5ac/190[1-5]/*|5ac/8600/*", ACTION=="remove", RUN+="/usr/bin/usbmuxd -x"
   ```

   > Note: the shipped rule file may contain a literal `@sbindir@` placeholder instead of a real path if your package wasn't built correctly. Replace it with the actual path to your `usbmuxd` binary (commonly `/usr/bin/usbmuxd` or `/usr/lib/usbmuxd/usbmuxd`; check with `which usbmuxd` or `command -v usbmuxd`).

   Reload your udev rules:
   ```bash
   sudo udevadm control --reload-rules
   ```

   Hot plug should now work as expected.
