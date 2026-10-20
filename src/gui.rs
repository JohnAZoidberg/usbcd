// Desktop window showing USB-C port status in plain language.
//
// Rebuilds the port list from sysfs whenever udev reports a USB-C change, so plugging
// and unplugging updates the window live.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::glib;

use crate::display;
use crate::monitor;
use crate::summary::{self, CableSummary, Overall, PortSummary, PowerFlow};
use crate::sysfs;

const APP_ID: &str = "io.github.usbc.Usbc";

/// Wait for the kernel to finish PD discovery before rescanning; a single
/// plug event produces a burst of partner/cable/altmode uevents.
const DEBOUNCE: Duration = Duration::from_millis(750);

/// Below this window width the ports stack in one column instead of two.
const NARROW_WIDTH_SP: f64 = 720.0;

pub fn run() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_window);
    // clap already consumed our arguments; don't let GTK see "gui".
    app.run_with_args::<&str>(&[])
}

struct Ui {
    window: adw::ApplicationWindow,
    stack: gtk::Stack,
    ports_box: gtk::Box,
    ports_grid: gtk::Grid,
    columns: Cell<i32>,
    report: gtk::Label,
    toasts: adw::ToastOverlay,
}

fn build_window(app: &adw::Application) {
    if let Some(window) = app.active_window() {
        window.present();
        return;
    }

    let refresh_button = gtk::Button::builder()
        .icon_name("view-refresh-symbolic")
        .tooltip_text("Refresh")
        .build();
    let header = adw::HeaderBar::new();
    header.pack_start(&refresh_button);

    let ports_box = gtk::Box::new(gtk::Orientation::Vertical, 24);
    let ports_grid = gtk::Grid::builder()
        .row_spacing(24)
        .column_spacing(24)
        .column_homogeneous(true)
        .build();

    let report = gtk::Label::builder()
        .xalign(0.0)
        .selectable(true)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .css_classes(["monospace", "caption"])
        .build();
    let report_row = adw::ExpanderRow::builder()
        .title("Show raw port data")
        .subtitle("Include this when asking for help")
        .build();
    report_row.add_row(&report);

    let copy_button = gtk::Button::builder()
        .icon_name("edit-copy-symbolic")
        .tooltip_text("Copy technical details")
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    let details_group = adw::PreferencesGroup::builder()
        .title("Technical Details")
        .header_suffix(&copy_button)
        .build();
    details_group.add(&report_row);

    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(12)
        .margin_end(12)
        .build();
    page.append(&ports_box);
    page.append(&details_group);

    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&adw::Clamp::builder().maximum_size(1200).child(&page).build())
        .build();

    let empty = adw::StatusPage::builder()
        .icon_name("dialog-question-symbolic")
        .title("No USB-C Ports Found")
        .description(
            "Your computer didn't report any USB-C ports to Linux. \
             This can happen when the USB-C driver fails to start during boot. \
             Restarting the computer often fixes it.\n\n\
             Advanced: run <tt>sudo modprobe -r ucsi_acpi &amp;&amp; sudo modprobe ucsi_acpi</tt>",
        )
        .build();

    let stack = gtk::Stack::new();
    stack.add_named(&scrolled, Some("ports"));
    stack.add_named(&empty, Some("empty"));

    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&stack));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&toasts));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("USB-C Status")
        .default_width(960)
        .default_height(720)
        // Breakpoints need a fixed minimum size; this is the GNOME HIG minimum.
        .width_request(360)
        .height_request(294)
        .content(&toolbar)
        .build();

    let narrow = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
        adw::BreakpointConditionLengthType::MaxWidth,
        NARROW_WIDTH_SP,
        adw::LengthUnit::Sp,
    ));
    window.add_breakpoint(narrow.clone());

    let ui = Rc::new(Ui {
        window,
        stack,
        ports_box,
        ports_grid,
        columns: Cell::new(2),
        report,
        toasts,
    });

    narrow.connect_apply({
        let ui = ui.clone();
        move |_| set_columns(&ui, 1)
    });
    narrow.connect_unapply({
        let ui = ui.clone();
        move |_| set_columns(&ui, 2)
    });

    refresh_button.connect_clicked({
        let ui = ui.clone();
        move |_| refresh(&ui)
    });
    copy_button.connect_clicked({
        let ui = ui.clone();
        move |_| {
            ui.window.clipboard().set_text(&ui.report.text());
            ui.toasts.add_toast(adw::Toast::new("Copied to clipboard"));
        }
    });

    watch_udev({
        let ui = ui.clone();
        move || refresh(&ui)
    });

    refresh(&ui);
    ui.window.present();
}

