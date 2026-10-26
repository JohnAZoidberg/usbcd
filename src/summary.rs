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
    /// Alt modes the cable plug answered for, e.g. "Thunderbolt". A cable
    /// can report these even when its e-marker identity is missing.
    pub cable_features: Vec<&'static str>,
    pub issues: Vec<Issue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceSummary {
    pub kind: DeviceKind,
    /// Short capability tags, e.g. "USB4", "Thunderbolt", "Video", "USB PD 3.0".
    pub features: Vec<&'static str>,
    pub power: PowerFlow,
    /// False when there is no Discover Identity answer, so `kind` is a guess
    /// from the power role and `features` can only name the PD version.
    pub identified: bool,
    /// Why there is no answer: this computer's port driver never asked. The
    /// kernel only exposes partner identity when the controller firmware
    /// relays PD messages, which much UCSI firmware doesn't. Nothing would
    /// ever be identified on such a port, so it isn't the device's fault.
    pub identity_unavailable: bool,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PowerFlow {
    /// The laptop is drawing power from this device.
    ///
    /// `max_milliwatts` is the most the device offers, if known. With a PD
    /// contract (`pd`) it comes from the partner's source capabilities. Without
    /// PD the partner only advertises a Type-C current at 5 V, which caps
    /// charging at 7.5 W or 15 W and is why a laptop charges slowly from it.
    Receiving {
        max_milliwatts: Option<u32>,
        pd: bool,
        /// What was actually negotiated, from the kernel's power supply for
        /// the port (selected PDO voltage times requested current).
        negotiated_milliwatts: Option<u32>,
        /// Every way the device offers to supply power (its source PDOs), in
        /// the order it listed them. Empty when it sent none, or when the
        /// kernel registered the contract before the partner device.
        offers: Vec<PowerOffer>,
    },
    /// A charger has a contract with the laptop but its power path is off,
    /// usually because the laptop is charging from another port. The
    /// contract is real (VBUS is at the negotiated voltage), the sink FET
    /// is just open. The kernel reports this as "Not charging", from the
    /// UCSI 2.0 sink path status bit.
    Standby {
        max_milliwatts: Option<u32>,
        negotiated_milliwatts: Option<u32>,
    },
    /// The laptop is powering this device.
    ///
    /// `max_milliwatts` is the most the port offers, from its own source
    /// capabilities. `negotiated_milliwatts` is what the kernel reports for
    /// the contract; under PD it has no values while sourcing.
    Supplying {
        max_milliwatts: Option<u32>,
        negotiated_milliwatts: Option<u32>,
    },
    Unknown,
}

/// One entry of a charger's source capabilities, in plain language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerOffer {
    /// Headline: "60 W", or "Adjustable voltage (PPS)" when the power
    /// depends on the voltage chosen.
    pub title: String,
    /// The electrical terms: "20 V · 3 A", "3.3–21 V · up to 3 A".
    pub detail: String,
    /// The contract in force selected this offer.
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CableSummary {
    /// The cable has an e-marker chip and reported its ratings.
    Identified {
        active: bool,
        speed: &'static str,
        max_watts: Option<u32>,
    },
    /// No e-marker identity, but the port driver knows whether the cable is
    /// active or passive from its own cable query.
    TypeOnly { active: bool },
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
        device: summarize_device(port),
        cable: summarize_cable(port.cable.as_ref()),
        cable_features: port
            .cable
            .as_ref()
            .map(cable_features)
            .unwrap_or_default(),
        issues: quality::check_cable_quality(port)
            .into_iter()
            .map(issue_for)
            .collect(),
    }
}

fn summarize_device(port: &Port) -> Option<DeviceSummary> {
    if let Some(partner) = &port.partner {
        return Some(summarize_partner(port, partner));
    }

    // No partner device, but the port's power supply says we are sinking:
    // the kernel knows about a contract before (or without) registering the
    // partner. power_operation_mode alone is not enough, it can be stale
    // after a detach.
    if !port.is_connected() {
        return None;
    }
    let kind = if port.effective_power_role() == PowerRole::Sink {
        DeviceKind::Charger
    } else {
        DeviceKind::Unknown
    };
    Some(DeviceSummary {
        kind,
        features: pd_feature(port, None).into_iter().collect(),
        power: power_flow(port, None),
        identified: false,
        identity_unavailable: false,
    })
}

