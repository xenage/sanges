use crate::sagens::ui::{BadgeStyle, Theme};

use super::super::HelpTopic;
use super::super::help::PageSpec;

pub(super) fn page_for<'a>(topic: HelpTopic, theme: &'a Theme) -> PageSpec<'a> {
    match topic {
        HelpTopic::Box => box_page(theme),
        HelpTopic::BoxList => box_list_page(theme),
        HelpTopic::BoxNew => box_new_page(theme),
        HelpTopic::BoxStart => box_start_page(theme),
        HelpTopic::BoxStop => box_stop_page(theme),
        HelpTopic::BoxRemove => box_remove_page(theme),
        HelpTopic::BoxSet => box_set_page(theme),
        HelpTopic::BoxExec => box_exec_page(theme),
        HelpTopic::BoxFs => box_fs_page(theme),
        HelpTopic::BoxFsList => box_fs_list_page(theme),
        HelpTopic::BoxFsUpload => box_fs_upload_page(theme),
        HelpTopic::BoxFsDownload => box_fs_download_page(theme),
        HelpTopic::BoxCheckpoint => box_checkpoint_page(theme),
        HelpTopic::BoxCheckpointCreate => box_checkpoint_create_page(theme),
        HelpTopic::BoxCheckpointList => box_checkpoint_list_page(theme),
        HelpTopic::BoxCheckpointRestore => box_checkpoint_restore_page(theme),
        HelpTopic::BoxCheckpointFork => box_checkpoint_fork_page(theme),
        HelpTopic::BoxCheckpointDelete => box_checkpoint_delete_page(theme),
        _ => unreachable!("non-box help topic reached box pages"),
    }
}

fn box_page(theme: &Theme) -> PageSpec<'_> {
    PageSpec {
        title: "sagens box",
        about: "Operate and configure durable BOX workspaces.",
        badge: Some(theme.badge("BOX", BadgeStyle::Info)),
        usage: &["sagens box <command>"],
        commands: &[
            ("list | ps", "List BOXes with runtime settings and status."),
            ("new", "Create a new BOX, optionally from a named image."),
            ("start", "Start a BOX runtime."),
            ("stop", "Stop a BOX runtime."),
            ("rm", "Remove a BOX and its workspace."),
            ("set", "Change per-BOX runtime settings."),
            ("exec", "Run bash or python in a BOX."),
            ("fs", "List, upload, and download workspace files."),
            ("checkpoint", "Manage workspace checkpoints."),
        ],
        examples: &[
            "sagens box new --image chromium",
            "sagens box start <UUID>",
            "sagens box exec <UUID> bash",
            "sagens box set <UUID> fs_size_mib 2GiB",
        ],
        notes: &[
            "The short `box set <setting> <value>` form auto-targets the only BOX when exactly one exists.",
        ],
    }
}

fn box_list_page(theme: &Theme) -> PageSpec<'_> {
    PageSpec {
        title: "sagens box list",
        about: "List BOXes with live runtime usage and configured limits.",
        badge: Some(theme.badge("Read-only", BadgeStyle::Success)),
        usage: &["sagens box list", "sagens box ps"],
        commands: &[],
        examples: &["sagens box list", "sagens box ps"],
        notes: &[
            "CPU, RAM, FS size, and process count are shown as `live / configured` while the BOX is running.",
            "Stopped BOXes show `0 / configured`, and `NET` shows only the BOX network setting.",
        ],
    }
}

fn box_new_page(theme: &Theme) -> PageSpec<'_> {
    PageSpec {
        title: "sagens box new",
        about: "Create a new BOX workspace.",
        badge: Some(theme.badge("Create", BadgeStyle::Success)),
        usage: &["sagens box new [--image NAME]"],
        commands: &[],
        examples: &["sagens box new", "sagens box new --image chromium"],
        notes: &[
            "New BOXes inherit runtime defaults, host-aware maximum caps, and the selected immutable image.",
        ],
    }
}

fn box_start_page(theme: &Theme) -> PageSpec<'_> {
    PageSpec {
        title: "sagens box start",
        about: "Start a BOX runtime.",
        badge: Some(theme.badge("Runtime", BadgeStyle::Success)),
        usage: &["sagens box start <BOX_ID>"],
        commands: &[],
        examples: &["sagens box start <BOX_ID>"],
        notes: &["The BOX runtime is ephemeral; the workspace stays durable across stops."],
    }
}

fn box_stop_page(theme: &Theme) -> PageSpec<'_> {
    PageSpec {
        title: "sagens box stop",
        about: "Stop a BOX runtime without deleting its workspace.",
        badge: Some(theme.badge("Runtime", BadgeStyle::Warning)),
        usage: &["sagens box stop <BOX_ID>"],
        commands: &[],
        examples: &["sagens box stop <BOX_ID>"],
        notes: &[
            "Stop the BOX before changing CPU, RAM, FS size, process count, or network settings.",
        ],
    }
}

