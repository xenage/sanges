mod checkpoints;
mod records;
mod state;

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use sagens_host::backend::ShellSession;
use sagens_host::boxes::{BoxManager, BoxRecord, BoxSettingValue, BoxStatus};
use sagens_host::protocol::{
    CommandStream, ExecExit, ExecRequest, ExecutionEvent, OutputStream, ShellEvent, ShellRequest,
};
use sagens_host::workspace::{
    CheckpointRestoreMode, FileKind, FileNode, ReadFileResult, WorkspaceCheckpointRecord,
    WorkspaceCheckpointSummary,
};
use tokio::sync::mpsc;
use uuid::Uuid;

use super::spawn::StubShellDriver;
use checkpoints::{
    apply_snapshot, capture_snapshot, checkpoint_exists, remove_box_state, rollback_checkpoints,
    workspace_changes,
};
use records::new_box_record;
use state::default_box_settings;

pub(crate) use state::StubBoxManager;

#[async_trait]
impl BoxManager for StubBoxManager {
    async fn list_boxes(&self) -> sagens_host::Result<Vec<BoxRecord>> {
        Ok(self.state.lock().await.boxes.values().cloned().collect())
    }

    #[rustfmt::skip]
    async fn get_box(&self, box_id: Uuid) -> sagens_host::Result<BoxRecord> { Ok(self.state.lock().await.boxes.get(&box_id).cloned().expect("box")) }

    async fn create_box(&self) -> sagens_host::Result<BoxRecord> {
        self.create_named_box(None).await
    }

    async fn create_named_box(&self, name: Option<String>) -> sagens_host::Result<BoxRecord> {
        let mut record = new_box_record(name);
        record.settings = Some(default_box_settings());
        self.state
            .lock()
            .await
            .boxes
            .insert(record.box_id, record.clone());
        Ok(record)
    }

    async fn set_box_setting(
        &self,
        box_id: Uuid,
        setting: BoxSettingValue,
    ) -> sagens_host::Result<BoxRecord> {
        let mut state = self.state.lock().await;
        let record = state.boxes.get_mut(&box_id).expect("box");
        let settings = record.settings.get_or_insert_with(default_box_settings);
        match setting {
            BoxSettingValue::CpuCores { value } => settings.cpu_cores.current = value,
            BoxSettingValue::MemoryMb { value } => settings.memory_mb.current = value,
            BoxSettingValue::FsSizeMib { value } => settings.fs_size_mib.current = value,
            BoxSettingValue::MaxProcesses { value } => settings.max_processes.current = value,
            BoxSettingValue::NetworkEnabled { value } => settings.network_enabled.current = value,
        }
        Ok(record.clone())
    }

    async fn start_box(&self, box_id: Uuid) -> sagens_host::Result<BoxRecord> {
        let mut state = self.state.lock().await;
        let record = state.boxes.get_mut(&box_id).expect("box");
        record.status = BoxStatus::Running;
        record.last_start_at_ms = Some(2);
        Ok(record.clone())
    }

    async fn stop_box(&self, box_id: Uuid) -> sagens_host::Result<BoxRecord> {
        let mut state = self.state.lock().await;
        let record = state.boxes.get_mut(&box_id).expect("box");
        record.status = BoxStatus::Stopped;
        record.last_stop_at_ms = Some(3);
        Ok(record.clone())
    }

    async fn remove_box(&self, box_id: Uuid) -> sagens_host::Result<()> {
        let mut state = self.state.lock().await;
        remove_box_state(&mut state, box_id);
        Ok(())
    }

