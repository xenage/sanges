use std::collections::BTreeMap;

use crate::box_api::InteractiveTarget;
use crate::{BoxSettingValue, CheckpointRestoreMode, Result, SandboxError};

use super::help::render_usage_hint;
use super::parse::{help_only, parse_uuid, single_arg, single_arg_from};
use super::{
    BoxCommand, BoxSetCommand, CheckpointCommand, Command, ExecCommand, ExecTarget, HelpTopic,
};

pub(super) fn parse_box_start(args: Vec<String>) -> Result<Command> {
    if help_only(&args) {
        return Ok(Command::Help(HelpTopic::BoxStart));
    }
    Ok(Command::Box(BoxCommand::Start(parse_uuid(single_arg(
        args,
        render_usage_hint(HelpTopic::BoxStart).as_str(),
    )?)?)))
}

pub(super) fn parse_box_stop(args: Vec<String>) -> Result<Command> {
    if help_only(&args) {
        return Ok(Command::Help(HelpTopic::BoxStop));
    }
    Ok(Command::Box(BoxCommand::Stop(parse_uuid(single_arg(
        args,
        render_usage_hint(HelpTopic::BoxStop).as_str(),
    )?)?)))
}

pub(super) fn parse_box_remove(args: Vec<String>) -> Result<Command> {
    if help_only(&args) {
        return Ok(Command::Help(HelpTopic::BoxRemove));
    }
    Ok(Command::Box(BoxCommand::Remove(parse_uuid(single_arg(
        args,
        render_usage_hint(HelpTopic::BoxRemove).as_str(),
    )?)?)))
}

pub(super) fn parse_box_set(args: Vec<String>) -> Result<Command> {
    if args.is_empty() || help_only(&args) {
        return Ok(Command::Help(HelpTopic::BoxSet));
    }
    let (box_id, setting, value) = match args.as_slice() {
        [setting, value] => (None, setting.clone(), value.clone()),
        [box_id, setting, value] => (
            Some(parse_uuid(box_id.clone())?),
            setting.clone(),
            value.clone(),
        ),
        _ => {
            return Err(SandboxError::invalid(render_usage_hint(HelpTopic::BoxSet)));
        }
    };
    Ok(Command::Box(BoxCommand::Set(BoxSetCommand {
        box_id,
        value: parse_box_setting(setting, value)?,
    })))
}

pub(super) fn parse_box_exec(mut args: Vec<String>) -> Result<Command> {
    if args.is_empty() || help_only(&args) {
        return Ok(Command::Help(HelpTopic::BoxExec));
    }
    let leading_interactive = match args.first().map(String::as_str) {
        Some("-i" | "--interactive") => {
            let _ = args.remove(0);
            true
        }
        _ => false,
    };
    let box_id = parse_uuid(single_arg_from(
        &mut args,
        render_usage_hint(HelpTopic::BoxExec).as_str(),
    )?)?;
    let target = match single_arg_from(
        &mut args,
        "box exec expects `bash` or `python` after <BOX_ID>",
    )? {
        value if value == "bash" && should_open_interactive(&args, leading_interactive) => {
            if is_trailing_interactive_flag(&args) {
                args.clear();
            }
            ExecTarget::Interactive(InteractiveTarget::Bash)
        }
        value if value == "python" && should_open_interactive(&args, leading_interactive) => {
            if is_trailing_interactive_flag(&args) {
                args.clear();
            }
            ExecTarget::Interactive(InteractiveTarget::Python)
        }
        value if value == "bash" => ExecTarget::Bash(args.join(" ")),
        value if value == "python" => ExecTarget::Python(args),
        value => {
            return Err(SandboxError::invalid(format!(
                "unsupported exec target {value}; expected bash or python"
            )));
        }
    };
    Ok(Command::Box(BoxCommand::Exec(ExecCommand {
        box_id,
        target,
    })))
}

