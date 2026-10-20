use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DataRole {
    #[default]
    None,
    Device,
    Host,
}

impl fmt::Display for DataRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DataRole::None => write!(f, "none"),
            DataRole::Device => write!(f, "device"),
            DataRole::Host => write!(f, "host"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PowerRole {
    #[default]
    None,
    Sink,
    Source,
}

impl fmt::Display for PowerRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PowerRole::None => write!(f, "none"),
            PowerRole::Sink => write!(f, "sink"),
            PowerRole::Source => write!(f, "source"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PdRevision {
    #[default]
    None,
    V2_0,
    V3_0,
    V3_1,
    V3_2,
}

impl fmt::Display for PdRevision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PdRevision::None => write!(f, "none"),
            PdRevision::V2_0 => write!(f, "PD 2.0"),
            PdRevision::V3_0 => write!(f, "PD 3.0"),
            PdRevision::V3_1 => write!(f, "PD 3.1"),
            PdRevision::V3_2 => write!(f, "PD 3.2"),
        }
    }
}

/// CC orientation of the connector, from `portN/orientation`. The kernel only
/// exposes the file when the port driver knows it (UCSI 2.0 and later).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    /// CC1.
    Normal,
    /// CC2.
    Reverse,
    Unknown,
}

impl fmt::Display for Orientation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Orientation::Normal => write!(f, "normal"),
            Orientation::Reverse => write!(f, "reverse"),
            Orientation::Unknown => write!(f, "unknown"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AltMode {
    pub svid: u16,
    pub vdo: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CableType {
    Passive,
    Active,
    #[default]
    Unknown,
}

impl fmt::Display for CableType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CableType::Passive => write!(f, "passive"),
            CableType::Active => write!(f, "active"),
            CableType::Unknown => write!(f, "unknown"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PdIdentity {
    pub id_header_vdo: u32,
    pub cert_stat_vdo: u32,
    pub product_vdo: u32,
    pub product_type_vdo1: u32,
    pub product_type_vdo2: u32,
    pub product_type_vdo3: u32,
}

impl PdIdentity {
    /// False when Discover Identity never produced an answer (all VDOs zero).
    pub fn is_present(&self) -> bool {
        self.id_header_vdo != 0
    }
}

#[derive(Debug, Clone)]
pub enum Pdo {
    FixedSupply {
        voltage_mv: u32,
        current_ma: u32,
    },
    VariableSupply {
        min_voltage_mv: u32,
        max_voltage_mv: u32,
        current_ma: u32,
    },
    Battery {
        min_voltage_mv: u32,
        max_voltage_mv: u32,
        power_mw: u32,
    },
    ProgrammableSupply {
        min_voltage_mv: u32,
        max_voltage_mv: u32,
        max_current_ma: u32,
    },
    /// SPR Adjustable Voltage Supply (PD 3.2): 9 to 20 V in 100 mV steps,
    /// with a current limit per voltage range.
    SprAvs {
        max_current_9v_to_15v_ma: u32,
        max_current_15v_to_20v_ma: u32,
    },
}

#[derive(Debug, Clone, Default)]
pub struct PowerCapabilities {
    pub source: Vec<Pdo>,
    pub sink: Vec<Pdo>,
}

/// The power the port is actually exchanging, from the kernel's
/// `*-source-psy-*` power supply next to the port. The kernel derives these
/// from the RDO and the partner's source PDOs while sinking, and from the
/// Type-C current advertisement otherwise. Nothing here is measured.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Contract {
    /// The port is sinking from a connected partner.
    pub online: bool,
    /// "Charging", "Discharging", "Not charging" or "Unknown".
    pub status: String,
    /// A PD contract is in force (`usb_type` is `PD` or `PD_PPS`).
    pub pd: bool,
    /// The active PD contract is a PPS APDO.
    pub pps: bool,
    pub voltage_mv: u32,
    pub current_ma: u32,
    pub current_max_ma: u32,
    pub voltage_min_mv: u32,
    pub voltage_max_mv: u32,
}

impl Contract {
    pub fn milliwatts(&self) -> u32 {
        ((self.voltage_mv as u64) * (self.current_ma as u64) / 1000) as u32
    }

    /// Voltage and current are known (not the all-zero placeholder the kernel
    /// gives while sourcing under PD, where it has no partner source PDOs to
    /// resolve the RDO against).
    pub fn has_values(&self) -> bool {
        self.voltage_mv > 0 && self.current_ma > 0
    }
}

#[derive(Debug, Clone, Default)]
pub struct Partner {
    pub identity: PdIdentity,
    pub pd_revision: PdRevision,
    pub alt_modes: Vec<AltMode>,
    pub power: Option<PowerCapabilities>,
    /// `supports_usb_power_delivery`, when the kernel exposes it.
    pub supports_pd: Option<bool>,
    /// `number_of_alternate_modes`; hidden by the kernel until discovery ran.
    pub num_alt_modes: Option<u32>,
    /// `accessory_mode`, e.g. "none", "analog_audio", "debug".
    pub accessory_mode: Option<String>,
    /// The kernel exposes `identity` attributes for this partner, so an
    /// all-zero identity means the partner didn't answer Discover Identity.
    /// False when the port driver never asks: UCSI only relays PD messages
    /// when the controller firmware supports GET_PD_MESSAGE, and much laptop
    /// firmware doesn't. Then no partner is ever identified on this port.
    pub identity_exposed: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Cable {
    pub identity: PdIdentity,
    pub pd_revision: PdRevision,
    /// From the e-marker identity when present, else from the kernel's
    /// `type` attribute, which the port driver fills from its own cable query.
    pub cable_type: CableType,
    pub plug_alt_modes: Vec<AltMode>,
    /// `plug_type`: "type-a", "type-b", "type-c", "captive" or "unknown".
    pub plug_type: Option<String>,
}

/// Active power operation mode for a Type-C port, as reported by the kernel
/// in `/sys/class/typec/portN/power_operation_mode`.
///
/// Treat it as a label, not as proof of a connection: the kernel only updates
/// it when the port driver reports a power-mode change, and some UCSI
/// firmware doesn't on detach, so it can read `usb_power_delivery` long after
/// the charger is gone. Use `Port::is_connected()` instead.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PowerOpMode {
    /// No contract — default USB current (500 mA / 900 mA).
    #[default]
    Default,
    /// Legacy Type-C current advertisement (no PD).
    Current15A,
    Current30A,
    /// USB PD contract active.
    UsbPowerDelivery,
    Other(String),
    None,
}

impl fmt::Display for PowerOpMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PowerOpMode::Default => write!(f, "default"),
            PowerOpMode::Current15A => write!(f, "1.5A"),
            PowerOpMode::Current30A => write!(f, "3.0A"),
            PowerOpMode::UsbPowerDelivery => write!(f, "PD"),
            PowerOpMode::Other(s) => write!(f, "{s}"),
            PowerOpMode::None => write!(f, "unknown"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Port {
    pub number: u32,
    pub data_role: DataRole,
    pub power_role: PowerRole,
    pub power_operation_mode: PowerOpMode,
    pub partner: Option<Partner>,
    pub cable: Option<Cable>,
    pub power: Option<PowerCapabilities>,
    /// None when the kernel doesn't expose `orientation` for this port.
    pub orientation: Option<Orientation>,
    /// USB modes the port supports (`usb_capability`), and the one in
    /// brackets that it uses by default.
    pub usb_capability: Vec<String>,
    pub usb_capability_default: Option<String>,
    pub vconn_source: Option<bool>,
    pub contract: Option<Contract>,
}

impl Port {
    /// Something is attached. The partner device is the primary signal; the
    /// power supply's `online` covers a sink contract the kernel knows about
    /// before the partner device appears. `power_operation_mode` alone is not
    /// trusted, see `PowerOpMode`.
    pub fn is_connected(&self) -> bool {
        self.partner.is_some() || self.contract.as_ref().is_some_and(|c| c.online)
    }

    /// The power role, corrected by the power supply. The typec `power_role`
    /// attribute only changes when the port driver reports a role change,
    /// and some UCSI firmware leaves it at `source` while a charger is
    /// powering the computer. The power supply's `online` is read from the
    /// live connector status and is only set while sinking, so it wins. The
    /// converse doesn't hold: a sink can be offline (tcpm while charging is
    /// disabled), so an offline supply doesn't make the port a source.
    pub fn effective_power_role(&self) -> PowerRole {
        if self.contract.as_ref().is_some_and(|c| c.online) {
            PowerRole::Sink
        } else {
            self.power_role
        }
    }

    /// Whether the partner talks PD. The partner's `supports_usb_power_delivery`
    /// is set once at registration, which can happen during the Type-C
    /// current phase before the PD contract, so a live PD contract wins.
    pub fn partner_supports_pd(&self) -> Option<bool> {
        if self.is_connected() && self.contract.as_ref().is_some_and(|c| c.pd) {
            return Some(true);
        }
        let partner = self.partner.as_ref()?;
        if partner.pd_revision != PdRevision::None || partner.power.is_some() {
            return Some(true);
        }
        partner.supports_pd
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CableWarning {
    /// Partner supports DisplayPort Alt Mode, but the cable does not.
    InvalidDpCable,
    /// Partner supports USB4, but the cable restricts the connection to TBT.
    InvalidUsb4ValidTbtCable,
    /// Partner supports USB4, but the cable prevents USB4 and TBT mode entry.
    InvalidUsb4Cable,
    /// Partner supports TBT, but the cable prevents TBT mode entry.
    InvalidTbtCable,
    /// USB speed is limited by the cable.
    SpeedLimitingCable,
}

impl CableWarning {
    /// Stable identifier for machine-readable output.
    pub fn id(&self) -> &'static str {
        match self {
            CableWarning::InvalidDpCable => "invalid_dp_cable",
            CableWarning::InvalidUsb4ValidTbtCable => "invalid_usb4_valid_tbt_cable",
            CableWarning::InvalidUsb4Cable => "invalid_usb4_cable",
            CableWarning::InvalidTbtCable => "invalid_tbt_cable",
            CableWarning::SpeedLimitingCable => "speed_limiting_cable",
        }
    }
}

impl fmt::Display for CableWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CableWarning::InvalidDpCable => write!(
                f,
                "Cable may not support display output — try a DisplayPort-rated USB-C cable"
            ),
            CableWarning::InvalidUsb4ValidTbtCable => write!(
                f,
                "USB4 device connected but cable only supports Thunderbolt — use a USB4 cable for full speed"
            ),
            CableWarning::InvalidUsb4Cable => write!(
                f,
                "USB4 device connected but cable is incompatible — use a USB4 cable"
            ),
            CableWarning::InvalidTbtCable => write!(
                f,
                "Thunderbolt device connected but cable is incompatible — use a Thunderbolt cable"
            ),
            CableWarning::SpeedLimitingCable => write!(
                f,
                "Cable is limiting connection speed — device supports faster speeds than the cable allows"
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModeEntryResult {
    Success,
    CableError,
    PartnerError,
}
