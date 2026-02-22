use crate::types::*;
use crate::vdo::*;

/// Run all cable quality checks for a port and return any warnings.
pub fn check_cable_quality(port: &Port) -> Vec<CableWarning> {
    let mut warnings = Vec::new();

    let partner = match &port.partner {
        Some(p) => p,
        None => return warnings,
    };

    let can_enter_usb4 = can_enter_usb4(partner, &port.cable);
    let can_enter_tbt = can_enter_tbt(partner, &port.cable);
    let mut invalid_dp_cable = false;
    let can_enter_dp = can_enter_dp(partner, &port.cable, &mut invalid_dp_cable);

    // USB4 first
    if can_enter_usb4 == ModeEntryResult::Success {
        if cable_limiting_usb_speed(partner, port.cable.as_ref(), false) {
            warnings.push(CableWarning::SpeedLimitingCable);
        }
        return warnings;
    }

    // TBT next
    if can_enter_tbt == ModeEntryResult::Success {
        if can_enter_usb4 == ModeEntryResult::CableError {
            warnings.push(CableWarning::InvalidUsb4ValidTbtCable);
        } else if cable_limiting_usb_speed(partner, port.cable.as_ref(), true) {
            warnings.push(CableWarning::SpeedLimitingCable);
        }
        return warnings;
    }

    // DP / fallback
    if can_enter_usb4 == ModeEntryResult::CableError {
        warnings.push(CableWarning::InvalidUsb4Cable);
    } else if can_enter_tbt == ModeEntryResult::CableError {
        warnings.push(CableWarning::InvalidTbtCable);
    } else if invalid_dp_cable {
        warnings.push(CableWarning::InvalidDpCable);
    } else if cable_limiting_usb_speed(partner, port.cable.as_ref(), false) {
        warnings.push(CableWarning::SpeedLimitingCable);
    }

    // Also check DP cable validity for non-emarked cables (only if we haven't
    // already flagged something).
    if warnings.is_empty() && can_enter_dp && invalid_dp_cable {
        warnings.push(CableWarning::InvalidDpCable);
    }

    warnings
}

// --- Partner capability checks (from partner.cc) ---

fn partner_supports_dp(partner: &Partner) -> bool {
    partner.alt_modes.iter().any(|m| {
        m.svid == DP_ALT_MODE_SID && (m.vdo & DP_MODE_SNK) != 0
    })
}

fn partner_supports_tbt(partner: &Partner) -> bool {
    partner.alt_modes.iter().any(|m| m.svid == TBT_ALT_MODE_VID)
}

fn partner_supports_usb4(partner: &Partner) -> bool {
    let product_type = field(
        partner.identity.id_header_vdo,
        ID_HEADER_VDO_PRODUCT_TYPE_OFFSET,
        ID_HEADER_VDO_PRODUCT_TYPE_MASK,
    );
    if product_type != ID_HEADER_VDO_PRODUCT_TYPE_UFP_HUB
        && product_type != ID_HEADER_VDO_PRODUCT_TYPE_UFP_PERIPHERAL
    {
        return false;
    }
    let cap = field(
        partner.identity.product_type_vdo1,
        DEVICE_CAPABILITY_OFFSET,
        DEVICE_CAPABILITY_MASK,
    );
    (cap & DEVICE_CAPABILITY_USB4) != 0
}

// --- Cable identity checks (from cable.cc) ---

fn cable_dp_identity_check(cable: &Cable) -> bool {
    let product_type = field(
        cable.identity.id_header_vdo,
        ID_HEADER_VDO_PRODUCT_TYPE_OFFSET,
        ID_HEADER_VDO_PRODUCT_TYPE_MASK,
    );

    // Passive cables must not be USB 2.0 only.
    if product_type == ID_HEADER_VDO_PRODUCT_TYPE_CABLE_PASSIVE
        && (cable.identity.product_type_vdo1 & USB_SPEED_MASK) == USB_SPEED_20
    {
        return false;
    }

    // Active cables must support modal operation and provide a DP Alt Mode SVID.
    if product_type == ID_HEADER_VDO_PRODUCT_TYPE_CABLE_ACTIVE {
        if (cable.identity.id_header_vdo & ID_HEADER_VDO_MODAL_OPERATION) == 0 {
            return false;
        }
        if !cable.plug_alt_modes.iter().any(|m| m.svid == DP_ALT_MODE_SID) {
            return false;
        }
    }

    true
}

