// Console-subsystem binary — no windows_subsystem attribute.
// This is a thin CLI wrapper that opens SQLite directly,
// performs the requested operation, and exits.

mod cli;
mod config;
mod db;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Some(cmd) = cli::parse() else {
        eprintln!("Ecp Clipboard CLI");
        eprintln!();
        eprintln!("Usage:");
        eprintln!("  ecp list [N]           List recent N entries (default: 20)");
        eprintln!("  ecp paste <N>          Copy entry #N back to clipboard");
        eprintln!("  ecp search <keyword>   Search history with FTS5");
        eprintln!("  ecp clear              Delete all history");
        std::process::exit(1);
    };

    let config = config::AppConfig::load()?;
    cli::run(cmd, &config)
}
