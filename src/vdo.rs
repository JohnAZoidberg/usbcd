// PD VDO bit-field constants, ported from typecd/pd_vdo_constants.h

// USB PD spec rev 3.0, v 2.0.
// Table 6-29 ID Header VDO.
pub const ID_HEADER_VDO_MODAL_OPERATION: u32 = 1 << 26;
pub const ID_HEADER_VDO_PRODUCT_TYPE_OFFSET: u32 = 27;
pub const ID_HEADER_VDO_PRODUCT_TYPE_DFP_OFFSET: u32 = 23;
pub const ID_HEADER_VDO_PRODUCT_TYPE_MASK: u32 = 0x7;
pub const ID_HEADER_VDO_PRODUCT_TYPE_CABLE_PASSIVE: u32 = 0x3;
pub const ID_HEADER_VDO_PRODUCT_TYPE_CABLE_ACTIVE: u32 = 0x4;
pub const ID_HEADER_VDO_PRODUCT_TYPE_UFP_HUB: u32 = 0x1;
pub const ID_HEADER_VDO_PRODUCT_TYPE_UFP_PERIPHERAL: u32 = 0x2;
pub const ID_HEADER_VDO_PRODUCT_TYPE_UFP_AMA: u32 = 0x5;
pub const ID_HEADER_VDO_PRODUCT_TYPE_POWER_BRICK: u32 = 0x3;

// USB PD spec rev 3.1, v 1.8.
// Table 6-29 ID Header VDO - Connector type.
pub const ID_HEADER_VDO_CONNECTOR_TYPE_OFFSET: u32 = 21;
pub const ID_HEADER_VDO_CONNECTOR_TYPE_MASK: u32 = 0x3;
pub const ID_HEADER_VDO_CONNECTOR_TYPE_RECEPTACLE: u32 = 0x2;
pub const ID_HEADER_VDO_CONNECTOR_TYPE_PLUG: u32 = 0x3;

// Table 6-35 UFP VDO 1
pub const DEVICE_CAPABILITY_OFFSET: u32 = 24;
pub const DEVICE_CAPABILITY_MASK: u32 = 0xF;
pub const DEVICE_CAPABILITY_BILLBOARD: u32 = 0x2;
pub const DEVICE_CAPABILITY_USB4: u32 = 0x8;

// USB speed fields
pub const USB_SPEED_MASK: u32 = 0x7;
pub const USB_SPEED_20: u32 = 0x0;
pub const USB_SUPER_SPEED_32_GEN1: u32 = 0x1;
pub const USB_SUPER_SPEED_32_OR_40_GEN2: u32 = 0x2;
pub const USB_40_SUPER_SPEED_GEN3: u32 = 0x3;
pub const USB_40_SUPER_SPEED_GEN4: u32 = 0x4;

// Speed values for PD rev 2.0
pub const USB_SUPER_SPEED_31_GEN1: u32 = 0x1;
pub const USB_SUPER_SPEED_31_GEN2: u32 = 0x2;

// Active Cable VDO1
pub const ACTIVE_CABLE_VDO1_VERSION_OFFSET: u32 = 21;
pub const ACTIVE_CABLE_VDO1_VERSION_MASK: u32 = 0x7;
pub const ACTIVE_CABLE_VDO1_VERSION_13: u32 = 0x3;

// Active Cable VDO2
pub const ACTIVE_CABLE_VDO2_USB4_SUPPORTED: u32 = 1 << 8;

// TBT3 Cable Discover Mode VDO
pub const TBT3_CABLE_ROUNDED_SUPPORT_OFFSET: u32 = 19;
pub const TBT3_CABLE_ROUNDED_SUPPORT_MASK: u32 = 0x3;
pub const TBT3_CABLE_ROUNDED_3_4_GEN: u32 = 0x1;
pub const TBT3_CABLE_SPEED_OFFSET: u32 = 16;
pub const TBT3_CABLE_SPEED_MASK: u32 = 0x7;
pub const TBT3_CABLE_SPEED_10G_20G: u32 = 0x3;
pub const TBT3_CABLE_MODE_OFFSET: u32 = 0;
pub const TBT3_CABLE_MODE_MASK: u32 = 0xffff;
pub const TBT3_CABLE_MODE_TBT: u32 = 0x1;

// Alt Mode SVIDs
pub const DP_ALT_MODE_SID: u16 = 0xff01;
pub const DP_MODE_SNK: u32 = 0x1;
pub const DP_MODE_RECEPTACLE: u32 = 0x40;

pub const TBT_ALT_MODE_VID: u16 = 0x8087;