fn box_remove_page(theme: &Theme) -> PageSpec<'_> {
    PageSpec {
        title: "sagens box rm",
        about: "Remove a BOX and its workspace state.",
        badge: Some(theme.badge("Destructive", BadgeStyle::Danger)),
        usage: &["sagens box rm <BOX_ID>"],
        commands: &[],
        examples: &["sagens box rm <BOX_ID>"],
        notes: &[
            "Removes the persistent workspace and checkpoint lineage after stopping the runtime.",
        ],
    }
}

fn box_set_page(theme: &Theme) -> PageSpec<'_> {
    PageSpec {
        title: "sagens box set",
        about: "Update per-BOX runtime settings.",
        badge: Some(theme.badge("Settings", BadgeStyle::Info)),
        usage: &[
            "sagens box set <BOX_ID> <setting> <value>",
            "sagens box set <setting> <value>",
        ],
        commands: &[
            ("cpu_cores", "Set CPU cores for the BOX runtime."),
            ("memory_mb", "Set RAM in MiB. Accepts MiB/GiB suffixes."),
            ("fs_size_mib", "Resize the persistent workspace disk."),
            ("max_processes", "Set the process-count ceiling."),
            (
                "network_enabled",
                "Enable or disable networking when allowed.",
            ),
        ],
        examples: &[
            "sagens box set <BOX_ID> cpu_cores 4",
            "sagens box set memory_mb 2GiB",
            "sagens box set <BOX_ID> fs_size_mib 1536",
            "sagens box set <BOX_ID> network_enabled false",
        ],
        notes: &[
            "The short form without `BOX_ID` works only when exactly one BOX exists.",
            "BOXes must be stopped before settings can be changed.",
        ],
    }
}

fn box_exec_page(theme: &Theme) -> PageSpec<'_> {
    PageSpec {
        title: "sagens box exec",
        about: "Run bash or python in a BOX.",
        badge: Some(theme.badge("Exec", BadgeStyle::Accent)),
        usage: &[
            "sagens box exec <BOX_ID> bash <command...>",
            "sagens box exec <BOX_ID> python <args...>",
            "sagens box exec -i <BOX_ID> bash",
        ],
        commands: &[],
        examples: &[
            "sagens box exec <BOX_ID> bash \"echo hello\"",
            "sagens box exec <BOX_ID> python -c \"print('hi')\"",
            "sagens box exec -i <BOX_ID> bash",
        ],
        notes: &[
            "BOX must already be running; use `sagens box start <BOX_ID>` first.",
            "Interactive mode opens a real shell when `bash` or `python` is used without trailing arguments.",
        ],
    }
}

fn box_fs_page(theme: &Theme) -> PageSpec<'_> {
    PageSpec {
        title: "sagens box fs",
        about: "Inspect and move files in the BOX workspace.",
        badge: Some(theme.badge("Filesystem", BadgeStyle::Info)),
        usage: &["sagens box fs <BOX_ID> <command>"],
        commands: &[
            ("ls", "List files under a path."),
            ("upload", "Upload a local file or directory."),
            ("download", "Download a file or directory."),
        ],
        examples: &[
            "sagens box fs <BOX_ID> ls /workspace",
            "sagens box fs <BOX_ID> upload ./local ./workspace/local",
            "sagens box fs <BOX_ID> download /workspace/out ./out",
        ],
        notes: &["BOX must already be running; use `sagens box start <BOX_ID>` first."],
    }
}

fn box_fs_list_page(theme: &Theme) -> PageSpec<'_> {
    PageSpec {
        title: "sagens box fs ls",
        about: "List files under a workspace path.",
        badge: Some(theme.badge("Filesystem", BadgeStyle::Info)),
        usage: &["sagens box fs <BOX_ID> ls [PATH]"],
        commands: &[],
        examples: &[
            "sagens box fs <BOX_ID> ls",
            "sagens box fs <BOX_ID> ls /workspace/src",
        ],
        notes: &["Defaults to `/workspace` when no path is provided."],
    }
}

fn box_fs_upload_page(theme: &Theme) -> PageSpec<'_> {
    PageSpec {
        title: "sagens box fs upload",
        about: "Upload a file or directory into the workspace.",
        badge: Some(theme.badge("Filesystem", BadgeStyle::Success)),
        usage: &["sagens box fs <BOX_ID> upload <LOCAL_PATH> <REMOTE_PATH>"],
        commands: &[],
        examples: &["sagens box fs <BOX_ID> upload ./dist /workspace/dist"],
        notes: &["Directories are created recursively when needed."],
    }
}