fn cable_tbt3_identity_check(cable: &Cable) -> bool {
    let product_type = field(
        cable.identity.id_header_vdo,
        ID_HEADER_VDO_PRODUCT_TYPE_OFFSET,
        ID_HEADER_VDO_PRODUCT_TYPE_MASK,
    );

    // Active cables: check for TBT Alt Mode VID.
    // Note: original uses bitwise & which means active (0x4) & 0x4 = true.
    if (product_type & ID_HEADER_VDO_PRODUCT_TYPE_CABLE_ACTIVE) != 0 {
        return cable.plug_alt_modes.iter().any(|m| m.svid == TBT_ALT_MODE_VID);
    }

    // Must be passive.
    if (product_type & ID_HEADER_VDO_PRODUCT_TYPE_CABLE_PASSIVE) == 0 {
        return false;
    }

    let usb_speed = cable.identity.product_type_vdo1 & USB_SPEED_MASK;
    match cable.pd_revision {
        PdRevision::V3_0 | PdRevision::V3_1 | PdRevision::V3_2 => {
            usb_speed == USB_SUPER_SPEED_32_GEN1
                || usb_speed == USB_SUPER_SPEED_32_OR_40_GEN2
                || usb_speed == USB_40_SUPER_SPEED_GEN3
                || usb_speed == USB_40_SUPER_SPEED_GEN4
        }
        // PD 2.0
        _ => {
            usb_speed == USB_SUPER_SPEED_31_GEN1 || usb_speed == USB_SUPER_SPEED_31_GEN2
        }
    }
}

fn cable_usb4_identity_check(cable: &Cable) -> bool {
    let cable_type = field(
        cable.identity.id_header_vdo,
        ID_HEADER_VDO_PRODUCT_TYPE_OFFSET,
        ID_HEADER_VDO_PRODUCT_TYPE_MASK,
    );

    if cable_type == ID_HEADER_VDO_PRODUCT_TYPE_CABLE_ACTIVE {
        let vdo_version = field(
            cable.identity.product_type_vdo1,
            ACTIVE_CABLE_VDO1_VERSION_OFFSET,
            ACTIVE_CABLE_VDO1_VERSION_MASK,
        );

        if vdo_version == ACTIVE_CABLE_VDO1_VERSION_13 {
            // Bit is inverted: set means USB4 is NOT supported.
            return (cable.identity.product_type_vdo2 & ACTIVE_CABLE_VDO2_USB4_SUPPORTED) == 0;
        }

        // For VDO version != 1.3: need modal operation + TBT SVID + rounded support.
        if (cable.identity.id_header_vdo & ID_HEADER_VDO_MODAL_OPERATION) == 0 {
            return false;
        }
        if !cable.plug_alt_modes.iter().any(|m| m.svid == TBT_ALT_MODE_VID) {
            return false;
        }
        // Check rounded support in TBT VDO.
        for mode in &cable.plug_alt_modes {
            if mode.svid != TBT_ALT_MODE_VID {
                continue;
            }
            let rounded = field(mode.vdo, TBT3_CABLE_ROUNDED_SUPPORT_OFFSET, TBT3_CABLE_ROUNDED_SUPPORT_MASK);
            if rounded == TBT3_CABLE_ROUNDED_3_4_GEN {
                return true;
            }
        }
        return false;
    }

    if cable_type == ID_HEADER_VDO_PRODUCT_TYPE_CABLE_PASSIVE {
        let speed = cable.identity.product_type_vdo1 & USB_SPEED_MASK;
        return speed != USB_SPEED_20;
    }

    false
}

// --- Mode entry checks (from port.cc) ---

fn can_enter_dp(partner: &Partner, cable: &Option<Cable>, invalid_cable: &mut bool) -> bool {
    if !partner_supports_dp(partner) {
        return false;
    }

    let partner_is_receptacle = partner.alt_modes.iter().any(|m| {
        m.svid == DP_ALT_MODE_SID && (m.vdo & DP_MODE_RECEPTACLE) != 0
    });

    *invalid_cable = false;

    if partner_is_receptacle {
        match cable {
            Some(c) if c.identity.id_header_vdo != 0 => {
                // Emarked cable — check identity.
                if !cable_dp_identity_check(c) {
                    *invalid_cable = true;
                    return false;
                }
            }
            _ => {
                // Non-emarked or no cable — warn but allow.
                *invalid_cable = true;
                return true;
            }
        }
    }

    true
}

