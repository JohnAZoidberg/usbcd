use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataRole {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerRole {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdRevision {
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

#[derive(Debug, Clone)]
pub struct AltMode {
    pub svid: u16,
    pub vdo: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CableType {
    Passive,
    Active,
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

#[derive(Debug, Clone)]
pub struct PdIdentity {
    pub id_header_vdo: u32,
    pub cert_stat_vdo: u32,
    pub product_vdo: u32,
    pub product_type_vdo1: u32,
    pub product_type_vdo2: u32,
    pub product_type_vdo3: u32,
}

impl Default for PdIdentity {
    fn default() -> Self {
        Self {
            id_header_vdo: 0,
            cert_stat_vdo: 0,
            product_vdo: 0,
            product_type_vdo1: 0,
            product_type_vdo2: 0,
            product_type_vdo3: 0,
        }
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
}

#[derive(Debug, Clone, Default)]
pub struct PowerCapabilities {
    pub source: Vec<Pdo>,
    pub sink: Vec<Pdo>,
}

#[derive(Debug, Clone)]
pub struct Partner {
    pub identity: PdIdentity,
    pub pd_revision: PdRevision,
    pub alt_modes: Vec<AltMode>,
    pub power: Option<PowerCapabilities>,
}

#[derive(Debug, Clone)]
pub struct Cable {
    pub identity: PdIdentity,
    pub pd_revision: PdRevision,
    pub cable_type: CableType,
    pub plug_alt_modes: Vec<AltMode>,
}

#[derive(Debug, Clone)]
pub struct Port {
    pub number: u32,
    pub data_role: DataRole,
    pub power_role: PowerRole,
    pub partner: Option<Partner>,
    pub cable: Option<Cable>,
    pub power: Option<PowerCapabilities>,
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
