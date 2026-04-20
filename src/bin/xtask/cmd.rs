use std::env;
use std::process::Command;
use std::sync::OnceLock;

pub(crate) fn tool_command(tool: &str) -> Command {
    if has_rtk() {
        let mut command = Command::new("rtk");
        command.arg(tool);
        return command;
    }
    Command::new(tool)
}

fn has_rtk() -> bool {
    static HAS_RTK: OnceLock<bool> = OnceLock::new();
    *HAS_RTK.get_or_init(|| tool_on_path("rtk"))
}

fn tool_on_path(tool: &str) -> bool {
    let paths = env::var_os("PATH")
        .map(|value| env::split_paths(&value).collect::<Vec<_>>())
        .unwrap_or_default();
    paths
        .into_iter()
        .map(|dir| dir.join(tool))
        .any(|path| path.is_file())
}
