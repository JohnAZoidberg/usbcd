use crate::quality;
use crate::types::*;
use crate::vdo;
use std::fmt::Write;

/// `writeln!` into a String, which cannot fail.
macro_rules! w {
    ($out:expr) => {{
        let _ = writeln!($out);
    }};
    ($out:expr, $($arg:tt)*) => {{
        let _ = writeln!($out, $($arg)*);
    }};
}

pub fn print_status(ports: &[Port], verbose: bool) -> bool {
    let (text, any_warnings) = render_status(ports, verbose);
    print!("{text}");
    any_warnings
}

/// Render the status report as text. Returns the text and whether any
/// warnings were found.
pub fn render_status(ports: &[Port], verbose: bool) -> (String, bool) {
    let mut buf = String::new();
    let out = &mut buf;
    let mut any_warnings = false;

    if ports.is_empty() {
        w!(out, "No USB Type-C ports found.");
        return (buf, false);
    }

    for port in ports {
        write_port(out, port, verbose);
        let warnings = quality::check_cable_quality(port);
        if !warnings.is_empty() {
            any_warnings = true;
            w!(out, "  Warnings:");
            for w in &warnings {
                w!(out, "    \u{26a0} {w}");
            }
        }
        w!(out);
    }

    (buf, any_warnings)
}

/// One line describing the negotiated power, e.g. "20.0V 3.0A 60W (PD,
/// charging)". None when nothing is connected or the kernel has no values.
pub fn contract_summary(port: &Port) -> Option<String> {
    let c = port.contract.as_ref()?;
    if !port.is_connected() {
        return None;
    }
    let kind = if c.pps {
        "PPS"
    } else if c.pd {
        "PD"
    } else {
        match port.power_operation_mode {
            PowerOpMode::Current15A => "USB Type-C 1.5A",
            PowerOpMode::Current30A => "USB Type-C 3.0A",
            _ => "USB default",
        }
    };
    let values = || {
        format!(
            "{:.1}V {:.1}A {}W",
            c.voltage_mv as f64 / 1000.0,
            c.current_ma as f64 / 1000.0,
            c.milliwatts() / 1000
        )
    };
    match port.effective_power_role() {
        PowerRole::Sink => Some(if c.has_values() {
            format!("{} ({kind}, {})", values(), c.status.to_lowercase())
        } else {
            format!("{kind}, {}", c.status.to_lowercase())
        }),
        PowerRole::Source => {
            if c.has_values() {
                Some(format!("{} ({kind}, supplying)", values()))
            } else if c.pd {
                // The kernel resolves the RDO against the partner's source
                // PDOs, which it only has while sinking.
                Some("PD, supplying (values not exposed by the kernel while sourcing)".to_string())
            } else {
                None
            }
        }
        PowerRole::None => None,
    }
}

/// The effective power role, noting the kernel's attribute when it is stale.
pub fn power_role_label(port: &Port) -> String {
    let role = port.effective_power_role();
    if role == port.power_role {
        role.to_string()
    } else {
        format!("{role} (kernel says {})", port.power_role)
    }
}

fn write_port(out: &mut String, port: &Port, verbose: bool) {
    let mut tags = Vec::new();
    // The mode is a label only; see PowerOpMode. It is shown when something
    // is connected, where it is fresh.
    if port.is_connected() {
        match &port.power_operation_mode {
            PowerOpMode::UsbPowerDelivery => tags.push("PD".to_string()),
            PowerOpMode::Current15A => tags.push("USB 1.5A".to_string()),
            PowerOpMode::Current30A => tags.push("USB 3.0A".to_string()),
            PowerOpMode::Other(s) => tags.push(s.clone()),
            PowerOpMode::Default | PowerOpMode::None => {}
        }
        match port.orientation {
            Some(Orientation::Normal) => tags.push("CC1".to_string()),
            Some(Orientation::Reverse) => tags.push("CC2".to_string()),
            _ => {}
        }
        if port.vconn_source == Some(true) {
            tags.push("VCONN".to_string());
        }
    }
    let tag_str = tags
        .iter()
        .map(|t| format!(", {t}"))
        .collect::<String>();
    w!(
        out,
        "Port {} [{}/{}{tag_str}]",
        port.number,
        port.data_role,
        power_role_label(port)
    );

    if verbose {
        if !port.usb_capability.is_empty() {
            let caps: Vec<String> = port
                .usb_capability
                .iter()
                .map(|c| {
                    if port.usb_capability_default.as_deref() == Some(c) {
                        format!("[{c}]")
                    } else {
                        c.clone()
                    }
                })
                .collect();
            w!(out, "  USB capability: {}", caps.join(" "));
        }
        if let Some(power) = &port.power {
            if !power.source.is_empty() {
                w!(out, "  Port Source Capabilities:");
                write_pdo_list(out, &power.source, "    ");
            }
            if !power.sink.is_empty() {
                w!(out, "  Port Sink Capabilities:");
                write_pdo_list(out, &power.sink, "    ");
            }
        }
    }

    match &port.partner {
        Some(partner) => write_partner(out, partner, port, verbose),
        None => {
            if port.is_connected() {
                // The power supply says we are sinking but the partner device
                // isn't registered (yet).
                w!(out, "  Partner: connected (no partner device reported by the kernel)");
            } else {
                w!(out, "  (nothing connected)");
            }
        }
    }

    if let Some(contract) = contract_summary(port) {
        w!(out, "  Contract: {contract}");
    }

    if let Some(cable) = &port.cable {
        write_cable(out, cable, verbose);
    }
}

