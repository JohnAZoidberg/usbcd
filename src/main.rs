mod display;
mod monitor;
mod notify;
mod quality;
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
    /// Show USB-C port state, connected devices, cable info, and quality warnings
    Status {
        /// Show verbose VDO field breakdown and per-PDO listing
        #[arg(short, long)]
        verbose: bool,
    },
    /// Watch for USB-C events and send desktop notifications for cable quality issues
    Monitor,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Status { verbose } => {
            let ports = sysfs::scan_ports();
            let has_warnings = display::print_status(&ports, verbose);
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
    }
}
