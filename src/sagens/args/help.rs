use crate::sagens::ui::{BadgeStyle, Theme};

use super::HelpTopic;
use super::help_pages::page_for;
use crate::{Result, SandboxError};

pub(super) struct PageSpec<'a> {
    pub(super) title: &'a str,
    pub(super) about: &'a str,
    pub(super) badge: Option<String>,
    pub(super) usage: &'a [&'a str],
    pub(super) commands: &'a [(&'a str, &'a str)],
    pub(super) examples: &'a [&'a str],
    pub(super) notes: &'a [&'a str],
}

pub fn render_help(topic: HelpTopic) -> String {
    let theme = Theme::stdout();
    render_page(&theme, page_for(topic, &theme))
}

pub(super) fn parse_help_topic(args: &[String]) -> Result<HelpTopic> {
    let values = args.iter().map(String::as_str).collect::<Vec<_>>();
    let topic = match values.as_slice() {
        [] => HelpTopic::Root,
        ["start"] => HelpTopic::Start,
        ["quit"] => HelpTopic::Quit,
        ["update"] => HelpTopic::Update,
        ["daemon"] => HelpTopic::Daemon,
        ["daemon", "log"] => HelpTopic::DaemonLog,
        ["admin"] => HelpTopic::Admin,
        ["admin", "add"] => HelpTopic::AdminAdd,
        ["admin", "remove", "me"] => HelpTopic::AdminRemoveMe,
        ["box"] => HelpTopic::Box,
        ["box", "list"] | ["box", "ps"] => HelpTopic::BoxList,
        ["box", "new"] => HelpTopic::BoxNew,
        ["box", "start"] => HelpTopic::BoxStart,
        ["box", "stop"] => HelpTopic::BoxStop,
        ["box", "rm"] => HelpTopic::BoxRemove,
        ["box", "set"] => HelpTopic::BoxSet,
        ["box", "exec"] => HelpTopic::BoxExec,
        ["box", "fs"] => HelpTopic::BoxFs,
        ["box", "fs", "ls"] => HelpTopic::BoxFsList,
        ["box", "fs", "upload"] => HelpTopic::BoxFsUpload,
        ["box", "fs", "download"] => HelpTopic::BoxFsDownload,
        ["box", "checkpoint"] => HelpTopic::BoxCheckpoint,
        ["box", "checkpoint", "create"] => HelpTopic::BoxCheckpointCreate,
        ["box", "checkpoint", "list"] => HelpTopic::BoxCheckpointList,
        ["box", "checkpoint", "restore"] => HelpTopic::BoxCheckpointRestore,
        ["box", "checkpoint", "fork"] => HelpTopic::BoxCheckpointFork,
        ["box", "checkpoint", "delete"] => HelpTopic::BoxCheckpointDelete,
        _ => {
            return Err(SandboxError::invalid(format!(
                "unsupported help topic {}\n\n{}",
                values.join(" "),
                short_usage()
            )));
        }
    };
    Ok(topic)
}

