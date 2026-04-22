use std::path::Path;
use std::sync::Arc;
use std::{io::Seek, io::SeekFrom, io::Write};

use crate::auth::{
    AdminCredential, AdminStore, BoxCredentialStore, UserConfig, read_user_config,
    write_user_config,
};
use crate::boxes::{BoxManager, LocalBoxService};
use crate::host_log;
use crate::runtime::{AgentSandboxService, SandboxService};
use crate::sagens::config::{
    SagensPaths, build_runtime_config_for_endpoint, validate_host_process_binary,
};
use crate::sagens::recovery::{
    recorded_daemon_uses_binary, recover_startup_state, terminate_recorded_daemon,
};
use crate::{Result, SandboxError, serve_box_api_websocket};

const DAEMON_WAIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

pub async fn run_foreground(paths: &SagensPaths, host_binary: &Path) -> Result<()> {
    validate_host_process_binary(host_binary)?;
    let config = build_runtime_config_for_endpoint(&paths.state_dir, &paths.endpoint)?;
    host_log::emit(
        "daemon",
        format!(
            "starting endpoint={} isolation_mode={:?} state_dir={}",
            config.control.bind_addr,
            config.isolation_mode,
            paths.state_dir.display()
        ),
    );
    let runtime: Arc<dyn SandboxService> =
        Arc::new(AgentSandboxService::new(config.clone()).await?);
    let service: Arc<dyn BoxManager> = Arc::new(
        LocalBoxService::new(
            config.state_dir.clone(),
            config.workspace.clone(),
            config.default_policy,
            config.isolation_mode,
            runtime,
        )
        .await?,
    );
    let admin_store = Arc::new(AdminStore::new(&config.state_dir));
    let box_credential_store = Arc::new(BoxCredentialStore::new(&config.state_dir));
    bootstrap_admin_if_needed(&admin_store).await?;
    write_pid_file(&paths.pid_path).await?;
    let handle = serve_box_api_websocket(
        config.control.bind_addr,
        service,
        admin_store,
        box_credential_store,
        config.isolation_mode,
    )
    .await?;
    host_log::emit(
        "daemon",
        format!(
            "listening endpoint=ws://{} isolation_mode={:?}",
            handle.addr, config.isolation_mode
        ),
    );
    println!(
        "sagens daemon listening on ws://{} ({:?} isolation)",
        handle.addr, config.isolation_mode
    );
    let result = handle.wait().await;
    match &result {
        Ok(()) => host_log::emit("daemon", "shutdown complete"),
        Err(error) => host_log::emit("daemon", format!("shutdown with error: {error}")),
    }
    let _ = cleanup_pid_file(paths).await;
    result
}

pub async fn ensure_started(paths: &SagensPaths, host_binary: &Path) -> Result<(UserConfig, bool)> {
    let mut user_config = ensure_user_config(paths).await?;
    let daemon_healthy = daemon_is_healthy(&user_config).await;
    let daemon_matches_binary = recorded_daemon_uses_binary(paths, host_binary).await?;
    if daemon_healthy && daemon_matches_binary != Some(false) {
        return Ok((user_config, true));
    }
    if daemon_healthy {
        let _ = terminate_recorded_daemon(paths).await?;
    }
    recover_startup_state(paths, &mut user_config).await?;
    validate_host_process_binary(host_binary)?;
    let _ = build_runtime_config_for_endpoint(&paths.state_dir, &paths.endpoint)?;
    if let Some(parent) = paths.state_dir.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| SandboxError::io("creating daemon parent state directory", error))?;
    }
    tokio::fs::create_dir_all(&paths.state_dir)
        .await
        .map_err(|error| SandboxError::io("creating daemon state directory", error))?;
    spawn_background_daemon(paths, host_binary, &user_config)
        .map_err(|error| SandboxError::io("spawning sagens daemon", error))?;
    wait_for_daemon(&user_config, &paths.daemon_log_path).await?;
    Ok((user_config, false))
}

