// Plain-language description of a port, for people who don't know what a VDO is.
//
// The CLI shows the raw protocol view; this module answers the questions a
// normal user actually has: what is plugged in, is it charging the laptop, how
// fast is the cable, and is anything wrong.

use crate::quality;
use crate::types::*;
use crate::vdo::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortSummary {
    pub number: u32,
    /// None when nothing is plugged in.
    pub device: Option<DeviceSummary>,
    /// Only meaningful when a device is connected.
    pub cable: CableSummary,
    pub issues: Vec<Issue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceSummary {
    pub kind: DeviceKind,
    /// Short capability tags, e.g. "USB4", "Thunderbolt", "Video".
    pub features: Vec<&'static str>,
    pub power: PowerFlow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Dock,
    Display,
    VideoAdapter,
    UsbDevice,
    Computer,
    Charger,
    Unknown,
}

// Only the GUI renders these.
#[cfg_attr(not(feature = "gui"), allow(dead_code))]
impl DeviceKind {
    pub fn label(self) -> &'static str {
        match self {
            DeviceKind::Dock => "Dock or hub",
            DeviceKind::Display => "Display",
            DeviceKind::VideoAdapter => "Video adapter",
            DeviceKind::UsbDevice => "USB device",
            DeviceKind::Computer => "Computer or phone",
            DeviceKind::Charger => "Charger",
            DeviceKind::Unknown => "USB-C device",
        }
    }

    pub fn icon_name(self) -> &'static str {
        match self {
            DeviceKind::Dock => "thunderbolt-symbolic",
            DeviceKind::Display | DeviceKind::VideoAdapter => "video-display-symbolic",
            DeviceKind::UsbDevice | DeviceKind::Unknown => "media-removable-symbolic",
            DeviceKind::Computer => "computer-symbolic",
            DeviceKind::Charger => "ac-adapter-symbolic",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerFlow {
    /// The laptop is drawing power from this device. `max_watts` is the most
    /// the device offers, if it told us.
    Receiving { max_watts: Option<u32> },
    /// The laptop is powering this device.
    Supplying,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CableSummary {
    /// The cable has an e-marker chip and reported its ratings.
    Identified {
        active: bool,
        speed: &'static str,
        max_watts: Option<u32>,
    },
    /// No e-marker response: either a cable without one, or the port
    /// couldn't query it (SOP' discovery needs the VCONN source side).
    Unidentified,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub warning: CableWarning,
    pub title: &'static str,
    pub advice: &'static str,
}

/// Overall state across all ports, for the headline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overall {
    NoPorts,
    NothingConnected,
    AllGood,
    Issues(usize),
}

pub fn overall(summaries: &[PortSummary]) -> Overall {
    if summaries.is_empty() {
        return Overall::NoPorts;
    }
    let issues: usize = summaries.iter().map(|s| s.issues.len()).sum();
    if issues > 0 {
        Overall::Issues(issues)
    } else if summaries.iter().any(|s| s.device.is_some()) {
        Overall::AllGood
    } else {
        Overall::NothingConnected
    }
}

pub fn summarize(port: &Port) -> PortSummary {
    PortSummary {
        number: port.number,
        device: port.partner.as_ref().map(|p| summarize_device(port, p)),
        cable: summarize_cable(port.cable.as_ref()),
        issues: quality::check_cable_quality(port)
            .into_iter()
            .map(issue_for)
            .collect(),
    }
}

fn summarize_device(port: &Port, partner: &Partner) -> DeviceSummary {
    let power = match port.power_role {
        PowerRole::Sink => PowerFlow::Receiving {
            max_watts: partner
                .power
                .as_ref()
                .and_then(|p| max_fixed_watts(&p.source)),
        },
        PowerRole::Source => PowerFlow::Supplying,
        PowerRole::None => PowerFlow::Unknown,
    };

    let mut features = Vec::new();
    if supports_usb4(partner) {
        features.push("USB4");
    }
    if partner.alt_modes.iter().any(|m| m.svid == TBT_ALT_MODE_VID) {
        features.push("Thunderbolt");
    }
    if supports_dp(partner) {
        features.push("Video");
    }

    DeviceSummary {
        kind: device_kind(port, partner),
        features,
        power,
    }
}

fn device_kind(port: &Port, partner: &Partner) -> DeviceKind {
    let id = partner.identity.id_header_vdo;
    let ufp = field(id, ID_HEADER_VDO_PRODUCT_TYPE_OFFSET, ID_HEADER_VDO_PRODUCT_TYPE_MASK);
    let dfp = field(id, ID_HEADER_VDO_PRODUCT_TYPE_DFP_OFFSET, ID_HEADER_VDO_PRODUCT_TYPE_MASK);

    match ufp {
        ID_HEADER_VDO_PRODUCT_TYPE_UFP_HUB => return DeviceKind::Dock,
        ID_HEADER_VDO_PRODUCT_TYPE_UFP_AMA => return DeviceKind::VideoAdapter,
        ID_HEADER_VDO_PRODUCT_TYPE_UFP_PERIPHERAL if supports_dp(partner) => {
            return DeviceKind::Display
        }
        ID_HEADER_VDO_PRODUCT_TYPE_UFP_PERIPHERAL => return DeviceKind::UsbDevice,
        _ => {}
    }

    // DFP product types (Table 6-29): 2 = host, 3 = power brick.
    match dfp {
        2 => DeviceKind::Computer,
        3 => DeviceKind::Charger,
        _ if port.power_role == PowerRole::Sink => DeviceKind::Charger,
        _ => DeviceKind::Unknown,
    }
}

fn supports_dp(partner: &Partner) -> bool {
    partner
        .alt_modes
        .iter()
        .any(|m| m.svid == DP_ALT_MODE_SID && (m.vdo & DP_MODE_SNK) != 0)
}

fn supports_usb4(partner: &Partner) -> bool {
    let ufp = field(
        partner.identity.id_header_vdo,
        ID_HEADER_VDO_PRODUCT_TYPE_OFFSET,
        ID_HEADER_VDO_PRODUCT_TYPE_MASK,
    );
    if ufp != ID_HEADER_VDO_PRODUCT_TYPE_UFP_HUB && ufp != ID_HEADER_VDO_PRODUCT_TYPE_UFP_PERIPHERAL {
        return false;
    }
    let cap = field(
        partner.identity.product_type_vdo1,
        DEVICE_CAPABILITY_OFFSET,
        DEVICE_CAPABILITY_MASK,
    );
    (cap & DEVICE_CAPABILITY_USB4) != 0
}

fn max_fixed_watts(pdos: &[Pdo]) -> Option<u32> {
    pdos.iter()
        .filter_map(|pdo| match pdo {
            Pdo::FixedSupply { voltage_mv, current_ma } => {
                Some(((*voltage_mv as u64) * (*current_ma as u64) / 1_000_000) as u32)
            }
            _ => None,
        })
        .max()
}

fn summarize_cable(cable: Option<&Cable>) -> CableSummary {
    let cable = match cable {
        Some(c) if c.identity.id_header_vdo != 0 => c,
        _ => return CableSummary::Unidentified,
    };
    let vdo1 = cable.identity.product_type_vdo1;
    let active = cable.cable_type == CableType::Active;

    // Active cable VDO1 bit 4 set means VBUS is not carried through.
    let carries_power = !active || field(vdo1, 4, 0x1) == 0;
    let max_watts = if carries_power { cable_watts(vdo1) } else { None };

    CableSummary::Identified {
        active,
        speed: speed_label(vdo1 & USB_SPEED_MASK),
        max_watts,
    }
}

/// Power rating from the cable VDO1 VBUS current (bits 6:5) and max VBUS
/// voltage (bits 10:9) fields.
fn cable_watts(vdo1: u32) -> Option<u32> {
    let amps = match field(vdo1, 5, 0x3) {
        1 => 3,
        2 => 5,
        _ => return None,
    };
    // 50 V-rated 5 A cables are EPR cables, good for 48 V × 5 A.
    if amps == 5 && field(vdo1, 9, 0x3) == 3 {
        Some(240)
    } else {
        Some(amps * 20)
    }
}

/// USB-IF consumer-facing speed names for the cable VDO speed field.
pub fn speed_label(speed: u32) -> &'static str {
    match speed {
        USB_SPEED_20 => "USB 2.0 (480 Mbps)",
        USB_SUPER_SPEED_32_GEN1 => "5 Gbps",
        USB_SUPER_SPEED_32_OR_40_GEN2 => "10 Gbps",
        USB_40_SUPER_SPEED_GEN3 => "40 Gbps",
        USB_40_SUPER_SPEED_GEN4 => "80 Gbps",
        _ => "unknown speed",
    }
}

fn issue_for(warning: CableWarning) -> Issue {
    let (title, advice) = match warning {
        CableWarning::InvalidDpCable => (
            "This cable may not carry video",
            "The device can show a picture over USB-C, but this cable isn't built for it. \
             Try a cable rated for 10 Gbps or faster, or one marked for DisplayPort.",
        ),
        CableWarning::InvalidUsb4ValidTbtCable => (
            "Cable is holding this USB4 device back",
            "The connection is falling back to Thunderbolt 3. \
             A cable labelled USB4 or Thunderbolt 4 will unlock full speed.",
        ),
        CableWarning::InvalidUsb4Cable => (
            "This cable can't run the device at full speed",
            "The device supports USB4 but this cable doesn't. \
             Use a cable labelled USB4 (40 Gbps) or Thunderbolt 4.",
        ),
        CableWarning::InvalidTbtCable => (
            "This cable doesn't support Thunderbolt",
            "The device needs Thunderbolt for full functionality. \
             Use a Thunderbolt 3, Thunderbolt 4, or USB4 cable.",
        ),
        CableWarning::SpeedLimitingCable => (
            "Cable is slowing this device down",
            "The device supports faster data transfer than this cable. \
             A higher-rated cable will make it faster.",
        ),
    };
    Issue {
        warning,
        title,
        advice,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NEKTECK_USB2: (u32, u32) = (0x18002e98, 0xc1082040);
    const HP_USB32_GEN1: (u32, u32) = (0x180003f0, 0x11084851);
    const OWC_DOCK: (u32, u32) = (0x4cc01e91, 0xd00003b);

    fn port(power_role: PowerRole, partner: Option<Partner>, cable: Option<Cable>) -> Port {
        Port {
            number: 0,
            data_role: DataRole::Host,
            power_role,
            partner,
            cable,
            power: None,
        }
    }

    fn partner(id_header: u32, vdo1: u32, alt_modes: Vec<AltMode>) -> Partner {
        Partner {
            identity: PdIdentity {
                id_header_vdo: id_header,
                product_type_vdo1: vdo1,
                ..Default::default()
            },
            pd_revision: PdRevision::V3_0,
            alt_modes,
            power: None,
        }
    }

    fn passive_cable((id_header, vdo1): (u32, u32)) -> Cable {
        Cable {
            identity: PdIdentity {
                id_header_vdo: id_header,
                product_type_vdo1: vdo1,
                ..Default::default()
            },
            pd_revision: PdRevision::V3_0,
            cable_type: CableType::Passive,
            plug_alt_modes: vec![],
        }
    }

    fn owc_dock() -> Partner {
        partner(
            OWC_DOCK.0,
            OWC_DOCK.1,
            vec![
                AltMode { svid: DP_ALT_MODE_SID, vdo: 0x1c0045 },
                AltMode { svid: TBT_ALT_MODE_VID, vdo: 0x1 },
            ],
        )
    }

    #[test]
    fn test_nothing_connected() {
        let s = summarize(&port(PowerRole::Source, None, None));
        assert!(s.device.is_none());
        assert!(s.issues.is_empty());
        assert_eq!(overall(&[s]), Overall::NothingConnected);
    }

    #[test]
    fn test_no_ports() {
        assert_eq!(overall(&[]), Overall::NoPorts);
    }

    #[test]
    fn test_dock_with_usb2_cable_has_issue() {
        let s = summarize(&port(
            PowerRole::Sink,
            Some(owc_dock()),
            Some(passive_cable(NEKTECK_USB2)),
        ));
        let device = s.device.as_ref().unwrap();
        assert_eq!(device.kind, DeviceKind::Dock);
        assert_eq!(device.features, vec!["USB4", "Thunderbolt", "Video"]);
        assert_eq!(device.power, PowerFlow::Receiving { max_watts: None });
        assert_eq!(
            s.cable,
            CableSummary::Identified {
                active: false,
                speed: "USB 2.0 (480 Mbps)",
                max_watts: Some(100),
            }
        );
        assert_eq!(s.issues.len(), 1);
        assert_eq!(s.issues[0].warning, CableWarning::InvalidUsb4Cable);
        assert_eq!(overall(&[s]), Overall::Issues(1));
    }

    #[test]
    fn test_charger_watts() {
        let mut charger = partner(0x18000000, 0, vec![]);
        charger.power = Some(PowerCapabilities {
            source: vec![
                Pdo::FixedSupply { voltage_mv: 5000, current_ma: 3000 },
                Pdo::FixedSupply { voltage_mv: 20000, current_ma: 3250 },
                Pdo::ProgrammableSupply {
                    min_voltage_mv: 3300,
                    max_voltage_mv: 21000,
                    max_current_ma: 5000,
                },
            ],
            sink: vec![],
        });
        let s = summarize(&port(PowerRole::Sink, Some(charger), None));
        let device = s.device.unwrap();
        assert_eq!(device.kind, DeviceKind::Charger);
        assert_eq!(device.power, PowerFlow::Receiving { max_watts: Some(65) });
        assert_eq!(s.cable, CableSummary::Unidentified);
    }

    #[test]
    fn test_peripheral_with_dp_is_display() {
        // UFP product type = peripheral, USB capable as device.
        let display = partner(
            0x500004b4,
            0x8,
            vec![AltMode { svid: DP_ALT_MODE_SID, vdo: 0x405 }],
        );
        let s = summarize(&port(
            PowerRole::Source,
            Some(display),
            Some(passive_cable(HP_USB32_GEN1)),
        ));
        assert_eq!(s.device.as_ref().unwrap().kind, DeviceKind::Display);
        assert_eq!(s.device.as_ref().unwrap().power, PowerFlow::Supplying);
        assert!(s.issues.is_empty());
        assert_eq!(overall(&[s]), Overall::AllGood);
    }

    #[test]
    fn test_ama_is_video_adapter() {
        let ama = partner(
            0x6c0004b4,
            0x8,
            vec![AltMode { svid: DP_ALT_MODE_SID, vdo: 0x405 }],
        );
        let s = summarize(&port(PowerRole::Source, Some(ama), None));
        assert_eq!(s.device.unwrap().kind, DeviceKind::VideoAdapter);
    }

    #[test]
    fn test_unidentified_partner_is_unknown_when_supplying() {
        let s = summarize(&port(PowerRole::Source, Some(partner(0, 0, vec![])), None));
        assert_eq!(s.device.unwrap().kind, DeviceKind::Unknown);
    }

    #[test]
    fn test_cable_watts() {
        // 3 A, 20 V
        assert_eq!(cable_watts(1 << 5), Some(60));
        // 5 A, 20 V
        assert_eq!(cable_watts(2 << 5), Some(100));
        // 5 A, 50 V (EPR)
        assert_eq!(cable_watts((2 << 5) | (3 << 9)), Some(240));
        assert_eq!(cable_watts(0), None);
    }

    #[test]
    fn test_active_cable_without_vbus_has_no_power_rating() {
        let cable = Cable {
            identity: PdIdentity {
                id_header_vdo: 0x240020c2,
                // 5 A, VBUS not through cable (bit 4)
                product_type_vdo1: (2 << 5) | (1 << 4) | 0x3,
                ..Default::default()
            },
            pd_revision: PdRevision::V3_0,
            cable_type: CableType::Active,
            plug_alt_modes: vec![],
        };
        assert_eq!(
            summarize_cable(Some(&cable)),
            CableSummary::Identified {
                active: true,
                speed: "40 Gbps",
                max_watts: None,
            }
        );
    }
}
