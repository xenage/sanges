use crate::boxes::BoxStatus;
use crate::config::IsolationMode;
use crate::sagens::ui::{Align, BadgeStyle, Cell, Theme};
use crate::workspace::FileKind;

pub(super) fn styled_status_cell(theme: &Theme, status: BoxStatus) -> Cell {
    let (label, badge_style) = match status {
        BoxStatus::Created => ("CREATED", BadgeStyle::Info),
        BoxStatus::Running => ("RUNNING", BadgeStyle::Success),
        BoxStatus::Stopped => ("STOPPED", BadgeStyle::Warning),
        BoxStatus::Failed => ("FAILED", BadgeStyle::Danger),
        BoxStatus::Removing => ("REMOVING", BadgeStyle::Muted),
    };
    Cell::rendered(label, theme.badge(label, badge_style), Align::Center)
}

pub(super) fn styled_kind_cell(theme: &Theme, kind: &FileKind) -> Cell {
    let (label, style) = match kind {
        FileKind::File => ("FILE", BadgeStyle::Info),
        FileKind::Directory => ("DIR", BadgeStyle::Success),
        FileKind::Symlink => ("LINK", BadgeStyle::Accent),
    };
    Cell::rendered(label, theme.badge(label, style), Align::Center)
}

pub(super) fn isolation_mode_label(mode: IsolationMode) -> &'static str {
    match mode {
        IsolationMode::Compat => "compat",
        IsolationMode::Secure => "secure",
    }
}
