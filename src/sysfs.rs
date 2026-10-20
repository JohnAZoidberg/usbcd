use crate::types::*;
use std::fs;
use std::path::{Path, PathBuf};

const TYPEC_SYSFS: &str = "/sys/class/typec";

/// Scan all USB Type-C ports from sysfs. `USBC_SYSFS_ROOT` overrides the
/// location, for trying out the UI against a fake tree.
pub fn scan_ports() -> Vec<Port> {
    match std::env::var_os("USBC_SYSFS_ROOT") {
        Some(root) => scan_ports_at(Path::new(&root)),
        None => scan_ports_at(Path::new(TYPEC_SYSFS)),
    }
}

/// Scan a single port by number.
pub fn scan_port(n: u32) -> Option<Port> {
    scan_port_at(Path::new(TYPEC_SYSFS), n)
}

pub fn scan_ports_at(base: &Path) -> Vec<Port> {
    let mut ports = Vec::new();
    let entries = match fs::read_dir(base) {
        Ok(e) => e,
        Err(_) => return ports,
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(num) = parse_port_number(&name) {
            if let Some(port) = read_port(base, num) {
                ports.push(port);
            }
        }
    }
    ports.sort_by_key(|p| p.number);
    ports
}

pub fn scan_port_at(base: &Path, n: u32) -> Option<Port> {
    read_port(base, n)
}

fn parse_port_number(name: &str) -> Option<u32> {
    let s = name.strip_prefix("port")?;
    // Only match bare "portN", not "port0-partner" etc.
    if s.contains('-') {
        return None;
    }
    s.parse().ok()
}

fn read_port(base: &Path, n: u32) -> Option<Port> {
    let port_dir = base.join(format!("port{n}"));
    if !port_dir.is_dir() {
        return None;
    }

    let data_role = read_role_file(&port_dir.join("data_role"));
    let power_role = read_power_role_file(&port_dir.join("power_role"));
    let power_operation_mode = read_power_operation_mode(&port_dir.join("power_operation_mode"));
    let (usb_capability, usb_capability_default) =
        read_usb_capability(&port_dir.join("usb_capability"));

    let partner = read_partner(base, n);
    let cable = read_cable(base, n);
    let power = read_power_capabilities(&port_dir);

    Some(Port {
        number: n,
        data_role,
        power_role,
        power_operation_mode,
        partner,
        cable,
        power,
        orientation: read_orientation(&port_dir.join("orientation")),
        usb_capability,
        usb_capability_default,
        vconn_source: read_yes_no(&port_dir.join("vconn_source")),
        contract: read_contract(&port_dir, n),
    })
}

fn read_power_operation_mode(path: &Path) -> PowerOpMode {
    let s = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return PowerOpMode::None,
    };
    match s.trim() {
        "default" => PowerOpMode::Default,
        "1.5A" => PowerOpMode::Current15A,
        "3.0A" => PowerOpMode::Current30A,
        "usb_power_delivery" => PowerOpMode::UsbPowerDelivery,
        other => PowerOpMode::Other(other.to_string()),
    }
}

fn read_orientation(path: &Path) -> Option<Orientation> {
    let s = read_trimmed(path)?;
    Some(match s.as_str() {
        "normal" => Orientation::Normal,
        "reverse" => Orientation::Reverse,
        _ => Orientation::Unknown,
    })
}

/// `usb_capability` reads like "usb2 [usb3]": the supported modes, with the
/// default one in brackets.
fn read_usb_capability(path: &Path) -> (Vec<String>, Option<String>) {
    let Some(s) = read_trimmed(path) else {
        return (Vec::new(), None);
    };
    let mut supported = Vec::new();
    let mut default = None;
    for token in s.split_whitespace() {
        match token.strip_prefix('[').and_then(|t| t.strip_suffix(']')) {
            Some(inner) => {
                default = Some(inner.to_string());
                supported.push(inner.to_string());
            }
            None => supported.push(token.to_string()),
        }
    }
    (supported, default)
}

