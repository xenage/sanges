use std::path::PathBuf;

use tokio::fs;
use uuid::Uuid;

use crate::{Result, SandboxError};

use super::BoxRecord;

#[derive(Debug, Clone)]
pub struct BoxStore {
    state_dir: PathBuf,
}

impl BoxStore {
    pub fn new(state_dir: impl Into<PathBuf>) -> Self {
        Self {
            state_dir: state_dir.into(),
        }
    }

    pub async fn ensure_layout(&self) -> Result<()> {
        fs::create_dir_all(self.boxes_dir())
            .await
            .map_err(|error| SandboxError::io("creating BOX registry directory", error))
    }

    pub async fn list(&self) -> Result<Vec<BoxRecord>> {
        let mut records = Vec::new();
        let mut entries = fs::read_dir(self.boxes_dir())
            .await
            .map_err(|error| SandboxError::io("reading BOX registry directory", error))?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|error| SandboxError::io("iterating BOX registry directory", error))?
        {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            if path
                .file_stem()
                .and_then(|value| value.to_str())
                .and_then(|value| Uuid::parse_str(value).ok())
                .is_none()
            {
                continue;
            }
            records.push(self.read_path(path).await?);
        }
        records.sort_by_key(|record| (record.created_at_ms, record.box_id));
        Ok(records)
    }

    pub async fn read(&self, box_id: Uuid) -> Result<BoxRecord> {
        let path = self.box_path(box_id);
        if !fs::try_exists(&path)
            .await
            .map_err(|error| SandboxError::io("checking BOX registry entry", error))?
        {
            return Err(SandboxError::not_found(format!("unknown BOX {box_id}")));
        }
        self.read_path(path).await
    }

    pub async fn write(&self, record: &BoxRecord) -> Result<()> {
        self.ensure_layout().await?;
        let path = self.box_path(record.box_id);
        let temp = path.with_extension("json.tmp");
        let mut persisted = record.clone();
        persisted.runtime_usage = None;
        let payload = serde_json::to_vec_pretty(&persisted)
            .map_err(|error| SandboxError::json("encoding BOX registry entry", error))?;
        fs::write(&temp, payload)
            .await
            .map_err(|error| SandboxError::io("writing BOX registry entry", error))?;
        fs::rename(&temp, &path)
            .await
            .map_err(|error| SandboxError::io("replacing BOX registry entry", error))
    }

    pub async fn remove(&self, box_id: Uuid) -> Result<()> {
        let path = self.box_path(box_id);
        if !fs::try_exists(&path)
            .await
            .map_err(|error| SandboxError::io("checking BOX registry entry", error))?
        {
            return Err(SandboxError::not_found(format!("unknown BOX {box_id}")));
        }
        fs::remove_file(path)
            .await
            .map_err(|error| SandboxError::io("removing BOX registry entry", error))
    }

    async fn read_path(&self, path: PathBuf) -> Result<BoxRecord> {
        let bytes = fs::read(path)
            .await
            .map_err(|error| SandboxError::io("reading BOX registry entry", error))?;
        serde_json::from_slice(&bytes)
            .map_err(|error| SandboxError::json("decoding BOX registry entry", error))
    }

    fn boxes_dir(&self) -> PathBuf {
        self.state_dir.join("boxes")
    }

    fn box_path(&self, box_id: Uuid) -> PathBuf {
        self.boxes_dir().join(format!("{box_id}.json"))
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    use uuid::Uuid;

    use super::{BoxRecord, BoxStore};
    use crate::boxes::{BoxRuntimeUsage, BoxStatus};

    #[tokio::test]
    async fn writes_and_reads_box_records() {
        let temp = tempdir().expect("tempdir");
        let store = BoxStore::new(temp.path());
        let record = BoxRecord {
            box_id: Uuid::new_v4(),
            name: None,
            status: BoxStatus::Created,
            settings: None,
            runtime_usage: None,
            workspace_path: temp.path().join("workspace.raw"),
            active_sandbox_id: None,
            created_at_ms: 1,
            last_start_at_ms: None,
            last_stop_at_ms: None,
            last_error: None,
        };
        store.write(&record).await.expect("write");
        let restored = store.read(record.box_id).await.expect("read");
        assert_eq!(restored.box_id, record.box_id);
        assert_eq!(restored.status, BoxStatus::Created);
    }

    #[tokio::test]
    async fn runtime_usage_is_not_persisted() {
        let temp = tempdir().expect("tempdir");
        let store = BoxStore::new(temp.path());
        let record = BoxRecord {
            box_id: Uuid::new_v4(),
            name: None,
            status: BoxStatus::Running,
            settings: None,
            runtime_usage: Some(BoxRuntimeUsage {
                cpu_millicores: 250,
                memory_used_mib: 128,
                fs_used_mib: 64,
                process_count: 4,
            }),
            workspace_path: temp.path().join("workspace.raw"),
            active_sandbox_id: None,
            created_at_ms: 1,
            last_start_at_ms: None,
            last_stop_at_ms: None,
            last_error: None,
        };

        store.write(&record).await.expect("write");
        let restored = store.read(record.box_id).await.expect("read");

        assert_eq!(restored.runtime_usage, None);
    }

    #[tokio::test]
    async fn list_ignores_non_box_json_files() {
        let temp = tempdir().expect("tempdir");
        let store = BoxStore::new(temp.path());
        let record = BoxRecord {
            box_id: Uuid::new_v4(),
            name: None,
            status: BoxStatus::Created,
            settings: None,
            runtime_usage: None,
            workspace_path: temp.path().join("workspace.raw"),
            active_sandbox_id: None,
            created_at_ms: 1,
            last_start_at_ms: None,
            last_stop_at_ms: None,
            last_error: None,
        };
        store.write(&record).await.expect("write");
        tokio::fs::write(
            temp.path().join("boxes").join("credentials.json"),
            br#"{"version":1,"boxes":[]}"#,
        )
        .await
        .expect("write credentials registry");

        let listed = store.list().await.expect("list boxes");

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].box_id, record.box_id);
    }
}
