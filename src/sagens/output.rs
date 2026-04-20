#[path = "output/cells.rs"]
mod cells;
#[path = "output/format.rs"]
mod format;
#[cfg(test)]
#[path = "output/tests.rs"]
mod tests;

use std::io::{self, Write};

use crate::auth::AdminCredentialBundle;
use crate::boxes::BoxRecord;
use crate::config::IsolationMode;
use crate::sagens::ui::{Align, BadgeStyle, Cell, Theme};
use crate::workspace::{FileNode, WorkspaceChange, WorkspaceCheckpointRecord};

use self::cells::{isolation_mode_label, styled_change_cell, styled_kind_cell, styled_status_cell};
use self::format::{
    fallback_settings, format_box_cpu_setting, format_box_fs_setting, format_box_memory_setting,
    format_box_network_setting, format_box_process_setting, format_bytes,
};

pub fn print_help(text: &str) -> io::Result<()> {
    println!("{text}");
    io::stdout().flush()
}

pub fn print_start_message(
    endpoint: &str,
    already_running: bool,
    isolation_mode: IsolationMode,
) -> io::Result<()> {
    let theme = Theme::stdout();
    let badge = if already_running {
        theme.badge("already running", BadgeStyle::Warning)
    } else {
        theme.badge("started", BadgeStyle::Success)
    };
    println!(
        "{} {}  {}  {}",
        theme.title("sagens daemon"),
        badge,
        theme.code(endpoint),
        theme.dim(format!("mode={}", isolation_mode_label(isolation_mode)))
    );
    io::stdout().flush()
}

pub fn print_quit_message(was_running: bool) -> io::Result<()> {
    let theme = Theme::stdout();
    let badge = if was_running {
        theme.badge("stopped", BadgeStyle::Warning)
    } else {
        theme.badge("already stopped", BadgeStyle::Muted)
    };
    println!("{} {}", theme.title("sagens daemon"), badge);
    io::stdout().flush()
}

pub fn print_box_action(action: &str, box_record: &BoxRecord) -> io::Result<()> {
    let theme = Theme::stdout();
    let fallback = fallback_settings();
    let settings = box_record.settings.as_ref().unwrap_or(&fallback);
    println!(
        "{} {}",
        theme.title("BOX"),
        theme.badge(action, BadgeStyle::Success)
    );
    println!();
    println!(
        "{}",
        theme.table(
            vec![Cell::plain("Field"), Cell::plain("Value")],
            vec![
                vec![
                    Cell::plain("BOX ID"),
                    Cell::rendered(
                        box_record.box_id.to_string(),
                        theme.code(box_record.box_id.to_string()),
                        Align::Left,
                    ),
                ],
                vec![
                    Cell::plain("Name"),
                    Cell::plain(box_record.name.as_deref().unwrap_or("—")),
                ],
                vec![
                    Cell::plain("Status"),
                    styled_status_cell(&theme, box_record.status),
                ],
                vec![
                    Cell::plain("CPU"),
                    Cell::plain(format_box_cpu_setting(box_record, settings)),
                ],
                vec![
                    Cell::plain("RAM"),
                    Cell::plain(format_box_memory_setting(box_record, settings)),
                ],
                vec![
                    Cell::plain("FS"),
                    Cell::plain(format_box_fs_setting(box_record, settings)),
                ],
                vec![
                    Cell::plain("Processes"),
                    Cell::plain(format_box_process_setting(box_record, settings)),
                ],
                vec![
                    Cell::plain("Network"),
                    Cell::plain(format_box_network_setting(&settings.network_enabled)),
                ],
                vec![
                    Cell::plain("Workspace"),
                    Cell::rendered(
                        box_record.workspace_path.display().to_string(),
                        theme.dim(box_record.workspace_path.display().to_string()),
                        Align::Left,
                    ),
                ],
            ],
        )
    );
    io::stdout().flush()
}

pub fn print_box_table(boxes: &[BoxRecord]) -> io::Result<()> {
    let theme = Theme::stdout();
    println!(
        "{} {}",
        theme.title("BOX inventory"),
        theme.badge(&format!("{} total", boxes.len()), BadgeStyle::Info)
    );
    println!();
    if boxes.is_empty() {
        println!(
            "{}",
            theme.dim("No BOXes found. Create one with `sagens box new`.")
        );
        return io::stdout().flush();
    }

    let rows = boxes
        .iter()
        .map(|box_record| {
            let fallback = fallback_settings();
            let settings = box_record.settings.as_ref().unwrap_or(&fallback);
            vec![
                Cell::rendered(
                    box_record.box_id.to_string(),
                    theme.code(box_record.box_id.to_string()),
                    Align::Left,
                ),
                styled_status_cell(&theme, box_record.status),
                Cell::plain(format_box_cpu_setting(box_record, settings)),
                Cell::plain(format_box_memory_setting(box_record, settings)),
                Cell::plain(format_box_fs_setting(box_record, settings)),
                Cell::plain(format_box_process_setting(box_record, settings)),
                Cell::plain(format_box_network_setting(&settings.network_enabled)),
            ]
        })
        .collect::<Vec<_>>();

    println!(
        "{}",
        theme.table(
            vec![
                Cell::plain("BOX"),
                Cell::plain("STATUS"),
                Cell::plain("CPU"),
                Cell::plain("RAM"),
                Cell::plain("FS"),
                Cell::plain("PROC"),
                Cell::plain("NET"),
            ],
            rows,
        )
    );
    io::stdout().flush()
}

pub fn print_removed(box_id: uuid::Uuid) -> io::Result<()> {
    let theme = Theme::stdout();
    println!(
        "{} {} {}",
        theme.title("BOX"),
        theme.badge("removed", BadgeStyle::Danger),
        theme.code(box_id.to_string())
    );
    io::stdout().flush()
}

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
                Cell::plain("SIZE")
            ],
            rows,
        )
    );
    io::stdout().flush()
}

pub fn print_changes(changes: &[WorkspaceChange]) -> io::Result<()> {
    let theme = Theme::stdout();
    println!(
        "{} {}",
        theme.title("Workspace diff"),
        theme.badge(&format!("{} changes", changes.len()), BadgeStyle::Accent)
    );
    println!();
    if changes.is_empty() {
        println!("{}", theme.dim("No tracked changes."));
        return io::stdout().flush();
    }
    let rows = changes
        .iter()
        .map(|change| {
            vec![
                styled_change_cell(&theme, change.git_label()),
                Cell::plain(&change.path),
            ]
        })
        .collect::<Vec<_>>();
    println!(
        "{}",
        theme.table(vec![Cell::plain("CHANGE"), Cell::plain("PATH")], rows)
    );
    io::stdout().flush()
}