/// The negotiated power, from the `*-source-psy-*` power supply the port
/// driver registers on the port's parent device.
fn read_contract(port_dir: &Path, port_num: u32) -> Option<Contract> {
    let dir = find_source_psy(&port_dir.join("device/power_supply"), port_num)?;
    let usb_type = read_trimmed(&dir.join("usb_type"))
        .map(|s| extract_active_role(&s))
        .unwrap_or_default();
    Some(Contract {
        online: read_decimal_file(&dir.join("online")).unwrap_or(0) != 0,
        status: read_trimmed(&dir.join("status")).unwrap_or_else(|| "Unknown".to_string()),
        pd: usb_type == "PD" || usb_type == "PD_PPS",
        pps: usb_type == "PD_PPS",
        voltage_mv: micro_to_milli(&dir.join("voltage_now")),
        current_ma: micro_to_milli(&dir.join("current_now")),
        current_max_ma: micro_to_milli(&dir.join("current_max")),
        voltage_min_mv: micro_to_milli(&dir.join("voltage_min")),
        voltage_max_mv: micro_to_milli(&dir.join("voltage_max")),
    })
}

/// Pick this port's supply among those registered on the parent device.
/// UCSI registers one supply per connector on a single device and names it
/// with the connector number, which is the port index plus one; tcpm has one
/// device and one supply per port.
fn find_source_psy(psy_root: &Path, port_num: u32) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = fs::read_dir(psy_root)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().contains("-source-psy-"))
                .unwrap_or(false)
        })
        .collect();
    candidates.sort();
    if candidates.len() == 1 {
        return candidates.pop();
    }
    candidates.into_iter().find(|p| {
        p.file_name()
            .and_then(|n| trailing_number(&n.to_string_lossy()))
            == Some(port_num + 1)
    })
}

fn trailing_number(name: &str) -> Option<u32> {
    let digits: String = name
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    digits.parse().ok()
}

/// Power-supply class values are in microvolts and microamps.
fn micro_to_milli(path: &Path) -> u32 {
    read_decimal_file(path).unwrap_or(0) / 1000
}

fn read_partner(base: &Path, port_num: u32) -> Option<Partner> {
    let partner_dir = base.join(format!("port{port_num}/port{port_num}-partner"));
    if !partner_dir.is_dir() {
        return None;
    }

    let identity = read_pd_identity(&partner_dir);
    let pd_revision = read_pd_revision(&partner_dir);
    let alt_modes = read_alt_modes(base, port_num, "partner");
    let power = read_power_capabilities(&partner_dir);

    Some(Partner {
        identity,
        pd_revision,
        alt_modes,
        power,
        supports_pd: read_yes_no(&partner_dir.join("supports_usb_power_delivery")),
        num_alt_modes: read_decimal_file(&partner_dir.join("number_of_alternate_modes")),
        accessory_mode: read_trimmed(&partner_dir.join("accessory_mode")),
        identity_exposed: partner_dir.join("identity").is_dir(),
    })
}

fn read_cable(base: &Path, port_num: u32) -> Option<Cable> {
    let cable_dir = base.join(format!("port{port_num}/port{port_num}-cable"));
    if !cable_dir.is_dir() {
        return None;
    }

    let identity = read_pd_identity(&cable_dir);
    let pd_revision = read_pd_revision(&cable_dir);
    let mut cable_type = determine_cable_type(&identity);
    if cable_type == CableType::Unknown {
        // No e-marker identity: the port driver may still know from its own
        // cable query (UCSI GET_CABLE_PROPERTY).
        cable_type = match read_trimmed(&cable_dir.join("type")).as_deref() {
            Some("active") => CableType::Active,
            Some("passive") => CableType::Passive,
            _ => CableType::Unknown,
        };
    }
    let plug_alt_modes = read_alt_modes(base, port_num, "plug0");

    Some(Cable {
        identity,
        pd_revision,
        cable_type,
        plug_alt_modes,
        plug_type: read_trimmed(&cable_dir.join("plug_type")),
    })
}

