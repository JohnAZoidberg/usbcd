mod billboard;
mod display;
#[cfg(feature = "gui")]
mod gui;
mod json;
mod monitor;
mod notify;
mod quality;
#[cfg(any(feature = "gui", test))]
mod summary;
#[allow(dead_code)]
mod sysfs;
#[allow(dead_code)]
mod types;
#[allow(dead_code)]
mod vdo;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "usbc", about = "USB Type-C cable quality warning tool")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show USB-C port state, connected devices, cable info, contract and quality warnings
    Status {
        /// Show verbose VDO field breakdown and per-PDO listing
        #[arg(short, long)]
        verbose: bool,
        /// Print the port state as JSON instead of text
        #[arg(long)]
        json: bool,
    },
    /// Watch for USB-C events, log connection and contract changes, and send
    /// desktop notifications for cable quality issues
    Monitor,
    /// Open a window showing USB-C status in plain language
    #[cfg(feature = "gui")]
    Gui,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Status { verbose, json } => {
            let ports = sysfs::scan_ports();
            let has_warnings = if json {
                print!("{}", json::render_json(&ports));
                ports
                    .iter()
                    .any(|p| !quality::check_cable_quality(p).is_empty())
            } else {
                display::print_status(&ports, verbose)
            };
            if has_warnings {
                std::process::exit(1);
            }
        }
        Commands::Monitor => {
            if let Err(e) = monitor::run_monitor().await {
                eprintln!("usbc monitor error: {e}");
                std::process::exit(1);
            }
        }
        #[cfg(feature = "gui")]
        Commands::Gui => {
            std::process::exit(gui::run().get().into());
        }
    }
}