pub(super) fn parse_box_checkpoint(mut args: Vec<String>) -> Result<Command> {
    if args.is_empty() || help_only(&args) {
        return Ok(Command::Help(HelpTopic::BoxCheckpoint));
    }
    let subcommand = single_arg_from(
        &mut args,
        render_usage_hint(HelpTopic::BoxCheckpoint).as_str(),
    )?;
    if help_only(&args) {
        return Ok(Command::Help(match subcommand.as_str() {
            "create" => HelpTopic::BoxCheckpointCreate,
            "list" => HelpTopic::BoxCheckpointList,
            "restore" => HelpTopic::BoxCheckpointRestore,
            "fork" => HelpTopic::BoxCheckpointFork,
            "delete" => HelpTopic::BoxCheckpointDelete,
            _ => HelpTopic::BoxCheckpoint,
        }));
    }
    let box_id = parse_uuid(single_arg_from(
        &mut args,
        render_usage_hint(HelpTopic::BoxCheckpoint).as_str(),
    )?)?;
    let command = match subcommand.as_str() {
        "create" => {
            let mut name = None;
            let mut metadata = BTreeMap::new();
            while !args.is_empty() {
                match args.remove(0).as_str() {
                    "--name" => {
                        name = Some(single_arg_from(
                            &mut args,
                            render_usage_hint(HelpTopic::BoxCheckpointCreate).as_str(),
                        )?);
                    }
                    "--meta" => {
                        let raw = single_arg_from(
                            &mut args,
                            render_usage_hint(HelpTopic::BoxCheckpointCreate).as_str(),
                        )?;
                        let (key, value) = raw.split_once('=').ok_or_else(|| {
                            SandboxError::invalid(format!(
                                "invalid checkpoint metadata {raw}; expected KEY=VALUE"
                            ))
                        })?;
                        metadata.insert(key.into(), value.into());
                    }
                    other => {
                        return Err(SandboxError::invalid(format!(
                            "unknown checkpoint create flag {other}"
                        )));
                    }
                }
            }
            CheckpointCommand::Create {
                box_id,
                name,
                metadata,
            }
        }
        "list" if args.is_empty() => CheckpointCommand::List { box_id },
        "restore" => {
            let checkpoint_id = single_arg_from(
                &mut args,
                render_usage_hint(HelpTopic::BoxCheckpointRestore).as_str(),
            )?;
            let mut mode = CheckpointRestoreMode::Rollback;
            while !args.is_empty() {
                match args.remove(0).as_str() {
                    "--mode" => {
                        mode = match single_arg_from(
                            &mut args,
                            render_usage_hint(HelpTopic::BoxCheckpointRestore).as_str(),
                        )?
                        .as_str()
                        {
                            "rollback" => CheckpointRestoreMode::Rollback,
                            "replace" => CheckpointRestoreMode::Replace,
                            other => {
                                return Err(SandboxError::invalid(format!(
                                    "unsupported checkpoint restore mode {other}"
                                )));
                            }
                        };
                    }
                    other => {
                        return Err(SandboxError::invalid(format!(
                            "unknown checkpoint restore flag {other}"
                        )));
                    }
                }
            }
            CheckpointCommand::Restore {
                box_id,
                checkpoint_id,
                mode,
            }
        }
        "fork" => {
            let checkpoint_id = single_arg_from(
                &mut args,
                render_usage_hint(HelpTopic::BoxCheckpointFork).as_str(),
            )?;
            let mut new_box_name = None;
            while !args.is_empty() {
                match args.remove(0).as_str() {
                    "--name" => {
                        new_box_name = Some(single_arg_from(
                            &mut args,
                            render_usage_hint(HelpTopic::BoxCheckpointFork).as_str(),
                        )?);
                    }
                    other => {
                        return Err(SandboxError::invalid(format!(
                            "unknown checkpoint fork flag {other}"
                        )));
                    }
                }
            }
            CheckpointCommand::Fork {
                box_id,
                checkpoint_id,
                new_box_name,
            }
        }
        "delete" => CheckpointCommand::Delete {
            box_id,
            checkpoint_id: single_arg_from(
                &mut args,
                render_usage_hint(HelpTopic::BoxCheckpointDelete).as_str(),
            )?,
        },
        other => {
            return Err(SandboxError::invalid(format!(
                "unsupported checkpoint command {other}; expected create|list|restore|fork|delete"
            )));
        }
    };
    Ok(Command::Box(BoxCommand::Checkpoint(command)))
}

