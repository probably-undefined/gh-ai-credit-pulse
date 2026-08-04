use clap::{Parser, Subcommand};
use gh_ai_credit_pulse::collector::{
    Collector, DashboardData, DEFAULT_RETENTION_DAYS, Window, default_db_path,
};
use serde_json::json;
use std::fs::File;
use std::io::{self, BufWriter};
use std::path::PathBuf;
use std::process::ExitCode;
use std::str::FromStr;
use std::time::Duration;

#[derive(Debug, Parser)]
#[command(about = "Collect and summarize GitHub Copilot usage")]
struct Cli {
    #[arg(long, global = true, value_name = "PATH")]
    db: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Fetch, persist, and print dashboard JSON.
    Sample {
        #[arg(long, default_value = "24h", value_parser = Window::VALUES)]
        window: String,
        #[arg(long, default_value_t = 20.0)]
        timeout: f64,
        #[arg(long, default_value_t = DEFAULT_RETENTION_DAYS)]
        retention_days: u32,
    },
    /// Print dashboard JSON without fetching.
    Dashboard {
        #[arg(long, default_value = "24h", value_parser = Window::VALUES)]
        window: String,
    },
    /// Export all normalized samples as CSV.
    Export {
        #[arg(short, long, default_value = "-", value_name = "PATH")]
        output: PathBuf,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            let payload = json!({"status": "error", "fresh": false, "error": error.to_string()});
            println!("{}", serde_json::to_string(&payload).expect("error payload is serializable"));
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<u8, Box<dyn std::error::Error>> {
    let collector = Collector::open(cli.db.unwrap_or_else(default_db_path))?;
    match cli.command {
        Command::Sample {
            window,
            timeout,
            retention_days,
        } => {
            if !timeout.is_finite() || timeout <= 0.0 {
                return Err("--timeout must be a positive number".into());
            }
            let dashboard = collector.sample(
                Window::from_str(&window)?,
                Duration::from_secs_f64(timeout),
                retention_days,
            )?;
            let code = if dashboard.error.is_some() { 2 } else { 0 };
            emit(&dashboard)?;
            Ok(code)
        }
        Command::Dashboard { window } => {
            emit(&collector.dashboard(Window::from_str(&window)?)?)?;
            Ok(0)
        }
        Command::Export { output } => {
            if output.as_os_str() == "-" {
                collector.export(io::stdout().lock())?;
            } else {
                collector.export(BufWriter::new(File::create(output)?))?;
            }
            Ok(0)
        }
    }
}

fn emit(dashboard: &DashboardData) -> Result<(), serde_json::Error> {
    println!("{}", serde_json::to_string(dashboard)?);
    Ok(())
}