fn determine_cable_type(identity: &PdIdentity) -> CableType {
    use crate::vdo::*;
    let product_type = field(identity.id_header_vdo, ID_HEADER_VDO_PRODUCT_TYPE_OFFSET, ID_HEADER_VDO_PRODUCT_TYPE_MASK);
    match product_type {
        ID_HEADER_VDO_PRODUCT_TYPE_CABLE_PASSIVE => CableType::Passive,
        ID_HEADER_VDO_PRODUCT_TYPE_CABLE_ACTIVE => CableType::Active,
        _ => CableType::Unknown,
    }
}

fn read_pd_identity(dir: &Path) -> PdIdentity {
    let id_dir = dir.join("identity");
    PdIdentity {
        id_header_vdo: read_hex_file(&id_dir.join("id_header")),
        cert_stat_vdo: read_hex_file(&id_dir.join("cert_stat")),
        product_vdo: read_hex_file(&id_dir.join("product")),
        product_type_vdo1: read_hex_file(&id_dir.join("product_type_vdo1")),
        product_type_vdo2: read_hex_file(&id_dir.join("product_type_vdo2")),
        product_type_vdo3: read_hex_file(&id_dir.join("product_type_vdo3")),
    }
}

/// The PD revision from `usb_power_delivery_revision`, or from the linked
/// `usb_power_delivery/revision` when the former reads "0.0": UCSI fills the
/// PD object from the partner's capabilities but not the attribute.
fn read_pd_revision(dir: &Path) -> PdRevision {
    let parse = |path: PathBuf| match read_trimmed(&path).as_deref() {
        Some("2.0") => PdRevision::V2_0,
        Some("3.0") => PdRevision::V3_0,
        Some("3.1") => PdRevision::V3_1,
        Some("3.2") => PdRevision::V3_2,
        _ => PdRevision::None,
    };
    match parse(dir.join("usb_power_delivery_revision")) {
        PdRevision::None => parse(dir.join("usb_power_delivery/revision")),
        rev => rev,
    }
}

fn read_alt_modes(base: &Path, port_num: u32, kind: &str) -> Vec<AltMode> {
    // Alt modes live as directories: portN-partner.M or portN-plug0.M
    let mut modes = Vec::new();
    let parent = if kind == "partner" {
        base.join(format!("port{port_num}/port{port_num}-partner"))
    } else {
        // SOP' plug alt modes: portN-plug0.M inside portN-cable/portN-plug0/
        base.join(format!("port{port_num}/port{port_num}-cable/port{port_num}-plug0"))
    };

    let prefix = format!("port{port_num}-{kind}.");

    let entries = match fs::read_dir(&parent) {
        Ok(e) => e,
        Err(_) => return modes,
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.starts_with(&prefix) {
            continue;
        }
        let mode_dir = entry.path();
        if !mode_dir.is_dir() {
            continue;
        }
        let svid = read_hex_file_u16(&mode_dir.join("svid"));
        let vdo = read_hex_file(&mode_dir.join("vdo"));
        modes.push(AltMode { svid, vdo });
    }
    modes
}

fn read_power_capabilities(dir: &Path) -> Option<PowerCapabilities> {
    let pd_dir = dir.join("usb_power_delivery");
    if !pd_dir.exists() {
        return None;
    }

    let source = read_pdos(&pd_dir.join("source-capabilities"));
    let sink = read_pdos(&pd_dir.join("sink-capabilities"));

    if source.is_empty() && sink.is_empty() {
        return None;
    }

    Some(PowerCapabilities { source, sink })
}

