use std::collections::HashMap;
use std::os::unix::io::AsRawFd;
use std::time::Duration;

use crate::notify;
use crate::quality;
use crate::sysfs;
use crate::types::{CableWarning, Port};
use crate::vdo;

/// Run the udev monitor loop: watch for typec events, debounce, check quality,
/// and send desktop notifications.
pub async fn run_monitor() -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("usbc monitor: starting...");

    // Initial scan of already-connected devices.
    let ports = sysfs::scan_ports();
    log_initial_state(&ports);
    for port in &ports {
        let warnings = quality::check_cable_quality(port);
        for w in &warnings {
            eprintln!("  port {}: {w}", port.number);
            if let Err(e) = notify::send_notification(port.number, w).await {
                eprintln!("  failed to send notification: {e}");
            }
        }
    }

    // Track previous state per port.
    let mut prev_connected: HashMap<u32, bool> = HashMap::new();
    let mut prev_warnings: HashMap<u32, Vec<CableWarning>> = HashMap::new();
    for port in &ports {
        prev_connected.insert(port.number, port.partner.is_some());
        prev_warnings.insert(port.number, quality::check_cable_quality(port));
    }

    let mut rx = spawn_udev_watcher();

    eprintln!("usbc monitor: watching for USB-C events...");

    let debounce = Duration::from_millis(2000);

    loop {
        if rx.recv().await.is_none() {
            eprintln!("usbc monitor: udev thread exited");
            break;
        }

        tokio::time::sleep(debounce).await;
        while rx.try_recv().is_ok() {}

        let ports = sysfs::scan_ports();
        for port in &ports {
            let was_connected = prev_connected.get(&port.number).copied().unwrap_or(false);
            let is_connected = port.partner.is_some();

            // Log connection changes.
            if is_connected && !was_connected {
                log_connect(port);
            } else if !is_connected && was_connected {
                eprintln!("port {}: disconnected", port.number);
            }

            // Check for new warnings.
            let warnings = quality::check_cable_quality(port);
            let prev = prev_warnings.get(&port.number);
            for w in &warnings {
                let is_new = match prev {
                    Some(pw) => !pw.contains(w),
                    None => true,
                };
                if is_new {
                    eprintln!("  port {}: {w}", port.number);
                    if let Err(e) = notify::send_notification(port.number, w).await {
                        eprintln!("  failed to send notification: {e}");
                    }
                }
            }

            prev_connected.insert(port.number, is_connected);
            prev_warnings.insert(port.number, warnings);
        }

        // Clean up ports that disappeared.
        let active: Vec<u32> = ports.iter().map(|p| p.number).collect();
        prev_connected.retain(|k, _| active.contains(k));
        prev_warnings.retain(|k, _| active.contains(k));
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

fn log_initial_state(ports: &[Port]) {
    if ports.is_empty() {
        eprintln!("  no USB-C ports found");
        return;
    }
    for port in ports {
        if port.partner.is_some() {
            log_connect(port);
        }
    }
}

fn log_connect(port: &Port) {
    let partner = match &port.partner {
        Some(p) => p,
        None => return,
    };

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

    let cable_str = match &port.cable {
        Some(c) => {
            let speed = c.identity.product_type_vdo1 & vdo::USB_SPEED_MASK;
            let speed_name = vdo::usb_speed_name(speed, c.pd_revision);
            format!("{} cable ({})", c.cable_type, speed_name)
        }
        None => "no cable info".to_string(),
    };

    eprintln!(
        "port {}: connected — {} [{}/{}], {}",
        port.number, type_str, port.data_role, port.power_role, cable_str
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
}
