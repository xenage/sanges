use std::process::Command;

pub(crate) fn tool_command(tool: &str) -> Command {
    Command::new(tool)
}