fn power_flow(port: &Port, partner: Option<&Partner>) -> PowerFlow {
    match port.effective_power_role() {
        PowerRole::Sink => {
            let source_pdos: &[Pdo] = partner
                .and_then(|p| p.power.as_ref())
                .map(|p| p.source.as_slice())
                .unwrap_or(&[]);
            let pd_milliwatts = max_fixed_milliwatts(source_pdos);
            let contract = port.contract.as_ref().filter(|c| c.online);
            let pd = pd_milliwatts.is_some()
                || contract.is_some_and(|c| c.pd)
                || port.power_operation_mode == PowerOpMode::UsbPowerDelivery;
            let max_milliwatts = pd_milliwatts.or_else(|| match port.power_operation_mode {
                PowerOpMode::Current15A => Some(5000 * 1500 / 1000),
                PowerOpMode::Current30A => Some(5000 * 3000 / 1000),
                _ => None,
            });
            let negotiated = contract.filter(|c| c.has_values());
            let negotiated_milliwatts = negotiated.map(|c| c.milliwatts());
            // Only an explicit "Not charging" means the path is off: older
            // UCSI and other port drivers don't report it, so treat anything
            // else as charging.
            if contract.is_some_and(|c| c.status == "Not charging") {
                return PowerFlow::Standby {
                    max_milliwatts,
                    negotiated_milliwatts,
                };
            }
            PowerFlow::Receiving {
                max_milliwatts,
                pd,
                negotiated_milliwatts,
                offers: power_offers(source_pdos, negotiated),
            }
        }
        PowerRole::Source => PowerFlow::Supplying {
            max_milliwatts: port
                .power
                .as_ref()
                .and_then(|p| max_fixed_milliwatts(&p.source)),
            // The supply is never online while sourcing, so don't filter on it.
            negotiated_milliwatts: port
                .contract
                .as_ref()
                .filter(|c| c.has_values())
                .map(|c| c.milliwatts()),
        },
        PowerRole::None => PowerFlow::Unknown,
    }
}

/// "65 W", or "7.5 W" when it isn't a whole number.
pub fn format_watts(milliwatts: u32) -> String {
    if milliwatts % 1000 == 0 {
        format!("{} W", milliwatts / 1000)
    } else {
        format!("{:.1} W", milliwatts as f64 / 1000.0)
    }
}

/// "20", "3.3" or "3.25": as few decimals as the value needs.
fn format_milli(value: u32) -> String {
    if value % 1000 == 0 {
        format!("{}", value / 1000)
    } else if value % 100 == 0 {
        format!("{:.1}", value as f64 / 1000.0)
    } else {
        format!("{:.2}", value as f64 / 1000.0)
    }
}

fn format_volt_range(min_mv: u32, max_mv: u32) -> String {
    format!("{}–{} V", format_milli(min_mv), format_milli(max_mv))
}

fn milliwatts(voltage_mv: u32, current_ma: u32) -> u32 {
    ((voltage_mv as u64) * (current_ma as u64) / 1_000) as u32
}

/// The partner's source PDOs as offers, with the one the contract selected
/// marked. The kernel doesn't expose the RDO's object position, so the match
/// is by the contract's voltage: for a PPS contract the adjustable range
/// containing it, otherwise the fixed offer at exactly that voltage.
fn power_offers(pdos: &[Pdo], contract: Option<&Contract>) -> Vec<PowerOffer> {
    let active = contract.and_then(|c| active_offer(pdos, c));
    pdos.iter()
        .enumerate()
        .map(|(i, pdo)| {
            let (title, detail) = describe_pdo(pdo);
            PowerOffer {
                title,
                detail,
                active: active == Some(i),
            }
        })
        .collect()
}