fn write_partner(out: &mut String, partner: &Partner, port: &Port, verbose: bool) {
    let identified = partner.identity.is_present();
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
    } else {
        match port.partner_supports_pd() {
            Some(true) => caps.push("PD".to_string()),
            Some(false) => caps.push("no PD".to_string()),
            None => {}
        }
    }
    if partner_has_usb4(partner) {
        caps.push("USB4".to_string());
    }
    if let Some(mode) = &partner.accessory_mode {
        if mode != "none" {
            caps.push(format!("{mode} accessory"));
        }
    }
    if !identified {
        caps.push("no identity reported".to_string());
    }
    let caps_str = if caps.is_empty() {
        String::new()
    } else {
        format!(" ({})", caps.join(", "))
    };

    w!(out, "  Partner: {type_str}{caps_str}");

    if verbose {
        // ID Header VDO
        if partner.identity.id_header_vdo != 0 {
            w!(out, "    ID Header: {:#010x}", partner.identity.id_header_vdo);
            vdo::write_vdo_fields(
                out,
                partner.identity.id_header_vdo,
                vdo::ID_HEADER_VDO_FIELDS,
                "    ",
            );
        }

        // UFP VDO1
        if partner.identity.product_type_vdo1 != 0
            && (product_type == vdo::ID_HEADER_VDO_PRODUCT_TYPE_UFP_HUB
                || product_type == vdo::ID_HEADER_VDO_PRODUCT_TYPE_UFP_PERIPHERAL)
        {
            w!(
                out,
                "    UFP VDO1: {:#010x}",
                partner.identity.product_type_vdo1
            );
            vdo::write_vdo_fields(
                out,
                partner.identity.product_type_vdo1,
                vdo::UFP_VDO1_FIELDS,
                "    ",
            );
        }

        // DFP VDO (VDO3)
        if partner.identity.product_type_vdo3 != 0 {
            w!(
                out,
                "    DFP VDO (VDO3): {:#010x}",
                partner.identity.product_type_vdo3
            );
            vdo::write_vdo_fields(
                out,
                partner.identity.product_type_vdo3,
                vdo::DFP_VDO_FIELDS,
                "    ",
            );
        }
    } else {
        // Non-verbose: show DFP capability from VDO3 if partner is dual-role
        if partner.identity.product_type_vdo3 != 0 && is_dual_role_partner(partner) {
            let dfp_type = vdo::dfp_product_type_name(partner.identity.id_header_vdo);
            if dfp_type != "not DFP" {
                let host_cap = vdo::field(
                    partner.identity.product_type_vdo3,
                    24,
                    0x7,
                );
                if host_cap != 0 {
                    let host_desc = match host_cap {
                        1 => "USB 2.0",
                        2 => "USB 3.2",
                        4 => "USB4",
                        _ => "unknown",
                    };
                    w!(out, "    DFP: {dfp_type} ({host_desc})");
                }
            }
        }
    }

    if !partner.alt_modes.is_empty() {
        let modes: Vec<String> = partner
            .alt_modes
            .iter()
            .map(|m| {
                let name = alt_mode_name(m.svid);
                format!("{name} ({:#06x})", m.svid)
            })
            .collect();
        w!(out, "    Alt Modes: {}", modes.join(", "));
    } else if partner.num_alt_modes == Some(0) && identified {
        w!(out, "    Alt Modes: none");
    }

    // Power info
    if !verbose {
        // Normal mode: one-line summaries. What the laptop offers when it is
        // sourcing, and what the partner offers. The laptop's own sink
        // capabilities are not the contract, so they only appear in verbose.
        if let Some(power) = &port.power {
            if port.effective_power_role() == PowerRole::Source && !power.source.is_empty() {
                w!(
                    out,
                    "    Port can source: {}",
                    format_power_summary(&power.source)
                );
            }
        }
        if let Some(power) = &partner.power {
            if !power.source.is_empty() {
                w!(
                    out,
                    "    Partner can source: {}",
                    format_power_summary(&power.source)
                );
            }
            if !power.sink.is_empty() {
                w!(
                    out,
                    "    Partner can sink: {}",
                    format_power_summary(&power.sink)
                );
            }
        }
    } else {
        // Verbose mode: full PDO breakdown for partner
        if let Some(power) = &partner.power {
            if !power.source.is_empty() {
                w!(out, "    Partner Source Capabilities:");
                write_pdo_list(out, &power.source, "      ");
            }
            if !power.sink.is_empty() {
                w!(out, "    Partner Sink Capabilities:");
                write_pdo_list(out, &power.sink, "      ");
            }
        }
    }
}

