# USB device permissions (UDEV rules) for Linux

This guide configures the USB permissions that iDescriptor needs to communicate with Apple devices in WTF, DFU, restore, recovery, and KIS modes on Linux.

iDescriptor's dependency check looks specifically for `/etc/udev/rules.d/99-idevice.rules`. An equivalent rule installed elsewhere can still grant device access, but the app will not detect it.

The Flatpak build skips this dependency check, but the host system still needs suitable UDEV permissions.

## Manual configuration

Run these commands from your normal user account.

### 1. Create the `idevice` group

Create the group if it does not already exist:

```sh
getent group idevice >/dev/null || sudo groupadd idevice
```

### 2. Add your user to the group

Add the current user without removing any existing supplementary groups:

```sh
sudo usermod --append --groups idevice "$USER"
```

### 3. Install the UDEV rule

The rule grants read/write access to the `idevice` group only for the Apple USB product IDs supported by iDescriptor's recovery-device library:

```sh
printf '%s\n' 'SUBSYSTEM=="usb", ATTR{idVendor}=="05ac", ATTR{idProduct}=="1222|1227|1280|1281|1881", MODE="0660", GROUP="idevice"' | sudo tee /etc/udev/rules.d/99-idevice.rules >/dev/null
```

This uses group-scoped mode `0660`; it does not make every Apple USB device writable by every local user.

### 4. Reload the rules

```sh
sudo udevadm control --reload-rules
sudo udevadm trigger
```

### 5. Refresh your login session

Log out and back in so your session receives the new group membership, then reconnect the device. You can confirm membership with:

```sh
id --groups --name
```

The output should include `idevice`. Re-run iDescriptor's dependency check after reconnecting the device.
