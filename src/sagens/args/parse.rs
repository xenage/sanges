use crate::{Result, SandboxError};

use super::help::{parse_help_topic, render_usage_hint, short_usage};
use super::parse_box::{
    parse_box_checkpoint, parse_box_exec, parse_box_remove, parse_box_set, parse_box_start,
    parse_box_stop,
};
use super::{AdminCommand, BoxCommand, Command, DaemonCommand, DaemonLogCommand, HelpTopic};

pub fn parse(mut args: Vec<String>) -> Result<Command> {
    if args.is_empty() {
        return Ok(Command::Help(HelpTopic::Root));
    }
    match args.remove(0).as_str() {
        "help" | "-h" | "--help" => Ok(Command::Help(parse_help_topic(&args)?)),
        "start" => parse_leaf(args, HelpTopic::Start, Command::Start),
        "quit" => parse_leaf(args, HelpTopic::Quit, Command::Quit),
        "update" => parse_leaf(args, HelpTopic::Update, Command::Update),
        "daemon" => parse_daemon(args),
        "admin" => parse_admin(args),
        "box" => parse_box(args),
        other => Err(SandboxError::invalid(format!(
            "unknown command {other}\n\n{}",
            short_usage()
        ))),
    }
}

fn parse_leaf(args: Vec<String>, topic: HelpTopic, command: Command) -> Result<Command> {
    if args.is_empty() {
        return Ok(command);
    }
    if help_only(&args) {
        return Ok(Command::Help(topic));
    }
    Err(SandboxError::invalid(render_usage_hint(topic)))
}

fn parse_daemon(mut args: Vec<String>) -> Result<Command> {
    if args.is_empty() {
        return Ok(Command::Daemon(DaemonCommand::Run));
    }
    if help_only(&args) {
        return Ok(Command::Help(HelpTopic::Daemon));
    }
    match args.remove(0).as_str() {
        "log" => parse_daemon_log(args),
        other => Err(SandboxError::invalid(format!(
            "unknown daemon command {other}\n\n{}",
            render_usage_hint(HelpTopic::Daemon)
        ))),
    }
}

fn parse_daemon_log(mut args: Vec<String>) -> Result<Command> {
    if args.is_empty() {
        return Ok(Command::Daemon(DaemonCommand::Log(DaemonLogCommand {
            tail: None,
            follow: false,
        })));
    }
    if help_only(&args) {
        return Ok(Command::Help(HelpTopic::DaemonLog));
    }

    let mut tail = None;
    let mut follow = false;
    while !args.is_empty() {
        let arg = args.remove(0);
        match arg.as_str() {
            "--follow" | "-f" => follow = true,
            "--tail" => {
                let value =
                    single_arg_from(&mut args, render_usage_hint(HelpTopic::DaemonLog).as_str())?;
                tail = Some(parse_tail_lines(&value)?);
            }
            _ if arg.starts_with("--tail=") => {
                tail = Some(parse_tail_lines(&arg["--tail=".len()..])?);
            }
            _ if is_help_flag(&arg) => return Ok(Command::Help(HelpTopic::DaemonLog)),
            _ => {
                return Err(SandboxError::invalid(render_usage_hint(
                    HelpTopic::DaemonLog,
                )));
            }
        }
    }

    Ok(Command::Daemon(DaemonCommand::Log(DaemonLogCommand {
        tail,
        follow,
    })))
}

fn parse_admin(mut args: Vec<String>) -> Result<Command> {
    if args.is_empty() || help_only(&args) {
        return Ok(Command::Help(HelpTopic::Admin));
    }
    match args.remove(0).as_str() {
        "add" => {
            ensure_no_extra_args(&args, HelpTopic::AdminAdd)?;
            Ok(Command::Admin(AdminCommand::Add))
        }
        "remove" if args.len() == 1 && args[0] == "me" => {
            Ok(Command::Admin(AdminCommand::RemoveMe))
        }
        "remove" if help_only(&args) => Ok(Command::Help(HelpTopic::AdminRemoveMe)),
        other => Err(SandboxError::invalid(format!(
            "unknown admin command {other}\n\n{}",
            render_usage_hint(HelpTopic::Admin)
        ))),
    }
}