fn read_pdos(caps_dir: &Path) -> Vec<Pdo> {
    let entries = match fs::read_dir(caps_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut pdos: Vec<(u32, Pdo)> = Vec::new();

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let pdo_dir = entry.path();
        if !pdo_dir.is_dir() {
            continue;
        }

        // Parse "N:type_name" format
        let (position, pdo_type) = match name_str.split_once(':') {
            Some((pos, typ)) => match pos.parse::<u32>() {
                Ok(n) => (n, typ),
                Err(_) => continue,
            },
            None => continue,
        };

        let pdo = match pdo_type {
            "fixed_supply" => {
                let voltage_mv = read_decimal_file(&pdo_dir.join("voltage"));
                let current_ma = read_decimal_file(&pdo_dir.join("maximum_current"))
                    .or_else(|| read_decimal_file(&pdo_dir.join("operational_current")))
                    .unwrap_or(0);
                Pdo::FixedSupply {
                    voltage_mv: voltage_mv.unwrap_or(0),
                    current_ma,
                }
            }
            "variable_supply" => {
                let min_voltage_mv = read_decimal_file(&pdo_dir.join("minimum_voltage")).unwrap_or(0);
                let max_voltage_mv = read_decimal_file(&pdo_dir.join("maximum_voltage")).unwrap_or(0);
                let current_ma = read_decimal_file(&pdo_dir.join("maximum_current"))
                    .or_else(|| read_decimal_file(&pdo_dir.join("operational_current")))
                    .unwrap_or(0);
                Pdo::VariableSupply {
                    min_voltage_mv,
                    max_voltage_mv,
                    current_ma,
                }
            }
            "battery" => {
                let min_voltage_mv = read_decimal_file(&pdo_dir.join("minimum_voltage")).unwrap_or(0);
                let max_voltage_mv = read_decimal_file(&pdo_dir.join("maximum_voltage")).unwrap_or(0);
                let power_mw = read_decimal_file(&pdo_dir.join("maximum_power"))
                    .or_else(|| read_decimal_file(&pdo_dir.join("operational_power")))
                    .unwrap_or(0);
                Pdo::Battery {
                    min_voltage_mv,
                    max_voltage_mv,
                    power_mw,
                }
            }
            "programmable_supply" => {
                let min_voltage_mv = read_decimal_file(&pdo_dir.join("minimum_voltage")).unwrap_or(0);
                let max_voltage_mv = read_decimal_file(&pdo_dir.join("maximum_voltage")).unwrap_or(0);
                let max_current_ma = read_decimal_file(&pdo_dir.join("maximum_current")).unwrap_or(0);
                Pdo::ProgrammableSupply {
                    min_voltage_mv,
                    max_voltage_mv,
                    max_current_ma,
                }
            }
            // Kernel 6.x and later export SPR AVS APDOs; EPR AVS is not
            // exported at all, so there is nothing else to parse.
            "spr_adjustable_voltage_supply" => Pdo::SprAvs {
                max_current_9v_to_15v_ma: read_decimal_file(&pdo_dir.join("maximum_current_9V_to_15V"))
                    .unwrap_or(0),
                max_current_15v_to_20v_ma: read_decimal_file(&pdo_dir.join("maximum_current_15V_to_20V"))
                    .unwrap_or(0),
            },
            _ => continue,
        };

        pdos.push((position, pdo));
    }

    pdos.sort_by_key(|(pos, _)| *pos);
    pdos.into_iter().map(|(_, pdo)| pdo).collect()
}

fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn read_yes_no(path: &Path) -> Option<bool> {
    match read_trimmed(path)?.as_str() {
        "yes" => Some(true),
        "no" => Some(false),
        _ => None,
    }
}

fn read_decimal_file(path: &Path) -> Option<u32> {
    let s = fs::read_to_string(path).ok()?;
    let s = s.trim();
    // Kernel may emit value with "mV", "mA", "mW" suffix — strip any non-digit suffix.
    let num_str: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    num_str.parse().ok()
}

fn read_hex_file(path: &Path) -> u32 {
    let s = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    parse_hex_u32(s.trim())
}

fn read_hex_file_u16(path: &Path) -> u16 {
    let s = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    parse_hex_u16(s.trim())
}

fn parse_hex_u32(s: &str) -> u32 {
    let s = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    u32::from_str_radix(s, 16).unwrap_or(0)
}

