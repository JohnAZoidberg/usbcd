// Machine-readable `usbc status --json`, without a serializer dependency.
//
// The shape mirrors the sysfs model: one object per port with the partner,
// cable, PDO lists and contract as read, plus the quality warnings by id.

use crate::quality;
use crate::types::*;
use std::fmt::Write;

pub fn render_json(ports: &[Port]) -> String {
    let mut out = String::new();
    out.push_str("{\"ports\":[");
    for (i, port) in ports.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        write_port(&mut out, port);
    }
    out.push_str("]}\n");
    out
}

fn write_port(out: &mut String, port: &Port) {
    let warnings = quality::check_cable_quality(port);
    out.push('{');
    field(out, "number", &port.number.to_string());
    field(out, "data_role", &quoted(&port.data_role.to_string()));
    field(out, "power_role", &quoted(&port.power_role.to_string()));
    field(out, "power_operation_mode", &quoted(&opmode_id(&port.power_operation_mode)));
    field(out, "connected", &port.is_connected().to_string());
    field(
        out,
        "orientation",
        &port
            .orientation
            .map(|o| quoted(&o.to_string()))
            .unwrap_or_else(|| "null".into()),
    );
    field(out, "usb_capability", &string_list(&port.usb_capability));
    field(
        out,
        "usb_capability_default",
        &port
            .usb_capability_default
            .as_deref()
            .map(quoted)
            .unwrap_or_else(|| "null".into()),
    );
    field(out, "vconn_source", &opt_bool(port.vconn_source));
    field(out, "contract", &contract(port.contract.as_ref()));
    field(out, "partner", &partner(port.partner.as_ref()));
    field(out, "cable", &cable(port.cable.as_ref()));
    field(out, "source_pdos", &pdos(port.power.as_ref().map(|p| &p.source[..])));
    field(out, "sink_pdos", &pdos(port.power.as_ref().map(|p| &p.sink[..])));
    let ids: Vec<String> = warnings.iter().map(|w| quoted(w.id())).collect();
    field_last(out, "warnings", &format!("[{}]", ids.join(",")));
    out.push('}');
}

fn opmode_id(mode: &PowerOpMode) -> String {
    match mode {
        PowerOpMode::Default => "default".into(),
        PowerOpMode::Current15A => "1.5A".into(),
        PowerOpMode::Current30A => "3.0A".into(),
        PowerOpMode::UsbPowerDelivery => "usb_power_delivery".into(),
        PowerOpMode::Other(s) => s.clone(),
        PowerOpMode::None => "unknown".into(),
    }
}

fn contract(c: Option<&Contract>) -> String {
    let Some(c) = c else { return "null".into() };
    let mut s = String::from("{");
    field(&mut s, "online", &c.online.to_string());
    field(&mut s, "status", &quoted(&c.status));
    field(&mut s, "pd", &c.pd.to_string());
    field(&mut s, "pps", &c.pps.to_string());
    field(&mut s, "voltage_mv", &c.voltage_mv.to_string());
    field(&mut s, "current_ma", &c.current_ma.to_string());
    field(&mut s, "current_max_ma", &c.current_max_ma.to_string());
    field(&mut s, "voltage_min_mv", &c.voltage_min_mv.to_string());
    field_last(&mut s, "voltage_max_mv", &c.voltage_max_mv.to_string());
    s.push('}');
    s
}

fn identity(id: &PdIdentity) -> String {
    if !id.is_present() {
        return "null".into();
    }
    format!(
        "{{\"id_header\":\"{:#010x}\",\"cert_stat\":\"{:#010x}\",\"product\":\"{:#010x}\",\
         \"product_type_vdo1\":\"{:#010x}\",\"product_type_vdo2\":\"{:#010x}\",\"product_type_vdo3\":\"{:#010x}\"}}",
        id.id_header_vdo,
        id.cert_stat_vdo,
        id.product_vdo,
        id.product_type_vdo1,
        id.product_type_vdo2,
        id.product_type_vdo3
    )
}

fn alt_modes(modes: &[AltMode]) -> String {
    let items: Vec<String> = modes
        .iter()
        .map(|m| format!("{{\"svid\":\"{:#06x}\",\"vdo\":\"{:#010x}\"}}", m.svid, m.vdo))
        .collect();
    format!("[{}]", items.join(","))
}

fn partner(p: Option<&Partner>) -> String {
    let Some(p) = p else { return "null".into() };
    let mut s = String::from("{");
    field(&mut s, "identity", &identity(&p.identity));
    field(&mut s, "pd_revision", &pd_revision(p.pd_revision));
    field(&mut s, "supports_pd", &opt_bool(p.supports_pd));
    field(
        &mut s,
        "number_of_alternate_modes",
        &p.num_alt_modes
            .map(|n| n.to_string())
            .unwrap_or_else(|| "null".into()),
    );
    field(
        &mut s,
        "accessory_mode",
        &p.accessory_mode
            .as_deref()
            .map(quoted)
            .unwrap_or_else(|| "null".into()),
    );
    field(&mut s, "alt_modes", &alt_modes(&p.alt_modes));
    field(&mut s, "source_pdos", &pdos(p.power.as_ref().map(|c| &c.source[..])));
    field_last(&mut s, "sink_pdos", &pdos(p.power.as_ref().map(|c| &c.sink[..])));
    s.push('}');
    s
}

