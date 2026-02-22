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