fn can_enter_tbt(partner: &Partner, cable: &Option<Cable>) -> ModeEntryResult {
    // Check partner modal operation.
    if (partner.identity.id_header_vdo & ID_HEADER_VDO_MODAL_OPERATION) == 0 {
        return ModeEntryResult::PartnerError;
    }
    if !partner_supports_tbt(partner) {
        return ModeEntryResult::PartnerError;
    }

    let cable = match cable {
        Some(c) => c,
        None => return ModeEntryResult::CableError,
    };

    if !cable_tbt3_identity_check(cable) {
        return ModeEntryResult::CableError;
    }

    ModeEntryResult::Success
}

fn can_enter_usb4(partner: &Partner, cable: &Option<Cable>) -> ModeEntryResult {
    if !partner_supports_usb4(partner) {
        return ModeEntryResult::PartnerError;
    }

    let cable = match cable {
        Some(c) => c,
        None => return ModeEntryResult::CableError,
    };

    if !cable_usb4_identity_check(cable) {
        return ModeEntryResult::CableError;
    }

    ModeEntryResult::Success
}

// --- Speed limiting check (from port.cc CableLimitingUSBSpeed) ---

fn is_captive_cable(partner: &Partner, cable: Option<&Cable>) -> bool {
    // Check cable plug type.
    if let Some(c) = cable {
        let plug_type = field(
            c.identity.product_type_vdo1,
            CABLE_VDO1_PLUG_TYPE_OFFSET,
            CABLE_VDO1_PLUG_TYPE_MASK,
        );
        if plug_type == CABLE_VDO1_PLUG_TYPE_CAPTIVE {
            return true;
        }
    }

    // Check partner connector type.
    let connector_type = field(
        partner.identity.id_header_vdo,
        ID_HEADER_VDO_CONNECTOR_TYPE_OFFSET,
        ID_HEADER_VDO_CONNECTOR_TYPE_MASK,
    );
    if connector_type == ID_HEADER_VDO_CONNECTOR_TYPE_PLUG {
        return true;
    }

    // Check partner DP alt mode receptacle bit (absence = captive).
    if partner_supports_dp(partner) {
        for m in &partner.alt_modes {
            if m.svid == DP_ALT_MODE_SID && (m.vdo & DP_MODE_RECEPTACLE) == 0 {
                return true;
            }
        }
    }

    false
}

