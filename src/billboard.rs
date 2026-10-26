// USB Billboard devices (USB Billboard Device Class 1.2x).
//
// A Type-C device that wants an alternate mode, typically DisplayPort, and
// doesn't get it enumerates a Billboard over USB 2.0. Its BOS lists every
// mode the device offers and whether it was entered. The kernel exposes the
// BOS to everyone as `bos_descriptors`, so this works without root, which
// matters on laptops whose UCSI firmware reports no partner alt modes and no
// cable identity: the Billboard is then the only sign the mode failed.

use crate::types::*;
use std::fs;
use std::path::Path;

pub const USB_SYSFS: &str = "/sys/bus/usb/devices";

const DT_DEVICE_CAPABILITY: u8 = 0x10;
const CAP_BILLBOARD: u8 = 0x0d;
/// Fixed part of the Billboard capability, before the per-mode entries.
const BILLBOARD_HEADER_LEN: usize = 44;
/// Offset of `bmConfigured`, 2 bits per mode, mode 0 in the low bits.
const BM_CONFIGURED_OFFSET: usize = 8;
/// `bmConfigured` has room for this many modes.
const MAX_MODES: usize = 128;

/// Attach each Billboard on the USB bus to the Type-C port it came through.
/// A Billboard that can't be placed is dropped rather than guessed.
pub fn attach_billboards(ports: &mut [Port], usb_base: &Path) {
    for billboard in scan_billboards_at(usb_base) {
        let n = linked_port(usb_base, &billboard.device).or_else(|| sole_host_port(ports));
        if let Some(port) = n.and_then(|n| ports.iter_mut().find(|p| p.number == n)) {
            port.billboards.push(billboard);
        }
    }
}

/// Every USB device whose BOS carries a Billboard capability.
pub fn scan_billboards_at(usb_base: &Path) -> Vec<Billboard> {
    let Ok(entries) = fs::read_dir(usb_base) else {
        return Vec::new();
    };
    let mut billboards: Vec<Billboard> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            // Devices only: skip interfaces ("3-3:1.0") and root hubs ("usb3").
            if name.contains(':') || name.starts_with("usb") {
                return None;
            }
            let bos = fs::read(entry.path().join("bos_descriptors")).ok()?;
            let (preferred, modes) = parse_bos(&bos)?;
            Some(Billboard {
                device: name,
                preferred,
                modes,
            })
        })
        .collect();
    billboards.sort_by(|a, b| a.device.cmp(&b.device));
    billboards
}

/// The preferred mode index and the modes of the Billboard capability in a
/// raw BOS descriptor, or None when it has none.
pub fn parse_bos(bos: &[u8]) -> Option<(Option<usize>, Vec<BillboardMode>)> {
    let mut offset = *bos.first()? as usize;
    while offset + 3 <= bos.len() {
        let len = bos[offset] as usize;
        if len < 3 || offset + len > bos.len() {
            return None;
        }
        let cap = &bos[offset..offset + len];
        if cap[1] == DT_DEVICE_CAPABILITY && cap[2] == CAP_BILLBOARD {
            return parse_billboard_capability(cap);
        }
        offset += len;
    }
    None
}

fn parse_billboard_capability(cap: &[u8]) -> Option<(Option<usize>, Vec<BillboardMode>)> {
    if cap.len() < BILLBOARD_HEADER_LEN {
        return None;
    }
    let count = (cap[4] as usize).min(MAX_MODES);
    let mut modes = Vec::with_capacity(count);
    for i in 0..count {
        let entry = BILLBOARD_HEADER_LEN + 4 * i;
        let Some(bytes) = cap.get(entry..entry + 4) else {
            break;
        };
        let bits = (cap[BM_CONFIGURED_OFFSET + i / 4] >> ((i % 4) * 2)) & 0b11;
        modes.push(BillboardMode {
            svid: u16::from_le_bytes([bytes[0], bytes[1]]),
            mode: bytes[2],
            status: match bits {
                0b00 => AltModeStatus::Error,
                0b01 => AltModeStatus::NotAttempted,
                0b10 => AltModeStatus::Unsuccessful,
                _ => AltModeStatus::Entered,
            },
        });
    }
    let preferred = Some(cap[5] as usize).filter(|&p| p < modes.len());
    Some((preferred, modes))
}

