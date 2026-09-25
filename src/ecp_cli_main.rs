// Console-subsystem binary — no windows_subsystem attribute.
// This is a thin CLI wrapper that opens SQLite directly,
// performs the requested operation, and exits.

use ecp_clipboard::{cli, config};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Some(cmd) = cli::parse() else {
        eprintln!("Ecp Clipboard CLI");
        eprintln!();
        eprintln!("Usage:");
        eprintln!("  ecp list [N]           List recent N entries (default: 20)");
        eprintln!("  ecp paste <N>          Copy entry #N back to clipboard");
        eprintln!("  ecp paste --id <ID>    Copy a stable history entry ID");
        eprintln!("  ecp search <query>     Search history");
        eprintln!("  ecp clear              Delete all history");
        std::process::exit(1);
    };

    let config = config::AppConfig::load()?;
    cli::run(cmd, &config)
}
