//! 桌面宠物正式个体与版本化存档的纯数据模型。
//!
//! 本模块不负责文件 I/O、缓存生成或 pet_id 随机生成；app 层负责生成正式 ID、
//! 原子写入存档和管理缓存事务。这里只定义可序列化的数据与最小校验。

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PetInteraction {
    Poke,
    Touch,
}

use crate::pet::{AppearanceParams, Candidate, PetName, PetSeed, PetSpecies};

pub const PET_SAVE_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PetIndividual {
    pub pet_id: String,
    pub species: PetSpecies,
    pub root_seed: PetSeed,
    pub appearance: AppearanceParams,
    pub name: PetName,
    pub generator_version: u16,
    pub asset_version: u16,
    pub generation_signature: u64,
    /// Unix seconds supplied by the app layer.
    pub adopted_at: u64,
}

impl PetIndividual {
    /// 将候选升级为正式个体；正式 ID 必须由 app 层在确认事务中生成。
    pub fn from_candidate(
        candidate: Candidate,
        pet_id: String,
        adopted_at: u64,
    ) -> Result<Self, PetSaveError> {
        if pet_id.trim().is_empty() {
            return Err(PetSaveError::EmptyPetId);
        }
        Ok(Self {
            pet_id,
            species: candidate.species,
            root_seed: candidate.root_seed,
            appearance: candidate.appearance,
            name: candidate.name,
            generator_version: candidate.generator_version,
            asset_version: candidate.asset_version,
            generation_signature: candidate.generation_signature,
            adopted_at,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PetPosition {
    pub monitor_id: String,
    /// 屏内相对坐标，范围校验和钳制由 app/pin 层负责。
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PetActivity {
    Quiet,
    Normal,
    Lively,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PetSize {
    Small,
    Medium,
    Large,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PetStats {
    pub companion_seconds: u64,
    pub touch_count: u64,
    pub poke_count: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PetSave {
    pub version: u16,
    pub adoption: Option<PetIndividual>,
    pub position: Option<PetPosition>,
    pub activity: PetActivity,
    pub size: PetSize,
    pub visible: bool,
    pub stats: PetStats,
}

impl Default for PetSave {
    fn default() -> Self {
        Self {
            version: PET_SAVE_VERSION,
            adoption: None,
            position: None,
            activity: PetActivity::Normal,
            size: PetSize::Medium,
            visible: true,
            stats: PetStats::default(),
        }
    }
}

impl PetSave {
    /// 版本迁移入口；当前版本没有字段迁移，但所有加载与写入都经过这里。
    pub fn migrate(self) -> Result<Self, PetSaveError> {
        if self.version != PET_SAVE_VERSION {
            return Err(PetSaveError::InvalidVersion);
        }
        self.validate()?;
        Ok(self)
    }

    pub fn record_interaction(&mut self, interaction: PetInteraction) {
        match interaction {
            PetInteraction::Poke => self.stats.poke_count += 1,
            PetInteraction::Touch => self.stats.touch_count += 1,
        }
    }

    pub fn validate(&self) -> Result<(), PetSaveError> {
        if self.version == 0 {
            return Err(PetSaveError::InvalidVersion);
        }
        if let Some(pet) = &self.adoption {
            if pet.pet_id.trim().is_empty() {
                return Err(PetSaveError::EmptyPetId);
            }
            if pet.name.zh.trim().is_empty() || pet.name.en.trim().is_empty() {
                return Err(PetSaveError::EmptyPetName);
            }
        }
        if let Some(position) = &self.position {
            if position.monitor_id.trim().is_empty()
                || !position.x.is_finite()
                || !position.y.is_finite()
            {
                return Err(PetSaveError::InvalidPosition);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PetSaveError {
    #[error("宠物 ID 不能为空")]
    EmptyPetId,
    #[error("宠物名称不能为空")]
    EmptyPetName,
    #[error("宠物存档版本无效")]
    InvalidVersion,
    #[error("宠物位置无效")]
    InvalidPosition,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pet::{generate_candidate, ASSET_VERSION, GENERATOR_VERSION};

    fn candidate() -> Candidate {
        generate_candidate(
            PetSpecies::Cat,
            PetSeed::from_bytes([3; 16]),
            GENERATOR_VERSION,
            ASSET_VERSION,
        )
    }

    #[test]
    fn candidate_becomes_formal_individual_only_with_pet_id() {
        let pet = PetIndividual::from_candidate(candidate(), "pet-1".into(), 123).unwrap();
        assert_eq!(pet.pet_id, "pet-1");
        assert_eq!(pet.adopted_at, 123);
        assert_eq!(pet.generator_version, GENERATOR_VERSION);
        assert_eq!(pet.asset_version, ASSET_VERSION);
    }

    #[test]
    fn empty_pet_id_is_rejected() {
        assert_eq!(
            PetIndividual::from_candidate(candidate(), "  ".into(), 0).unwrap_err(),
            PetSaveError::EmptyPetId
        );
    }

    #[test]
    fn default_save_is_unadopted() {
        let save = PetSave::default();
        assert_eq!(save.version, PET_SAVE_VERSION);
        assert!(save.adoption.is_none());
        assert!(save.position.is_none());
        assert_eq!(save.activity, PetActivity::Normal);
        assert_eq!(save.size, PetSize::Medium);
        assert!(save.visible);
    }

    #[test]
    fn record_interaction_updates_only_matching_counter() {
        let mut save = PetSave::default();
        save.record_interaction(PetInteraction::Poke);
        save.record_interaction(PetInteraction::Touch);
        save.record_interaction(PetInteraction::Touch);
        assert_eq!(save.stats.poke_count, 1);
        assert_eq!(save.stats.touch_count, 2);
    }

    #[test]
    fn migrate_accepts_current_save() {
        let save = PetSave::default();
        assert_eq!(save.clone().migrate().unwrap(), save);
    }

    #[test]
    fn migrate_rejects_unknown_version() {
        let save = PetSave {
            version: PET_SAVE_VERSION + 1,
            ..PetSave::default()
        };
        assert!(matches!(save.migrate(), Err(PetSaveError::InvalidVersion)));
    }

    #[test]
    fn save_roundtrip_preserves_adoption_and_preferences() {
        let pet = PetIndividual::from_candidate(candidate(), "pet-1".into(), 123).unwrap();
        let save = PetSave {
            adoption: Some(pet),
            position: Some(PetPosition {
                monitor_id: "display-1".into(),
                x: 0.8,
                y: 0.9,
            }),
            activity: PetActivity::Lively,
            size: PetSize::Large,
            visible: false,
            stats: PetStats {
                companion_seconds: 42,
                touch_count: 3,
                poke_count: 7,
            },
            ..PetSave::default()
        };
        let json = serde_json::to_vec(&save).unwrap();
        let decoded: PetSave = serde_json::from_slice(&json).unwrap();
        assert_eq!(decoded, save);
        decoded.validate().unwrap();
    }

    #[test]
    fn old_minimal_save_loads_with_defaults() {
        let save: PetSave = serde_json::from_str(r#"{"version":1,"adoption":null}"#).unwrap();
        assert_eq!(save, PetSave::default());
    }

    #[test]
    fn invalid_position_is_rejected() {
        let save = PetSave {
            position: Some(PetPosition {
                monitor_id: "display-1".into(),
                x: f32::NAN,
                y: 0.5,
            }),
            ..PetSave::default()
        };
        assert_eq!(save.validate().unwrap_err(), PetSaveError::InvalidPosition);
    }
}
