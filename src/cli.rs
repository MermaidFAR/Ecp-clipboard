use std::error::Error;

use crate::config::AppConfig;
use crate::db::{Database, EntryKind};

pub enum CliCommand {
    List { limit: usize },
    Paste { index: usize },
    Search { query: String },
    Clear,
}

/// Parse CLI subcommands from process arguments.
/// Returns `None` when no recognized CLI subcommand is present.
pub fn parse() -> Option<CliCommand> {
    let mut args = std::env::args().skip(1).peekable();

    match args.next().as_deref()? {
        "list" => {
            let limit = args.next().and_then(|s| s.parse().ok()).unwrap_or(20);
            Some(CliCommand::List { limit })
        }
        "paste" => {
            let n: usize = args.next()?.parse().ok()?;
            if n == 0 {
                return None;
            }
            Some(CliCommand::Paste { index: n - 1 })
        }
        "search" => {
            let query = args.collect::<Vec<_>>().join(" ");
            if query.is_empty() {
                None
            } else {
                Some(CliCommand::Search { query })
            }
        }
        "clear" => Some(CliCommand::Clear),
        _ => None,
    }
}

/// Execute a CLI command against the database.
/// Opens SQLite directly, performs the operation, prints output, and returns.
/// No GUI, tray, or background threads are started.
pub fn run(cmd: CliCommand, config: &AppConfig) -> Result<(), Box<dyn Error>> {
    let db_path = config.database_path()?;
    let db = Database::open(&db_path)?;

    match cmd {
        CliCommand::List { limit } => {
            let entries = db.list_recent(limit)?;
            if entries.is_empty() {
                println!("(no history)");
            } else {
                for (i, entry) in entries.iter().enumerate() {
                    println!(
                        "{:>3}  [{:<10}]  {}",
                        i + 1,
                        entry.kind.as_str(),
                        preview(&entry.content, entry.kind)
                    );
                }
            }
        }

        CliCommand::Paste { index } => {
            let entries = db.list_recent(index + 1)?;
            match entries.into_iter().nth(index) {
                None => {
                    eprintln!("error: index out of range");
                    std::process::exit(1);
                }
                Some(entry) => {
                    if entry.kind == EntryKind::Image {
                        eprintln!("error: image entries cannot be pasted via CLI");
                        std::process::exit(1);
                    }
                    let mut clipboard = arboard::Clipboard::new()?;
                    clipboard.set_text(&entry.content)?;
                    println!("copied: {}", preview(&entry.content, entry.kind));
                }
            }
        }

        CliCommand::Search { query } => {
            let entries = db.search(&query, 20)?;
            if entries.is_empty() {
                println!("(no results)");
            } else {
                for (i, entry) in entries.iter().enumerate() {
                    println!(
                        "{:>3}  [{:<10}]  {}",
                        i + 1,
                        entry.kind.as_str(),
                        preview(&entry.content, entry.kind)
                    );
                }
            }
        }

        CliCommand::Clear => {
            let count = db.delete_all()?;
            println!("cleared {count} entries");
        }
    }

    Ok(())
}

fn preview(content: &str, kind: EntryKind) -> String {
    if kind == EntryKind::Image {
        return "(image)".to_string();
    }
    let single_line = content.lines().collect::<Vec<_>>().join(" ↵ ");
    if single_line.chars().count() > 80 {
        let truncated: String = single_line.chars().take(77).collect();
        format!("{truncated}...")
    } else {
        single_line
    }
}

