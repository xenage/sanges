use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{Result, SandboxError};

const USER_CONFIG_VERSION: u32 = 1;
const ADMIN_REGISTRY_VERSION: u32 = 1;
const BOX_CREDENTIAL_REGISTRY_VERSION: u32 = 1;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserConfig {
    pub version: u32,
    pub admin_uuid: Uuid,
    pub admin_token: String,
    pub endpoint: String,
}

impl UserConfig {
    pub fn new(endpoint: String) -> Self {
        Self {
            version: USER_CONFIG_VERSION,
            admin_uuid: Uuid::new_v4(),
            admin_token: generate_token(),
            endpoint,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdminCredential {
    pub admin_uuid: Uuid,
    pub admin_token: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdminRegistry {
    pub version: u32,
    pub admins: Vec<AdminRecord>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdminRecord {
    pub admin_uuid: Uuid,
    pub token_hash: String,
    pub active: bool,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdminCredentialBundle {
    pub admin_uuid: Uuid,
    pub admin_token: String,
    pub endpoint: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BoxCredentialBundle {
    pub box_id: Uuid,
    pub box_token: String,
    pub endpoint: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BoxCredentialRegistry {
    pub version: u32,
    pub boxes: Vec<BoxCredentialRecord>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BoxCredentialRecord {
    pub box_id: Uuid,
    pub token_hash: String,
    pub active: bool,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug)]
pub struct AdminStore {
    path: PathBuf,
    file_lock: Mutex<()>,
}

#[derive(Debug)]
pub struct BoxCredentialStore {
    path: PathBuf,
    file_lock: Mutex<()>,
}

impl AdminStore {
    pub fn new(state_dir: &Path) -> Self {
        Self {
            path: state_dir.join("admins").join("registry.json"),
            file_lock: Mutex::new(()),
        }
    }

    pub fn exists_path(&self) -> &Path {
        &self.path
    }

    pub async fn bootstrap(&self, credential: &AdminCredential) -> Result<bool> {
        let _guard = self.file_lock.lock().await;
        let mut registry = self.read_registry().await?;
        if registry.admins.iter().any(|admin| admin.active) {
            return Ok(false);
        }
        registry.admins.push(new_record(credential));
        self.write_registry(&registry).await?;
        Ok(true)
    }

    pub async fn authenticate(&self, admin_uuid: Uuid, admin_token: &str) -> Result<bool> {
        let _guard = self.file_lock.lock().await;
        let mut registry = self.read_registry().await?;
        let token_hash = hash_token(admin_token);
        let mut matched = false;
        for admin in &mut registry.admins {
            if admin.admin_uuid == admin_uuid && admin.active && admin.token_hash == token_hash {
                admin.updated_at_ms = now_ms();
                matched = true;
                break;
            }
        }
        if matched {
            self.write_registry(&registry).await?;
        }
        Ok(matched)
    }

    pub async fn add_admin(&self, endpoint: String) -> Result<AdminCredentialBundle> {
        let _guard = self.file_lock.lock().await;
        let mut registry = self.read_registry().await?;
        let credential = AdminCredential {
            admin_uuid: Uuid::new_v4(),
            admin_token: generate_token(),
        };
        registry.admins.push(new_record(&credential));
        self.write_registry(&registry).await?;
        Ok(AdminCredentialBundle {
            admin_uuid: credential.admin_uuid,
            admin_token: credential.admin_token,
            endpoint,
        })
    }

    pub async fn remove_admin(&self, admin_uuid: Uuid) -> Result<()> {
        let _guard = self.file_lock.lock().await;
        let mut registry = self.read_registry().await?;
        let active_count = registry.admins.iter().filter(|admin| admin.active).count();
        if active_count <= 1 {
            return Err(SandboxError::conflict(
                "refusing to remove the last active admin",
            ));
        }
        let mut removed = false;
        for admin in &mut registry.admins {
            if admin.admin_uuid == admin_uuid && admin.active {
                admin.active = false;
                admin.updated_at_ms = now_ms();
                removed = true;
                break;
            }
        }
        if !removed {
            return Err(SandboxError::not_found(format!(
                "unknown active admin {admin_uuid}"
            )));
        }
        self.write_registry(&registry).await
    }

    async fn read_registry(&self) -> Result<AdminRegistry> {
        if !tokio::fs::try_exists(&self.path)
            .await
            .map_err(|error| SandboxError::io("checking admin registry", error))?
        {
            return Ok(AdminRegistry {
                version: ADMIN_REGISTRY_VERSION,
                admins: Vec::new(),
            });
        }
        let bytes = tokio::fs::read(&self.path)
            .await
            .map_err(|error| SandboxError::io("reading admin registry", error))?;
        serde_json::from_slice(&bytes)
            .map_err(|error| SandboxError::json("decoding admin registry", error))
    }

    async fn write_registry(&self, registry: &AdminRegistry) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|error| SandboxError::io("creating admin registry directory", error))?;
        }
        let bytes = serde_json::to_vec_pretty(registry)
            .map_err(|error| SandboxError::json("encoding admin registry", error))?;
        tokio::fs::write(&self.path, bytes)
            .await
            .map_err(|error| SandboxError::io("writing admin registry", error))?;
        set_private_file_permissions(&self.path).await
    }
}

impl BoxCredentialStore {
    pub fn new(state_dir: &Path) -> Self {
        Self {
            path: state_dir.join("boxes").join("credentials.json"),
            file_lock: Mutex::new(()),
        }
    }

    pub async fn issue(&self, endpoint: String, box_id: Uuid) -> Result<BoxCredentialBundle> {
        let _guard = self.file_lock.lock().await;
        let mut registry = self.read_registry().await?;
        let bundle = BoxCredentialBundle {
            box_id,
            box_token: generate_token(),
            endpoint,
        };
        let now = now_ms();
        let mut replaced = false;
        for record in &mut registry.boxes {
            if record.box_id == box_id {
                record.token_hash = hash_token(&bundle.box_token);
                record.active = true;
                record.updated_at_ms = now;
                replaced = true;
                break;
            }
        }
        if !replaced {
            registry.boxes.push(BoxCredentialRecord {
                box_id,
                token_hash: hash_token(&bundle.box_token),
                active: true,
                created_at_ms: now,
                updated_at_ms: now,
            });
        }
        self.write_registry(&registry).await?;
        Ok(bundle)
    }

    pub async fn authenticate(&self, box_id: Uuid, box_token: &str) -> Result<bool> {
        let _guard = self.file_lock.lock().await;
        let mut registry = self.read_registry().await?;
        let token_hash = hash_token(box_token);
        let mut matched = false;
        for record in &mut registry.boxes {
            if record.box_id == box_id && record.active && record.token_hash == token_hash {
                record.updated_at_ms = now_ms();
                matched = true;
                break;
            }
        }
        if matched {
            self.write_registry(&registry).await?;
        }
        Ok(matched)
    }

    async fn read_registry(&self) -> Result<BoxCredentialRegistry> {
        if !tokio::fs::try_exists(&self.path)
            .await
            .map_err(|error| SandboxError::io("checking box credential registry", error))?
        {
            return Ok(BoxCredentialRegistry {
                version: BOX_CREDENTIAL_REGISTRY_VERSION,
                boxes: Vec::new(),
            });
        }
        let bytes = tokio::fs::read(&self.path)
            .await
            .map_err(|error| SandboxError::io("reading box credential registry", error))?;
        serde_json::from_slice(&bytes)
            .map_err(|error| SandboxError::json("decoding box credential registry", error))
    }

    async fn write_registry(&self, registry: &BoxCredentialRegistry) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|error| {
                SandboxError::io("creating box credential registry directory", error)
            })?;
        }
        let bytes = serde_json::to_vec_pretty(registry)
            .map_err(|error| SandboxError::json("encoding box credential registry", error))?;
        tokio::fs::write(&self.path, bytes)
            .await
            .map_err(|error| SandboxError::io("writing box credential registry", error))?;
        set_private_file_permissions(&self.path).await
    }
}

pub async fn read_user_config(path: &Path) -> Result<UserConfig> {
    validate_user_config_permissions(path).await?;
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|error| SandboxError::io("reading sagens user config", error))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| SandboxError::json("decoding sagens user config", error))
}