fn box_fs_download_page(theme: &Theme) -> PageSpec<'_> {
    PageSpec {
        title: "sagens box fs download",
        about: "Download a file or directory from the workspace.",
        badge: Some(theme.badge("Filesystem", BadgeStyle::Success)),
        usage: &["sagens box fs <BOX_ID> download <REMOTE_PATH> <LOCAL_PATH>"],
        commands: &[],
        examples: &["sagens box fs <BOX_ID> download /workspace/build ./build"],
        notes: &["Directory downloads recreate the remote tree under the local target path."],
    }
}

fn box_checkpoint_page(theme: &Theme) -> PageSpec<'_> {
    PageSpec {
        title: "sagens box checkpoint",
        about: "Manage checkpoint lineage for a BOX workspace.",
        badge: Some(theme.badge("Checkpoint", BadgeStyle::Accent)),
        usage: &["sagens box checkpoint <command> <BOX_ID> ..."],
        commands: &[
            ("create", "Create a checkpoint."),
            ("list", "List checkpoints."),
            ("restore", "Restore a checkpoint."),
            ("fork", "Create a new BOX from a checkpoint."),
            ("delete", "Delete a checkpoint."),
        ],
        examples: &[
            "sagens box checkpoint create <BOX_ID> --name baseline",
            "sagens box checkpoint list <BOX_ID>",
            "sagens box checkpoint restore <BOX_ID> <CHECKPOINT_ID> --mode replace",
        ],
        notes: &[
            "Checkpoint storage is product-level and intentionally hides storage backend details.",
        ],
    }
}

fn box_checkpoint_create_page(theme: &Theme) -> PageSpec<'_> {
    PageSpec {
        title: "sagens box checkpoint create",
        about: "Create a checkpoint for the BOX workspace.",
        badge: Some(theme.badge("Checkpoint", BadgeStyle::Success)),
        usage: &["sagens box checkpoint create <BOX_ID> [--name NAME] [--meta KEY=VALUE]..."],
        commands: &[],
        examples: &["sagens box checkpoint create <BOX_ID> --name before-upgrade --meta env=dev"],
        notes: &["Metadata is stored as string key/value pairs."],
    }
}

fn box_checkpoint_list_page(theme: &Theme) -> PageSpec<'_> {
    PageSpec {
        title: "sagens box checkpoint list",
        about: "List checkpoints for a BOX.",
        badge: Some(theme.badge("Checkpoint", BadgeStyle::Info)),
        usage: &["sagens box checkpoint list <BOX_ID>"],
        commands: &[],
        examples: &["sagens box checkpoint list <BOX_ID>"],
        notes: &["The table includes checkpoint id, name, timestamp, and metadata."],
    }
}

fn box_checkpoint_restore_page(theme: &Theme) -> PageSpec<'_> {
    PageSpec {
        title: "sagens box checkpoint restore",
        about: "Restore a checkpoint into the BOX workspace.",
        badge: Some(theme.badge("Checkpoint", BadgeStyle::Warning)),
        usage: &[
            "sagens box checkpoint restore <BOX_ID> <CHECKPOINT_ID> [--mode rollback|replace]",
        ],
        commands: &[],
        examples: &[
            "sagens box checkpoint restore <BOX_ID> <CHECKPOINT_ID>",
            "sagens box checkpoint restore <BOX_ID> <CHECKPOINT_ID> --mode replace",
        ],
        notes: &[
            "When the BOX is running, the runtime is stopped before restore and restarted afterward.",
        ],
    }
}

fn box_checkpoint_fork_page(theme: &Theme) -> PageSpec<'_> {
    PageSpec {
        title: "sagens box checkpoint fork",
        about: "Create a new BOX from a checkpoint snapshot.",
        badge: Some(theme.badge("Checkpoint", BadgeStyle::Success)),
        usage: &["sagens box checkpoint fork <BOX_ID> <CHECKPOINT_ID> [--name NAME]"],
        commands: &[],
        examples: &["sagens box checkpoint fork <BOX_ID> <CHECKPOINT_ID> --name sandbox-copy"],
        notes: &[
            "The forked BOX inherits the source BOX settings and starts a fresh checkpoint lineage.",
        ],
    }
}

fn box_checkpoint_delete_page(theme: &Theme) -> PageSpec<'_> {
    PageSpec {
        title: "sagens box checkpoint delete",
        about: "Delete a checkpoint from lineage.",
        badge: Some(theme.badge("Checkpoint", BadgeStyle::Danger)),
        usage: &["sagens box checkpoint delete <BOX_ID> <CHECKPOINT_ID>"],
        commands: &[],
        examples: &["sagens box checkpoint delete <BOX_ID> <CHECKPOINT_ID>"],
        notes: &[
            "If the deleted checkpoint was the head, lineage head moves to its source checkpoint.",
        ],
    }
}