fn active_offer(pdos: &[Pdo], c: &Contract) -> Option<usize> {
    let covers = |min: u32, max: u32| (min..=max).contains(&c.voltage_mv);
    if c.pps {
        return pdos
            .iter()
            .position(|p| match p {
                Pdo::ProgrammableSupply {
                    min_voltage_mv,
                    max_voltage_mv,
                    ..
                } => covers(*min_voltage_mv, *max_voltage_mv),
                _ => false,
            })
            .or_else(|| {
                pdos.iter()
                    .position(|p| matches!(p, Pdo::ProgrammableSupply { .. }))
            });
    }
    pdos.iter()
        .position(|p| matches!(p, Pdo::FixedSupply { voltage_mv, .. } if *voltage_mv == c.voltage_mv))
        // AVS runs 9 to 20 V in 100 mV steps; only considered when no fixed
        // offer sits at this voltage.
        .or_else(|| {
            pdos.iter()
                .position(|p| matches!(p, Pdo::SprAvs { .. }) && covers(9000, 20000))
        })
}

fn describe_pdo(pdo: &Pdo) -> (String, String) {
    match *pdo {
        Pdo::FixedSupply {
            voltage_mv,
            current_ma,
        } => (
            format_watts(milliwatts(voltage_mv, current_ma)),
            format!("{} V · {} A", format_milli(voltage_mv), format_milli(current_ma)),
        ),
        Pdo::ProgrammableSupply {
            min_voltage_mv,
            max_voltage_mv,
            max_current_ma,
        } => (
            "Adjustable voltage (PPS)".to_string(),
            format!(
                "{} · up to {} A",
                format_volt_range(min_voltage_mv, max_voltage_mv),
                format_milli(max_current_ma)
            ),
        ),
        Pdo::SprAvs {
            max_current_9v_to_15v_ma,
            max_current_15v_to_20v_ma,
        } => (
            "Adjustable voltage (AVS)".to_string(),
            format!(
                "9–20 V · up to {} A below 15 V, {} A above",
                format_milli(max_current_9v_to_15v_ma),
                format_milli(max_current_15v_to_20v_ma)
            ),
        ),
        Pdo::VariableSupply {
            min_voltage_mv,
            max_voltage_mv,
            current_ma,
        } => (
            "Variable voltage".to_string(),
            format!(
                "{} · {} A",
                format_volt_range(min_voltage_mv, max_voltage_mv),
                format_milli(current_ma)
            ),
        ),
        Pdo::Battery {
            min_voltage_mv,
            max_voltage_mv,
            power_mw,
        } => (
            format!("{} (battery)", format_watts(power_mw)),
            format_volt_range(min_voltage_mv, max_voltage_mv),
        ),
    }
}

fn summarize_partner(port: &Port, partner: &Partner) -> DeviceSummary {
    let power = power_flow(port, Some(partner));

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
    features.extend(pd_feature(port, Some(partner)));

    DeviceSummary {
        kind: device_kind(port, partner),
        features,
        power,
        identified: partner.identity.is_present(),
        identity_unavailable: !partner.identity_exposed,
    }
}