fn parse_tail_lines(value: &str) -> Result<usize> {
    let lines = value
        .parse::<usize>()
        .map_err(|error| SandboxError::invalid(format!("invalid --tail value {value}: {error}")))?;
    if lines == 0 {
        return Err(SandboxError::invalid("--tail must be greater than zero"));
    }
    Ok(lines)
}

fn parse_box(mut args: Vec<String>) -> Result<Command> {
    if args.is_empty() || help_only(&args) {
        return Ok(Command::Help(HelpTopic::Box));
    }
    match args.remove(0).as_str() {
        "list" | "ps" => {
            ensure_no_extra_args(&args, HelpTopic::BoxList)?;
            Ok(Command::Box(BoxCommand::List))
        }
        "new" => {
            ensure_no_extra_args(&args, HelpTopic::BoxNew)?;
            Ok(Command::Box(BoxCommand::New))
        }
        "start" => parse_box_start(args),
        "stop" => parse_box_stop(args),
        "rm" => parse_box_remove(args),
        "set" => parse_box_set(args),
        "exec" => parse_box_exec(args),
        "fs" => parse_box_fs(args),
        "checkpoint" => parse_box_checkpoint(args),
        other => Err(SandboxError::invalid(format!(
            "unknown box command {other}\n\n{}",
            render_usage_hint(HelpTopic::Box)
        ))),
    }
}

fn parse_box_fs(mut args: Vec<String>) -> Result<Command> {
    if args.is_empty() || help_only(&args) {
        return Ok(Command::Help(HelpTopic::BoxFs));
    }
    let box_id = parse_uuid(single_arg_from(
        &mut args,
        render_usage_hint(HelpTopic::BoxFs).as_str(),
    )?)?;
    let subcommand = single_arg_from(&mut args, render_usage_hint(HelpTopic::BoxFs).as_str())?;
    let command = match subcommand.as_str() {
        "ls" if help_only(&args) => return Ok(Command::Help(HelpTopic::BoxFsList)),
        "ls" => super::FsCommand::List {
            box_id,
            path: args.first().cloned().unwrap_or_else(|| "/workspace".into()),
        },
        "upload" if help_only(&args) => return Ok(Command::Help(HelpTopic::BoxFsUpload)),
        "upload" => super::FsCommand::Upload {
            box_id,
            local_path: single_arg_from(
                &mut args,
                render_usage_hint(HelpTopic::BoxFsUpload).as_str(),
            )?,
            remote_path: single_arg_from(
                &mut args,
                render_usage_hint(HelpTopic::BoxFsUpload).as_str(),
            )?,
        },
        "download" if help_only(&args) => return Ok(Command::Help(HelpTopic::BoxFsDownload)),
        "download" => super::FsCommand::Download {
            box_id,
            remote_path: single_arg_from(
                &mut args,
                render_usage_hint(HelpTopic::BoxFsDownload).as_str(),
            )?,
            local_path: single_arg_from(
                &mut args,
                render_usage_hint(HelpTopic::BoxFsDownload).as_str(),
            )?,
        },
        other => {
            return Err(SandboxError::invalid(format!(
                "unsupported fs command {other}; expected ls|upload|download"
            )));
        }
    };
    Ok(Command::Box(BoxCommand::Fs(command)))
}

pub(super) fn help_only(args: &[String]) -> bool {
    args.len() == 1 && is_help_flag(&args[0])
}

pub(super) fn ensure_no_extra_args(args: &[String], topic: HelpTopic) -> Result<()> {
    if args.is_empty() || help_only(args) {
        return Ok(());
    }
    Err(SandboxError::invalid(render_usage_hint(topic)))
}

pub(super) fn single_arg(args: Vec<String>, usage: &str) -> Result<String> {
    if args.len() != 1 {
        return Err(SandboxError::invalid(usage.to_string()));
    }
    args.into_iter()
        .next()
        .ok_or_else(|| SandboxError::invalid(usage.to_string()))
}

pub(super) fn single_arg_from(args: &mut Vec<String>, usage: &str) -> Result<String> {
    if args.is_empty() {
        return Err(SandboxError::invalid(usage.to_string()));
    }
    Ok(args.remove(0))
}

pub(super) fn parse_uuid(value: String) -> Result<uuid::Uuid> {
    uuid::Uuid::parse_str(&value)
        .map_err(|error| SandboxError::invalid(format!("invalid BOX UUID {value}: {error}")))
}

pub(super) fn is_help_flag(value: &str) -> bool {
    matches!(value, "-h" | "--help")
}