fn parse_hex_u16(s: &str) -> u16 {
    let s = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    u16::from_str_radix(s, 16).unwrap_or(0)
}

fn read_role_file(path: &Path) -> DataRole {
    let s = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return DataRole::None,
    };
    // Format: "[host] device" or "host [device]" or just "host"/"device"
    let active = extract_active_role(&s);
    match active.as_str() {
        "host" => DataRole::Host,
        "device" => DataRole::Device,
        _ => DataRole::None,
    }
}

fn read_power_role_file(path: &Path) -> PowerRole {
    let s = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return PowerRole::None,
    };
    let active = extract_active_role(&s);
    match active.as_str() {
        "source" => PowerRole::Source,
        "sink" => PowerRole::Sink,
        _ => PowerRole::None,
    }
}

/// Extract the active role from sysfs format like "[host] device".
fn extract_active_role(s: &str) -> String {
    let s = s.trim();
    // Look for [bracketed] value first (dual-role port)
    if let Some(start) = s.find('[') {
        if let Some(end) = s.find(']') {
            return s[start + 1..end].trim().to_string();
        }
    }
    // Fall back to the whole string
    s.to_string()
}

/// Create a fake sysfs tree for testing.
#[cfg(test)]
pub fn create_fake_port(
    base: &Path,
    port_num: u32,
    data_role: &str,
    power_role: &str,
) -> std::path::PathBuf {
    let port_dir = base.join(format!("port{port_num}"));
    fs::create_dir_all(&port_dir).unwrap();
    fs::write(port_dir.join("data_role"), data_role).unwrap();
    fs::write(port_dir.join("power_role"), power_role).unwrap();
    // Default to "default" so the port looks idle unless overridden.
    fs::write(port_dir.join("power_operation_mode"), "default").unwrap();
    port_dir
}

#[cfg(test)]
pub fn set_fake_power_operation_mode(base: &Path, port_num: u32, mode: &str) {
    let port_dir = base.join(format!("port{port_num}"));
    fs::write(port_dir.join("power_operation_mode"), mode).unwrap();
}

/// Write one attribute file below the fake tree, creating directories.
#[cfg(test)]
pub fn write_fake_attr(base: &Path, rel_path: &str, value: &str) {
    let path = base.join(rel_path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, value).unwrap();
}

/// Create a fake `*-source-psy-*` power supply for a port, as the kernel's
/// power_supply class would expose it: values in microvolts and microamps,
/// and `usb_type` with the active type in brackets.
#[cfg(test)]
pub fn create_fake_contract(
    base: &Path,
    port_num: u32,
    psy_name: &str,
    attrs: &[(&str, &str)],
) {
    let dir = base.join(format!("port{port_num}/device/power_supply/{psy_name}"));
    fs::create_dir_all(&dir).unwrap();
    for (key, value) in attrs {
        fs::write(dir.join(key), value).unwrap();
    }
}

#[cfg(test)]
pub fn create_fake_partner(
    base: &Path,
    port_num: u32,
    identity: &PdIdentity,
    pd_revision: &str,
    alt_modes: &[(u16, u32)],
) {
    let partner_dir = base.join(format!("port{port_num}/port{port_num}-partner"));
    let id_dir = partner_dir.join("identity");
    fs::create_dir_all(&id_dir).unwrap();
    fs::write(id_dir.join("id_header"), format!("{:#010x}", identity.id_header_vdo)).unwrap();
    fs::write(id_dir.join("cert_stat"), format!("{:#010x}", identity.cert_stat_vdo)).unwrap();
    fs::write(id_dir.join("product"), format!("{:#010x}", identity.product_vdo)).unwrap();
    fs::write(id_dir.join("product_type_vdo1"), format!("{:#010x}", identity.product_type_vdo1)).unwrap();
    fs::write(id_dir.join("product_type_vdo2"), format!("{:#010x}", identity.product_type_vdo2)).unwrap();
    fs::write(id_dir.join("product_type_vdo3"), format!("{:#010x}", identity.product_type_vdo3)).unwrap();
    fs::write(partner_dir.join("usb_power_delivery_revision"), pd_revision).unwrap();

    for (i, (svid, vdo)) in alt_modes.iter().enumerate() {
        let mode_dir = partner_dir.join(format!("port{port_num}-partner.{i}"));
        fs::create_dir_all(&mode_dir).unwrap();
        fs::write(mode_dir.join("svid"), format!("{svid:04x}")).unwrap();
        fs::write(mode_dir.join("vdo"), format!("{vdo:#010x}")).unwrap();
    }
}