fn spawn_background_daemon(
    paths: &SagensPaths,
    host_binary: &Path,
    user_config: &UserConfig,
) -> std::io::Result<()> {
    let stdout = std::fs::File::create(&paths.daemon_log_path)?;
    let stderr = stdout.try_clone()?;
    let mut command = std::process::Command::new(host_binary);
    command
        .arg("daemon")
        .env("SAGENS_ENDPOINT", &user_config.endpoint)
        .env(
            "SAGENS_BOOTSTRAP_ADMIN_UUID",
            user_config.admin_uuid.to_string(),
        )
        .env("SAGENS_BOOTSTRAP_ADMIN_TOKEN", &user_config.admin_token)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(stdout))
        .stderr(std::process::Stdio::from(stderr));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        unsafe {
            // Detach the daemon from the foreground job so the parent CLI can exit cleanly.
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
    }
    command.spawn().map(|_| ())
}

pub async fn print_log(paths: &SagensPaths, tail: Option<usize>, follow: bool) -> Result<()> {
    let path = &paths.daemon_log_path;
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "==> {} <==", path.display())
        .map_err(|error| SandboxError::io("writing daemon log header", error))?;

    let initial = if follow {
        read_log_or_wait(path).await?
    } else {
        std::fs::read(path).map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => SandboxError::not_found(format!(
                "daemon log not found at {}; run `sagens start` first",
                path.display()
            )),
            _ => SandboxError::io("reading daemon log", error),
        })?
    };
    let initial_text = render_log_bytes(&initial, tail);
    if !initial_text.is_empty() {
        write!(stdout, "{initial_text}")
            .map_err(|error| SandboxError::io("writing daemon log output", error))?;
        if !initial_text.ends_with('\n') {
            writeln!(stdout)
                .map_err(|error| SandboxError::io("terminating daemon log output", error))?;
        }
    }
    stdout
        .flush()
        .map_err(|error| SandboxError::io("flushing daemon log output", error))?;

    if !follow {
        return Ok(());
    }

    let mut offset = initial.len() as u64;
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        match std::fs::metadata(path) {
            Ok(metadata) => {
                if metadata.len() < offset {
                    writeln!(stdout, "\n==> {} (restarted) <==", path.display()).map_err(
                        |error| SandboxError::io("writing daemon log restart header", error),
                    )?;
                    offset = 0;
                }
                if metadata.len() == offset {
                    continue;
                }
                let mut file = std::fs::File::open(path)
                    .map_err(|error| SandboxError::io("opening daemon log for follow", error))?;
                file.seek(SeekFrom::Start(offset))
                    .map_err(|error| SandboxError::io("seeking daemon log", error))?;
                let mut chunk = Vec::new();
                std::io::Read::read_to_end(&mut file, &mut chunk)
                    .map_err(|error| SandboxError::io("reading daemon log follow chunk", error))?;
                offset += chunk.len() as u64;
                if !chunk.is_empty() {
                    write!(stdout, "{}", String::from_utf8_lossy(&chunk)).map_err(|error| {
                        SandboxError::io("writing daemon log follow chunk", error)
                    })?;
                    stdout.flush().map_err(|error| {
                        SandboxError::io("flushing daemon log follow chunk", error)
                    })?;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(SandboxError::io("reading daemon log metadata", error)),
        }
    }
}