fn write_cable(out: &mut String, cable: &Cable, verbose: bool) {
    let identified = cable.identity.is_present();

    let mut info = Vec::new();
    if cable.pd_revision != PdRevision::None {
        info.push(format!("{}", cable.pd_revision));
    }
    if identified {
        let speed = cable.identity.product_type_vdo1 & vdo::USB_SPEED_MASK;
        info.push(vdo::usb_speed_name(speed, cable.pd_revision).to_string());
    }
    if let Some(plug) = &cable.plug_type {
        if plug != "unknown" {
            info.push(format!("{plug} plug"));
        }
    }
    if !identified {
        info.push("no e-marker data".to_string());
    }
    let info_str = if info.is_empty() {
        String::new()
    } else {
        format!(" ({})", info.join(", "))
    };

    w!(out, "  Cable: {}{info_str}", cable.cable_type);

    if verbose {
        // Cable VDO1
        if cable.identity.product_type_vdo1 != 0 {
            let fields = match cable.cable_type {
                CableType::Active => vdo::ACTIVE_CABLE_VDO1_FIELDS,
                _ => vdo::PASSIVE_CABLE_VDO1_FIELDS,
            };
            w!(
                out,
                "    Cable VDO1: {:#010x}",
                cable.identity.product_type_vdo1
            );
            vdo::write_vdo_fields(out, cable.identity.product_type_vdo1, fields, "    ");
        }

        // Active Cable VDO2
        if cable.cable_type == CableType::Active && cable.identity.product_type_vdo2 != 0 {
            w!(
                out,
                "    Cable VDO2: {:#010x}",
                cable.identity.product_type_vdo2
            );
            vdo::write_vdo_fields(
                out,
                cable.identity.product_type_vdo2,
                vdo::ACTIVE_CABLE_VDO2_FIELDS,
                "    ",
            );
        }
    }

    if !cable.plug_alt_modes.is_empty() {
        let modes: Vec<String> = cable
            .plug_alt_modes
            .iter()
            .map(|m| {
                let name = alt_mode_name(m.svid);
                format!("{name} ({:#06x})", m.svid)
            })
            .collect();
        w!(out, "    SOP' Alt Modes: {}", modes.join(", "));
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

/// Check if partner has both UFP and DFP product types set (dual-role).
fn is_dual_role_partner(partner: &Partner) -> bool {
    let ufp = vdo::field(
        partner.identity.id_header_vdo,
        vdo::ID_HEADER_VDO_PRODUCT_TYPE_OFFSET,
        vdo::ID_HEADER_VDO_PRODUCT_TYPE_MASK,
    );
    let dfp = vdo::field(
        partner.identity.id_header_vdo,
        vdo::ID_HEADER_VDO_PRODUCT_TYPE_DFP_OFFSET,
        vdo::ID_HEADER_VDO_PRODUCT_TYPE_MASK,
    );
    ufp != 0 && dfp != 0
}

fn alt_mode_name(svid: u16) -> &'static str {
    match svid {
        vdo::DP_ALT_MODE_SID => "DP",
        vdo::TBT_ALT_MODE_VID => "TBT",
        _ => "Vendor",
    }
}

// --- PDO formatting ---

fn write_pdo_list(out: &mut String, pdos: &[Pdo], indent: &str) {
    for (i, pdo) in pdos.iter().enumerate() {
        w!(out, "{indent}{}: {}", i + 1, format_pdo(pdo));
    }
}

fn format_pdo(pdo: &Pdo) -> String {
    match pdo {
        Pdo::FixedSupply {
            voltage_mv,
            current_ma,
        } => {
            let watts = (*voltage_mv as u64) * (*current_ma as u64) / 1_000_000;
            format!(
                "{:.1}V {:.1}A {watts}W [fixed]",
                *voltage_mv as f64 / 1000.0,
                *current_ma as f64 / 1000.0,
            )
        }
        Pdo::VariableSupply {
            min_voltage_mv,
            max_voltage_mv,
            current_ma,
        } => format!(
            "{:.1}-{:.1}V {:.1}A [variable]",
            *min_voltage_mv as f64 / 1000.0,
            *max_voltage_mv as f64 / 1000.0,
            *current_ma as f64 / 1000.0,
        ),
        Pdo::Battery {
            min_voltage_mv,
            max_voltage_mv,
            power_mw,
        } => format!(
            "{:.1}-{:.1}V {:.1}W [battery]",
            *min_voltage_mv as f64 / 1000.0,
            *max_voltage_mv as f64 / 1000.0,
            *power_mw as f64 / 1000.0,
        ),
        Pdo::ProgrammableSupply {
            min_voltage_mv,
            max_voltage_mv,
            max_current_ma,
        } => format!(
            "{:.1}-{:.1}V {:.1}A [PPS]",
            *min_voltage_mv as f64 / 1000.0,
            *max_voltage_mv as f64 / 1000.0,
            *max_current_ma as f64 / 1000.0,
        ),
        Pdo::SprAvs {
            max_current_9v_to_15v_ma,
            max_current_15v_to_20v_ma,
        } => format!(
            "9.0-20.0V {:.2}A (to 15V) / {:.2}A (to 20V) [SPR AVS]",
            *max_current_9v_to_15v_ma as f64 / 1000.0,
            *max_current_15v_to_20v_ma as f64 / 1000.0,
        ),
    }
}

/// Format a one-line power summary from a list of PDOs.
/// E.g. "5/9/15/20V 5A 100W (PPS 3.3-21.0V)"
fn format_power_summary(pdos: &[Pdo]) -> String {
    let mut voltages = Vec::new();
    let mut max_current_ma: u32 = 0;
    let mut max_power_mw: u64 = 0;
    let mut pps_ranges = Vec::new();
    let mut has_avs = false;

    for pdo in pdos {
        match pdo {
            Pdo::FixedSupply {
                voltage_mv,
                current_ma,
            } => {
                voltages.push(*voltage_mv);
                if *current_ma > max_current_ma {
                    max_current_ma = *current_ma;
                }
                let power = (*voltage_mv as u64) * (*current_ma as u64) / 1000;
                if power > max_power_mw {
                    max_power_mw = power;
                }
            }
            Pdo::ProgrammableSupply {
                min_voltage_mv,
                max_voltage_mv,
                ..
            } => {
                pps_ranges.push((*min_voltage_mv, *max_voltage_mv));
            }
            Pdo::SprAvs { .. } => has_avs = true,
            _ => {}
        }
    }

    if voltages.is_empty() {
        return "no fixed PDOs".to_string();
    }

    let voltage_str: Vec<String> = voltages
        .iter()
        .map(|v| {
            let fv = *v as f64 / 1000.0;
            if fv.fract() == 0.0 {
                format!("{:.0}", fv)
            } else {
                format!("{:.1}", fv)
            }
        })
        .collect();

    let current_str = {
        let fc = max_current_ma as f64 / 1000.0;
        if fc.fract() == 0.0 {
            format!("{:.0}A", fc)
        } else {
            format!("{:.1}A", fc)
        }
    };

    let watts = max_power_mw / 1000;

    let mut result = format!("{}V {} {watts}W", voltage_str.join("/"), current_str);

    if !pps_ranges.is_empty() {
        let pps_strs: Vec<String> = pps_ranges
            .iter()
            .map(|(min, max)| {
                format!(
                    "{:.1}-{:.1}V",
                    *min as f64 / 1000.0,
                    *max as f64 / 1000.0
                )
            })
            .collect();
        result.push_str(&format!(" (PPS {})", pps_strs.join(", ")));
    }
    if has_avs {
        result.push_str(" (AVS)");
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sink_port(contract: Option<Contract>, partner: Option<Partner>) -> Port {
        Port {
            number: 2,
            data_role: DataRole::Host,
            power_role: PowerRole::Sink,
            power_operation_mode: PowerOpMode::UsbPowerDelivery,
            contract,
            partner,
            ..Default::default()
        }
    }

    fn charging(voltage_mv: u32, current_ma: u32) -> Contract {
        Contract {
            online: true,
            status: "Charging".into(),
            pd: true,
            voltage_mv,
            current_ma,
            ..Default::default()
        }
    }

    #[test]
    fn test_contract_line_while_sinking() {
        let port = sink_port(Some(charging(20000, 3000)), None);
        assert_eq!(
            contract_summary(&port).as_deref(),
            Some("20.0V 3.0A 60W (PD, charging)")
        );
        let (text, warnings) = render_status(&[port], false);
        assert!(!warnings);
        assert!(text.contains("Port 2 [host/sink, PD]\n"));
        assert!(text.contains("  Partner: connected (no partner device reported by the kernel)\n"));
        assert!(text.contains("  Contract: 20.0V 3.0A 60W (PD, charging)\n"));
        assert!(!text.contains("Port can sink"));
    }

    #[test]
    fn test_stale_mode_without_supply_online_is_nothing_connected() {
        let offline = Contract {
            online: false,
            status: "Not charging".into(),
            pd: true,
            ..Default::default()
        };
        let port = sink_port(Some(offline), None);
        assert_eq!(contract_summary(&port), None);
        let (text, _) = render_status(&[port], false);
        assert!(text.contains("Port 2 [host/sink]\n"));
        assert!(text.contains("  (nothing connected)\n"));
    }

    #[test]
    fn test_type_c_source_contract() {
        let port = Port {
            number: 3,
            data_role: DataRole::Host,
            power_role: PowerRole::Source,
            power_operation_mode: PowerOpMode::Current30A,
            orientation: Some(Orientation::Reverse),
            partner: Some(Partner {
                supports_pd: Some(false),
                ..Default::default()
            }),
            contract: Some(Contract {
                online: false,
                status: "Discharging".into(),
                voltage_mv: 5000,
                current_ma: 3000,
                ..Default::default()
            }),
            ..Default::default()
        };
        let (text, _) = render_status(&[port], false);
        assert!(text.contains("Port 3 [host/source, USB 3.0A, CC2]\n"));
        assert!(text.contains("  Partner: device (no PD, no identity reported)\n"));
        assert!(text.contains("  Contract: 5.0V 3.0A 15W (USB Type-C 3.0A, supplying)\n"));
    }

    #[test]
    fn test_cable_without_identity_uses_kernel_attributes() {
        let port = sink_port(
            Some(charging(20000, 3000)),
            Some(Partner {
                supports_pd: Some(true),
                ..Default::default()
            }),
        );
        let port = Port {
            cable: Some(Cable {
                cable_type: CableType::Passive,
                plug_type: Some("type-c".into()),
                ..Default::default()
            }),
            ..port
        };
        let (text, _) = render_status(&[port], false);
        assert!(text.contains("  Partner: device (PD, no identity reported)\n"));
        assert!(text.contains("  Cable: passive (type-c plug, no e-marker data)\n"));
    }

    #[test]
    fn test_contract_overrides_stale_partner_pd_flag() {
        let port = sink_port(
            Some(charging(20000, 3000)),
            Some(Partner {
                supports_pd: Some(false),
                ..Default::default()
            }),
        );
        assert_eq!(port.partner_supports_pd(), Some(true));
        let (text, _) = render_status(&[port], false);
        assert!(text.contains("  Partner: device (PD, no identity reported)\n"));
    }

    #[test]
    fn test_online_supply_overrides_stale_source_role() {
        let port = Port {
            power_role: PowerRole::Source,
            ..sink_port(Some(charging(20000, 4700)), Some(Partner::default()))
        };
        assert_eq!(
            contract_summary(&port).as_deref(),
            Some("20.0V 4.7A 94W (PD, charging)")
        );
        let (text, _) = render_status(&[port], false);
        assert!(text.contains("Port 2 [host/sink (kernel says source), PD]\n"));
    }

    #[test]
    fn test_pd_source_without_values() {
        let port = Port {
            power_role: PowerRole::Source,
            power_operation_mode: PowerOpMode::UsbPowerDelivery,
            partner: Some(Partner::default()),
            contract: Some(Contract {
                pd: true,
                status: "Discharging".into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            contract_summary(&port).as_deref(),
            Some("PD, supplying (values not exposed by the kernel while sourcing)")
        );
    }

    #[test]
    fn test_format_avs_pdo() {
        let s = format_pdo(&Pdo::SprAvs {
            max_current_9v_to_15v_ma: 3000,
            max_current_15v_to_20v_ma: 2250,
        });
        assert_eq!(s, "9.0-20.0V 3.00A (to 15V) / 2.25A (to 20V) [SPR AVS]");
    }
}
