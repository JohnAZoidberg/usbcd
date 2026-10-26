use crate::types::CableWarning;

/// Send a desktop notification for a cable warning via D-Bus
/// (org.freedesktop.Notifications.Notify).
pub async fn send_notification(port_num: u32, warning: &CableWarning) -> Result<(), zbus::Error> {
    let connection = zbus::Connection::session().await?;

    let (summary, body, urgency) = warning_text(port_num, warning);

    // org.freedesktop.Notifications.Notify method:
    // Notify(app_name, replaces_id, app_icon, summary, body, actions, hints, expire_timeout)
    let _ = connection
        .call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &(
                "usbc",                           // app_name
                0u32,                              // replaces_id
                "dialog-warning",                  // app_icon
                summary,                           // summary
                body,                              // body
                &[] as &[&str],                    // actions
                // hints: a{sv}, so a map; a slice of pairs would be a(sv)
                std::collections::HashMap::from([("urgency", zbus::zvariant::Value::from(urgency))]),
                -1i32,                             // expire_timeout (-1 = server default)
            ),
        )
        .await?;

    Ok(())
}

fn warning_text(port_num: u32, warning: &CableWarning) -> (&'static str, String, u8) {
    let summary = "USB-C Cable Warning";
    match warning {
        CableWarning::InvalidDpCable => (
            summary,
            format!(
                "Port {port_num}: Your USB-C cable may not support display output. \
                 Try a DisplayPort-rated USB-C cable."
            ),
            1, // normal urgency
        ),
        CableWarning::InvalidUsb4ValidTbtCable => (
            summary,
            format!(
                "Port {port_num}: A USB4 device is connected but your cable only supports \
                 Thunderbolt. Use a USB4 cable for full speed."
            ),
            1,
        ),
        CableWarning::InvalidUsb4Cable => (
            summary,
            format!(
                "Port {port_num}: A USB4 device is connected but your cable is not compatible. \
                 Use a USB4 cable."
            ),
            1,
        ),
        CableWarning::InvalidTbtCable => (
            summary,
            format!(
                "Port {port_num}: A Thunderbolt device is connected but your cable is not \
                 compatible. Use a Thunderbolt cable."
            ),
            1,
        ),
        CableWarning::SpeedLimitingCable => (
            summary,
            format!(
                "Port {port_num}: Your cable is limiting the connection speed. \
                 The connected device supports faster speeds than the cable allows."
            ),
            0, // low urgency
        ),
        CableWarning::AltModeNotEntered(svid) => (
            summary,
            format!(
                "Port {port_num}: The connected device wants {} but it was never set up. \
                 The cable may be charge-only or USB 2.0; try a full-featured USB-C cable, \
                 or another port.",
                crate::types::alt_mode_label(*svid)
            ),
            1,
        ),
    }
}
