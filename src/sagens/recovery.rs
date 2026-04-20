use std::net::TcpListener;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use crate::auth::{UserConfig, write_user_config};
use crate::sagens::config::SagensPaths;
use crate::sagens::config::parse_endpoint_addr;
use crate::sagens::daemon::cleanup_pid_file;
use crate::{Result, SandboxError};

pub(super) async fn recover_startup_state(
    paths: &SagensPaths,
    user_config: &mut UserConfig,
) -> Result<()> {
    cleanup_stale_daemon(paths).await?;
    if std::env::var_os("SAGENS_ENDPOINT").is_some() {
        return Ok(());
    }
    if endpoint_is_bindable(&user_config.endpoint)? {
        return Ok(());
    }
    let next_endpoint = reserve_local_endpoint(&user_config.endpoint)?;
    if next_endpoint != user_config.endpoint {
        user_config.endpoint = next_endpoint;
        write_user_config(&paths.user_config_path, user_config).await?;
    }
    Ok(())
}

pub(super) async fn terminate_recorded_daemon(paths: &SagensPaths) -> Result<bool> {
    let Some(pid) = read_pid_file(paths).await? else {
        cleanup_pid_file(paths).await?;
        return Ok(false);
    };
    let process_exists = process_exists(pid);
    if process_exists && process_looks_like_sagens(pid)? {
        terminate_process(pid).await?;
    }
    cleanup_pid_file(paths).await?;
    Ok(process_exists)
}

async fn cleanup_stale_daemon(paths: &SagensPaths) -> Result<()> {
    let _ = terminate_recorded_daemon(paths).await?;
    Ok(())
}

async fn read_pid_file(paths: &SagensPaths) -> Result<Option<u32>> {
    let contents = match tokio::fs::read_to_string(&paths.pid_path).await {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(SandboxError::io("reading daemon pid file", error)),
    };
    let trimmed = contents.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    trimmed
        .parse::<u32>()
        .map(Some)
        .map_err(|error| SandboxError::invalid(format!("invalid daemon pid file: {error}")))
}

pub(super) async fn recorded_daemon_uses_binary(
    paths: &SagensPaths,
    host_binary: &Path,
) -> Result<Option<bool>> {
    let Some(pid) = read_pid_file(paths).await? else {
        return Ok(None);
    };
    if !process_exists(pid) || !process_looks_like_sagens(pid)? {
        return Ok(Some(false));
    }
    Ok(Some(process_command_matches_binary(pid, host_binary)?))
}

fn endpoint_is_bindable(endpoint: &str) -> Result<bool> {
    let addr = parse_endpoint_addr(endpoint)?;
    match TcpListener::bind(addr) {
        Ok(listener) => {
            drop(listener);
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => Ok(false),
        Err(error) => Err(SandboxError::io("probing daemon endpoint", error)),
    }
}

fn reserve_local_endpoint(endpoint: &str) -> Result<String> {
    let addr = parse_endpoint_addr(endpoint)?;
    let listener = TcpListener::bind((addr.ip(), 0))
        .map_err(|error| SandboxError::io("reserving replacement daemon endpoint", error))?;
    let port = listener
        .local_addr()
        .map_err(|error| SandboxError::io("reading replacement daemon endpoint", error))?
        .port();
    Ok(format!("ws://{}:{port}", addr.ip()))
}

fn process_looks_like_sagens(pid: u32) -> Result<bool> {
    let command = process_command(pid)?;
    Ok(command.contains("sagens"))
}

fn process_command_matches_binary(pid: u32, host_binary: &Path) -> Result<bool> {
    let command = process_command(pid)?;
    let host_binary = host_binary.to_string_lossy();
    Ok(command.contains(host_binary.as_ref()))
}

fn process_command(pid: u32) -> Result<String> {
    #[cfg(unix)]
    {
        let output = Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "command="])
            .output()
            .map_err(|error| SandboxError::io("running ps for daemon recovery", error))?;
        if !output.status.success() {
            return Ok(String::new());
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        Ok(String::new())
    }
}

async fn terminate_process(pid: u32) -> Result<()> {
    #[cfg(unix)]
    {
        if pid == std::process::id() {
            return Ok(());
        }
        let pid = pid as i32;
        kill(pid, libc::SIGTERM)?;
        wait_for_exit(pid as u32, Duration::from_secs(2)).await?;
        if process_exists(pid as u32) {
            kill(pid, libc::SIGKILL)?;
            wait_for_exit(pid as u32, Duration::from_secs(1)).await?;
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        Ok(())
    }
}

async fn wait_for_exit(pid: u32, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    while process_exists(pid) && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Ok(())
}

#[cfg(unix)]
fn kill(pid: i32, signal: i32) -> Result<()> {
    let status = unsafe { libc::kill(pid, signal) };
    if status == 0 {
        Ok(())
    } else {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::NotFound {
            Ok(())
        } else {
            Err(SandboxError::io("signaling daemon pid", error))
        }
    }
}

fn process_exists(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let status = unsafe { libc::kill(pid as i32, 0) };
        if status == 0 {
            return true;
        }
        let error = std::io::Error::last_os_error();
        error.raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn rewrites_endpoint_when_port_is_occupied_and_pid_file_is_stale() {
        let temp = tempdir().expect("tempdir");
        let endpoint_listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let occupied_addr = endpoint_listener.local_addr().expect("addr");
        let paths = SagensPaths {
            state_dir: temp.path().join("state"),
            user_config_path: temp.path().join("config.json"),
            endpoint: format!("ws://{occupied_addr}"),
            pid_path: temp.path().join("state/daemon.pid"),
        };
        tokio::fs::create_dir_all(&paths.state_dir)
            .await
            .expect("state dir");
        tokio::fs::write(&paths.pid_path, b"999999\n")
            .await
            .expect("pid file");
        let mut user_config = UserConfig::new(paths.endpoint.clone());
        user_config.endpoint = paths.endpoint.clone();
        write_user_config(&paths.user_config_path, &user_config)
            .await
            .expect("config");

        recover_startup_state(&paths, &mut user_config)
            .await
            .expect("recovery");

        assert_ne!(user_config.endpoint, format!("ws://{occupied_addr}"));
        assert!(
            !tokio::fs::try_exists(&paths.pid_path)
                .await
                .expect("pid exists")
        );
        let persisted = tokio::fs::read_to_string(&paths.user_config_path)
            .await
            .expect("persisted config");
        assert!(persisted.contains(&user_config.endpoint));
    }

    #[test]
    fn keeps_endpoint_when_it_is_bindable() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let addr = listener.local_addr().expect("addr");
        drop(listener);
        assert!(endpoint_is_bindable(&format!("ws://{addr}")).expect("bindable"));
    }
}