/// "USB PD 3.0", or "USB PD" when the partner talks PD but its revision is
/// unknown.
fn pd_feature(port: &Port, partner: Option<&Partner>) -> Option<&'static str> {
    match partner.map(|p| p.pd_revision) {
        Some(PdRevision::V2_0) => Some("USB PD 2.0"),
        Some(PdRevision::V3_0) => Some("USB PD 3.0"),
        Some(PdRevision::V3_1) => Some("USB PD 3.1"),
        Some(PdRevision::V3_2) => Some("USB PD 3.2"),
        _ if port.partner_supports_pd() == Some(true) => Some("USB PD"),
        _ => None,
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
        _ if port.effective_power_role() == PowerRole::Sink => DeviceKind::Charger,
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

fn max_fixed_milliwatts(pdos: &[Pdo]) -> Option<u32> {
    pdos.iter()
        .filter_map(|pdo| match pdo {
            Pdo::FixedSupply { voltage_mv, current_ma } => Some(milliwatts(*voltage_mv, *current_ma)),
            _ => None,
        })
        .max()
}

fn summarize_cable(cable: Option<&Cable>) -> CableSummary {
    let cable = match cable {
        Some(c) if c.identity.is_present() => c,
        Some(c) if c.cable_type != CableType::Unknown => {
            return CableSummary::TypeOnly {
                active: c.cable_type == CableType::Active,
            }
        }
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

fn cable_features(cable: &Cable) -> Vec<&'static str> {
    let mut features = Vec::new();
    if let Some(tbt) = cable.plug_alt_modes.iter().find(|m| m.svid == TBT_ALT_MODE_VID) {
        let tbt_mode = field(tbt.vdo, TBT3_CABLE_MODE_OFFSET, TBT3_CABLE_MODE_MASK);
        let speed = field(tbt.vdo, TBT3_CABLE_SPEED_OFFSET, TBT3_CABLE_SPEED_MASK);
        features.push(
            if tbt_mode == TBT3_CABLE_MODE_TBT && speed == TBT3_CABLE_SPEED_10G_20G {
                "Thunderbolt 40 Gbps"
            } else {
                "Thunderbolt"
            },
        );
    }
    if cable.plug_alt_modes.iter().any(|m| m.svid == DP_ALT_MODE_SID) {
        features.push("DisplayPort");
    }
    features
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
        CableWarning::AltModeNotEntered(DP_ALT_MODE_SID) => (
            "The display connection never started",
            "The device asked for video over USB-C, but it was never set up. \
             This usually means a charge-only or USB 2.0 cable: try a cable rated \
             for 10 Gbps or faster. Some ports can't drive a display, so another \
             port may also help.",
        ),
        CableWarning::AltModeNotEntered(TBT_ALT_MODE_VID) => (
            "Thunderbolt never started",
            "The device asked for Thunderbolt, but it was never set up. \
             Use a Thunderbolt 3, Thunderbolt 4, or USB4 cable, or try another port.",
        ),
        CableWarning::AltModeNotEntered(_) => (
            "A device feature never started",
            "The device asked for a special USB-C mode, but it was never set up. \
             Try a full-featured USB-C cable, or another port.",
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
            power_operation_mode: PowerOpMode::Default,
            partner,
            cable,
            ..Default::default()
        }
    }

    fn port_with_mode(power_role: PowerRole, mode: PowerOpMode) -> Port {
        Port {
            power_operation_mode: mode,
            ..port(power_role, None, None)
        }
    }

    /// A port whose power supply reports it sinking, as the kernel does for a
    /// charger, with or without a partner device.
    fn sinking_port(mode: PowerOpMode, pd: bool, voltage_mv: u32, current_ma: u32) -> Port {
        Port {
            power_operation_mode: mode,
            contract: Some(Contract {
                online: true,
                status: "Charging".into(),
                pd,
                voltage_mv,
                current_ma,
                ..Default::default()
            }),
            ..port(PowerRole::Sink, None, None)
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
            ..Default::default()
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
            ..Default::default()
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
    fn test_silent_charger_is_detected_from_power_supply() {
        // No partner directory, but the port's power supply is online with a
        // PD contract.
        let s = summarize(&sinking_port(PowerOpMode::UsbPowerDelivery, true, 20000, 3000));
        let device = s.device.as_ref().unwrap();
        assert_eq!(device.kind, DeviceKind::Charger);
        assert!(!device.identified);
        assert_eq!(device.features, vec!["USB PD"]);
        assert_eq!(
            device.power,
            PowerFlow::Receiving {
                max_milliwatts: None,
                pd: true,
                negotiated_milliwatts: Some(60_000),
                offers: vec![],
            }
        );
        assert_eq!(overall(&[s]), Overall::AllGood);
    }

    #[test]
    fn test_charger_with_sink_path_off_is_standby() {
        // Second charger while another port charges: the contract is live
        // but the EC told the PD controller to keep the sink FET open.
        let mut port = sinking_port(PowerOpMode::UsbPowerDelivery, true, 20000, 3000);
        port.contract.as_mut().unwrap().status = "Not charging".into();
        port.partner = Some(Partner {
            supports_pd: Some(true),
            ..Default::default()
        });
        let device = summarize(&port).device.unwrap();
        assert_eq!(device.kind, DeviceKind::Charger);
        assert_eq!(
            device.power,
            PowerFlow::Standby {
                max_milliwatts: None,
                negotiated_milliwatts: Some(60_000),
            }
        );
    }

    #[test]
    fn test_stale_power_mode_alone_is_nothing_connected() {
        // power_operation_mode still says PD after a detach, no partner, and
        // the power supply is offline.
        let mut port = port_with_mode(PowerRole::Sink, PowerOpMode::UsbPowerDelivery);
        port.contract = Some(Contract {
            online: false,
            status: "Not charging".into(),
            pd: true,
            ..Default::default()
        });
        let s = summarize(&port);
        assert!(s.device.is_none());
        assert_eq!(overall(&[s]), Overall::NothingConnected);

        // Without any power supply the mode alone is not trusted either.
        let s = summarize(&port_with_mode(PowerRole::Sink, PowerOpMode::UsbPowerDelivery));
        assert!(s.device.is_none());
    }

    #[test]
    fn test_silent_partner_while_sourcing_is_unknown() {
        let mut port = port_with_mode(PowerRole::Source, PowerOpMode::Current30A);
        port.partner = Some(Partner::default());
        let s = summarize(&port);
        let device = s.device.as_ref().unwrap();
        assert_eq!(device.kind, DeviceKind::Unknown);
        assert_eq!(
            device.power,
            PowerFlow::Supplying {
                max_milliwatts: None,
                negotiated_milliwatts: None,
            }
        );
    }

    #[test]
    fn test_supplying_reports_watts() {
        let mut port = port_with_mode(PowerRole::Source, PowerOpMode::Current15A);
        port.partner = Some(Partner::default());
        port.power = Some(PowerCapabilities {
            source: vec![Pdo::FixedSupply { voltage_mv: 5000, current_ma: 3000 }],
            sink: vec![],
        });
        port.contract = Some(Contract {
            online: false,
            status: "Discharging".into(),
            voltage_mv: 5000,
            current_ma: 1500,
            ..Default::default()
        });
        assert_eq!(
            summarize(&port).device.unwrap().power,
            PowerFlow::Supplying {
                max_milliwatts: Some(15_000),
                negotiated_milliwatts: Some(7_500),
            }
        );
    }

    #[test]
    fn test_online_supply_overrides_stale_source_role() {
        // UCSI left power_role at "source", but the port's supply is online:
        // a charger is powering the computer.
        let mut port = sinking_port(PowerOpMode::UsbPowerDelivery, true, 20000, 4700);
        port.power_role = PowerRole::Source;
        port.partner = Some(Partner {
            supports_pd: Some(true),
            ..Default::default()
        });
        let device = summarize(&port).device.unwrap();
        assert_eq!(device.kind, DeviceKind::Charger);
        assert_eq!(
            device.power,
            PowerFlow::Receiving {
                max_milliwatts: None,
                pd: true,
                negotiated_milliwatts: Some(94_000),
                offers: vec![],
            }
        );
    }

    #[test]
    fn test_legacy_current_is_reported_as_watts() {
        let s = summarize(&sinking_port(PowerOpMode::Current15A, false, 5000, 1500));
        assert_eq!(
            s.device.unwrap().power,
            PowerFlow::Receiving {
                max_milliwatts: Some(7_500),
                pd: false,
                negotiated_milliwatts: Some(7_500),
                offers: vec![],
            }
        );

        // Also when the partner did identify itself but offered no PDOs.
        let mut port = port(PowerRole::Sink, Some(partner(0x18000000, 0, vec![])), None);
        port.power_operation_mode = PowerOpMode::Current30A;
        assert_eq!(
            summarize(&port).device.unwrap().power,
            PowerFlow::Receiving {
                max_milliwatts: Some(15_000),
                pd: false,
                negotiated_milliwatts: None,
                offers: vec![],
            }
        );
    }

    #[test]
    fn test_cable_alt_modes_without_identity() {
        // Passive Thunderbolt 3 cable whose e-marker identity the kernel
        // didn't get, but whose SOP' alt modes it did.
        let cable = Cable {
            cable_type: CableType::Passive,
            plug_alt_modes: vec![
                AltMode { svid: TBT_ALT_MODE_VID, vdo: 0x00030001 },
                AltMode { svid: 0x05ac, vdo: 0x5 },
            ],
            ..Default::default()
        };
        let s = summarize(&port(PowerRole::Sink, Some(Partner::default()), Some(cable)));
        assert_eq!(s.cable, CableSummary::TypeOnly { active: false });
        assert_eq!(s.cable_features, vec!["Thunderbolt 40 Gbps"]);

        let dp_cable = Cable {
            plug_alt_modes: vec![AltMode { svid: DP_ALT_MODE_SID, vdo: 0 }],
            ..Default::default()
        };
        assert_eq!(cable_features(&dp_cable), vec!["DisplayPort"]);
    }

    #[test]
    fn test_cable_known_only_to_the_kernel() {
        let cable = Cable {
            cable_type: CableType::Passive,
            plug_type: Some("type-c".into()),
            ..Default::default()
        };
        assert_eq!(
            summarize_cable(Some(&cable)),
            CableSummary::TypeOnly { active: false }
        );
        assert_eq!(summarize_cable(Some(&Cable::default())), CableSummary::Unidentified);
    }

    #[test]
    fn test_format_watts() {
        assert_eq!(format_watts(65_000), "65 W");
        assert_eq!(format_watts(7_500), "7.5 W");
        assert_eq!(format_watts(15_000), "15 W");
    }

    #[test]
    fn test_idle_power_mode_is_nothing_connected() {
        for mode in [PowerOpMode::Default, PowerOpMode::None] {
            let s = summarize(&port_with_mode(PowerRole::Sink, mode));
            assert!(s.device.is_none());
        }
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
        assert!(device.identified);
        assert_eq!(device.features, vec!["USB4", "Thunderbolt", "Video", "USB PD 3.0"]);
        assert_eq!(
            device.power,
            PowerFlow::Receiving {
                max_milliwatts: None,
                pd: false,
                negotiated_milliwatts: None,
                offers: vec![],
            }
        );
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
        assert_eq!(
            device.power,
            PowerFlow::Receiving {
                max_milliwatts: Some(65_000),
                pd: true,
                negotiated_milliwatts: None,
                // No contract known, so nothing is marked in use.
                offers: vec![
                    offer("15 W", "5 V · 3 A", false),
                    offer("65 W", "20 V · 3.25 A", false),
                    offer("Adjustable voltage (PPS)", "3.3–21 V · up to 5 A", false),
                ],
            }
        );
        assert_eq!(s.cable, CableSummary::Unidentified);
    }

    fn offer(title: &str, detail: &str, active: bool) -> PowerOffer {
        PowerOffer {
            title: title.into(),
            detail: detail.into(),
            active,
        }
    }

    fn offers_of(port: &Port) -> Vec<PowerOffer> {
        match summarize(port).device.unwrap().power {
            PowerFlow::Receiving { offers, .. } => offers,
            other => panic!("not receiving: {other:?}"),
        }
    }

    #[test]
    fn test_charger_offers_mark_the_negotiated_one() {
        // A 60 W charger on a Framework Laptop 13: the UCSI firmware never
        // exposes the identity, but the PDOs and the contract are there.
        let charger = Partner {
            power: Some(PowerCapabilities {
                source: vec![
                    Pdo::FixedSupply { voltage_mv: 5000, current_ma: 3000 },
                    Pdo::FixedSupply { voltage_mv: 9000, current_ma: 3000 },
                    Pdo::FixedSupply { voltage_mv: 15000, current_ma: 3000 },
                    Pdo::FixedSupply { voltage_mv: 20000, current_ma: 3000 },
                    Pdo::ProgrammableSupply {
                        min_voltage_mv: 3300,
                        max_voltage_mv: 21000,
                        max_current_ma: 3000,
                    },
                ],
                sink: vec![],
            }),
            ..Default::default()
        };
        let mut port = sinking_port(PowerOpMode::UsbPowerDelivery, true, 20000, 3000);
        port.partner = Some(charger);

        let summary = summarize(&port);
        let device = summary.device.as_ref().unwrap();
        assert!(!device.identified);
        assert!(device.identity_unavailable);

        let offers = offers_of(&port);
        assert_eq!(offers.len(), 5);
        assert_eq!(offers[0], offer("15 W", "5 V · 3 A", false));
        assert_eq!(offers[3], offer("60 W", "20 V · 3 A", true));
        assert_eq!(
            offers[4],
            offer("Adjustable voltage (PPS)", "3.3–21 V · up to 3 A", false)
        );
        assert_eq!(offers.iter().filter(|o| o.active).count(), 1);

        // Renegotiated to 9 V: the mark follows the contract.
        port.contract.as_mut().unwrap().voltage_mv = 9000;
        let offers = offers_of(&port);
        assert!(offers[1].active);
        assert!(!offers[3].active);

        // A PPS contract at 11 V marks the adjustable range, not a fixed offer.
        let c = port.contract.as_mut().unwrap();
        c.pps = true;
        c.voltage_mv = 11000;
        c.current_ma = 2000;
        let offers = offers_of(&port);
        assert!(offers[4].active);
        assert_eq!(offers.iter().filter(|o| o.active).count(), 1);

        // Offline supply: the offers stay, nothing is in use.
        port.contract.as_mut().unwrap().online = false;
        assert!(offers_of(&port).iter().all(|o| !o.active));
    }

    #[test]
    fn test_identity_zero_but_exposed_is_the_devices_fault() {
        // The kernel asked (identity attributes exist) and got nothing back.
        let silent = Partner {
            identity_exposed: true,
            ..Default::default()
        };
        let s = summarize(&port(PowerRole::Source, Some(silent), None));
        let device = s.device.unwrap();
        assert!(!device.identified);
        assert!(!device.identity_unavailable);
    }

    #[test]
    fn test_describe_pdo() {
        assert_eq!(
            describe_pdo(&Pdo::FixedSupply { voltage_mv: 20000, current_ma: 2250 }),
            ("45 W".to_string(), "20 V · 2.25 A".to_string())
        );
        assert_eq!(
            describe_pdo(&Pdo::SprAvs {
                max_current_9v_to_15v_ma: 3000,
                max_current_15v_to_20v_ma: 2250,
            }),
            (
                "Adjustable voltage (AVS)".to_string(),
                "9–20 V · up to 3 A below 15 V, 2.25 A above".to_string()
            )
        );
        assert_eq!(
            describe_pdo(&Pdo::VariableSupply {
                min_voltage_mv: 5000,
                max_voltage_mv: 21000,
                current_ma: 900
            }),
            ("Variable voltage".to_string(), "5–21 V · 0.9 A".to_string())
        );
        assert_eq!(
            describe_pdo(&Pdo::Battery {
                min_voltage_mv: 5000,
                max_voltage_mv: 21000,
                power_mw: 60000
            }),
            ("60 W (battery)".to_string(), "5–21 V".to_string())
        );
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
        assert_eq!(
            s.device.as_ref().unwrap().power,
            PowerFlow::Supplying {
                max_milliwatts: None,
                negotiated_milliwatts: None,
            }
        );
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
        let device = s.device.unwrap();
        assert_eq!(device.kind, DeviceKind::Unknown);
        // Partner directory exists but the ID header is zero: still unidentified.
        assert!(!device.identified);
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
            ..Default::default()
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