fn parse_box_setting(setting: String, value: String) -> Result<BoxSettingValue> {
    match setting.trim().to_ascii_lowercase().as_str() {
        "cpu" | "cpu_cores" => Ok(BoxSettingValue::CpuCores {
            value: parse_u32_value("cpu_cores", &value)?,
        }),
        "memory" | "ram" | "memory_mb" | "ram_mb" => Ok(BoxSettingValue::MemoryMb {
            value: u32::try_from(parse_mib_value("memory_mb", &value)?).map_err(|_| {
                SandboxError::invalid("memory_mb is too large to fit into a 32-bit value")
            })?,
        }),
        "fs" | "disk" | "fs_size" | "fs_size_mib" | "workspace_mib" | "disk_mib" => {
            Ok(BoxSettingValue::FsSizeMib {
                value: parse_mib_value("fs_size_mib", &value)?,
            })
        }
        "proc" | "process_count" | "max_processes" => Ok(BoxSettingValue::MaxProcesses {
            value: parse_u32_value("max_processes", &value)?,
        }),
        "network" | "net" | "network_enabled" => Ok(BoxSettingValue::NetworkEnabled {
            value: parse_bool_value("network_enabled", &value)?,
        }),
        other => Err(SandboxError::invalid(format!(
            "unsupported BOX setting {other}; expected cpu_cores|memory_mb|fs_size_mib|max_processes|network_enabled"
        ))),
    }
}

fn parse_u32_value(name: &str, raw: &str) -> Result<u32> {
    raw.trim()
        .replace('_', "")
        .parse::<u32>()
        .map_err(|error| SandboxError::invalid(format!("invalid {name} value {raw}: {error}")))
}

fn parse_bool_value(name: &str, raw: &str) -> Result<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" | "enabled" => Ok(true),
        "0" | "false" | "no" | "off" | "disabled" => Ok(false),
        _ => Err(SandboxError::invalid(format!(
            "invalid {name} value {raw}; expected true|false"
        ))),
    }
}

fn parse_mib_value(name: &str, raw: &str) -> Result<u64> {
    let normalized = raw.trim().replace('_', "").to_ascii_lowercase();
    let (digits, factor) = if let Some(stripped) = normalized.strip_suffix("gib") {
        (stripped, 1024)
    } else if let Some(stripped) = normalized.strip_suffix("gb") {
        (stripped, 1024)
    } else if let Some(stripped) = normalized.strip_suffix('g') {
        (stripped, 1024)
    } else if let Some(stripped) = normalized.strip_suffix("mib") {
        (stripped, 1)
    } else if let Some(stripped) = normalized.strip_suffix("mb") {
        (stripped, 1)
    } else if let Some(stripped) = normalized.strip_suffix('m') {
        (stripped, 1)
    } else {
        (normalized.as_str(), 1)
    };
    let value = digits
        .parse::<u64>()
        .map_err(|error| SandboxError::invalid(format!("invalid {name} value {raw}: {error}")))?;
    value
        .checked_mul(factor)
        .ok_or_else(|| SandboxError::invalid(format!("{name} value {raw} is too large")))
}

fn should_open_interactive(args: &[String], leading_interactive: bool) -> bool {
    leading_interactive || args.is_empty() || is_trailing_interactive_flag(args)
}

fn is_trailing_interactive_flag(args: &[String]) -> bool {
    args.len() == 1 && matches!(args[0].as_str(), "-i" | "--interactive")
}
