//! 宠物存档文件服务：安全读取、校验、损坏备份和原子写入。

use std::path::{Path, PathBuf};

use gesso_core::{PetSave, PetSaveError, PET_SAVE_VERSION};

pub struct PetStore {
    path: PathBuf,
}

impl PetStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> Result<PetSave, PetStoreError> {
        if !self.path.exists() {
            return Ok(PetSave::default());
        }
        let bytes = std::fs::read(&self.path).map_err(PetStoreError::Io)?;
        let save: PetSave = serde_json::from_slice(strip_bom(&bytes))
            .map_err(|e| self.backup_and_error(PetStoreError::Json(e)))?;
        if save.version != PET_SAVE_VERSION {
            return Err(PetStoreError::UnsupportedVersion(save.version));
        }
        save.validate()
            .map_err(|e| self.backup_and_error(PetStoreError::Invalid(e)))?;
        Ok(save)
    }

    pub fn save(&self, save: &PetSave) -> Result<(), PetStoreError> {
        save.validate().map_err(PetStoreError::Invalid)?;
        if save.version != PET_SAVE_VERSION {
            return Err(PetStoreError::UnsupportedVersion(save.version));
        }
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(PetStoreError::Io)?;
        }
        ensure_replaceable(&self.path)?;
        let tmp = self.path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(save).map_err(PetStoreError::Json)?;
        if let Err(error) = std::fs::write(&tmp, bytes) {
            let _ = std::fs::remove_file(&tmp);
            return Err(PetStoreError::Io(error));
        }
        if let Err(error) = replace_file(&tmp, &self.path) {
            let _ = std::fs::remove_file(&tmp);
            return Err(PetStoreError::Io(error));
        }
        Ok(())
    }

    fn backup_and_error(&self, error: PetStoreError) -> PetStoreError {
        let backup = backup_path(&self.path);
        match std::fs::rename(&self.path, &backup) {
            Ok(()) => PetStoreError::CorruptBackedUp {
                backup,
                source: Box::new(error),
            },
            Err(e) => PetStoreError::CorruptBackupFailed {
                source: Box::new(error),
                backup_error: e.to_string(),
            },
        }
    }

    #[cfg(test)]
    fn path(&self) -> &Path {
        &self.path
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PetStoreError {
    #[error("宠物存档 IO 错误：{0}")]
    Io(std::io::Error),
    #[error("宠物存档 JSON 错误：{0}")]
    Json(serde_json::Error),
    #[error("宠物存档数据无效：{0}")]
    Invalid(PetSaveError),
    #[error("不支持的宠物存档版本：{0}")]
    UnsupportedVersion(u16),
    #[error("宠物存档已损坏，已备份到 {backup:?}：{source}")]
    CorruptBackedUp {
        backup: PathBuf,
        source: Box<PetStoreError>,
    },
    #[error("宠物存档损坏，且备份失败：{source}；备份错误：{backup_error}")]
    CorruptBackupFailed {
        source: Box<PetStoreError>,
        backup_error: String,
    },
    #[error("现有宠物存档不可覆盖：{0}")]
    ExistingFileNotReplaceable(String),
}

fn ensure_replaceable(path: &Path) -> Result<(), PetStoreError> {
    if !path.exists() {
        return Ok(());
    }
    let bytes = std::fs::read(path).map_err(PetStoreError::Io)?;
    let existing: PetSave = serde_json::from_slice(strip_bom(&bytes))
        .map_err(|error| PetStoreError::ExistingFileNotReplaceable(error.to_string()))?;
    if existing.version != PET_SAVE_VERSION {
        return Err(PetStoreError::ExistingFileNotReplaceable(format!(
            "不支持的版本 {}",
            existing.version
        )));
    }
    existing
        .validate()
        .map_err(|error| PetStoreError::ExistingFileNotReplaceable(error.to_string()))
}

fn replace_file(tmp: &Path, target: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        };
        let source: Vec<u16> = tmp
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let destination: Vec<u16> = target
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let flags = MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH;
        unsafe {
            MoveFileExW(
                windows::core::PCWSTR(source.as_ptr()),
                windows::core::PCWSTR(destination.as_ptr()),
                flags,
            )
        }
        .map_err(|error| std::io::Error::other(error.to_string()))
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(tmp, target)
    }
}