#[cfg(test)]
pub fn create_fake_cable(
    base: &Path,
    port_num: u32,
    identity: &PdIdentity,
    pd_revision: &str,
    plug_alt_modes: &[(u16, u32)],
) {
    let cable_dir = base.join(format!("port{port_num}/port{port_num}-cable"));
    let id_dir = cable_dir.join("identity");
    fs::create_dir_all(&id_dir).unwrap();
    fs::write(id_dir.join("id_header"), format!("{:#010x}", identity.id_header_vdo)).unwrap();
    fs::write(id_dir.join("cert_stat"), format!("{:#010x}", identity.cert_stat_vdo)).unwrap();
    fs::write(id_dir.join("product"), format!("{:#010x}", identity.product_vdo)).unwrap();
    fs::write(id_dir.join("product_type_vdo1"), format!("{:#010x}", identity.product_type_vdo1)).unwrap();
    fs::write(id_dir.join("product_type_vdo2"), format!("{:#010x}", identity.product_type_vdo2)).unwrap();
    fs::write(id_dir.join("product_type_vdo3"), format!("{:#010x}", identity.product_type_vdo3)).unwrap();
    fs::write(cable_dir.join("usb_power_delivery_revision"), pd_revision).unwrap();

    // SOP' plug alt modes go in portN-cable/portN-plug0/portN-plug0.M
    let plug_dir = cable_dir.join(format!("port{port_num}-plug0"));
    fs::create_dir_all(&plug_dir).unwrap();
    for (i, (svid, vdo)) in plug_alt_modes.iter().enumerate() {
        let mode_dir = plug_dir.join(format!("port{port_num}-plug0.{i}"));
        fs::create_dir_all(&mode_dir).unwrap();
        fs::write(mode_dir.join("svid"), format!("{svid:04x}")).unwrap();
        fs::write(mode_dir.join("vdo"), format!("{vdo:#010x}")).unwrap();
    }
}

