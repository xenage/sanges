use std::io::{self, Write};

use crate::auth::AdminCredentialBundle;
use crate::sagens::ui::{Align, BadgeStyle, Cell, Theme};
use crate::workspace::{FileNode, WorkspaceCheckpointRecord};

use super::cells::styled_kind_cell;
use super::format::format_bytes;

pub fn print_checkpoint_id(checkpoint_id: &str) -> io::Result<()> {
    let theme = Theme::stdout();
    println!(
        "{} {} {}",
        theme.title("checkpoint"),
        theme.badge("created", BadgeStyle::Success),
        theme.code(checkpoint_id)
    );
    io::stdout().flush()
}

pub fn print_checkpoint_restore_ok(checkpoint_id: &str) -> io::Result<()> {
    let theme = Theme::stdout();
    println!(
        "{} {} {}",
        theme.title("checkpoint"),
        theme.badge("restored", BadgeStyle::Warning),
        theme.code(checkpoint_id)
    );
    io::stdout().flush()
}

pub fn print_checkpoint_delete_ok(checkpoint_id: &str) -> io::Result<()> {
    let theme = Theme::stdout();
    println!(
        "{} {} {}",
        theme.title("checkpoint"),
        theme.badge("deleted", BadgeStyle::Danger),
        theme.code(checkpoint_id)
    );
    io::stdout().flush()
}

pub fn print_checkpoints(checkpoints: &[WorkspaceCheckpointRecord]) -> io::Result<()> {
    let theme = Theme::stdout();
    println!(
        "{} {}",
        theme.title("Checkpoint lineage"),
        theme.badge(&format!("{} total", checkpoints.len()), BadgeStyle::Accent)
    );
    println!();
    if checkpoints.is_empty() {
        println!("{}", theme.dim("No checkpoints found."));
        return io::stdout().flush();
    }

    let rows = checkpoints
        .iter()
        .map(|checkpoint| {
            let name = checkpoint.summary.name.as_deref().unwrap_or("—");
            let metadata =
                serde_json::to_string(&checkpoint.summary.metadata).unwrap_or_else(|_| "{}".into());
            vec![
                Cell::rendered(
                    checkpoint.summary.checkpoint_id.clone(),
                    theme.code(&checkpoint.summary.checkpoint_id),
                    Align::Left,
                ),
                Cell::plain(name),
                Cell::right(checkpoint.summary.created_at_ms.to_string()),
                Cell::plain(metadata),
            ]
        })
        .collect::<Vec<_>>();

    println!(
        "{}",
        theme.table(
            vec![
                Cell::plain("CHECKPOINT"),
                Cell::plain("NAME"),
                Cell::plain("CREATED_MS"),
                Cell::plain("METADATA"),
            ],
            rows,
        )
    );
    io::stdout().flush()
}

pub fn print_admin_bundle(bundle: &AdminCredentialBundle) -> io::Result<()> {
    let theme = Theme::stdout();
    println!(
        "{} {}",
        theme.title("admin"),
        theme.badge("added", BadgeStyle::Success)
    );
    println!();
    println!(
        "{}",
        theme.table(
            vec![Cell::plain("Field"), Cell::plain("Value")],
            vec![
                vec![
                    Cell::plain("Admin UUID"),
                    Cell::rendered(
                        bundle.admin_uuid.to_string(),
                        theme.code(bundle.admin_uuid.to_string()),
                        Align::Left,
                    ),
                ],
                vec![
                    Cell::plain("Admin token"),
                    Cell::rendered(
                        bundle.admin_token.clone(),
                        theme.code(bundle.admin_token.clone()),
                        Align::Left,
                    ),
                ],
                vec![
                    Cell::plain("Endpoint"),
                    Cell::rendered(
                        bundle.endpoint.clone(),
                        theme.code(bundle.endpoint.clone()),
                        Align::Left,
                    ),
                ],
            ],
        )
    );
    io::stdout().flush()
}

pub fn print_admin_removed() -> io::Result<()> {
    let theme = Theme::stdout();
    println!(
        "{} {}",
        theme.title("admin"),
        theme.badge("removed", BadgeStyle::Danger)
    );
    io::stdout().flush()
}

pub fn print_files(entries: &[FileNode]) -> io::Result<()> {
    let theme = Theme::stdout();
    println!(
        "{} {}",
        theme.title("Workspace files"),
        theme.badge(&format!("{} entries", entries.len()), BadgeStyle::Info)
    );
    println!();
    if entries.is_empty() {
        println!("{}", theme.dim("No files matched the requested path."));
        return io::stdout().flush();
    }
    let rows = entries
        .iter()
        .map(|entry| {
            vec![
                styled_kind_cell(&theme, &entry.kind),
                Cell::plain(&entry.path),
                Cell::right(format_bytes(entry.size)),
            ]
        })
        .collect::<Vec<_>>();
    println!(
        "{}",
        theme.table(
            vec![
                Cell::plain("TYPE"),
                Cell::plain("PATH"),
                Cell::plain("SIZE"),
            ],
            rows,
        )
    );
    io::stdout().flush()
}