// AMA VDO
pub const AMA_VDO_USB_SPEED_MASK: u32 = 0x7;
pub const AMA_VDO_USB_SPEED_BILLBOARD: u32 = 0x3;

// Cable VDO1 plug type
pub const CABLE_VDO1_PLUG_TYPE_OFFSET: u32 = 18;
pub const CABLE_VDO1_PLUG_TYPE_MASK: u32 = 0x3;
pub const CABLE_VDO1_PLUG_TYPE_CAPTIVE: u32 = 0x3;

// Helper to extract a bit field from a VDO
pub fn field(vdo: u32, offset: u32, mask: u32) -> u32 {
    (vdo >> offset) & mask
}

pub fn usb_speed_name(speed: u32, pd_rev: super::types::PdRevision) -> &'static str {
    use super::types::PdRevision;
    match (speed, pd_rev) {
        (USB_SPEED_20, _) => "USB 2.0",
        (USB_SUPER_SPEED_31_GEN1, PdRevision::V2_0) => "USB 3.1 Gen1",
        (USB_SUPER_SPEED_31_GEN2, PdRevision::V2_0) => "USB 3.1 Gen1x2",
        (USB_SUPER_SPEED_32_GEN1, _) => "USB 3.2 Gen1",
        (USB_SUPER_SPEED_32_OR_40_GEN2, _) => "USB 3.2 Gen2",
        (USB_40_SUPER_SPEED_GEN3, _) => "USB4 Gen3",
        (USB_40_SUPER_SPEED_GEN4, _) => "USB4 Gen4",
        _ => "Unknown",
    }
}

// --- Verbose VDO field decoding ---

/// A single bit-field definition within a VDO.
pub struct VdoField {
    pub name: &'static str,
    pub offset: u32,
    pub mask: u32,
    /// Value-to-description map. If empty, the raw value is printed.
    pub values: &'static [(u32, &'static str)],
}

/// Decode and write all fields of a VDO, indented under a header.
pub fn write_vdo_fields(out: &mut String, vdo: u32, fields: &[VdoField], indent: &str) {
    use std::fmt::Write;
    for f in fields {
        let val = field(vdo, f.offset, f.mask);
        let desc = f.values.iter().find(|(v, _)| *v == val).map(|(_, d)| *d);
        let _ = match desc {
            Some(d) => writeln!(out, "{indent}  {}: {d}", f.name),
            None => {
                if f.mask <= 0xf {
                    writeln!(out, "{indent}  {}: {val}", f.name)
                } else {
                    writeln!(out, "{indent}  {}: {val:#x}", f.name)
                }
            }
        };
    }
}

// --- Field tables ---

const USB_SPEED_VALUES: &[(u32, &str)] = &[
    (0, "USB 2.0"),
    (1, "USB 3.2 Gen1"),
    (2, "USB 3.2 Gen2"),
    (3, "USB4 Gen3"),
    (4, "USB4 Gen4"),
];

const CONNECTOR_TYPE_VALUES: &[(u32, &str)] = &[
    (0, "reserved"),
    (1, "reserved"),
    (2, "USB Type-C receptacle"),
    (3, "USB Type-C plug"),
];

const YESNO_VALUES: &[(u32, &str)] = &[
    (0, "no"),
    (1, "yes"),
];

const UFP_PRODUCT_TYPE_VALUES: &[(u32, &str)] = &[
    (0, "not UFP"),
    (1, "hub"),
    (2, "peripheral"),
    (3, "power brick"),
    (5, "alt mode adapter"),
];

const DFP_PRODUCT_TYPE_VALUES: &[(u32, &str)] = &[
    (0, "not DFP"),
    (1, "hub"),
    (2, "host"),
    (3, "power brick"),
];

const CABLE_PRODUCT_TYPE_VALUES: &[(u32, &str)] = &[
    (0, "not cable"),
    (3, "passive cable"),
    (4, "active cable"),
];

pub const ID_HEADER_VDO_FIELDS: &[VdoField] = &[
    VdoField { name: "USB Vendor ID", offset: 0, mask: 0xFFFF, values: &[] },
    VdoField { name: "Connector Type", offset: 21, mask: 0x3, values: CONNECTOR_TYPE_VALUES },
    VdoField { name: "Product Type (DFP)", offset: 23, mask: 0x7, values: DFP_PRODUCT_TYPE_VALUES },
    VdoField { name: "Modal Operation", offset: 26, mask: 0x1, values: YESNO_VALUES },
    VdoField { name: "Product Type (UFP)", offset: 27, mask: 0x7, values: UFP_PRODUCT_TYPE_VALUES },
    VdoField { name: "USB Capable as Device", offset: 30, mask: 0x1, values: YESNO_VALUES },
    VdoField { name: "USB Capable as Host", offset: 31, mask: 0x1, values: YESNO_VALUES },
];

