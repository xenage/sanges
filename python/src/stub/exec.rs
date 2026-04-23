use sagens_host::protocol::{ExecExit, ExecRequest};
use uuid::Uuid;

use super::state::{StubState, has_workspace_file, read_workspace_file, set_workspace_file};

pub(super) struct ExecPlan {
    pub(super) output: Vec<u8>,
    pub(super) status: ExecExit,
    pub(super) delay_ms: u64,
}

pub(super) fn run_exec_request(
    state: &mut StubState,
    box_id: Uuid,
    request: &ExecRequest,
) -> ExecPlan {
    if is_python_request(request) {
        return run_python_request(state, box_id, request.args.get(2).map(String::as_str));
    }
    run_shell_request(state, box_id, request.args.last().map(String::as_str))
}

fn is_python_request(request: &ExecRequest) -> bool {
    request.program == "/usr/bin/env"
        && request.args.first().is_some_and(|value| value == "python3")
        && request.args.get(1).is_some_and(|value| value == "-c")
}

fn run_python_request(state: &mut StubState, box_id: Uuid, script: Option<&str>) -> ExecPlan {
    let script = script.unwrap_or_default();
    if script.contains("Path('box.txt').write_text('persisted')") {
        set_workspace_file(state, box_id, "box.txt", b"persisted".to_vec());
        return success("python-e2e\n");
    }
    if script.contains("sagens_e2e_pkg.NAME") {
        let persisted = read_workspace_file(state, box_id, "box.txt");
        let persisted = String::from_utf8_lossy(&persisted);
        let package_name = if has_workspace_file(state, box_id, ".sandbox-pkgs/sagens_e2e_pkg.py")
        {
            "wheel-ok"
        } else {
            "missing"
        };
        return success(format!("{persisted} {package_name}\n"));
    }
    if script.contains("ok-from-raw-kernel") {
        return success("ok-from-raw-kernel\n");
    }
    success(format!("exec:{box_id}\n"))
}

fn run_shell_request(state: &mut StubState, box_id: Uuid, command: Option<&str>) -> ExecPlan {
    let command = command.unwrap_or_default();
    if command.contains("touch tracked.txt") {
        set_workspace_file(state, box_id, "tracked.txt", Vec::new());
        return success(Vec::new());
    }
    if command.contains("pip install")
        && command.contains("sagens-e2e-pkg")
        && command.contains(".sandbox-pkgs")
    {
        set_workspace_file(
            state,
            box_id,
            ".sandbox-pkgs/sagens_e2e_pkg.py",
            b"NAME = 'wheel-ok'\n".to_vec(),
        );
        return success(Vec::new());
    }
    let output = if command.contains("sleep") {
        format!("slow:{box_id}\n")
    } else if command.contains("hello-from-bash") {
        "hello-from-bash\n".into()
    } else {
        format!("exec:{box_id}\n")
    };
    let status = if command.contains("infinite") {
        ExecExit::Timeout
    } else if command.contains("ignore-term") {
        ExecExit::Killed
    } else {
        ExecExit::Success
    };
    ExecPlan {
        output: output.into_bytes(),
        status,
        delay_ms: if command.contains("sleep") { 150 } else { 0 },
    }
}

fn success(output: impl Into<Vec<u8>>) -> ExecPlan {
    ExecPlan {
        output: output.into(),
        status: ExecExit::Success,
        delay_ms: 0,
    }
}