fn strip_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes)
}

fn backup_path(path: &Path) -> PathBuf {
    let base = path.with_extension("json.corrupt");
    if !base.exists() {
        return base;
    }
    for i in 1.. {
        let p = path.with_extension(format!("json.corrupt.{i}"));
        if !p.exists() {
            return p;
        }
    }
    unreachable!("备份路径枚举无穷循环")
}

#[cfg(test)]
mod tests {
    use super::*;
    use gesso_core::{PetSave, PetStats};
    fn store() -> (PetStore, PathBuf) {
        let d = std::env::temp_dir().join(format!("gesso-pet-store-{}", gesso_core::generate_id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("pet-save-v1.json");
        (PetStore::new(p), d)
    }
    #[test]
    fn missing_save_loads_default_without_creating_file() {
        let (s, d) = store();
        assert_eq!(s.load().unwrap(), PetSave::default());
        assert!(!s.path().exists());
        std::fs::remove_dir_all(d).unwrap();
    }
    #[test]
    fn save_roundtrip_is_atomic_from_callers_view() {
        let (s, d) = store();
        let save = PetSave {
            stats: PetStats {
                touch_count: 2,
                ..Default::default()
            },
            ..Default::default()
        };
        s.save(&save).unwrap();
        assert_eq!(s.load().unwrap(), save);
        assert!(!s.path().with_extension("json.tmp").exists());
        std::fs::remove_dir_all(d).unwrap();
    }
    #[test]
    fn corrupt_json_is_backed_up_and_never_replaced_with_default() {
        let (s, d) = store();
        std::fs::write(s.path(), b"{not-json").unwrap();
        assert!(matches!(
            s.load().unwrap_err(),
            PetStoreError::CorruptBackedUp { .. }
        ));
        assert!(!s.path().exists());
        assert!(d.join("pet-save-v1.json.corrupt").exists());
        std::fs::remove_dir_all(d).unwrap();
    }
    #[test]
    fn unsupported_version_is_not_backed_up_or_overwritten() {
        let (s, d) = store();
        std::fs::write(s.path(), br#"{"version":99,"adoption":null}"#).unwrap();
        assert!(matches!(
            s.load().unwrap_err(),
            PetStoreError::UnsupportedVersion(99)
        ));
        assert!(s.path().exists());
        assert!(matches!(
            s.save(&PetSave::default()).unwrap_err(),
            PetStoreError::ExistingFileNotReplaceable(_)
        ));
        assert_eq!(
            std::fs::read_to_string(s.path()).unwrap(),
            r#"{"version":99,"adoption":null}"#
        );
        std::fs::remove_dir_all(d).unwrap();
    }
    #[test]
    fn repeated_save_replaces_existing_v1_save() {
        let (s, d) = store();
        s.save(&PetSave::default()).unwrap();
        let next = PetSave {
            visible: false,
            ..PetSave::default()
        };
        s.save(&next).unwrap();
        assert_eq!(s.load().unwrap(), next);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn corrupt_backup_uses_next_free_name() {
        let (s, d) = store();
        std::fs::write(s.path(), b"{not-json").unwrap();
        std::fs::write(d.join("pet-save-v1.json.corrupt"), b"old").unwrap();
        std::fs::write(d.join("pet-save-v1.json.corrupt.1"), b"old").unwrap();
        assert!(matches!(
            s.load().unwrap_err(),
            PetStoreError::CorruptBackedUp { .. }
        ));
        assert!(d.join("pet-save-v1.json.corrupt.2").exists());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn bom_is_accepted() {
        let (s, d) = store();
        let mut b = vec![0xEF, 0xBB, 0xBF];
        b.extend_from_slice(br#"{"version":1,"adoption":null}"#);
        std::fs::write(s.path(), b).unwrap();
        assert_eq!(s.load().unwrap(), PetSave::default());
        std::fs::remove_dir_all(d).unwrap();
    }
}
