use crate::quality;
use crate::types::*;
use crate::vdo;

pub fn print_status(ports: &[Port]) -> bool {
    let mut any_warnings = false;

    if ports.is_empty() {
        println!("No USB Type-C ports found.");
        return false;
    }

    for port in ports {
        print_port(port);
        let warnings = quality::check_cable_quality(port);
        if !warnings.is_empty() {
            any_warnings = true;
            println!("  Warnings:");
            for w in &warnings {
                println!("    \u{26a0} {w}");
            }
        }
        println!();
    }

    any_warnings
}

fn print_port(port: &Port) {
    println!(
        "Port {} [{}/{}]",
        port.number, port.data_role, port.power_role
    );

    match &port.partner {
        Some(partner) => print_partner(partner),
        None => println!("  (nothing connected)"),
    }

    if let Some(cable) = &port.cable {
        print_cable(cable);
    }
}

fn print_partner(partner: &Partner) {
    let product_type = vdo::field(
        partner.identity.id_header_vdo,
        vdo::ID_HEADER_VDO_PRODUCT_TYPE_OFFSET,
        vdo::ID_HEADER_VDO_PRODUCT_TYPE_MASK,
    );
    let type_str = match product_type {
        vdo::ID_HEADER_VDO_PRODUCT_TYPE_UFP_HUB => "hub",
        vdo::ID_HEADER_VDO_PRODUCT_TYPE_UFP_PERIPHERAL => "peripheral",
        vdo::ID_HEADER_VDO_PRODUCT_TYPE_UFP_AMA => "alt mode adapter",
        _ => "device",
    };

    let mut caps = Vec::new();
    if partner.pd_revision != PdRevision::None {
        caps.push(format!("{}", partner.pd_revision));
    }
    if partner_has_usb4(partner) {
        caps.push("USB4".to_string());
    }
    let caps_str = if caps.is_empty() {
        String::new()
    } else {
        format!(" ({})", caps.join(", "))
    };

    println!("  Partner: {type_str}{caps_str}");

    if !partner.alt_modes.is_empty() {
        let modes: Vec<String> = partner
            .alt_modes
            .iter()
            .map(|m| {
                let name = alt_mode_name(m.svid);
                format!("{name} ({:#06x})", m.svid)
            })
            .collect();
        println!("    Alt Modes: {}", modes.join(", "));
    }
}

fn print_cable(cable: &Cable) {
    let speed = cable.identity.product_type_vdo1 & vdo::USB_SPEED_MASK;
    let speed_str = vdo::usb_speed_name(speed, cable.pd_revision);

    let mut info = Vec::new();
    if cable.pd_revision != PdRevision::None {
        info.push(format!("{}", cable.pd_revision));
    }
    info.push(speed_str.to_string());
    let info_str = if info.is_empty() {
        String::new()
    } else {
        format!(" ({})", info.join(", "))
    };

    println!("  Cable: {}{info_str}", cable.cable_type);

    if !cable.plug_alt_modes.is_empty() {
        let modes: Vec<String> = cable
            .plug_alt_modes
            .iter()
            .map(|m| {
                let name = alt_mode_name(m.svid);
                format!("{name} ({:#06x})", m.svid)
            })
            .collect();
        println!("    SOP' Alt Modes: {}", modes.join(", "));
    }
}

fn partner_has_usb4(partner: &Partner) -> bool {
    let product_type = vdo::field(
        partner.identity.id_header_vdo,
        vdo::ID_HEADER_VDO_PRODUCT_TYPE_OFFSET,
        vdo::ID_HEADER_VDO_PRODUCT_TYPE_MASK,
    );
    if product_type != vdo::ID_HEADER_VDO_PRODUCT_TYPE_UFP_HUB
        && product_type != vdo::ID_HEADER_VDO_PRODUCT_TYPE_UFP_PERIPHERAL
    {
        return false;
    }
    let cap = vdo::field(
        partner.identity.product_type_vdo1,
        vdo::DEVICE_CAPABILITY_OFFSET,
        vdo::DEVICE_CAPABILITY_MASK,
    );
    (cap & vdo::DEVICE_CAPABILITY_USB4) != 0
}

fn alt_mode_name(svid: u16) -> &'static str {
    match svid {
        vdo::DP_ALT_MODE_SID => "DP",
        vdo::TBT_ALT_MODE_VID => "TBT",
        _ => "Vendor",
    }
}
