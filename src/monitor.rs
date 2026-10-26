use std::collections::HashMap;
use std::os::unix::io::AsRawFd;
use std::time::Duration;

use crate::display;
use crate::notify;
use crate::quality;
use crate::sysfs;
use crate::types::{CableWarning, Contract, DataRole, Port, PowerRole};
use crate::vdo;

/// Local time as `YYYY-MM-DD HH:MM:SS` for the event log. journald adds its
/// own stamps, but the monitor is also run from a terminal.
pub fn timestamp() -> String {
    let mut now: libc::time_t = 0;
    // SAFETY: plain libc calls on stack-owned values.
    unsafe {
        libc::time(&mut now);
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut tm);
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min,
            tm.tm_sec
        )
    }
}

macro_rules! log {
    ($($arg:tt)*) => {
        eprintln!("{} {}", timestamp(), format!($($arg)*))
    };
}

/// What the monitor compares between scans to decide what to log.
#[derive(Clone, PartialEq)]
struct PortState {
    connected: bool,
    power_role: PowerRole,
    data_role: DataRole,
    contract: Option<Contract>,
    warnings: Vec<CableWarning>,
}

fn state_of(port: &Port) -> PortState {
    PortState {
        connected: port.is_connected(),
        power_role: port.effective_power_role(),
        data_role: port.data_role,
        contract: if port.is_connected() {
            port.contract.clone()
        } else {
            None
        },
        warnings: quality::check_cable_quality(port),
    }
}

/// Run the udev monitor loop: watch for typec events, debounce, log what
/// changed, check quality, and send desktop notifications.
pub async fn run_monitor() -> Result<(), Box<dyn std::error::Error>> {
    log!("usbc monitor: starting");

    // Initial scan of already-connected devices.
    let ports = sysfs::scan_ports();
    if ports.is_empty() {
        log!("  no USB-C ports found");
    }
    let mut prev: HashMap<u32, PortState> = HashMap::new();
    for port in &ports {
        let state = state_of(port);
        if state.connected {
            log_connect(port);
        }
        for w in &state.warnings {
            log!("  port {}: {w}", port.number);
            if let Err(e) = notify::send_notification(port.number, w).await {
                log!("  failed to send notification: {e}");
            }
        }
        prev.insert(port.number, state);
    }

    let mut rx = spawn_udev_watcher();

    log!("usbc monitor: watching for USB-C events");

    // A single plug produces a burst of partner, cable, alt mode and PDO
    // uevents over a second or two while the kernel runs discovery.
    let debounce = Duration::from_millis(2000);

    loop {
        if rx.recv().await.is_none() {
            log!("usbc monitor: udev thread exited");
            break;
        }

        tokio::time::sleep(debounce).await;
        while rx.try_recv().is_ok() {}

        let ports = sysfs::scan_ports();
        for port in &ports {
            let new = state_of(port);
            let n = port.number;

            match prev.get(&n) {
                None => {
                    if new.connected {
                        log_connect(port);
                    }
                }
                Some(old) => {
                    if new.connected && !old.connected {
                        log_connect(port);
                    } else if !new.connected && old.connected {
                        log!("port {n}: disconnected");
                    } else if new.connected {
                        if new.power_role != old.power_role {
                            log!("port {n}: power role {} → {}", old.power_role, new.power_role);
                        }
                        if new.data_role != old.data_role {
                            log!("port {n}: data role {} → {}", old.data_role, new.data_role);
                        }
                        if new.contract != old.contract {
                            match display::contract_summary(port) {
                                Some(text) => log!("port {n}: contract {text}"),
                                None => log!("port {n}: contract ended"),
                            }
                        }
                    }
                }
            }

            // Notify on warnings that weren't there before.
            for w in &new.warnings {
                let is_new = match prev.get(&n) {
                    Some(old) => !old.warnings.contains(w),
                    None => true,
                };
                if is_new {
                    log!("  port {n}: {w}");
                    if let Err(e) = notify::send_notification(n, w).await {
                        log!("  failed to send notification: {e}");
                    }
                }
            }

            prev.insert(n, new);
        }

        // Clean up ports that disappeared.
        let active: Vec<u32> = ports.iter().map(|p| p.number).collect();
        prev.retain(|k, _| active.contains(k));
    }

    Ok(())
}