/// Call `on_change` (debounced) whenever the kernel reports a USB-C change.
fn watch_udev(on_change: impl Fn() + 'static) {
    let mut rx = monitor::spawn_udev_watcher();
    glib::spawn_future_local(async move {
        while rx.recv().await.is_some() {
            glib::timeout_future(DEBOUNCE).await;
            while rx.try_recv().is_ok() {}
            on_change();
        }
    });
}

fn refresh(ui: &Ui) {
    let ports = sysfs::scan_ports();
    let (report, _) = display::render_status(&ports, true);
    ui.report.set_text(report.trim_end());

    if ports.is_empty() {
        ui.stack.set_visible_child_name("empty");
        return;
    }
    ui.stack.set_visible_child_name("ports");

    while let Some(child) = ui.ports_box.first_child() {
        ui.ports_box.remove(&child);
    }

    let summaries: Vec<PortSummary> = ports.iter().map(summary::summarize).collect();
    ui.ports_box.append(&headline(summary::overall(&summaries)));

    while let Some(child) = ui.ports_grid.first_child() {
        ui.ports_grid.remove(&child);
    }
    let groups: Vec<gtk::Widget> = summaries
        .iter()
        .map(|s| {
            let group = port_group(s);
            // Rows take the height of the taller port; keep the shorter one at the top.
            group.set_valign(gtk::Align::Start);
            group.upcast()
        })
        .collect();
    place_ports(&ui.ports_grid, &groups, ui.columns.get());
    ui.ports_box.append(&ui.ports_grid);
}

fn set_columns(ui: &Ui, columns: i32) {
    ui.columns.set(columns);
    let mut groups = Vec::new();
    while let Some(child) = ui.ports_grid.first_child() {
        ui.ports_grid.remove(&child);
        groups.push(child);
    }
    place_ports(&ui.ports_grid, &groups, columns);
}

fn place_ports(grid: &gtk::Grid, groups: &[gtk::Widget], columns: i32) {
    for (i, group) in (0..).zip(groups) {
        grid.attach(group, i % columns, i / columns, 1, 1);
    }
}

fn headline(overall: Overall) -> gtk::Box {
    let (icon, css, title, subtitle) = match overall {
        Overall::Issues(n) => (
            "dialog-warning-symbolic",
            "warning",
            if n == 1 {
                "1 cable problem found".to_string()
            } else {
                format!("{n} cable problems found")
            },
            "A cable is preventing a device from working at its best. See below for what to do.",
        ),
        Overall::AllGood => (
            "object-select-symbolic",
            "success",
            "Everything looks good".to_string(),
            "Your cables support everything the connected devices can do.",
        ),
        Overall::NothingConnected | Overall::NoPorts => (
            "network-wired-disconnected-symbolic",
            "dim-label",
            "Nothing plugged in".to_string(),
            "Connect a device to a USB-C port to check its cable.",
        ),
    };

    let image = gtk::Image::builder()
        .icon_name(icon)
        .pixel_size(48)
        .css_classes([css])
        .build();
    let title = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .wrap(true)
        .css_classes(["title-2"])
        .build();
    let subtitle = gtk::Label::builder()
        .label(subtitle)
        .xalign(0.0)
        .wrap(true)
        .css_classes(["dim-label"])
        .build();

    let text = gtk::Box::new(gtk::Orientation::Vertical, 4);
    text.set_valign(gtk::Align::Center);
    text.append(&title);
    text.append(&subtitle);

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 18);
    row.set_margin_start(6);
    row.append(&image);
    row.append(&text);
    row
}