const CABLE_VBUS_CURRENT_VALUES: &[(u32, &str)] = &[
    (0, "reserved"),
    (1, "3A"),
    (2, "5A"),
    (3, "reserved"),
];

const CABLE_MAX_VBUS_VOLTAGE_VALUES: &[(u32, &str)] = &[
    (0, "20V"),
    (1, "30V"),
    (2, "40V"),
    (3, "50V"),
];

const CABLE_TERMINATION_VALUES: &[(u32, &str)] = &[
    (0, "VCONN not required"),
    (1, "VCONN not required"),
    (2, "one end active, one end passive, VCONN required"),
    (3, "both ends active, VCONN required"),
];

const CABLE_LATENCY_VALUES: &[(u32, &str)] = &[
    (0, "reserved"),
    (1, "<10ns (~1m)"),
    (2, "10-20ns (~2m)"),
    (3, "20-30ns (~3m)"),
    (4, "30-40ns (~4m)"),
    (5, "40-50ns (~5m)"),
    (6, "50-60ns (~6m)"),
    (7, "60-70ns (~7m)"),
    (8, ">70ns (>~7m)"),
    (9, "1000ns+"),
    (10, "2000ns+"),
    (11, "3000ns+"),
];

pub const PASSIVE_CABLE_VDO1_FIELDS: &[VdoField] = &[
    VdoField { name: "USB Highest Speed", offset: 0, mask: 0x7, values: USB_SPEED_VALUES },
    VdoField { name: "Vbus Current Handling", offset: 5, mask: 0x3, values: CABLE_VBUS_CURRENT_VALUES },
    VdoField { name: "Max Vbus Voltage", offset: 9, mask: 0x3, values: CABLE_MAX_VBUS_VOLTAGE_VALUES },
    VdoField { name: "Cable Termination Type", offset: 11, mask: 0x3, values: CABLE_TERMINATION_VALUES },
    VdoField { name: "Cable Latency", offset: 13, mask: 0xF, values: CABLE_LATENCY_VALUES },
    VdoField { name: "Connector Type", offset: 18, mask: 0x3, values: CONNECTOR_TYPE_VALUES },
];

const ACTIVE_CABLE_SBU_TYPE_VALUES: &[(u32, &str)] = &[
    (0, "SBU is passive"),
    (1, "SBU is active"),
];

const ACTIVE_CABLE_SBU_SUPPORT_VALUES: &[(u32, &str)] = &[
    (0, "SBU connections supported"),
    (1, "SBU connections not supported"),
];

const ACTIVE_CABLE_VBUS_THROUGH_VALUES: &[(u32, &str)] = &[
    (0, "yes"),
    (1, "no"),
];

const ACTIVE_CABLE_SOP_CONTROLLER_VALUES: &[(u32, &str)] = &[
    (0, "SOP'' controller not present"),
    (1, "SOP'' controller present"),
];

const ACTIVE_CABLE_VDO_VERSION_VALUES: &[(u32, &str)] = &[
    (0, "version 1.0"),
    (1, "reserved"),
    (2, "reserved"),
    (3, "version 1.3"),
];

pub const ACTIVE_CABLE_VDO1_FIELDS: &[VdoField] = &[
    VdoField { name: "USB Highest Speed", offset: 0, mask: 0x7, values: USB_SPEED_VALUES },
    VdoField { name: "SOP'' Controller Present", offset: 3, mask: 0x1, values: ACTIVE_CABLE_SOP_CONTROLLER_VALUES },
    VdoField { name: "Vbus Through Cable", offset: 4, mask: 0x1, values: ACTIVE_CABLE_VBUS_THROUGH_VALUES },
    VdoField { name: "Vbus Current Handling", offset: 5, mask: 0x3, values: CABLE_VBUS_CURRENT_VALUES },
    VdoField { name: "SBU Type", offset: 7, mask: 0x1, values: ACTIVE_CABLE_SBU_TYPE_VALUES },
    VdoField { name: "SBU Supported", offset: 8, mask: 0x1, values: ACTIVE_CABLE_SBU_SUPPORT_VALUES },
    VdoField { name: "Max Vbus Voltage", offset: 9, mask: 0x3, values: CABLE_MAX_VBUS_VOLTAGE_VALUES },
    VdoField { name: "Cable Termination Type", offset: 11, mask: 0x3, values: CABLE_TERMINATION_VALUES },
    VdoField { name: "Cable Latency", offset: 13, mask: 0xF, values: CABLE_LATENCY_VALUES },
    VdoField { name: "Connector Type", offset: 18, mask: 0x3, values: CONNECTOR_TYPE_VALUES },
    VdoField { name: "VDO Version", offset: 21, mask: 0x7, values: ACTIVE_CABLE_VDO_VERSION_VALUES },
];