/// The Type-C port a USB device hangs off, from the `connector` link the
/// kernel puts on the root hub port. Firmware without matching ACPI _PLD
/// data gets no link.
fn linked_port(usb_base: &Path, device: &str) -> Option<u32> {
    let (bus, path) = device.split_once('-')?;
    let root_port = path.split('.').next()?;
    let link = usb_base.join(format!("usb{bus}/{bus}-0:1.0/usb{bus}-port{root_port}/connector"));
    let target = fs::read_link(link).ok()?;
    target
        .file_name()?
        .to_string_lossy()
        .strip_prefix("port")?
        .parse()
        .ok()
}

/// Without a link, a Billboard can only have come through a connected port
/// where this computer is the USB host. If there is exactly one, it's that.
fn sole_host_port(ports: &[Port]) -> Option<u32> {
    let mut hosts = ports
        .iter()
        .filter(|p| p.is_connected() && p.data_role == DataRole::Host);
    let first = hosts.next()?;
    hosts.next().is_none().then_some(first.number)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sysfs;

    /// The BOS of the Realtek Billboard (0bda:54bb) in a USB-C monitor on a
    /// USB 2.0-only cable: DisplayPort, configuration not attempted.
    const REALTEK_MONITOR_BOS: [u8; 88] = [
        // BOS header: 4 capabilities, 0x58 bytes.
        0x05, 0x0f, 0x58, 0x00, 0x04,
        // USB 2.0 extension.
        0x07, 0x10, 0x02, 0x1e, 0xf4, 0x00, 0x00,
        // Container ID.
        0x14, 0x10, 0x04, 0x00, 0xaf, 0x4f, 0xac, 0x0d, 0x00, 0x87, 0x40, 0xee,
        0xb7, 0x58, 0x26, 0xa1, 0x07, 0xd7, 0x6c, 0x6b,
        // Billboard: 1 mode, preferred 0, VCONN 1 W, bmConfigured 01b.
        0x30, 0x10, 0x0d, 0x00, 0x01, 0x00, 0x00, 0x00,
        0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x21, 0x01, 0x00, 0x00,
        // wSVID 0xff01, bAlternateMode 1, no string.
        0x01, 0xff, 0x01, 0x00,
        // Billboard Alternate Mode capability for mode 0, VDO 0x000c0045.
        0x08, 0x10, 0x0f, 0x00, 0x45, 0x00, 0x0c, 0x00,
    ];

    /// Two modes, the second (TBT) entered, preferred index 1.
    fn two_mode_bos() -> Vec<u8> {
        let mut cap = vec![0u8; BILLBOARD_HEADER_LEN + 8];
        cap[0] = cap.len() as u8;
        cap[1] = DT_DEVICE_CAPABILITY;
        cap[2] = CAP_BILLBOARD;
        cap[4] = 2;
        cap[5] = 1;
        cap[BM_CONFIGURED_OFFSET] = 0b11_10;
        cap[44..48].copy_from_slice(&[0x01, 0xff, 0x01, 0x00]);
        cap[48..52].copy_from_slice(&[0x87, 0x80, 0x01, 0x00]);
        let mut bos = vec![0x05, 0x0f, 0, 0, 1];
        bos.extend(cap);
        let total = bos.len() as u16;
        bos[2..4].copy_from_slice(&total.to_le_bytes());
        bos
    }

    fn billboard(modes: Vec<BillboardMode>, preferred: Option<usize>) -> Billboard {
        Billboard {
            device: "3-3.5".into(),
            preferred,
            modes,
        }
    }

    #[test]
    fn test_parse_realtek_monitor() {
        let (preferred, modes) = parse_bos(&REALTEK_MONITOR_BOS).unwrap();
        assert_eq!(preferred, Some(0));
        assert_eq!(
            modes,
            vec![BillboardMode {
                svid: 0xff01,
                mode: 1,
                status: AltModeStatus::NotAttempted,
            }]
        );
        let b = billboard(modes, preferred);
        assert_eq!(b.failed_mode().map(|m| m.svid), Some(0xff01));
    }

    #[test]
    fn test_parse_two_modes() {
        let (preferred, modes) = parse_bos(&two_mode_bos()).unwrap();
        assert_eq!(preferred, Some(1));
        assert_eq!(modes.len(), 2);
        assert_eq!(modes[0].status, AltModeStatus::Unsuccessful);
        assert_eq!(modes[1].svid, 0x8087);
        assert_eq!(modes[1].status, AltModeStatus::Entered);
        // One mode is up, so the device is working: no failure to report.
        assert!(billboard(modes, preferred).failed_mode().is_none());
    }

    #[test]
    fn test_parse_bos_without_billboard() {
        // A hub's BOS: USB 2.0 extension and SuperSpeed capabilities only.
        let bos = [
            0x05, 0x0f, 0x16, 0x00, 0x02, 0x07, 0x10, 0x02, 0x06, 0x00, 0x00, 0x00, 0x0a, 0x10,
            0x03, 0x00, 0x0e, 0x00, 0x01, 0x0a, 0xff, 0x07,
        ];
        assert!(parse_bos(&bos).is_none());
        assert!(parse_bos(&[]).is_none());
    }

    #[test]
    fn test_parse_truncated() {
        // Cut into the mode entries: the capability length runs past the end.
        assert!(parse_bos(&REALTEK_MONITOR_BOS[..60]).is_none());
    }

    #[test]
    fn test_failed_mode_prefers_preferred() {
        let modes = vec![
            BillboardMode { svid: 0x1234, mode: 1, status: AltModeStatus::NotAttempted },
            BillboardMode { svid: 0xff01, mode: 1, status: AltModeStatus::Error },
        ];
        assert_eq!(billboard(modes.clone(), Some(1)).failed_mode().unwrap().svid, 0xff01);
        assert_eq!(billboard(modes, None).failed_mode().unwrap().svid, 0x1234);
    }

    fn fake_usb_device(usb: &Path, name: &str, bos: &[u8]) {
        let dir = usb.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("bos_descriptors"), bos).unwrap();
    }

    fn connected_host_port(typec: &Path, n: u32) {
        sysfs::create_fake_port(typec, n, "[host] device", "source [sink]");
        sysfs::write_fake_attr(typec, &format!("port{n}/port{n}-partner/supports_usb_power_delivery"), "yes\n");
    }

    #[test]
    fn test_attach_by_connector_link() {
        let dir = tempfile::tempdir().unwrap();
        let typec = dir.path().join("typec");
        let usb = dir.path().join("usb");
        connected_host_port(&typec, 0);
        connected_host_port(&typec, 1);
        fake_usb_device(&usb, "3-3.5", &REALTEK_MONITOR_BOS);
        fake_usb_device(&usb, "3-3", &[0x05, 0x0f, 0x05, 0x00, 0x00]);
        let root_port = usb.join("usb3/3-0:1.0/usb3-port3");
        fs::create_dir_all(&root_port).unwrap();
        std::os::unix::fs::symlink("../../../../typec/port1", root_port.join("connector")).unwrap();

        let mut ports = sysfs::scan_ports_at(&typec);
        attach_billboards(&mut ports, &usb);
        assert!(ports[0].billboards.is_empty());
        assert_eq!(ports[1].billboards.len(), 1);
        assert_eq!(ports[1].billboards[0].device, "3-3.5");
    }

    #[test]
    fn test_attach_to_sole_host_port() {
        let dir = tempfile::tempdir().unwrap();
        let typec = dir.path().join("typec");
        let usb = dir.path().join("usb");
        connected_host_port(&typec, 1);
        // A charger: connected, but this computer is the USB device.
        sysfs::create_fake_port(&typec, 0, "host [device]", "source [sink]");
        sysfs::write_fake_attr(&typec, "port0/port0-partner/supports_usb_power_delivery", "yes\n");
        sysfs::create_fake_port(&typec, 2, "[host] device", "[source] sink");
        fake_usb_device(&usb, "3-3.5", &REALTEK_MONITOR_BOS);

        let mut ports = sysfs::scan_ports_at(&typec);
        attach_billboards(&mut ports, &usb);
        assert!(ports[0].billboards.is_empty());
        assert_eq!(ports[1].billboards.len(), 1);
        assert!(ports[2].billboards.is_empty());
    }

    #[test]
    fn test_ambiguous_billboard_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let typec = dir.path().join("typec");
        let usb = dir.path().join("usb");
        connected_host_port(&typec, 0);
        connected_host_port(&typec, 1);
        fake_usb_device(&usb, "3-3.5", &REALTEK_MONITOR_BOS);

        let mut ports = sysfs::scan_ports_at(&typec);
        attach_billboards(&mut ports, &usb);
        assert!(ports.iter().all(|p| p.billboards.is_empty()));
    }
}