fn port_group(s: &PortSummary) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(format!("Port {}", s.number))
        .build();

    let Some(device) = &s.device else {
        group.set_description(Some("Nothing connected"));
        return group;
    };

    // A short feature list on an unidentified device would look like a bug,
    // so say why there's nothing more to show.
    let device_detail = match (device.identified, device.features.is_empty()) {
        (true, _) => device.features.join(" · "),
        (false, true) => "Device didn't report its details".to_string(),
        (false, false) => format!(
            "{} · no other details reported",
            device.features.join(" · ")
        ),
    };
    group.add(&row(
        device.kind.icon_name(),
        device.kind.label(),
        &device_detail,
    ));

    match device.power {
        PowerFlow::Receiving {
            max_milliwatts,
            pd,
            negotiated_milliwatts,
        } => {
            let detail = match (negotiated_milliwatts, max_milliwatts, pd) {
                (Some(now), Some(mw), true) if mw > now => format!(
                    "Charging at {}, up to {} available",
                    summary::format_watts(now),
                    summary::format_watts(mw)
                ),
                (Some(now), _, true) => format!("Charging at {}", summary::format_watts(now)),
                (None, Some(mw), true) => format!("Up to {} available", summary::format_watts(mw)),
                // No PD contract: plain 5 V charging, which is why it's slow.
                (_, Some(mw), false) => format!(
                    "Basic USB charging, up to {}. Fast charging isn't available",
                    summary::format_watts(mw)
                ),
                (Some(now), None, false) => format!(
                    "Basic USB charging at {}. Fast charging isn't available",
                    summary::format_watts(now)
                ),
                (None, None, _) => "Power available".to_string(),
            };
            group.add(&row("ac-adapter-symbolic", "Powering this computer", &detail));
        }
        PowerFlow::Standby {
            max_milliwatts,
            negotiated_milliwatts,
        } => {
            let detail = match negotiated_milliwatts.or(max_milliwatts) {
                Some(mw) => format!(
                    "{} ready, but this computer is drawing power from somewhere else",
                    summary::format_watts(mw)
                ),
                None => "Ready, but this computer is drawing power from somewhere else".to_string(),
            };
            group.add(&row("ac-adapter-symbolic", "Standing by", &detail));
        }
        PowerFlow::Supplying {
            max_milliwatts,
            negotiated_milliwatts,
        } => {
            let detail = match (negotiated_milliwatts, max_milliwatts) {
                (Some(now), Some(mw)) if mw > now => format!(
                    "Supplying {}, this port can give up to {}",
                    summary::format_watts(now),
                    summary::format_watts(mw)
                ),
                (Some(now), _) => format!("Supplying {}", summary::format_watts(now)),
                (None, Some(mw)) => format!(
                    "This computer is supplying power, up to {}",
                    summary::format_watts(mw)
                ),
                (None, None) => "This computer is supplying power to the device".to_string(),
            };
            group.add(&row("ac-adapter-symbolic", "Powered by this computer", &detail));
        }
        PowerFlow::Unknown => {}
    }

    let features = s.cable_features.iter().map(|f| f.to_string());
    // Alt modes can come without the e-marker ratings, so the rating is only
    // "not reported" in full when there's nothing else to say.
    let not_reported = if s.cable_features.is_empty() {
        "speed and power rating not reported"
    } else {
        "full rating not reported"
    };
    let parts: Vec<String> = match &s.cable {
        CableSummary::Identified {
            active,
            speed,
            max_watts,
        } => {
            let mut parts = vec![format!("Data up to {speed}")];
            if let Some(w) = max_watts {
                parts.push(format!("power up to {w} W"));
            }
            if *active {
                parts.push("active cable".to_string());
            }
            parts.extend(features);
            parts
        }
        CableSummary::TypeOnly { active } => {
            let kind = if *active { "Active cable" } else { "Passive cable" };
            let mut parts = vec![kind.to_string()];
            parts.extend(features);
            parts.push(not_reported.to_string());
            parts
        }
        // Often the computer simply can't ask (e.g. while it's being charged),
        // so don't blame the cable here.
        CableSummary::Unidentified => {
            let mut parts: Vec<String> = features.collect();
            parts.push(not_reported.to_string());
            parts
        }
    };
    let mut cable_detail = parts.join(" · ");
    // The list may start with a lowercase rating note.
    if let Some(first) = cable_detail.get(..1) {
        cable_detail.replace_range(..1, &first.to_uppercase());
    }
    group.add(&row("network-wired-symbolic", "Cable", &cable_detail));

    for issue in &s.issues {
        group.add(&warning_row(issue.title, issue.advice));
    }

    group
}

fn row(icon: &str, title: &str, subtitle: &str) -> adw::ActionRow {
    let r = adw::ActionRow::builder()
        .title(glib::markup_escape_text(title))
        .subtitle(glib::markup_escape_text(subtitle))
        .build();
    r.add_prefix(&gtk::Image::from_icon_name(icon));
    r
}

fn warning_row(title: &str, advice: &str) -> adw::ActionRow {
    let r = adw::ActionRow::builder()
        .title(glib::markup_escape_text(title))
        .subtitle(glib::markup_escape_text(advice))
        .build();
    r.add_prefix(
        &gtk::Image::builder()
            .icon_name("dialog-warning-symbolic")
            .css_classes(["warning"])
            .build(),
    );
    r
}