pub async fn write_user_config(path: &Path, config: &UserConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| SandboxError::io("creating sagens config directory", error))?;
    }
    let bytes = serde_json::to_vec_pretty(config)
        .map_err(|error| SandboxError::json("encoding sagens user config", error))?;
    tokio::fs::write(path, bytes)
        .await
        .map_err(|error| SandboxError::io("writing sagens user config", error))?;
    set_user_config_permissions(path).await
}

fn new_record(credential: &AdminCredential) -> AdminRecord {
    let now = now_ms();
    AdminRecord {
        admin_uuid: credential.admin_uuid,
        token_hash: hash_token(&credential.admin_token),
        active: true,
        created_at_ms: now,
        updated_at_ms: now,
    }
}

fn generate_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    let digest = hasher.finalize();
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

fn now_ms() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_millis() as u64,
        Err(_) => 0,
    }
}

async fn set_user_config_permissions(path: &Path) -> Result<()> {
    set_private_file_permissions(path).await
}

async fn set_private_file_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let permissions = std::fs::Permissions::from_mode(0o600);
        tokio::fs::set_permissions(path, permissions)
            .await
            .map_err(|error| SandboxError::io("setting sagens user config permissions", error))?;
    }
    Ok(())
}

async fn validate_user_config_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let metadata = tokio::fs::metadata(path)
            .await
            .map_err(|error| SandboxError::io("reading sagens user config metadata", error))?;
        let mode = metadata.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            return Err(SandboxError::invalid(format!(
                "sagens user config {} must have 0600 permissions",
                path.display()
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