const ACTIVE_CABLE_VDO2_ELEMENT_VALUES: &[(u32, &str)] = &[
    (0, "redriver"),
    (1, "retimer"),
];

const ACTIVE_CABLE_VDO2_CONNECTION_VALUES: &[(u32, &str)] = &[
    (0, "copper"),
    (1, "optical"),
];

pub const ACTIVE_CABLE_VDO2_FIELDS: &[VdoField] = &[
    VdoField { name: "USB4 Supported", offset: 8, mask: 0x1, values: YESNO_VALUES },
    VdoField { name: "Active Element", offset: 9, mask: 0x1, values: ACTIVE_CABLE_VDO2_ELEMENT_VALUES },
    VdoField { name: "Physical Connection", offset: 10, mask: 0x1, values: ACTIVE_CABLE_VDO2_CONNECTION_VALUES },
    VdoField { name: "USB4 Gen2 Supported", offset: 11, mask: 0x1, values: YESNO_VALUES },
    VdoField { name: "USB 3.2 Supported", offset: 12, mask: 0x1, values: YESNO_VALUES },
];

const UFP_ALT_MODE_VALUES: &[(u32, &str)] = &[
    (0, "no alternate modes"),
    (1, "supports TBT3 alternate mode"),
    (2, "supports alternate modes per USB Type-C spec"),
    (3, "supports TBT3 and alternate modes per USB Type-C spec"),
];

const UFP_DEVICE_CAPABILITY_VALUES: &[(u32, &str)] = &[
    (1, "USB 2.0 device"),
    (2, "billboard only"),
    (4, "USB 3.2 device"),
    (8, "USB4 device"),
];

pub const UFP_VDO1_FIELDS: &[VdoField] = &[
    VdoField { name: "USB Highest Speed", offset: 0, mask: 0x7, values: USB_SPEED_VALUES },
    VdoField { name: "Alternate Modes", offset: 3, mask: 0x7, values: UFP_ALT_MODE_VALUES },
    VdoField { name: "Device Capability", offset: 24, mask: 0xF, values: UFP_DEVICE_CAPABILITY_VALUES },
];

const DFP_HOST_CAPABILITY_VALUES: &[(u32, &str)] = &[
    (1, "USB 2.0 host"),
    (2, "USB 3.2 host"),
    (4, "USB4 host"),
];

pub const DFP_VDO_FIELDS: &[VdoField] = &[
    VdoField { name: "Port Number", offset: 0, mask: 0x1F, values: &[] },
    VdoField { name: "Connector Type", offset: 22, mask: 0x3, values: CONNECTOR_TYPE_VALUES },
    VdoField { name: "Host Capability", offset: 24, mask: 0x7, values: DFP_HOST_CAPABILITY_VALUES },
];

/// Return the UFP product type name from an ID Header VDO.
pub fn ufp_product_type_name(id_header: u32) -> &'static str {
    let val = field(id_header, ID_HEADER_VDO_PRODUCT_TYPE_OFFSET, ID_HEADER_VDO_PRODUCT_TYPE_MASK);
    UFP_PRODUCT_TYPE_VALUES
        .iter()
        .find(|(v, _)| *v == val)
        .map(|(_, d)| *d)
        .unwrap_or("unknown")
}

/// Return the DFP product type name from an ID Header VDO.
pub fn dfp_product_type_name(id_header: u32) -> &'static str {
    let val = field(id_header, ID_HEADER_VDO_PRODUCT_TYPE_DFP_OFFSET, ID_HEADER_VDO_PRODUCT_TYPE_MASK);
    DFP_PRODUCT_TYPE_VALUES
        .iter()
        .find(|(v, _)| *v == val)
        .map(|(_, d)| *d)
        .unwrap_or("unknown")
}

/// Return the cable product type name from an ID Header VDO.
pub fn cable_product_type_name(id_header: u32) -> &'static str {
    let val = field(id_header, ID_HEADER_VDO_PRODUCT_TYPE_OFFSET, ID_HEADER_VDO_PRODUCT_TYPE_MASK);
    CABLE_PRODUCT_TYPE_VALUES
        .iter()
        .find(|(v, _)| *v == val)
        .map(|(_, d)| *d)
        .unwrap_or("unknown")
}
