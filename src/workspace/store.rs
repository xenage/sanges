use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use ext4_lwext4::{FileBlockDevice, MkfsOptions, mkfs};
use tokio::fs;
use uuid::Uuid;

use crate::config::WorkspaceConfig;
use crate::workspace::validate_persisted_id;
use crate::{Result, SandboxError};

use super::{LocalLineageStore, WorkspaceLineageStore};

#[derive(Debug, Clone)]
pub struct WorkspaceLease {
    pub workspace_id: String,
    pub disk_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct RunLayout {
    pub sandbox_id: Uuid,
    pub root_dir: PathBuf,
    pub runtime_dir: PathBuf,
    pub vsock_socket: PathBuf,
    pub runner_config: PathBuf,
    pub runner_log: PathBuf,
    pub guest_console_log: PathBuf,
}

#[derive(Debug, Clone)]
pub struct WorkspaceStore {
    pub(super) state_dir: PathBuf,
    config: WorkspaceConfig,
    pub(super) lineage: LocalLineageStore,
}

impl WorkspaceStore {
    pub fn new(state_dir: impl Into<PathBuf>, config: WorkspaceConfig) -> Self {
        let state_dir = state_dir.into();
        Self {
            lineage: LocalLineageStore::new(state_dir.join("checkpoints")),
            state_dir,
            config,
        }
    }

    pub async fn ensure_layout(&self) -> Result<()> {
        for dir in [
            self.workspaces_dir(),
            self.runs_dir(),
            self.checkpoints_dir(),
        ] {
            create_private_dir(&dir).await?;
        }
        Ok(())
    }

    pub async fn prepare_workspace(&self, workspace_id: &str) -> Result<WorkspaceLease> {
        validate_persisted_id(workspace_id, "workspace_id")?;
        let dir = self.workspace_dir(workspace_id);
        let disk_path = dir.join("workspace.raw");
        create_private_dir(&dir).await?;
        self.lineage.ensure_workspace(workspace_id).await?;
        if !fs::try_exists(&disk_path)
            .await
            .map_err(|error| SandboxError::io("checking workspace disk", error))?
        {
            let file = fs::File::create(&disk_path)
                .await
                .map_err(|error| SandboxError::io("creating workspace disk", error))?;
            file.set_len(self.config.disk_size_mib * 1024 * 1024)
                .await
                .map_err(|error| SandboxError::io("sizing workspace disk", error))?;
            self.format_ext4(&disk_path)?;
        }
        Ok(WorkspaceLease {
            workspace_id: workspace_id.into(),
            disk_path,
        })
    }

    pub async fn workspace_disk_size_mib(&self, workspace_id: &str) -> Result<u64> {
        validate_persisted_id(workspace_id, "workspace_id")?;
        let disk_path = self.workspace_dir(workspace_id).join("workspace.raw");
        let metadata = fs::metadata(&disk_path)
            .await
            .map_err(|error| SandboxError::io("reading workspace disk metadata", error))?;
        Ok(metadata.len() / (1024 * 1024))
    }

    pub async fn resize_workspace(&self, workspace_id: &str, new_size_mib: u64) -> Result<()> {
        validate_persisted_id(workspace_id, "workspace_id")?;
        if new_size_mib < 64 {
            return Err(SandboxError::invalid(
                "workspace disk must be at least 64 MiB",
            ));
        }
        let lease = self.prepare_workspace(workspace_id).await?;
        let current_size_mib = self.workspace_disk_size_mib(workspace_id).await?;
        if current_size_mib == new_size_mib {
            return Ok(());
        }

        self.check_ext4(&lease.disk_path)?;
        if new_size_mib > current_size_mib {
            resize_file_len(&lease.disk_path, new_size_mib)?;
            self.resize_ext4(&lease.disk_path, new_size_mib)?;
        } else {
            self.resize_ext4(&lease.disk_path, new_size_mib)?;
            resize_file_len(&lease.disk_path, new_size_mib)?;
            self.check_ext4(&lease.disk_path)?;
        }
        Ok(())
    }

    pub async fn remove_workspace(&self, workspace_id: &str) -> Result<()> {
        validate_persisted_id(workspace_id, "workspace_id")?;
        for dir in [
            self.workspace_dir(workspace_id),
            self.workspace_checkpoints_dir(workspace_id),
        ] {
            if fs::try_exists(&dir)
                .await
                .map_err(|error| SandboxError::io("checking workspace state path", error))?
            {
                fs::remove_dir_all(&dir)
                    .await
                    .map_err(|error| SandboxError::io("removing workspace state path", error))?;
            }
        }
        Ok(())
    }

    pub async fn prepare_run(&self) -> Result<RunLayout> {
        let sandbox_id = Uuid::new_v4();
        let root_dir = self.runs_dir().join(sandbox_id.to_string());
        let runtime_dir = root_dir.join("runtime");
        create_private_dir(&root_dir).await?;
        create_private_dir(&runtime_dir).await?;
        let vsock_socket = PathBuf::from(format!("/tmp/asb-{}.sock", sandbox_id.simple()));
        Ok(RunLayout {
            sandbox_id,
            runtime_dir,
            vsock_socket,
            runner_config: root_dir.join("libkrun-runner.json"),
            runner_log: root_dir.join("libkrun-runner.log"),
            guest_console_log: root_dir.join("guest-console.log"),
            root_dir,
        })
    }

