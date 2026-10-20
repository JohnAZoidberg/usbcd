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

fn write_port(out: &mut String, port: &Port, verbose: bool) {
    // Annotate the header with the operation mode whenever a partner is
    // electrically connected — this is the only signal we have when the kernel
    // didn't expose a portN-partner directory (e.g. partner ignored Discover
    // Identity but a PD power contract is still in force).
    let mode_tag = match &port.power_operation_mode {
        PowerOpMode::UsbPowerDelivery => ", PD",
        PowerOpMode::Current15A => ", USB 1.5A",
        PowerOpMode::Current30A => ", USB 3.0A",
        PowerOpMode::Other(_) => ", connected",
        PowerOpMode::Default | PowerOpMode::None => "",
    };
    w!(
        out,
        "Port {} [{}/{}{mode_tag}]",
        port.number, port.data_role, port.power_role
    );

    if verbose {
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
            // No partner directory in sysfs. If the kernel reports an active
            // power contract anyway, surface that — otherwise the user is left
            // believing nothing is plugged in even though a charger is.
            if port.power_operation_mode.has_connection() {
                w!(
                    out,
                    "  Partner: connected ({} — no PD identity reported)",
                    port.power_operation_mode
                );
                write_local_power_summary(out, port, verbose);
            } else {
                w!(out, "  (nothing connected)");
            }
        }
    }

    if let Some(cable) = &port.cable {
        write_cable(out, cable, verbose);
    }
}

/// Write a one-line summary of the port's own PD capabilities, chosen by the
/// active power role. For a sink port this is the laptop's input limit; for a
/// source port this is what the laptop offers. Skipped in verbose mode because
/// the full PDO list is written above.
fn write_local_power_summary(out: &mut String, port: &Port, verbose: bool) {
    if verbose {
        return;
    }
    let Some(power) = &port.power else { return };
    match port.power_role {
        PowerRole::Sink if !power.sink.is_empty() => {
            w!(out, "    Port can sink: {}", format_power_summary(&power.sink));
        }
        PowerRole::Source if !power.source.is_empty() => {
            w!(
                out,
                "    Port can source: {}",
                format_power_summary(&power.source)
            );
        }
        _ => {}
    }
}

fn write_partner(out: &mut String, partner: &Partner, port: &Port, verbose: bool) {
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
    }

    // Power info
    if !verbose {
        // Normal mode: one-line power summary. Show the *relevant* side of the
        // port's caps based on the active power role — source-caps when the
        // laptop is sourcing, sink-caps when sinking (the input limit).
        if let Some(power) = &port.power {
            match port.power_role {
                PowerRole::Source if !power.source.is_empty() => {
                    w!(
                        out,
                        "    Port can source: {}",
                        format_power_summary(&power.source)
                    );
                }
                PowerRole::Sink if !power.sink.is_empty() => {
                    w!(
                        out,
                        "    Port can sink: {}",
                        format_power_summary(&power.sink)
                    );
                }
                _ => {}
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
    }
}

/// Format a one-line power summary from a list of PDOs.
/// E.g. "5/9/15/20V 5A 100W (PPS 3.3-21.0V)"
fn format_power_summary(pdos: &[Pdo]) -> String {
    let mut voltages = Vec::new();
    let mut max_current_ma: u32 = 0;
    let mut max_power_mw: u64 = 0;
    let mut pps_ranges = Vec::new();

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

    result
}