/// Create a fake usb_power_delivery directory tree for testing.
/// `role` is "source-capabilities" or "sink-capabilities".
/// `pdos` is a list of (dir_name, attributes) where dir_name is like "1:fixed_supply".
#[cfg(test)]
pub fn create_fake_pdo_caps(
    dir: &Path,
    role: &str,
    pdos: &[(&str, &[(&str, &str)])],
) {
    let caps_dir = dir.join("usb_power_delivery").join(role);
    for (name, attrs) in pdos {
        let pdo_dir = caps_dir.join(name);
        fs::create_dir_all(&pdo_dir).unwrap();
        for (key, value) in *attrs {
            fs::write(pdo_dir.join(key), value).unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_active_role() {
        assert_eq!(extract_active_role("[host] device"), "host");
        assert_eq!(extract_active_role("host [device]"), "device");
        assert_eq!(extract_active_role("[source] sink"), "source");
        assert_eq!(extract_active_role("source"), "source");
    }

    #[test]
    fn test_parse_hex() {
        assert_eq!(parse_hex_u32("0x18002e98"), 0x18002e98);
        assert_eq!(parse_hex_u32("0x00000000"), 0);
        assert_eq!(parse_hex_u32("18002e98"), 0x18002e98);
    }

    #[test]
    fn test_trailing_number() {
        assert_eq!(trailing_number("ucsi-source-psy-USBC000:003"), Some(3));
        assert_eq!(trailing_number("tcpm-source-psy-4-0022"), Some(22));
        assert_eq!(trailing_number("no-digits"), None);
    }

    #[test]
    fn test_scan_fake_sysfs() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();

        create_fake_port(base, 0, "[host] device", "[source] sink");
        let identity = PdIdentity {
            id_header_vdo: 0x6c0004b4,
            product_type_vdo1: 0x8,
            ..Default::default()
        };
        create_fake_partner(base, 0, &identity, "3.0", &[(0xff01, 0x405)]);

        let cable_id = PdIdentity {
            id_header_vdo: 0x18002e98,
            product_type_vdo1: 0xc1082040,
            ..Default::default()
        };
        create_fake_cable(base, 0, &cable_id, "3.0", &[]);

        let ports = scan_ports_at(base);
        assert_eq!(ports.len(), 1);
        let p = &ports[0];
        assert_eq!(p.number, 0);
        assert_eq!(p.data_role, DataRole::Host);
        assert_eq!(p.power_role, PowerRole::Source);
        assert!(p.partner.is_some());
        assert!(p.cable.is_some());
        assert!(p.is_connected());
        assert_eq!(p.orientation, None);
        assert!(p.contract.is_none());

        let partner = p.partner.as_ref().unwrap();
        assert_eq!(partner.identity.id_header_vdo, 0x6c0004b4);
        assert!(partner.identity_exposed);
        assert_eq!(partner.alt_modes.len(), 1);
        assert_eq!(partner.alt_modes[0].svid, 0xff01);
        assert_eq!(partner.supports_pd, None);

        let cable = p.cable.as_ref().unwrap();
        assert_eq!(cable.identity.id_header_vdo, 0x18002e98);
        assert_eq!(cable.cable_type, CableType::Passive);
    }

    #[test]
    fn test_port_attributes() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        create_fake_port(base, 1, "[host] device", "source [sink]");
        write_fake_attr(base, "port1/orientation", "reverse\n");
        write_fake_attr(base, "port1/usb_capability", "usb2 [usb3]\n");
        write_fake_attr(base, "port1/vconn_source", "no\n");

        let p = scan_port_at(base, 1).unwrap();
        assert_eq!(p.orientation, Some(Orientation::Reverse));
        assert_eq!(p.usb_capability, vec!["usb2", "usb3"]);
        assert_eq!(p.usb_capability_default.as_deref(), Some("usb3"));
        assert_eq!(p.vconn_source, Some(false));
    }

    #[test]
    fn test_contract_from_power_supply() {
        // Two connectors on one UCSI device: port 1 must pick the supply
        // named with connector number 2.
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        create_fake_port(base, 1, "[host] device", "source [sink]");
        set_fake_power_operation_mode(base, 1, "usb_power_delivery");
        create_fake_contract(
            base,
            1,
            "ucsi-source-psy-USBC000:001",
            &[("online", "0"), ("status", "Not charging"), ("usb_type", "[C] PD PD_PPS")],
        );
        create_fake_contract(
            base,
            1,
            "ucsi-source-psy-USBC000:002",
            &[
                ("online", "1"),
                ("status", "Charging"),
                ("usb_type", "C [PD] PD_PPS"),
                ("voltage_now", "20000000"),
                ("current_now", "3000000"),
                ("current_max", "3000000"),
                ("voltage_min", "5000000"),
                ("voltage_max", "20000000"),
            ],
        );

        let p = scan_port_at(base, 1).unwrap();
        let c = p.contract.as_ref().unwrap();
        assert!(c.online);
        assert!(c.pd);
        assert!(!c.pps);
        assert_eq!(c.status, "Charging");
        assert_eq!(c.voltage_mv, 20000);
        assert_eq!(c.current_ma, 3000);
        assert_eq!(c.voltage_max_mv, 20000);
        assert_eq!(c.milliwatts(), 60_000);
        // No partner directory yet, but the supply says we are sinking.
        assert!(p.partner.is_none());
        assert!(p.is_connected());
    }

    #[test]
    fn test_stale_power_mode_is_not_a_connection() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        create_fake_port(base, 0, "[host] device", "source [sink]");
        set_fake_power_operation_mode(base, 0, "usb_power_delivery");
        create_fake_contract(
            base,
            0,
            "ucsi-source-psy-USBC000:001",
            &[("online", "0"), ("status", "Not charging"), ("usb_type", "[C] PD PD_PPS")],
        );

        let p = scan_port_at(base, 0).unwrap();
        assert_eq!(p.power_operation_mode, PowerOpMode::UsbPowerDelivery);
        assert!(!p.is_connected());
    }

    #[test]
    fn test_partner_and_cable_kernel_attributes() {
        // A PD charger that never answered Discover Identity, on a cable the
        // port driver identified itself.
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        create_fake_port(base, 2, "[host] device", "source [sink]");
        write_fake_attr(base, "port2/port2-partner/supports_usb_power_delivery", "yes\n");
        write_fake_attr(base, "port2/port2-partner/number_of_alternate_modes", "0\n");
        write_fake_attr(base, "port2/port2-partner/accessory_mode", "none\n");
        write_fake_attr(base, "port2/port2-partner/usb_power_delivery_revision", "0.0\n");
        write_fake_attr(base, "port2/port2-partner/usb_power_delivery/revision", "3.2\n");
        write_fake_attr(base, "port2/port2-cable/type", "passive\n");
        write_fake_attr(base, "port2/port2-cable/plug_type", "type-c\n");

        let p = scan_port_at(base, 2).unwrap();
        let partner = p.partner.as_ref().unwrap();
        assert!(!partner.identity.is_present());
        assert!(!partner.identity_exposed);
        assert_eq!(partner.supports_pd, Some(true));
        assert_eq!(partner.num_alt_modes, Some(0));
        assert_eq!(partner.accessory_mode.as_deref(), Some("none"));
        assert_eq!(partner.pd_revision, PdRevision::V3_2);

        let cable = p.cable.as_ref().unwrap();
        assert!(!cable.identity.is_present());
        assert_eq!(cable.cable_type, CableType::Passive);
        assert_eq!(cable.plug_type.as_deref(), Some("type-c"));
    }

    #[test]
    fn test_read_pdos() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();

        create_fake_port(base, 0, "[host] device", "[source] sink");

        let port_dir = base.join("port0");
        create_fake_pdo_caps(
            &port_dir,
            "source-capabilities",
            &[
                ("1:fixed_supply", &[("voltage", "5000"), ("maximum_current", "3000")][..]),
                ("2:fixed_supply", &[("voltage", "9000"), ("maximum_current", "3000")]),
                ("3:fixed_supply", &[("voltage", "20000"), ("maximum_current", "5000")]),
                ("4:programmable_supply", &[
                    ("minimum_voltage", "3300"),
                    ("maximum_voltage", "21000"),
                    ("maximum_current", "5000"),
                ]),
                ("5:spr_adjustable_voltage_supply", &[
                    ("maximum_current_9V_to_15V", "3000mA"),
                    ("maximum_current_15V_to_20V", "2250mA"),
                    ("peak_current", "0"),
                ]),
            ],
        );

        let ports = scan_ports_at(base);
        assert_eq!(ports.len(), 1);
        let p = &ports[0];
        let power = p.power.as_ref().unwrap();
        assert_eq!(power.source.len(), 5);
        assert!(matches!(power.source[0], Pdo::FixedSupply { voltage_mv: 5000, current_ma: 3000 }));
        assert!(matches!(power.source[3], Pdo::ProgrammableSupply { min_voltage_mv: 3300, max_voltage_mv: 21000, max_current_ma: 5000 }));
        assert!(matches!(
            power.source[4],
            Pdo::SprAvs { max_current_9v_to_15v_ma: 3000, max_current_15v_to_20v_ma: 2250 }
        ));
        assert!(power.sink.is_empty());
    }
}
