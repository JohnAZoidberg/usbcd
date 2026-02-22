use crate::types::*;
use std::fs;
use std::path::Path;

const TYPEC_SYSFS: &str = "/sys/class/typec";

/// Scan all USB Type-C ports from sysfs.
pub fn scan_ports() -> Vec<Port> {
    scan_ports_at(Path::new(TYPEC_SYSFS))
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

    let partner = read_partner(base, n);
    let cable = read_cable(base, n);

    Some(Port {
        number: n,
        data_role,
        power_role,
        partner,
        cable,
    })
}

fn read_partner(base: &Path, port_num: u32) -> Option<Partner> {
    let partner_dir = base.join(format!("port{port_num}/port{port_num}-partner"));
    if !partner_dir.is_dir() {
        return None;
    }

    let identity = read_pd_identity(&partner_dir);
    let pd_revision = read_pd_revision(&partner_dir);
    let alt_modes = read_alt_modes(base, port_num, "partner");

    Some(Partner {
        identity,
        pd_revision,
        alt_modes,
    })
}

fn read_cable(base: &Path, port_num: u32) -> Option<Cable> {
    let cable_dir = base.join(format!("port{port_num}/port{port_num}-cable"));
    if !cable_dir.is_dir() {
        return None;
    }

    let identity = read_pd_identity(&cable_dir);
    let pd_revision = read_pd_revision(&cable_dir);
    let cable_type = determine_cable_type(&identity);
    let plug_alt_modes = read_alt_modes(base, port_num, "plug0");

    Some(Cable {
        identity,
        pd_revision,
        cable_type,
        plug_alt_modes,
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

fn read_pd_revision(dir: &Path) -> PdRevision {
    let path = dir.join("usb_power_delivery_revision");
    let s = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return PdRevision::None,
    };
    let s = s.trim();
    match s {
        "2.0" => PdRevision::V2_0,
        "3.0" => PdRevision::V3_0,
        "3.1" => PdRevision::V3_1,
        "3.2" => PdRevision::V3_2,
        _ => PdRevision::None,
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
    port_dir
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

        let partner = p.partner.as_ref().unwrap();
        assert_eq!(partner.identity.id_header_vdo, 0x6c0004b4);
        assert_eq!(partner.alt_modes.len(), 1);
        assert_eq!(partner.alt_modes[0].svid, 0xff01);

        let cable = p.cable.as_ref().unwrap();
        assert_eq!(cable.identity.id_header_vdo, 0x18002e98);
        assert_eq!(cable.cable_type, CableType::Passive);
    }
}