    async fn list_files(&self, box_id: Uuid, _: &str) -> sagens_host::Result<Vec<FileNode>> {
        Ok(self
            .state
            .lock()
            .await
            .files
            .get(&box_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn read_file(
        &self,
        box_id: Uuid,
        path: &str,
        _: usize,
    ) -> sagens_host::Result<ReadFileResult> {
        Ok(ReadFileResult {
            path: path.into(),
            data: self
                .state
                .lock()
                .await
                .file_data
                .get(&(box_id, path.into()))
                .cloned()
                .unwrap_or_default(),
            truncated: false,
        })
    }

    async fn write_file(
        &self,
        box_id: Uuid,
        path: &str,
        data: &[u8],
        _: bool,
    ) -> sagens_host::Result<()> {
        let mut state = self.state.lock().await;
        state.file_data.insert((box_id, path.into()), data.to_vec());
        state.files.insert(
            box_id,
            vec![FileNode {
                path: path.trim_start_matches("/workspace/").into(),
                kind: FileKind::File,
                size: data.len() as u64,
                digest: Some("digest".into()),
                target: None,
            }],
        );
        Ok(())
    }

    async fn make_dir(&self, _: Uuid, _: &str, _: bool) -> sagens_host::Result<()> {
        Ok(())
    }

    async fn remove_path(&self, _: Uuid, _: &str, _: bool) -> sagens_host::Result<()> {
        Ok(())
    }

    async fn exec(&self, box_id: Uuid, request: ExecRequest) -> sagens_host::Result<CommandStream> {
        let mut running = self.exec_running.lock().await;
        if !running.insert(box_id) {
            return Err(sagens_host::SandboxError::conflict(
                "parallel exec is not supported for a running BOX",
            ));
        }
        drop(running);
        let mut state = self.state.lock().await;
        state.files.insert(
            box_id,
            vec![FileNode {
                path: "tracked.txt".into(),
                kind: FileKind::File,
                size: 5,
                digest: Some("digest".into()),
                target: None,
            }],
        );
        state
            .file_data
            .insert((box_id, "/workspace/tracked.txt".into()), b"hello".to_vec());
        drop(state);

        let command = request.args.last().cloned().unwrap_or_default();
        let (tx, rx) = mpsc::channel(4);
        let exec_running = self.exec_running.clone();
        tokio::spawn(async move {
            let output = if command.contains("sleep") {
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                format!("slow:{box_id}\n")
            } else {
                format!("exec:{box_id}\n")
            };
            let _ = tx
                .send(ExecutionEvent::Output {
                    stream: OutputStream::Stdout,
                    data: output.into_bytes(),
                })
                .await;
            let status = if command.contains("infinite") {
                ExecExit::Timeout
            } else if command.contains("ignore-term") {
                ExecExit::Killed
            } else {
                ExecExit::Success
            };
            let _ = tx.send(ExecutionEvent::Exit { status }).await;
            exec_running.lock().await.remove(&box_id);
        });
        Ok(CommandStream::new(rx))
    }

    async fn open_shell(&self, _: Uuid, _: ShellRequest) -> sagens_host::Result<ShellSession> {
        let session_id = Uuid::new_v4();
        let (tx, rx) = mpsc::channel(8);
        Ok(ShellSession::new(
            session_id,
            rx,
            Arc::new(StubShellDriver { sender: tx }),
        ))
    }

    async fn checkpoint_create(
        &self,
        box_id: Uuid,
        name: Option<String>,
        metadata: BTreeMap<String, String>,
    ) -> sagens_host::Result<WorkspaceCheckpointRecord> {
        let mut state = self.state.lock().await;
        state.committed += 1;
        let checkpoint_id = format!("checkpoint-{}", state.committed);
        let source_checkpoint_id = state.checkpoint_heads.get(&box_id).cloned();
        let snapshot = capture_snapshot(&state, box_id);
        let checkpoint = WorkspaceCheckpointRecord {
            summary: WorkspaceCheckpointSummary {
                checkpoint_id: checkpoint_id.clone(),
                workspace_id: box_id.to_string(),
                name,
                metadata,
                created_at_ms: 10 + state.committed,
            },
            source_checkpoint_id,
            changes: workspace_changes(&snapshot),
        };
        state
            .checkpoints
            .entry(box_id)
            .or_default()
            .push(checkpoint.clone());
        state
            .checkpoint_snapshots
            .insert((box_id, checkpoint_id.clone()), snapshot);
        state.checkpoint_heads.insert(box_id, checkpoint_id);
        Ok(checkpoint)
    }

    async fn checkpoint_list(
        &self,
        box_id: Uuid,
    ) -> sagens_host::Result<Vec<WorkspaceCheckpointRecord>> {
        Ok(self
            .state
            .lock()
            .await
            .checkpoints
            .get(&box_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn checkpoint_restore(
        &self,
        box_id: Uuid,
        checkpoint_id: &str,
        mode: CheckpointRestoreMode,
    ) -> sagens_host::Result<WorkspaceCheckpointRecord> {
        let mut state = self.state.lock().await;
        let checkpoint = state
            .checkpoints
            .get(&box_id)
            .and_then(|items| {
                items
                    .iter()
                    .find(|item| item.summary.checkpoint_id == checkpoint_id)
                    .cloned()
            })
            .ok_or_else(|| {
                sagens_host::SandboxError::not_found(format!("unknown checkpoint {checkpoint_id}"))
            })?;
        if mode == CheckpointRestoreMode::Rollback {
            rollback_checkpoints(&mut state, box_id, checkpoint_id);
        }
        let snapshot = state
            .checkpoint_snapshots
            .get(&(box_id, checkpoint_id.to_string()))
            .cloned()
            .unwrap_or_default();
        apply_snapshot(&mut state, box_id, &snapshot);
        state
            .checkpoint_heads
            .insert(box_id, checkpoint_id.to_string());
        Ok(checkpoint)
    }

    async fn checkpoint_fork(
        &self,
        box_id: Uuid,
        checkpoint_id: &str,
        new_box_name: Option<String>,
    ) -> sagens_host::Result<BoxRecord> {
        {
            let state = self.state.lock().await;
            let exists = checkpoint_exists(&state, box_id, checkpoint_id);
            if !exists {
                return Err(sagens_host::SandboxError::not_found(format!(
                    "unknown checkpoint {checkpoint_id}"
                )));
            }
        }
        let record = self.create_named_box(new_box_name).await?;
        let mut state = self.state.lock().await;
        let snapshot = state
            .checkpoint_snapshots
            .get(&(box_id, checkpoint_id.to_string()))
            .cloned()
            .unwrap_or_default();
        apply_snapshot(&mut state, record.box_id, &snapshot);
        Ok(record)
    }

    async fn checkpoint_delete(
        &self,
        box_id: Uuid,
        checkpoint_id: &str,
    ) -> sagens_host::Result<()> {
        let mut state = self.state.lock().await;
        let removed = state.checkpoints.get(&box_id).and_then(|items| {
            items
                .iter()
                .find(|item| item.summary.checkpoint_id == checkpoint_id)
                .cloned()
        });
        if let Some(items) = state.checkpoints.get_mut(&box_id) {
            items.retain(|item| item.summary.checkpoint_id != checkpoint_id);
        }
        state
            .checkpoint_snapshots
            .remove(&(box_id, checkpoint_id.to_string()));
        if state
            .checkpoint_heads
            .get(&box_id)
            .is_some_and(|head| head == checkpoint_id)
        {
            match removed.and_then(|item| item.source_checkpoint_id) {
                Some(parent) => {
                    state.checkpoint_heads.insert(box_id, parent);
                }
                None => {
                    state.checkpoint_heads.remove(&box_id);
                }
            }
        }
        Ok(())
    }

    async fn shutdown_daemon(&self) -> sagens_host::Result<()> {
        let mut state = self.state.lock().await;
        for record in state.boxes.values_mut() {
            if record.status == BoxStatus::Running {
                record.status = BoxStatus::Stopped;
                record.last_stop_at_ms = Some(5);
            }
        }
        Ok(())
    }
}
