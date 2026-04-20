use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    File,
    Directory,
    Symlink,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileNode {
    pub path: String,
    pub kind: FileKind,
    pub size: u64,
    pub digest: Option<String>,
    pub target: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceSnapshot {
    entries: BTreeMap<String, FileNode>,
}

impl WorkspaceSnapshot {
    pub fn from_entries(entries: Vec<FileNode>) -> Self {
        Self {
            entries: entries
                .into_iter()
                .map(|entry| (entry.path.clone(), entry))
                .collect(),
        }
    }

    pub fn entries(&self) -> impl Iterator<Item = &FileNode> {
        self.entries.values()
    }

    pub fn diff(&self, current: &Self) -> Vec<WorkspaceChange> {
        let mut changes = Vec::new();
        let paths = self
            .entries
            .keys()
            .chain(current.entries.keys())
            .cloned()
            .collect::<BTreeSet<_>>();

        for path in paths {
            match (self.entries.get(&path), current.entries.get(&path)) {
                (None, Some(after)) => changes.push(WorkspaceChange {
                    path,
                    kind: WorkspaceChangeKind::Added,
                    kind_after: Some(after.kind.clone()),
                }),
                (Some(_), None) => changes.push(WorkspaceChange {
                    path,
                    kind: WorkspaceChangeKind::Deleted,
                    kind_after: None,
                }),
                (Some(before), Some(after)) => {
                    if before.kind != after.kind {
                        changes.push(WorkspaceChange {
                            path,
                            kind: WorkspaceChangeKind::TypeChanged,
                            kind_after: Some(after.kind.clone()),
                        });
                    } else if before != after {
                        changes.push(WorkspaceChange {
                            path,
                            kind: WorkspaceChangeKind::Modified,
                            kind_after: Some(after.kind.clone()),
                        });
                    }
                }
                (None, None) => {}
            }
        }

        changes
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceChangeKind {
    Added,
    Modified,
    Deleted,
    TypeChanged,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceChange {
    pub path: String,
    pub kind: WorkspaceChangeKind,
    pub kind_after: Option<FileKind>,
}

impl WorkspaceChange {
    pub fn git_label(&self) -> &'static str {
        match self.kind {
            WorkspaceChangeKind::Added => "A",
            WorkspaceChangeKind::Modified => "M",
            WorkspaceChangeKind::Deleted => "D",
            WorkspaceChangeKind::TypeChanged => "T",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReadFileResult {
    pub path: String,
    pub data: Vec<u8>,
    pub truncated: bool,
}

#[cfg(test)]
mod tests {
    use super::{FileKind, FileNode, WorkspaceChangeKind, WorkspaceSnapshot};

    #[test]
    fn diff_detects_modified_entries() {
        let before = WorkspaceSnapshot::from_entries(vec![FileNode {
            path: "tracked.txt".into(),
            kind: FileKind::File,
            size: 1,
            digest: Some("a".into()),
            target: None,
        }]);
        let after = WorkspaceSnapshot::from_entries(vec![FileNode {
            path: "tracked.txt".into(),
            kind: FileKind::File,
            size: 2,
            digest: Some("b".into()),
            target: None,
        }]);
        let diff = before.diff(&after);
        assert_eq!(diff.len(), 1);
        assert_eq!(diff[0].kind, WorkspaceChangeKind::Modified);
    }
}