fn cable_limiting_usb_speed(partner: &Partner, cable: Option<&Cable>, tbt3_alt_mode: bool) -> bool {
    let cable = match cable {
        Some(c) => c,
        None => return false,
    };

    let mut cable_speed = cable.identity.product_type_vdo1 & USB_SPEED_MASK;
    let mut partner_speed = partner.identity.product_type_vdo1 & USB_SPEED_MASK;

    // Validate cable type.
    let cable_type = field(
        cable.identity.id_header_vdo,
        ID_HEADER_VDO_PRODUCT_TYPE_OFFSET,
        ID_HEADER_VDO_PRODUCT_TYPE_MASK,
    );
    if cable_type != ID_HEADER_VDO_PRODUCT_TYPE_CABLE_ACTIVE
        && cable_type != ID_HEADER_VDO_PRODUCT_TYPE_CABLE_PASSIVE
    {
        return false;
    }

    if is_captive_cable(partner, Some(cable)) {
        return false;
    }

    if tbt3_alt_mode {
        partner_speed = USB_40_SUPER_SPEED_GEN3;
    } else {
        let partner_type = field(
            partner.identity.id_header_vdo,
            ID_HEADER_VDO_PRODUCT_TYPE_OFFSET,
            ID_HEADER_VDO_PRODUCT_TYPE_MASK,
        );
        let partner_pd = partner.pd_revision;

        match partner_pd {
            PdRevision::V2_0 => {
                if partner_type != ID_HEADER_VDO_PRODUCT_TYPE_UFP_AMA {
                    return false;
                }
            }
            PdRevision::V3_0 => {
                if partner_type != ID_HEADER_VDO_PRODUCT_TYPE_UFP_HUB
                    && partner_type != ID_HEADER_VDO_PRODUCT_TYPE_UFP_PERIPHERAL
                    && partner_type != ID_HEADER_VDO_PRODUCT_TYPE_UFP_AMA
                {
                    return false;
                }
            }
            PdRevision::V3_1 | PdRevision::V3_2 => {
                if partner_type != ID_HEADER_VDO_PRODUCT_TYPE_UFP_HUB
                    && partner_type != ID_HEADER_VDO_PRODUCT_TYPE_UFP_PERIPHERAL
                {
                    return false;
                }
            }
            _ => return false,
        }

        // Billboard-only AMA should not be compared.
        if (partner_pd == PdRevision::V2_0 || partner_pd == PdRevision::V3_0)
            && partner_type == ID_HEADER_VDO_PRODUCT_TYPE_UFP_AMA
            && partner_speed == AMA_VDO_USB_SPEED_BILLBOARD
        {
            return false;
        }
    }

    // Check TBT3 cables that may support higher speeds.
    for mode in &cable.plug_alt_modes {
        if mode.svid != TBT_ALT_MODE_VID {
            continue;
        }

        // TBT3 cable in TBT3 mode — no speed limit applies.
        if tbt3_alt_mode {
            return false;
        }

        let cable_tbt_mode = field(mode.vdo, TBT3_CABLE_MODE_OFFSET, TBT3_CABLE_MODE_MASK);
        let cable_tbt_speed = field(mode.vdo, TBT3_CABLE_SPEED_OFFSET, TBT3_CABLE_SPEED_MASK);
        let cable_tbt_rounded = field(
            mode.vdo,
            TBT3_CABLE_ROUNDED_SUPPORT_OFFSET,
            TBT3_CABLE_ROUNDED_SUPPORT_MASK,
        );

        if cable_tbt_mode == TBT3_CABLE_MODE_TBT
            && cable_tbt_speed == TBT3_CABLE_SPEED_10G_20G
        {
            if cable_type == ID_HEADER_VDO_PRODUCT_TYPE_CABLE_PASSIVE {
                cable_speed = USB_40_SUPER_SPEED_GEN4;
            } else if cable_type == ID_HEADER_VDO_PRODUCT_TYPE_CABLE_ACTIVE
                && cable_tbt_rounded == TBT3_CABLE_ROUNDED_3_4_GEN
            {
                cable_speed = USB_40_SUPER_SPEED_GEN3;
            }
        }

        break;
    }

    partner_speed > cable_speed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_partner(id_header: u32, vdo1: u32, pd_rev: PdRevision, alt_modes: Vec<AltMode>) -> Partner {
        Partner {
            identity: PdIdentity {
                id_header_vdo: id_header,
                product_type_vdo1: vdo1,
                ..Default::default()
            },
            pd_revision: pd_rev,
            alt_modes,
        }
    }

    fn make_cable(id_header: u32, vdo1: u32, vdo2: u32, pd_rev: PdRevision, alt_modes: Vec<AltMode>) -> Cable {
        let cable_type = {
            let pt = field(id_header, ID_HEADER_VDO_PRODUCT_TYPE_OFFSET, ID_HEADER_VDO_PRODUCT_TYPE_MASK);
            match pt {
                ID_HEADER_VDO_PRODUCT_TYPE_CABLE_PASSIVE => CableType::Passive,
                ID_HEADER_VDO_PRODUCT_TYPE_CABLE_ACTIVE => CableType::Active,
                _ => CableType::Unknown,
            }
        };
        Cable {
            identity: PdIdentity {
                id_header_vdo: id_header,
                product_type_vdo1: vdo1,
                product_type_vdo2: vdo2,
                ..Default::default()
            },
            pd_revision: pd_rev,
            cable_type,
            plug_alt_modes: alt_modes,
        }
    }

    // --- Cable DP identity checks ---

    #[test]
    fn test_nekteck_usb2_passive_fails_dp() {
        // Nekteck USB 2.0 passive cable: speed = 0 (USB 2.0)
        let cable = make_cable(0x18002e98, 0xc1082040, 0, PdRevision::V3_0, vec![]);
        // Product type VDO1 speed field: 0xc1082040 & 0x7 = 0 (USB 2.0)
        assert!(!cable_dp_identity_check(&cable));
    }

    #[test]
    fn test_hp_usb32_gen1_passive_passes_dp() {
        // HP USB 3.2 Gen1 cable: speed field = 1
        let cable = make_cable(0x180003f0, 0x11084851, 0, PdRevision::V3_0, vec![]);
        assert!(cable_dp_identity_check(&cable));
    }

    // --- Cable TBT3 identity checks ---

    #[test]
    fn test_nekteck_usb2_fails_tbt() {
        let cable = make_cable(0x18002e98, 0xc1082040, 0, PdRevision::V3_0, vec![]);
        assert!(!cable_tbt3_identity_check(&cable));
    }

    #[test]
    fn test_hp_usb32_gen1_passes_tbt() {
        let cable = make_cable(0x180003f0, 0x11084851, 0, PdRevision::V3_0, vec![]);
        assert!(cable_tbt3_identity_check(&cable));
    }

    #[test]
    fn test_belkin_tbt3_active_passes_tbt() {
        // Belkin TBT3 active cable (0x24 = active type, has TBT alt mode)
        let cable = make_cable(
            0x240020c2,
            0x21085858,
            0,
            PdRevision::V2_0,
            vec![
                AltMode { svid: TBT_ALT_MODE_VID, vdo: 0x430001 },
                AltMode { svid: 0x04b4, vdo: 0x1 },
            ],
        );
        assert!(cable_tbt3_identity_check(&cable));
    }

    #[test]
    fn test_hongju_usb31_gen1_pd20_passes_tbt() {
        // Hongju USB 3.1 Gen1 PD 2.0 cable: speed = 1
        let cable = make_cable(0x18005694, 0x84051, 0, PdRevision::V2_0, vec![]);
        assert!(cable_tbt3_identity_check(&cable));
    }

    // --- Cable USB4 identity checks ---

    #[test]
    fn test_nekteck_usb2_fails_usb4() {
        let cable = make_cable(0x18002e98, 0xc1082040, 0, PdRevision::V3_0, vec![]);
        assert!(!cable_usb4_identity_check(&cable));
    }

    #[test]
    fn test_cable_matters_20gbps_passes_usb4() {
        // Cable Matters 20 Gbps USB4 cable: passive, speed = 2
        let cable = make_cable(0x1860060f, 0x11084042, 0, PdRevision::V3_0, vec![]);
        assert!(cable_usb4_identity_check(&cable));
    }

    #[test]
    fn test_apple_tbt3_pro_active_passes_usb4() {
        // Apple TBT3 Pro cable: active, VDO version 1.3, VDO2 bit 8 clear
        let cable = make_cable(
            0x240005ac,
            0x434858da,
            0x5a5f0001,
            PdRevision::V3_0,
            vec![
                AltMode { svid: TBT_ALT_MODE_VID, vdo: 0xcb0001 },
                AltMode { svid: DP_ALT_MODE_SID, vdo: 0xc0c0c },
            ],
        );
        // VDO version: (0x434858da >> 21) & 0x7 = 0x3 = v1.3
        // VDO2 USB4 bit: 0x5a5f0001 & (1<<8) = 0x100 => 0x5a5f0001 & 0x100 = 0
        // Wait: 0x5a5f0001 = ...0000 0000 0001 => bit 8 is 0. So USB4 supported.
        assert!(cable_usb4_identity_check(&cable));
    }

    // --- Partner checks ---

    #[test]
    fn test_partner_supports_dp() {
        let partner = make_partner(
            0x6c0004b4, 0x8, PdRevision::V3_0,
            vec![AltMode { svid: DP_ALT_MODE_SID, vdo: 0x405 }],
        );
        assert!(partner_supports_dp(&partner));
    }

    #[test]
    fn test_partner_supports_tbt() {
        let partner = make_partner(
            0x4c0041c3, 0x0, PdRevision::V3_0,
            vec![
                AltMode { svid: TBT_ALT_MODE_VID, vdo: 0x1 },
                AltMode { svid: DP_ALT_MODE_SID, vdo: 0x1c05 },
            ],
        );
        assert!(partner_supports_tbt(&partner));
    }

    #[test]
    fn test_partner_supports_usb4() {
        // OWC TBT4 dock: UFP Hub (product type=1), device cap has USB4 bit
        let partner = make_partner(
            0x4cc01e91, 0xd00003b, PdRevision::V3_0,
            vec![
                AltMode { svid: DP_ALT_MODE_SID, vdo: 0x1c0045 },
                AltMode { svid: TBT_ALT_MODE_VID, vdo: 0x1 },
            ],
        );
        // product type: (0x4cc01e91 >> 27) & 0x7 = 0x9 >> ... let's check
        // 0x4cc01e91 = 0100 1100 1100 0000 0001 1110 1001 0001
        // bits 29-27: 001 = 1 = UFP Hub
        // device cap: (0xd00003b >> 24) & 0xf = 0xd & 0xf = 0xd
        // 0xd & 0x8 = 0x8 => USB4 supported
        assert!(partner_supports_usb4(&partner));
    }

    // --- Full quality checks ---

    #[test]
    fn test_owc_dock_with_nekteck_usb2() {
        // OWC TBT4 dock (USB4) + Nekteck USB 2.0 cable → InvalidUsb4Cable
        let partner = make_partner(
            0x4cc01e91, 0xd00003b, PdRevision::V3_0,
            vec![
                AltMode { svid: DP_ALT_MODE_SID, vdo: 0x1c0045 },
                AltMode { svid: TBT_ALT_MODE_VID, vdo: 0x1 },
            ],
        );
        let cable = make_cable(0x18002e98, 0xc1082040, 0, PdRevision::V3_0, vec![]);
        let port = Port {
            number: 0,
            data_role: DataRole::Host,
            power_role: PowerRole::Source,
            partner: Some(partner),
            cable: Some(cable),
        };
        let warnings = check_cable_quality(&port);
        assert!(warnings.contains(&CableWarning::InvalidUsb4Cable));
    }

    #[test]
    fn test_owc_dock_with_belkin_tbt3_active() {
        // OWC TBT4 dock (USB4) + Belkin TBT3 active cable → InvalidUsb4ValidTbtCable
        let partner = make_partner(
            0x4cc01e91, 0xd00003b, PdRevision::V3_0,
            vec![
                AltMode { svid: DP_ALT_MODE_SID, vdo: 0x1c0045 },
                AltMode { svid: TBT_ALT_MODE_VID, vdo: 0x1 },
            ],
        );
        // Belkin TBT3 active: type 0x4 (active), has TBT SVID but no rounded support
        let cable = make_cable(
            0x240020c2,
            0x21085858,
            0,
            PdRevision::V2_0,
            vec![
                AltMode { svid: TBT_ALT_MODE_VID, vdo: 0x430001 },
                AltMode { svid: 0x04b4, vdo: 0x1 },
            ],
        );
        let port = Port {
            number: 0,
            data_role: DataRole::Host,
            power_role: PowerRole::Source,
            partner: Some(partner),
            cable: Some(cable),
        };
        let warnings = check_cable_quality(&port);
        assert!(warnings.contains(&CableWarning::InvalidUsb4ValidTbtCable));
    }

    #[test]
    fn test_thinkpad_tbt3_dock_with_nekteck_usb2() {
        // ThinkPad TBT3 dock (TBT, no USB4) + Nekteck USB 2.0 cable → InvalidTbtCable
        let partner = make_partner(
            0x540017ef, 0x0, PdRevision::V3_0,
            vec![
                AltMode { svid: TBT_ALT_MODE_VID, vdo: 0x1 },
                AltMode { svid: DP_ALT_MODE_SID, vdo: 0xc3c47 },
            ],
        );
        let cable = make_cable(0x18002e98, 0xc1082040, 0, PdRevision::V3_0, vec![]);
        let port = Port {
            number: 0,
            data_role: DataRole::Host,
            power_role: PowerRole::Source,
            partner: Some(partner),
            cable: Some(cable),
        };
        let warnings = check_cable_quality(&port);
        assert!(warnings.contains(&CableWarning::InvalidTbtCable));
    }

    #[test]
    fn test_no_partner_no_warnings() {
        let port = Port {
            number: 0,
            data_role: DataRole::Host,
            power_role: PowerRole::Source,
            partner: None,
            cable: None,
        };
        let warnings = check_cable_quality(&port);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_speed_limiting_cable() {
        // OWC TBT4 dock (USB4 partner, speed = Gen3) + Anker USB 3.2 Gen2 cable
        let partner = make_partner(
            0x4cc01e91, 0xd00003b, PdRevision::V3_0,
            vec![
                AltMode { svid: DP_ALT_MODE_SID, vdo: 0x1c0045 },
                AltMode { svid: TBT_ALT_MODE_VID, vdo: 0x1 },
            ],
        );
        // Anker USB 3.2 Gen2: passive, speed = 2 (Gen2)
        let cable = make_cable(0x1c00291a, 0x11082032, 0, PdRevision::V2_0, vec![]);
        let port = Port {
            number: 0,
            data_role: DataRole::Host,
            power_role: PowerRole::Source,
            partner: Some(partner),
            cable: Some(cable),
        };
        // Partner supports USB4 and cable supports USB4 (passive, speed != USB2.0)
        // So USB4 mode entry succeeds. Then we check speed limiting.
        // Partner UFP VDO1 speed: 0xd00003b & 0x7 = 0x3 (Gen3)
        // Cable speed: 0x11082032 & 0x7 = 0x2 (Gen2)
        // partner_speed (3) > cable_speed (2) → speed limiting
        let warnings = check_cable_quality(&port);
        assert!(warnings.contains(&CableWarning::SpeedLimitingCable));
    }
}