    pub async fn recycle_run(&self, run: RunLayout) -> Result<RunLayout> {
        let sandbox_id = Uuid::new_v4();
        let root_dir = self.runs_dir().join(sandbox_id.to_string());
        let runtime_dir = root_dir.join("runtime");
        let vsock_socket = PathBuf::from(format!("/tmp/asb-{}.sock", sandbox_id.simple()));
        if fs::try_exists(&run.root_dir)
            .await
            .map_err(|error| SandboxError::io("checking recycled run root", error))?
        {
            fs::rename(&run.root_dir, &root_dir)
                .await
                .map_err(|error| SandboxError::io("recycling run root", error))?;
        } else {
            create_private_dir(&root_dir).await?;
        }
        create_private_dir(&runtime_dir).await?;
        if run.vsock_socket != vsock_socket
            && fs::try_exists(&run.vsock_socket)
                .await
                .map_err(|error| SandboxError::io("checking recycled vsock socket path", error))?
        {
            fs::remove_file(&run.vsock_socket)
                .await
                .map_err(|error| SandboxError::io("removing recycled vsock socket path", error))?;
        }
        Ok(RunLayout {
            sandbox_id,
            runtime_dir,
            vsock_socket,
            runner_config: root_dir.join("libkrun-runner.json"),
            runner_log: root_dir.join("libkrun-runner.log"),
            guest_console_log: root_dir.join("guest-console.log"),
            root_dir,
        })
    }

    pub async fn destroy_run(&self, run: &RunLayout) -> Result<()> {
        if fs::try_exists(&run.root_dir)
            .await
            .map_err(|error| SandboxError::io("checking run root", error))?
        {
            fs::remove_dir_all(&run.root_dir)
                .await
                .map_err(|error| SandboxError::io("removing run root", error))?;
        }
        if fs::try_exists(&run.vsock_socket)
            .await
            .map_err(|error| SandboxError::io("checking vsock socket path", error))?
        {
            fs::remove_file(&run.vsock_socket)
                .await
                .map_err(|error| SandboxError::io("removing vsock socket path", error))?;
        }
        Ok(())
    }

    fn workspaces_dir(&self) -> PathBuf {
        self.state_dir.join("workspaces")
    }

    fn runs_dir(&self) -> PathBuf {
        self.state_dir.join("runs")
    }

    fn workspace_dir(&self, workspace_id: &str) -> PathBuf {
        self.workspaces_dir().join(workspace_id)
    }

    pub(super) fn checkpoints_dir(&self) -> PathBuf {
        self.state_dir.join("checkpoints")
    }

    pub(super) fn workspace_checkpoints_dir(&self, workspace_id: &str) -> PathBuf {
        self.checkpoints_dir().join(workspace_id)
    }

    fn format_ext4(&self, disk_path: &Path) -> Result<()> {
        let device = FileBlockDevice::open(disk_path).map_err(|error| {
            SandboxError::backend(format!("opening ext4 workspace image: {error}"))
        })?;
        let options = MkfsOptions::ext4()
            .with_block_size(4096)
            .with_label("workspace");
        mkfs(device, &options).map_err(|error| {
            SandboxError::backend(format!("formatting ext4 workspace image: {error}"))
        })
    }

    fn resize_ext4(&self, disk_path: &Path, size_mib: u64) -> Result<()> {
        for (program, args) in resize_ext4_commands(disk_path, size_mib) {
            match Command::new(&program).args(&args).status() {
                Ok(status) if status.success() => return Ok(()),
                Ok(_) => continue,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(SandboxError::io(format!("spawning {program}"), error)),
            }
        }
        Err(SandboxError::UnsupportedHost(
            "ext4 resize tooling not found; install resize2fs".into(),
        ))
    }

    fn check_ext4(&self, disk_path: &Path) -> Result<()> {
        for (program, args) in ext4_check_commands(disk_path) {
            match Command::new(&program).args(&args).status() {
                Ok(status) if status.success() || status.code() == Some(1) => return Ok(()),
                Ok(_) => continue,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(SandboxError::io(format!("spawning {program}"), error)),
            }
        }
        Err(SandboxError::UnsupportedHost(
            "ext4 consistency tooling not found; install e2fsck".into(),
        ))
    }
}

fn resize_file_len(disk_path: &Path, size_mib: u64) -> Result<()> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(disk_path)
        .map_err(|error| SandboxError::io("opening workspace disk for resize", error))?;
    file.set_len(size_mib * 1024 * 1024)
        .map_err(|error| SandboxError::io("resizing workspace disk file", error))
}

async fn create_private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)
        .await
        .map_err(|error| SandboxError::io("creating runtime state directory", error))?;
    set_private_permissions(path).await
}

async fn set_private_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .await
            .map_err(|error| SandboxError::io("setting runtime directory permissions", error))?;
    }
    Ok(())
}

fn resize_ext4_commands(disk_path: &Path, size_mib: u64) -> Vec<(String, Vec<String>)> {
    let disk = disk_path.display().to_string();
    let size = format!("{size_mib}M");
    vec![
        ("resize2fs".into(), vec![disk.clone(), size.clone()]),
        (
            "/opt/homebrew/opt/e2fsprogs/sbin/resize2fs".into(),
            vec![disk, size],
        ),
    ]
}

fn ext4_check_commands(disk_path: &Path) -> Vec<(String, Vec<String>)> {
    let disk = disk_path.display().to_string();
    vec![
        (
            "e2fsck".into(),
            vec!["-f".into(), "-y".into(), disk.clone()],
        ),
        (
            "/opt/homebrew/opt/e2fsprogs/sbin/e2fsck".into(),
            vec!["-f".into(), "-y".into(), disk],
        ),
    ]
}

pub(super) fn now_ms() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_millis() as u64,
        Err(_) => 0,
    }
}