fn cable(c: Option<&Cable>) -> String {
    let Some(c) = c else { return "null".into() };
    let mut s = String::from("{");
    field(&mut s, "type", &quoted(&c.cable_type.to_string()));
    field(
        &mut s,
        "plug_type",
        &c.plug_type
            .as_deref()
            .map(quoted)
            .unwrap_or_else(|| "null".into()),
    );
    field(&mut s, "pd_revision", &pd_revision(c.pd_revision));
    field(&mut s, "identity", &identity(&c.identity));
    field_last(&mut s, "plug_alt_modes", &alt_modes(&c.plug_alt_modes));
    s.push('}');
    s
}

fn pd_revision(rev: PdRevision) -> String {
    match rev {
        PdRevision::None => "null".into(),
        PdRevision::V2_0 => quoted("2.0"),
        PdRevision::V3_0 => quoted("3.0"),
        PdRevision::V3_1 => quoted("3.1"),
        PdRevision::V3_2 => quoted("3.2"),
    }
}

fn pdos(list: Option<&[Pdo]>) -> String {
    let Some(list) = list else { return "[]".into() };
    let items: Vec<String> = list
        .iter()
        .map(|pdo| match pdo {
            Pdo::FixedSupply {
                voltage_mv,
                current_ma,
            } => format!(
                "{{\"type\":\"fixed\",\"voltage_mv\":{voltage_mv},\"current_ma\":{current_ma}}}"
            ),
            Pdo::VariableSupply {
                min_voltage_mv,
                max_voltage_mv,
                current_ma,
            } => format!(
                "{{\"type\":\"variable\",\"min_voltage_mv\":{min_voltage_mv},\"max_voltage_mv\":{max_voltage_mv},\"current_ma\":{current_ma}}}"
            ),
            Pdo::Battery {
                min_voltage_mv,
                max_voltage_mv,
                power_mw,
            } => format!(
                "{{\"type\":\"battery\",\"min_voltage_mv\":{min_voltage_mv},\"max_voltage_mv\":{max_voltage_mv},\"power_mw\":{power_mw}}}"
            ),
            Pdo::ProgrammableSupply {
                min_voltage_mv,
                max_voltage_mv,
                max_current_ma,
            } => format!(
                "{{\"type\":\"pps\",\"min_voltage_mv\":{min_voltage_mv},\"max_voltage_mv\":{max_voltage_mv},\"max_current_ma\":{max_current_ma}}}"
            ),
            Pdo::SprAvs {
                max_current_9v_to_15v_ma,
                max_current_15v_to_20v_ma,
            } => format!(
                "{{\"type\":\"spr_avs\",\"min_voltage_mv\":9000,\"max_voltage_mv\":20000,\"max_current_9v_to_15v_ma\":{max_current_9v_to_15v_ma},\"max_current_15v_to_20v_ma\":{max_current_15v_to_20v_ma}}}"
            ),
        })
        .collect();
    format!("[{}]", items.join(","))
}

fn string_list(items: &[String]) -> String {
    let q: Vec<String> = items.iter().map(|s| quoted(s)).collect();
    format!("[{}]", q.join(","))
}

fn opt_bool(v: Option<bool>) -> String {
    v.map(|b| b.to_string()).unwrap_or_else(|| "null".into())
}

fn field(out: &mut String, key: &str, value: &str) {
    let _ = write!(out, "\"{key}\":{value},");
}

fn field_last(out: &mut String, key: &str, value: &str) {
    let _ = write!(out, "\"{key}\":{value}");
}

fn quoted(s: &str) -> String {
    let mut q = String::with_capacity(s.len() + 2);
    q.push('"');
    for ch in s.chars() {
        match ch {
            '"' => q.push_str("\\\""),
            '\\' => q.push_str("\\\\"),
            '\n' => q.push_str("\\n"),
            '\r' => q.push_str("\\r"),
            '\t' => q.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(q, "\\u{:04x}", c as u32);
            }
            c => q.push(c),
        }
    }
    q.push('"');
    q
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quoted_escapes() {
        assert_eq!(quoted("a\"b\\c\n"), "\"a\\\"b\\\\c\\n\"");
    }

    #[test]
    fn test_render_minimal_port() {
        let port = Port {
            number: 2,
            data_role: DataRole::Host,
            power_role: PowerRole::Sink,
            power_operation_mode: PowerOpMode::UsbPowerDelivery,
            contract: Some(Contract {
                online: true,
                status: "Charging".into(),
                pd: true,
                voltage_mv: 20000,
                current_ma: 3000,
                ..Default::default()
            }),
            ..Default::default()
        };
        let json = render_json(&[port]);
        assert!(json.starts_with("{\"ports\":[{\"number\":2,"));
        assert!(json.contains("\"connected\":true"));
        assert!(json.contains("\"contract\":{\"online\":true,\"status\":\"Charging\",\"pd\":true,\"pps\":false,\"voltage_mv\":20000,\"current_ma\":3000,"));
        assert!(json.contains("\"partner\":null"));
        assert!(json.contains("\"warnings\":[]}"));
        assert!(json.ends_with("]}\n"));
    }
}
