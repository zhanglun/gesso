//! 宠物视觉缓存的最小事务存储。
//!
//! 缓存不是身份真源：先在临时目录生成并原子提交，再写正式存档。

use std::path::{Path, PathBuf};

use gesso_core::PetIndividual;

const CACHE_VERSION: &str = "v1";

pub struct PetCacheStore {
    root: PathBuf,
}

impl PetCacheStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// 将当前正式个体的缓存元数据原子提交到版本化目录，返回正式目录。
    pub fn commit(&self, pet: &PetIndividual) -> Result<PathBuf, PetCacheError> {
        let version_dir = self
            .root
            .join(CACHE_VERSION)
            .join(pet.asset_version.to_string());
        std::fs::create_dir_all(&version_dir).map_err(PetCacheError::Io)?;

        let final_dir = version_dir.join(&pet.pet_id);
        if final_dir.exists() {
            return Err(PetCacheError::AlreadyExists(final_dir));
        }

        let tmp_dir = version_dir.join(format!(".{}.tmp-{}", pet.pet_id, std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_dir);
        std::fs::create_dir(&tmp_dir).map_err(PetCacheError::Io)?;

        let result = (|| {
            let bytes = serde_json::to_vec_pretty(pet).map_err(PetCacheError::Json)?;
            std::fs::write(tmp_dir.join("pet.json"), bytes).map_err(PetCacheError::Io)?;
            std::fs::rename(&tmp_dir, &final_dir).map_err(PetCacheError::Io)?;
            Ok(final_dir.clone())
        })();

        if result.is_err() {
            let _ = std::fs::remove_dir_all(&tmp_dir);
        }
        result
    }

    pub fn remove(&self, cache_dir: &Path) -> Result<(), PetCacheError> {
        if cache_dir.exists() {
            std::fs::remove_dir_all(cache_dir).map_err(PetCacheError::Io)?;
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PetCacheError {
    #[error("宠物缓存 IO 错误：{0}")]
    Io(std::io::Error),
    #[error("宠物缓存 JSON 错误：{0}")]
    Json(serde_json::Error),
    #[error("宠物缓存目录已存在：{0:?}")]
    AlreadyExists(PathBuf),
}

#[cfg(test)]
mod tests {
    use super::*;
    use gesso_core::{generate_candidate, PetSeed, PetSpecies, ASSET_VERSION, GENERATOR_VERSION};

    #[test]
    fn commit_is_versioned_and_cleans_up() {
        let root =
            std::env::temp_dir().join(format!("gesso-pet-cache-{}", gesso_core::generate_id()));
        let store = PetCacheStore::new(root.clone());
        let pet = PetIndividual::from_candidate(
            generate_candidate(
                PetSpecies::Cat,
                PetSeed::from_bytes([4; 16]),
                GENERATOR_VERSION,
                ASSET_VERSION,
            ),
            "pet-test".into(),
            1,
        )
        .unwrap();

        let dir = store.commit(&pet).unwrap();
        assert_eq!(
            dir,
            root.join("v1")
                .join(ASSET_VERSION.to_string())
                .join("pet-test")
        );
        assert!(dir.join("pet.json").exists());
        assert!(store.commit(&pet).is_err());
        store.remove(&dir).unwrap();
        assert!(!dir.exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
