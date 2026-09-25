use std::error::Error;

use crate::clipboard_write::copy_entry;
use crate::config::AppConfig;
use crate::db::{Database, EntryKind};

#[derive(Debug, PartialEq)]
pub enum CliCommand {
    List { limit: usize },
    Paste { index: usize },
    PasteId { id: i64 },
    Search { query: String },
    Clear,
}

/// Parse CLI subcommands from process arguments.
/// Returns `None` when no recognized CLI subcommand is present.
pub fn parse() -> Option<CliCommand> {
    parse_args(std::env::args().skip(1))
}

pub fn parse_args(args: impl IntoIterator<Item = String>) -> Option<CliCommand> {
    let mut args = args.into_iter().peekable();

    match args.next().as_deref()? {
        "list" => {
            let limit = args.next().and_then(|s| s.parse().ok()).unwrap_or(20);
            Some(CliCommand::List { limit })
        }
        "paste" => {
            let first = args.next()?;
            if first == "--id" {
                let id = args.next()?.parse().ok()?;
                return Some(CliCommand::PasteId { id });
            }
            let n: usize = first.parse().ok()?;
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
    let mut db = Database::open_with_limits(&db_path, config.max_history, config.max_image_bytes)?;

    match cmd {
        CliCommand::List { limit } => {
            let entries = db.list_recent(limit)?;
            if entries.is_empty() {
                println!("(no history)");
            } else {
                for (i, entry) in entries.iter().enumerate() {
                    println!(
                        "{:>3}  id={:<8}  [{:<10}]  {}",
                        i + 1,
                        entry.id,
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
                    return Err("index out of range".into());
                }
                Some(entry) => {
                    copy_entry(&db, &entry)?;
                    println!("copied id={}", entry.id);
                }
            }
        }

        CliCommand::PasteId { id } => {
            let entry = db.get_by_id(id)?.ok_or("entry ID not found")?;
            copy_entry(&db, &entry)?;
            println!("copied id={}", entry.id);
        }

        CliCommand::Search { query } => {
            let entries = db.search(&query, 20)?;
            if entries.is_empty() {
                println!("(no results)");
            } else {
                for entry in &entries {
                    println!(
                        "id={:<8}  [{:<10}]  {}",
                        entry.id,
                        entry.kind.as_str(),
                        preview(&entry.content, entry.kind)
                    );
                }
            }
        }

        CliCommand::Clear => {
            let count = db.delete_all()?;
            #[cfg(windows)]
            let _ = crate::ipc::signal(crate::ipc::Signal::History);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_id_paste_and_legacy_recent_index_parse() {
        assert_eq!(
            parse_args(["paste", "--id", "42"].map(str::to_owned)),
            Some(CliCommand::PasteId { id: 42 })
        );
        assert_eq!(
            parse_args(["paste", "2"].map(str::to_owned)),
            Some(CliCommand::Paste { index: 1 })
        );
    }
}
