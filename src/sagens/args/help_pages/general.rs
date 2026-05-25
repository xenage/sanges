use crate::sagens::ui::{BadgeStyle, Theme};

use super::super::HelpTopic;
use super::super::help::{PageSpec, cli_badge};

pub(super) fn page_for<'a>(topic: HelpTopic, theme: &'a Theme) -> Option<PageSpec<'a>> {
    match topic {
        HelpTopic::Root => Some(root_page(theme)),
        HelpTopic::Start => Some(start_page(theme)),
        HelpTopic::Quit => Some(quit_page(theme)),
        HelpTopic::Update => Some(update_page(theme)),
        HelpTopic::Daemon => Some(daemon_page(theme)),
        HelpTopic::DaemonLog => Some(daemon_log_page(theme)),
        HelpTopic::Admin => Some(admin_page(theme)),
        HelpTopic::AdminAdd => Some(admin_add_page(theme)),
        HelpTopic::AdminRemoveMe => Some(admin_remove_me_page(theme)),
        _ => None,
    }
}

fn root_page(theme: &Theme) -> PageSpec<'_> {
    PageSpec {
        title: "sagens",
        about: "Local-first microVM sandboxes for agent workloads.",
        badge: cli_badge(theme),
        usage: &["sagens <command> [args]", "sagens help <command>"],
        commands: &[
            ("start", "Start the daemon and print endpoint/mode."),
            ("quit", "Gracefully stop the daemon if it is running."),
            (
                "update",
                "Download the latest release binary and replace this executable.",
            ),
            (
                "daemon",
                "Run the daemon in the foreground or inspect daemon logs.",
            ),
            ("admin", "Manage admin credentials."),
            ("image", "Build and manage local VM images."),
            ("box", "Create, inspect, configure, and operate BOXes."),
        ],
        examples: &[
            "sagens start",
            "sagens update",
            "sagens box new",
            "sagens box start <UUID>",
            "sagens box exec <UUID> bash",
        ],
        notes: &["Use `--help` on any command path for focused help, examples, and aliases."],
    }
}

fn start_page(theme: &Theme) -> PageSpec<'_> {
    PageSpec {
        title: "sagens start",
        about: "Start the daemon in the background if needed and print connection details.",
        badge: Some(theme.badge("Daemon", BadgeStyle::Success)),
        usage: &["sagens start"],
        commands: &[],
        examples: &["sagens start"],
        notes: &["Bootstraps user config when missing and reuses a healthy daemon when possible."],
    }
}

fn quit_page(theme: &Theme) -> PageSpec<'_> {
    PageSpec {
        title: "sagens quit",
        about: "Stop the daemon through the BOX API control path.",
        badge: Some(theme.badge("Daemon", BadgeStyle::Warning)),
        usage: &["sagens quit"],
        commands: &[],
        examples: &["sagens quit"],
        notes: &["Falls back to terminating a stale recorded daemon if graceful shutdown fails."],
    }
}

fn update_page(theme: &Theme) -> PageSpec<'_> {
    PageSpec {
        title: "sagens update",
        about: "Download the latest GitHub release for this platform and replace the current executable.",
        badge: Some(theme.badge("Self-update", BadgeStyle::Accent)),
        usage: &["sagens update"],
        commands: &[],
        examples: &["sagens update"],
        notes: &[
            "The release asset is selected automatically for linux/macos on x86_64/aarch64.",
            "Checksums are verified before the binary is staged and swapped into place.",
            "If a daemon is already running, restart it afterward so background work uses the new binary.",
        ],
    }
}

fn daemon_page(theme: &Theme) -> PageSpec<'_> {
    PageSpec {
        title: "sagens daemon",
        about: "Run the daemon in the foreground or inspect daemon logs.",
        badge: Some(theme.badge("Foreground", BadgeStyle::Muted)),
        usage: &[
            "sagens daemon",
            "sagens daemon log [--tail LINES] [--follow]",
        ],
        commands: &[("log", "Print the daemon log and optionally tail/follow it.")],
        examples: &["sagens daemon", "sagens daemon log --tail 200 --follow"],
        notes: &[
            "Bare `sagens daemon` runs the control-plane process in the foreground.",
            "The log subcommand reads `daemon.log` from the resolved state directory.",
        ],
    }
}

fn daemon_log_page(theme: &Theme) -> PageSpec<'_> {
    PageSpec {
        title: "sagens daemon log",
        about: "Read and follow the daemon log for sandbox startup and runtime failures.",
        badge: Some(theme.badge("Logs", BadgeStyle::Accent)),
        usage: &["sagens daemon log [--tail LINES] [--follow]"],
        commands: &[],
        examples: &[
            "sagens daemon log",
            "sagens daemon log --tail 100",
            "sagens daemon log --tail 200 --follow",
        ],
        notes: &[
            "When BOX startup fails, the daemon log records runner and guest console log paths.",
            "Use `--follow` to stream new daemon events while reproducing a flaky BOX start.",
        ],
    }
}

fn admin_page(theme: &Theme) -> PageSpec<'_> {
    PageSpec {
        title: "sagens admin",
        about: "Manage admin credentials for the local control plane.",
        badge: Some(theme.badge("Admin", BadgeStyle::Info)),
        usage: &["sagens admin <command>"],
        commands: &[
            ("add", "Issue a new admin credential bundle."),
            (
                "remove me",
                "Remove the active admin credential from the registry.",
            ),
        ],
        examples: &["sagens admin add", "sagens admin remove me"],
        notes: &["Admin commands require a healthy daemon and an authenticated user config."],
    }
}

fn admin_add_page(theme: &Theme) -> PageSpec<'_> {
    PageSpec {
        title: "sagens admin add",
        about: "Issue a new admin credential bundle.",
        badge: Some(theme.badge("Admin", BadgeStyle::Info)),
        usage: &["sagens admin add"],
        commands: &[],
        examples: &["sagens admin add"],
        notes: &["The command prints the new credential bundle with endpoint and token."],
    }
}

fn admin_remove_me_page(theme: &Theme) -> PageSpec<'_> {
    PageSpec {
        title: "sagens admin remove me",
        about: "Deactivate the current admin credential.",
        badge: Some(theme.badge("Admin", BadgeStyle::Danger)),
        usage: &["sagens admin remove me"],
        commands: &[],
        examples: &["sagens admin remove me"],
        notes: &["The registry refuses to remove the last active admin."],
    }
}