pub async fn quit(paths: &SagensPaths) -> Result<bool> {
    let config = match read_user_config(&paths.user_config_path).await {
        Ok(config) => config,
        Err(crate::SandboxError::Io { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            cleanup_pid_file(paths).await?;
            return Ok(false);
        }
        Err(error) => return Err(error),
    };
    let client = match healthy_client(&config).await {
        Ok(client) => client,
        Err(_) => {
            return terminate_recorded_daemon(paths).await;
        }
    };
    if client.shutdown_daemon().await.is_err() {
        return terminate_recorded_daemon(paths).await;
    }
    wait_for_daemon_shutdown(&config).await?;
    cleanup_pid_file(paths).await?;
    Ok(true)
}

async fn ensure_user_config(paths: &SagensPaths) -> Result<UserConfig> {
    if tokio::fs::try_exists(&paths.user_config_path)
        .await
        .map_err(|error| SandboxError::io("checking sagens user config", error))?
    {
        return read_user_config(&paths.user_config_path).await;
    }
    let config = UserConfig::new(paths.endpoint.clone());
    write_user_config(&paths.user_config_path, &config).await?;
    Ok(config)
}

async fn wait_for_daemon(config: &UserConfig, daemon_log_path: &Path) -> Result<()> {
    let deadline = std::time::Instant::now() + DAEMON_WAIT_TIMEOUT;
    loop {
        match healthy_client(config).await {
            Ok(_) => return Ok(()),
            Err(error) if std::time::Instant::now() >= deadline => {
                return Err(SandboxError::timeout(format!(
                    "timed out waiting for daemon at {}: {error}; inspect {} or run `sagens daemon log --tail 200`",
                    config.endpoint,
                    daemon_log_path.display()
                )));
            }
            Err(_) => {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }
}

async fn wait_for_daemon_shutdown(config: &UserConfig) -> Result<()> {
    let deadline = std::time::Instant::now() + DAEMON_WAIT_TIMEOUT;
    loop {
        match crate::BoxApiClient::connect(config).await {
            Ok(_) if std::time::Instant::now() >= deadline => {
                return Err(SandboxError::timeout(format!(
                    "timed out waiting for daemon to stop at {}",
                    config.endpoint
                )));
            }
            Ok(_) => {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            Err(_) => return Ok(()),
        }
    }
}

async fn healthy_client(config: &UserConfig) -> Result<crate::BoxApiClient> {
    let client = crate::BoxApiClient::connect(config).await?;
    let _ = client.list_boxes().await?;
    Ok(client)
}

async fn daemon_is_healthy(config: &UserConfig) -> bool {
    healthy_client(config).await.is_ok()
}

async fn bootstrap_admin_if_needed(admin_store: &AdminStore) -> Result<()> {
    let admin_uuid = std::env::var("SAGENS_BOOTSTRAP_ADMIN_UUID").ok();
    let admin_token = std::env::var("SAGENS_BOOTSTRAP_ADMIN_TOKEN").ok();
    let (Some(admin_uuid), Some(admin_token)) = (admin_uuid, admin_token) else {
        return Ok(());
    };
    let admin_uuid = uuid::Uuid::parse_str(&admin_uuid)
        .map_err(|error| SandboxError::invalid(format!("invalid bootstrap admin uuid: {error}")))?;
    let credential = AdminCredential {
        admin_uuid,
        admin_token,
    };
    bootstrap_admin(admin_store, &credential).await
}

pub(crate) async fn bootstrap_admin(
    admin_store: &AdminStore,
    credential: &AdminCredential,
) -> Result<()> {
    let _ = admin_store.bootstrap(credential).await?;
    Ok(())
}

async fn write_pid_file(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| SandboxError::io("creating daemon pid directory", error))?;
    }
    tokio::fs::write(path, format!("{}\n", std::process::id()))
        .await
        .map_err(|error| SandboxError::io("writing daemon pid file", error))
}

pub(super) async fn cleanup_pid_file(paths: &SagensPaths) -> Result<()> {
    match tokio::fs::remove_file(&paths.pid_path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(SandboxError::io("removing daemon pid file", error)),
    }
}

async fn read_log_or_wait(path: &Path) -> Result<Vec<u8>> {
    loop {
        match std::fs::read(path) {
            Ok(bytes) => return Ok(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
            Err(error) => return Err(SandboxError::io("reading daemon log", error)),
        }
    }
}

fn render_log_bytes(bytes: &[u8], tail: Option<usize>) -> String {
    let text = String::from_utf8_lossy(bytes).into_owned();
    match tail {
        Some(lines) => {
            let mut buffer = std::collections::VecDeque::with_capacity(lines);
            for line in text.lines() {
                if buffer.len() == lines {
                    buffer.pop_front();
                }
                buffer.push_back(line.to_string());
            }
            let mut rendered = buffer.into_iter().collect::<Vec<_>>().join("\n");
            if !rendered.is_empty() {
                rendered.push('\n');
            }
            rendered
        }
        None => text,
    }
}