/// Watch udev for USB-C changes on a dedicated thread. Each wakeup sends one
/// message; a single plug event usually produces a burst, so receivers should
/// debounce. The channel closes if the monitor can't be created.
pub fn spawn_udev_watcher() -> tokio::sync::mpsc::UnboundedReceiver<()> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<()>();
    std::thread::spawn(move || {
        let socket = match udev::MonitorBuilder::new()
            .and_then(|b| b.match_subsystem("typec"))
            .and_then(|b| b.match_subsystem("usb_power_delivery"))
            .and_then(|b| b.match_subsystem("power_supply"))
            // A Billboard enumerates after the alt mode timeout, once the
            // typec burst has settled.
            .and_then(|b| b.match_subsystem("usb"))
            .and_then(|b| b.listen())
        {
            Ok(s) => s,
            Err(e) => {
                eprintln!("usbc: failed to create udev monitor: {e}");
                return;
            }
        };

        // The udev socket needs poll() to block-wait for events, then
        // receive_device() to read them.
        let fd = socket.as_raw_fd();
        loop {
            let mut pollfd = libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            };
            let ret = unsafe { libc::poll(&mut pollfd as *mut _, 1, -1) };
            if ret < 0 {
                break;
            }
            if (pollfd.revents & libc::POLLIN) == 0 {
                continue;
            }

            while socket.iter().next().is_some() {}

            if tx.send(()).is_err() {
                break;
            }
        }
    });
    rx
}

fn log_connect(port: &Port) {
    let who = match &port.partner {
        Some(partner) if partner.identity.is_present() => {
            let product_type = vdo::field(
                partner.identity.id_header_vdo,
                vdo::ID_HEADER_VDO_PRODUCT_TYPE_OFFSET,
                vdo::ID_HEADER_VDO_PRODUCT_TYPE_MASK,
            );
            match product_type {
                vdo::ID_HEADER_VDO_PRODUCT_TYPE_UFP_HUB => "hub",
                vdo::ID_HEADER_VDO_PRODUCT_TYPE_UFP_PERIPHERAL => "peripheral",
                vdo::ID_HEADER_VDO_PRODUCT_TYPE_UFP_AMA => "alt mode adapter",
                _ => "device",
            }
            .to_string()
        }
        Some(_) => match port.partner_supports_pd() {
            Some(true) => "PD device (no identity)".to_string(),
            Some(false) => "non-PD device".to_string(),
            None => "device (no identity)".to_string(),
        },
        None => "partner (no partner device yet)".to_string(),
    };

    let cable_str = match &port.cable {
        Some(c) if c.identity.is_present() => {
            let speed = c.identity.product_type_vdo1 & vdo::USB_SPEED_MASK;
            let speed_name = vdo::usb_speed_name(speed, c.pd_revision);
            format!("{} cable ({})", c.cable_type, speed_name)
        }
        Some(c) => match &c.plug_type {
            Some(plug) => format!("{} cable, {plug} plug, no e-marker data", c.cable_type),
            None => format!("{} cable, no e-marker data", c.cable_type),
        },
        None => "no cable info".to_string(),
    };

    let contract_str = display::contract_summary(port)
        .map(|c| format!(", contract {c}"))
        .unwrap_or_default();

    log!(
        "port {}: connected — {who} [{}/{}], {cable_str}{contract_str}",
        port.number,
        port.data_role,
        display::power_role_label(port)
    );
}

#[allow(dead_code)]
fn extract_port_number(sysname: &str) -> Option<u32> {
    let s = sysname.strip_prefix("port")?;
    let num_str: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    num_str.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_port_number() {
        assert_eq!(extract_port_number("port0"), Some(0));
        assert_eq!(extract_port_number("port0-partner"), Some(0));
        assert_eq!(extract_port_number("port1-cable"), Some(1));
        assert_eq!(extract_port_number("port12-plug0"), Some(12));
        assert_eq!(extract_port_number("something"), None);
    }

    #[test]
    fn test_timestamp_shape() {
        let ts = timestamp();
        assert_eq!(ts.len(), 19);
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[10..11], " ");
    }
}
