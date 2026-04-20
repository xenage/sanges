use crate::{Result, SandboxError};

#[derive(Clone, Copy)]
pub(crate) struct BootConfig {
    pub(crate) tmpfs_mib: u32,
    pub(crate) uid: u32,
    pub(crate) gid: u32,
    pub(crate) max_processes: u32,
    pub(crate) max_open_files: u32,
    pub(crate) max_file_size_bytes: u64,
    pub(crate) network_enabled: bool,
    pub(crate) rpc_port: u32,
}

impl BootConfig {
    pub(crate) fn from_cmdline(path: &str) -> Result<Self> {
        let cmdline = std::fs::read_to_string(path)
            .map_err(|error| SandboxError::io("reading /proc/cmdline", error))?;
        let mut config = Self {
            tmpfs_mib: 256,
            uid: 65_534,
            gid: 65_534,
            max_processes: 256,
            max_open_files: 1024,
            max_file_size_bytes: 16 * 1024 * 1024,
            network_enabled: false,
            rpc_port: 11_000,
        };
        for token in cmdline.split_whitespace() {
            if let Some(value) = token.strip_prefix("sandbox.tmpfs_mib=") {
                config.tmpfs_mib = parse_u32(value, "sandbox.tmpfs_mib")?;
            } else if let Some(value) = token.strip_prefix("sandbox.uid=") {
                config.uid = parse_u32(value, "sandbox.uid")?;
            } else if let Some(value) = token.strip_prefix("sandbox.gid=") {
                config.gid = parse_u32(value, "sandbox.gid")?;
            } else if let Some(value) = token.strip_prefix("sandbox.max_processes=") {
                config.max_processes = parse_u32(value, "sandbox.max_processes")?;
            } else if let Some(value) = token.strip_prefix("sandbox.max_open_files=") {
                config.max_open_files = parse_u32(value, "sandbox.max_open_files")?;
            } else if let Some(value) = token.strip_prefix("sandbox.max_file_size_bytes=") {
                config.max_file_size_bytes = parse_u64(value, "sandbox.max_file_size_bytes")?;
            } else if let Some(value) = token.strip_prefix("sandbox.network_enabled=") {
                config.network_enabled = parse_u32(value, "sandbox.network_enabled")? != 0;
            } else if let Some(value) = token.strip_prefix("sandbox.rpc_port=") {
                config.rpc_port = parse_u32(value, "sandbox.rpc_port")?;
            }
        }
        Ok(config)
    }
}

fn parse_u32(value: &str, field: &str) -> Result<u32> {
    value
        .parse()
        .map_err(|_| SandboxError::invalid(format!("{field} must be an integer")))
}

fn parse_u64(value: &str, field: &str) -> Result<u64> {
    value
        .parse()
        .map_err(|_| SandboxError::invalid(format!("{field} must be an integer")))
}
