#[path = "output/cells.rs"]
mod cells;
#[path = "output/format.rs"]
mod format;
#[path = "output/lists.rs"]
mod lists;
#[cfg(test)]
#[path = "output/tests.rs"]
mod tests;

use std::io::{self, Write};

use crate::boxes::BoxRecord;
use crate::config::IsolationMode;
use crate::sagens::ui::{Align, BadgeStyle, Cell, Theme};
use crate::sagens::update::{SelfUpdateAction, SelfUpdateOutcome};

use self::cells::{isolation_mode_label, styled_status_cell};
use self::format::{
    fallback_settings, format_box_cpu_setting, format_box_fs_setting, format_box_memory_setting,
    format_box_network_setting, format_box_process_setting,
};
pub use self::lists::{
    print_admin_bundle, print_admin_removed, print_changes, print_checkpoint_delete_ok,
    print_checkpoint_id, print_checkpoint_restore_ok, print_checkpoints, print_files,
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

pub fn print_update_message(outcome: &SelfUpdateOutcome) -> io::Result<()> {
    let theme = Theme::stdout();
    let (badge, detail) = match outcome.action {
        SelfUpdateAction::AlreadyCurrent => (
            theme.badge("already latest", BadgeStyle::Muted),
            theme.dim(format!(
                "release={}  platform={}  binary={}",
                outcome.release_tag,
                outcome.platform,
                outcome.executable_path.display()
            )),
        ),
        SelfUpdateAction::Updated => (
            theme.badge("updated", BadgeStyle::Success),
            theme.dim(format!(
                "release={}  platform={}  binary={}",
                outcome.release_tag,
                outcome.platform,
                outcome.executable_path.display()
            )),
        ),
    };

    println!("{} {}", theme.title("sagens update"), badge);
    println!("{detail}");
    if matches!(outcome.action, SelfUpdateAction::Updated) {
        println!(
            "{}",
            theme.muted(
                "Restart a running daemon if you want background work to use the new binary."
            )
        );
    }
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
