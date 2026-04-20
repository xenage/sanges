use std::collections::BTreeMap;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ExecRequest {
    pub program: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: String,
    pub timeout_ms: Option<u64>,
    pub kill_grace_ms: u64,
}

impl ExecRequest {
    pub fn python(code: impl Into<String>) -> Self {
        Self {
            program: "/usr/bin/env".into(),
            args: vec!["python3".into(), "-u".into(), "-c".into(), code.into()],
            env: BTreeMap::new(),
            cwd: "/workspace".into(),
            timeout_ms: None,
            kill_grace_ms: 250,
        }
    }

    pub fn shell(command: impl Into<String>) -> Self {
        Self {
            program: "/bin/bash".into(),
            args: vec![
                "--noprofile".into(),
                "--norc".into(),
                "-lc".into(),
                command.into(),
            ],
            env: BTreeMap::new(),
            cwd: "/workspace".into(),
            timeout_ms: None,
            kill_grace_ms: 250,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ShellRequest {
    pub program: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: String,
}

impl Default for ShellRequest {
    fn default() -> Self {
        let mut env = BTreeMap::new();
        env.insert("TERM".into(), "dumb".into());
        env.insert("PS1".into(), "$ ".into());
        env.insert("INPUTRC".into(), "/dev/null".into());
        Self {
            program: "/bin/bash".into(),
            args: vec![
                "--noprofile".into(),
                "--norc".into(),
                "--noediting".into(),
                "-i".into(),
            ],
            env,
            cwd: "/workspace".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutputStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecExit {
    Success,
    ExitCode(i32),
    Timeout,
    Killed,
}
