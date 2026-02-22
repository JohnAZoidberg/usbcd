use std::collections::HashMap;
use std::os::unix::io::AsRawFd;
use std::time::Duration;

use crate::notify;
use crate::quality;
use crate::sysfs;
use crate::types::CableWarning;

/// Run the udev monitor loop: watch for typec events, debounce, check quality,
/// and send desktop notifications.
pub async fn run_monitor() -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("usbc monitor: starting...");

    // Initial scan of already-connected devices.
    let ports = sysfs::scan_ports();
    for port in &ports {
        let warnings = quality::check_cable_quality(port);
        for w in &warnings {
            eprintln!("Port {}: {w}", port.number);
            if let Err(e) = notify::send_notification(port.number, w).await {
                eprintln!("Failed to send notification: {e}");
            }
        }
    }

    // Set up udev monitor for typec subsystem.
    let socket = udev::MonitorBuilder::new()?
        .match_subsystem("typec")?
        .listen()?;

    eprintln!("usbc monitor: watching for USB-C events...");

    // Track previous warnings per port to avoid duplicate notifications.
    let mut prev_warnings: HashMap<u32, Vec<CableWarning>> = HashMap::new();
    for port in &ports {
        prev_warnings.insert(port.number, quality::check_cable_quality(port));
    }

    // Use a simple poll loop with debounce.
    let debounce = Duration::from_millis(500);
    let poll_interval = Duration::from_millis(100);

    loop {
        // Wait for a udev event.
        let event = tokio::task::spawn_blocking({
            let fd = socket.as_raw_fd();
            move || wait_for_event(fd)
        })
        .await?;

        if event.is_some() {
            // Debounce: wait for events to settle.
            tokio::time::sleep(debounce).await;

            // Drain any additional events that arrived during debounce.
            drain_events(&socket);

            // Re-scan all ports.
            let ports = sysfs::scan_ports();
            for port in &ports {
                let warnings = quality::check_cable_quality(port);
                let prev = prev_warnings.get(&port.number);

                // Only notify on new warnings.
                for w in &warnings {
                    let is_new = match prev {
                        Some(pw) => !pw.contains(w),
                        None => true,
                    };
                    if is_new {
                        eprintln!("Port {}: {w}", port.number);
                        if let Err(e) = notify::send_notification(port.number, w).await {
                            eprintln!("Failed to send notification: {e}");
                        }
                    }
                }

                prev_warnings.insert(port.number, warnings);
            }

            // Clean up ports that are no longer present.
            let active_ports: Vec<u32> = ports.iter().map(|p| p.number).collect();
            prev_warnings.retain(|k, _| active_ports.contains(k));
        } else {
            // No event, sleep a bit.
            tokio::time::sleep(poll_interval).await;
        }
    }
}

/// Extract port number from a udev sysname like "port0-partner" or "port1-cable".
#[allow(dead_code)]
fn extract_port_number(sysname: &str) -> Option<u32> {
    let s = sysname.strip_prefix("port")?;
    let num_str: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    num_str.parse().ok()
}

fn wait_for_event(fd: std::os::unix::io::RawFd) -> Option<u32> {
    let mut pollfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };

    let ret = unsafe { libc::poll(&mut pollfd as *mut _, 1, 1000) };

    if ret > 0 && (pollfd.revents & libc::POLLIN) != 0 {
        Some(u32::MAX) // sentinel: rescan all ports
    } else {
        None
    }
}

fn drain_events(socket: &udev::MonitorSocket) {
    for _event in socket.iter() {
        // Just consume pending events.
    }
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