pub(super) fn render_usage_hint(topic: HelpTopic) -> String {
    match topic {
        HelpTopic::Root => "usage: sagens <command> [args]".into(),
        HelpTopic::Start => "usage: sagens start".into(),
        HelpTopic::Quit => "usage: sagens quit".into(),
        HelpTopic::Update => "usage: sagens update".into(),
        HelpTopic::Daemon => "usage: sagens daemon [log [--tail LINES] [--follow]]".into(),
        HelpTopic::DaemonLog => "usage: sagens daemon log [--tail LINES] [--follow]".into(),
        HelpTopic::Admin => "usage: sagens admin <add|remove me>".into(),
        HelpTopic::AdminAdd => "usage: sagens admin add".into(),
        HelpTopic::AdminRemoveMe => "usage: sagens admin remove me".into(),
        HelpTopic::Box => "usage: sagens box <list|new|start|stop|rm|set|exec|fs|checkpoint>".into(),
        HelpTopic::BoxList => "usage: sagens box list".into(),
        HelpTopic::BoxNew => "usage: sagens box new".into(),
        HelpTopic::BoxStart => "usage: sagens box start <BOX_ID>".into(),
        HelpTopic::BoxStop => "usage: sagens box stop <BOX_ID>".into(),
        HelpTopic::BoxRemove => "usage: sagens box rm <BOX_ID>".into(),
        HelpTopic::BoxSet => "usage: sagens box set [BOX_ID] <setting> <value>".into(),
        HelpTopic::BoxExec => "usage: sagens box exec [-i] <BOX_ID> <bash|python> [args...]".into(),
        HelpTopic::BoxFs => "usage: sagens box fs <BOX_ID> <ls|upload|download> ...".into(),
        HelpTopic::BoxFsList => "usage: sagens box fs <BOX_ID> ls [PATH]".into(),
        HelpTopic::BoxFsUpload => {
            "usage: sagens box fs <BOX_ID> upload <LOCAL_PATH> <REMOTE_PATH>".into()
        }
        HelpTopic::BoxFsDownload => {
            "usage: sagens box fs <BOX_ID> download <REMOTE_PATH> <LOCAL_PATH>".into()
        }
        HelpTopic::BoxCheckpoint => {
            "usage: sagens box checkpoint <create|list|restore|fork|delete> <BOX_ID> ...".into()
        }
        HelpTopic::BoxCheckpointCreate => {
            "usage: sagens box checkpoint create <BOX_ID> [--name NAME] [--meta KEY=VALUE]..."
                .into()
        }
        HelpTopic::BoxCheckpointList => "usage: sagens box checkpoint list <BOX_ID>".into(),
        HelpTopic::BoxCheckpointRestore => {
            "usage: sagens box checkpoint restore <BOX_ID> <CHECKPOINT_ID> [--mode rollback|replace]".into()
        }
        HelpTopic::BoxCheckpointFork => {
            "usage: sagens box checkpoint fork <BOX_ID> <CHECKPOINT_ID> [--name NAME]".into()
        }
        HelpTopic::BoxCheckpointDelete => {
            "usage: sagens box checkpoint delete <BOX_ID> <CHECKPOINT_ID>".into()
        }
    }
}

pub(super) fn short_usage() -> &'static str {
    "usage: sagens <start|quit|update|daemon|admin|box> [args]"
}

fn render_page(theme: &Theme, page: PageSpec<'_>) -> String {
    let mut out = String::new();
    out.push_str(&theme.title(page.title));
    if let Some(badge) = page.badge {
        out.push(' ');
        out.push_str(&badge);
    }
    out.push('\n');
    out.push_str(&theme.muted(page.about));
    out.push('\n');
    out.push('\n');

    out.push_str(&theme.heading("Usage"));
    out.push('\n');
    for line in page.usage {
        out.push_str("  ");
        out.push_str(&theme.code(line));
        out.push('\n');
    }

    if !page.commands.is_empty() {
        out.push('\n');
        out.push_str(&theme.heading("Commands"));
        out.push('\n');
        for (name, description) in page.commands {
            out.push_str("  ");
            out.push_str(&theme.subheading(name));
            out.push_str("  ");
            out.push_str(description);
            out.push('\n');
        }
    }

    if !page.examples.is_empty() {
        out.push('\n');
        out.push_str(&theme.heading("Examples"));
        out.push('\n');
        for example in page.examples {
            out.push_str("  ");
            out.push_str(&theme.code(example));
            out.push('\n');
        }
    }

    if !page.notes.is_empty() {
        out.push('\n');
        out.push_str(&theme.heading("Notes"));
        out.push('\n');
        for note in page.notes {
            out.push_str("  • ");
            out.push_str(note);
            out.push('\n');
        }
    }

    out.trim_end().to_string()
}

pub(super) fn cli_badge(theme: &Theme) -> Option<String> {
    Some(theme.badge("CLI", BadgeStyle::Info))
}
