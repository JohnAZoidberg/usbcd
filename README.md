# usbc

USB Type-C cable quality warning tool for Linux.

Reads the kernel's `/sys/class/typec/` sysfs interface to detect when a USB-C
cable is limiting your connection. A USB 2.0 cable plugged into a Thunderbolt
dock, a passive cable that can't carry USB4, a DisplayPort monitor that won't
light up because the cable lacks the right alt mode -- `usbc` catches these
and tells you.

The cable quality detection logic is ported from ChromeOS's
[typecd](https://chromium.googlesource.com/chromiumos/platform2/+/HEAD/typecd/)
daemon. ChromeOS shows these warnings as system notifications; standard Linux
desktops (GNOME, KDE, Sway) have no equivalent. This tool fills that gap.

## What it detects

| Warning | Meaning |
|---|---|
| Invalid DP cable | Partner supports DisplayPort Alt Mode but the cable doesn't |
| Invalid USB4 cable | Partner supports USB4 but the cable prevents USB4 and TBT entry |
| Invalid USB4, valid TBT cable | Partner supports USB4 but the cable limits you to Thunderbolt |
| Invalid TBT cable | Partner supports Thunderbolt but the cable prevents TBT entry |
| Speed limiting cable | Cable's max USB speed is lower than what the device supports |

## Usage

### One-shot status check

```
$ usbc status
Port 0 [host/source]
  Partner: hub (PD 3.1, USB4)
    Alt Modes: DP (0xff01), TBT (0x8087)
  Cable: passive (PD 3.0, USB 3.2 Gen2)
  Warnings:
    ⚠ Cable is limiting connection speed — device supports faster speeds than the cable allows

Port 1 [host/source]
  (nothing connected)
```

Exit code is 1 if any warnings were emitted, 0 if clean. Useful in scripts:

```sh
usbc status || echo "Check your USB-C cables"
```

### Background monitor with desktop notifications

```
$ usbc monitor
```

Watches for udev `typec` subsystem events. When a device is plugged in, it
waits for the kernel to finish enumeration (500 ms debounce), runs the quality
checks, and sends a freedesktop.org desktop notification if the cable is
suboptimal. Already-connected devices are scanned on startup.

Notifications are sent via D-Bus (`org.freedesktop.Notifications`) and work
with any compliant notification daemon (mako, dunst, GNOME Shell, KDE).

### Desktop app

```
$ usbc gui
```

Opens a window that explains each USB-C port in plain language: what's
plugged in (dock, display, charger, ...), whether it's charging the computer
and at how many watts, how fast the cable is, and what to do if the cable is
holding a device back. It updates live as you plug and unplug things.

A **Technical Details** section at the bottom shows the same data as
`usbc status -v`, with a button to copy it for a support request.

The GUI is an optional build feature (`--features gui`) because it pulls in
GTK 4 and libadwaita; the CLI and monitor don't need them.

To try the UI without USB-C hardware, point it at a fake sysfs tree:
`USBC_SYSFS_ROOT=/path/to/fake usbc gui` (this also works for `usbc status`).

## Building

### Nix (recommended)

```sh
nix build
# binary is at ./result/bin/usbc

# or run directly
nix run . -- status
```

With the desktop app:

```sh
nix build .#gui
nix run .#gui -- gui
```

For development:

```sh
nix develop
cargo build
cargo test
```

### Cargo (non-Nix)

Requires `pkg-config` and `libudev` development headers.

Debian/Ubuntu:
```sh
sudo apt install pkg-config libudev-dev
```

Fedora:
```sh
sudo dnf install pkg-config systemd-devel
```

Arch:
```sh
sudo pacman -S pkgconf systemd-libs
```

Then:
```sh
cargo build --release
# binary is at target/release/usbc
```

For the desktop app (`usbc gui`) you also need GTK 4 (4.12+) and libadwaita
(1.5+) development files:

```sh
sudo apt install libgtk-4-dev libadwaita-1-dev        # Debian/Ubuntu
sudo dnf install gtk4-devel libadwaita-devel          # Fedora
sudo pacman -S gtk4 libadwaita                        # Arch

cargo build --release --features gui
```

## Install

Copy the binary somewhere on your PATH:

```sh
cargo install --path .
# or
cp target/release/usbc ~/.local/bin/
```

To get the desktop app in your application launcher, also install the
desktop entry:

```sh
cp usbc.desktop ~/.local/share/applications/
```

## Running as a systemd user service

A service file is included for running the monitor automatically on login:

```sh
cp usbc-monitor.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now usbc-monitor
```

The service starts after `graphical-session.target` so that the D-Bus session
bus and a notification daemon are available.

Check logs with:
```sh
journalctl --user -u usbc-monitor
```

## How it works

1. **sysfs reader** -- Enumerates `/sys/class/typec/portN`, reads PD identity
   VDOs, PD revision, data/power roles, and alt modes for partners and cables.

2. **Quality checks** -- Ported from typecd's `cable.cc`, `partner.cc`,
   `port.cc`, and `port_manager.cc`. Evaluates whether the cable supports the
   partner's best mode (USB4 > TBT > DP) and whether it limits USB speed.

3. **udev monitor** -- Subscribes to the `typec` subsystem, debounces rapid
   event bursts from a single plug event, then re-scans and evaluates.

4. **Notifications** -- Sends desktop notifications via the standard
   `org.freedesktop.Notifications` D-Bus interface using zbus.

## Requirements

- Linux kernel with USB Type-C sysfs support (`/sys/class/typec/`)
- libudev (for the monitor mode)
- A D-Bus session bus and notification daemon (for desktop notifications)

Works on any Linux system with USB Type-C ports: Framework laptops, ThinkPads,
Dell XPS, etc.

## Troubleshooting

### `/sys/class/typec/` is empty

The directory exists but contains no `portN` entries. This usually means the
UCSI (USB Type-C Connector System Interface) driver failed to initialize.

Check the kernel log:

```
journalctl -k -g ucsi
```

If you see `ucsi_acpi: error -ETIMEDOUT: PPM init failed`, the driver timed
out talking to the EC firmware. Reloading the module usually fixes it:

```
sudo modprobe -r ucsi_acpi && sudo modprobe ucsi_acpi
```

After this, ports should appear in `/sys/class/typec/` and `usbc status` will
work.

This is a known issue on some laptops where the EC doesn't respond to the
initial UCSI probe in time (race condition at boot). If it happens
consistently, you can add a reload to a udev rule or systemd unit that runs
after boot. A BIOS/EC firmware update may also fix it permanently.
