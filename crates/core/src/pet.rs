//! 桌面宠物的纯领域生成逻辑。
//!
//! 生成器只从版本化的占位组件池中确定性选择参数，不创建正式 `pet_id`，
//! 不访问系统随机源，也不读写缓存或存档。

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const GENERATOR_VERSION: u16 = 1;
pub const ASSET_VERSION: u16 = 1;
const CANDIDATE_COUNT: usize = 3;
const MAX_CANDIDATE_ATTEMPTS: u64 = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PetSpecies {
    Cat,
    Jelly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PetSeed([u8; 16]);

impl PetSeed {
    /// Creates a new session seed from the OS entropy source.
    pub fn random() -> Self {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).expect("系统熵源不可用");
        Self(bytes)
    }

    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(self) -> [u8; 16] {
        self.0
    }

    pub fn to_hex(self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    /// 从根 seed 派生独立命名空间，避免新增字段改变既有字段结果。
    pub fn derive(self, namespace: &str, counter: u64) -> Self {
        let low = stable_hash(self.0, namespace, counter);
        let high = stable_hash(self.0, namespace, counter ^ 0x9e37_79b9_7f4a_7c15);
        let mut bytes = [0u8; 16];
        bytes[..8].copy_from_slice(&low.to_le_bytes());
        bytes[8..].copy_from_slice(&high.to_le_bytes());
        Self(bytes)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AppearanceParams {
    pub body: u8,
    pub face: u8,
    pub eyes: u8,
    pub coat_color: u8,
    pub marking: u8,
    pub accent: u8,
    pub idle_variant: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PetName {
    pub zh: String,
    pub en: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    pub species: PetSpecies,
    pub root_seed: PetSeed,
    pub appearance: AppearanceParams,
    pub name: PetName,
    pub generation_signature: u64,
    pub generator_version: u16,
    pub asset_version: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CandidateGenerationError {
    #[error("无法在有限尝试内生成 3 个不同候选")]
    InsufficientUniqueCandidates,
}

/// 根据物种、seed 和生成器版本生成一个稳定候选。
pub fn generate_candidate(
    species: PetSpecies,
    root_seed: PetSeed,
    generator_version: u16,
    asset_version: u16,
) -> Candidate {
    let appearance = appearance_for(species, root_seed, generator_version, asset_version);
    let name = name_for(species, root_seed, generator_version, asset_version);
    let generation_signature = signature(
        species,
        &appearance,
        &name,
        generator_version,
        asset_version,
    );

    Candidate {
        species,
        root_seed,
        appearance,
        name,
        generation_signature,
        generator_version,
        asset_version,
    }
}

/// 生成一轮 3 个同物种候选。
///
/// `adoption_session_seed` 只属于未确认领养会话；确认后应保存候选的
/// `root_seed`，而不是保存这里的会话状态。
pub fn generate_candidates(
    species: PetSpecies,
    adoption_session_seed: PetSeed,
    reroll_generation: u64,
    generator_version: u16,
    asset_version: u16,
) -> Result<[Candidate; CANDIDATE_COUNT], CandidateGenerationError> {
    let round_seed = adoption_session_seed.derive("adoption/candidate", reroll_generation);
    let mut candidates = Vec::with_capacity(CANDIDATE_COUNT);

    for index in 0..CANDIDATE_COUNT {
        let mut found = None;
        for attempt in 0..MAX_CANDIDATE_ATTEMPTS {
            let root_seed = round_seed.derive("candidate/root", index as u64 * 100 + attempt);
            let candidate =
                generate_candidate(species, root_seed, generator_version, asset_version);
            let duplicate = candidates.iter().any(|previous: &Candidate| {
                previous.root_seed == candidate.root_seed
                    || previous.generation_signature == candidate.generation_signature
                    || previous.appearance == candidate.appearance
                    || previous.name == candidate.name
            });
            if !duplicate {
                found = Some(candidate);
                break;
            }
        }

        candidates.push(found.ok_or(CandidateGenerationError::InsufficientUniqueCandidates)?);
    }

    Ok(candidates.try_into().expect("候选数量由常量保证"))
}

fn appearance_for(
    species: PetSpecies,
    root_seed: PetSeed,
    generator_version: u16,
    asset_version: u16,
) -> AppearanceParams {
    let pick = |namespace: &str, count: u8| -> u8 {
        (stable_hash(
            root_seed.as_bytes(),
            namespace,
            version_counter(generator_version, asset_version),
        ) % u64::from(count)) as u8
    };

    let (body_count, face_count, eye_count, color_count, marking_count, accent_count) =
        match species {
            PetSpecies::Cat => (2, 3, 4, 5, 4, 3),
            PetSpecies::Jelly => (2, 3, 3, 5, 3, 4),
        };

    AppearanceParams {
        body: pick("appearance/body", body_count),
        face: pick("appearance/face", face_count),
        eyes: pick("appearance/eyes", eye_count),
        coat_color: pick("appearance/coat_color", color_count),
        marking: pick("appearance/marking", marking_count),
        accent: pick("appearance/accent", accent_count),
        idle_variant: pick("behavior/idle", 3),
    }
}

fn name_for(
    species: PetSpecies,
    root_seed: PetSeed,
    generator_version: u16,
    asset_version: u16,
) -> PetName {
    let names: &[(&str, &str)] = match species {
        PetSpecies::Cat => &[
            ("栗子", "Chestnut"),
            ("小眠", "Napper"),
            ("团云", "Cloudlet"),
            ("灰团", "Ashball"),
            ("月面", "Moonface"),
            ("绒绒", "Velvet"),
        ],
        PetSpecies::Jelly => &[
            ("轻弹", "Bouncer"),
            ("紫柚", "Pomelo"),
            ("软云", "Softcloud"),
            ("微光", "Glimmer"),
            ("水滴", "Dewdrop"),
            ("星冻", "Starjelly"),
        ],
    };
    let index = (stable_hash(
        root_seed.as_bytes(),
        "name",
        version_counter(generator_version, asset_version),
    ) % names.len() as u64) as usize;
    let (zh, en) = names[index];
    PetName {
        zh: zh.to_owned(),
        en: en.to_owned(),
    }
}

fn signature(
    species: PetSpecies,
    appearance: &AppearanceParams,
    name: &PetName,
    generator_version: u16,
    asset_version: u16,
) -> u64 {
    let species_tag = match species {
        PetSpecies::Cat => "cat",
        PetSpecies::Jelly => "jelly",
    };
    let mut bytes = Vec::new();
    bytes.extend_from_slice(species_tag.as_bytes());
    bytes.extend_from_slice(&generator_version.to_le_bytes());
    bytes.extend_from_slice(&asset_version.to_le_bytes());
    bytes.extend_from_slice(&[
        appearance.body,
        appearance.face,
        appearance.eyes,
        appearance.coat_color,
        appearance.marking,
        appearance.accent,
        appearance.idle_variant,
    ]);
    bytes.extend_from_slice(name.zh.as_bytes());
    bytes.extend_from_slice(name.en.as_bytes());
    stable_hash_bytes(&bytes)
}

fn version_counter(generator_version: u16, asset_version: u16) -> u64 {
    (u64::from(generator_version) << 32) | u64::from(asset_version)
}

// 固定算法：不使用 Rust 的 DefaultHasher，避免跨进程/版本的随机化影响结果。
fn stable_hash(seed: [u8; 16], namespace: &str, counter: u64) -> u64 {
    let mut bytes = Vec::with_capacity(seed.len() + namespace.len() + 8);
    bytes.extend_from_slice(&seed);
    bytes.extend_from_slice(namespace.as_bytes());
    bytes.extend_from_slice(&counter.to_le_bytes());
    stable_hash_bytes(&bytes)
}

fn stable_hash_bytes(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    // Avalanche step keeps nearby seeds from selecting the same small pool too often.
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^ (hash >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed() -> PetSeed {
        PetSeed::from_bytes([
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
            0xee, 0xff,
        ])
    }

    #[test]
    fn same_seed_is_deterministic() {
        let a = generate_candidate(PetSpecies::Cat, seed(), GENERATOR_VERSION, ASSET_VERSION);
        let b = generate_candidate(PetSpecies::Cat, seed(), GENERATOR_VERSION, ASSET_VERSION);
        assert_eq!(a, b);
    }

    #[test]
    fn namespace_derivation_is_stable_and_independent() {
        let base = seed();
        assert_eq!(
            base.derive("appearance/tail", 0),
            base.derive("appearance/tail", 0)
        );
        assert_ne!(
            base.derive("appearance/tail", 0),
            base.derive("appearance/clothes", 0)
        );
        let old = generate_candidate(PetSpecies::Cat, base, GENERATOR_VERSION, ASSET_VERSION);
        let changed_root = base.derive("appearance/tail", 0);
        let still_different = generate_candidate(
            PetSpecies::Cat,
            changed_root,
            GENERATOR_VERSION,
            ASSET_VERSION,
        );
        assert_ne!(old.root_seed, still_different.root_seed);
    }

    #[test]
    fn candidate_round_is_distinct_and_deterministic() {
        let first =
            generate_candidates(PetSpecies::Cat, seed(), 0, GENERATOR_VERSION, ASSET_VERSION)
                .unwrap();
        let second =
            generate_candidates(PetSpecies::Cat, seed(), 0, GENERATOR_VERSION, ASSET_VERSION)
                .unwrap();
        assert_eq!(first, second);
        assert_ne!(first[0].root_seed, first[1].root_seed);
        assert_ne!(first[1].root_seed, first[2].root_seed);
        assert_ne!(first[0].root_seed, first[2].root_seed);
        assert_ne!(first[0].generation_signature, first[1].generation_signature);
        assert_ne!(first[1].generation_signature, first[2].generation_signature);
        assert_ne!(first[0].generation_signature, first[2].generation_signature);
        assert_ne!(first[0].appearance, first[1].appearance);
        assert_ne!(first[1].appearance, first[2].appearance);
        assert_ne!(first[0].appearance, first[2].appearance);
        assert_ne!(first[0].name, first[1].name);
        assert_ne!(first[1].name, first[2].name);
        assert_ne!(first[0].name, first[2].name);
    }

    #[test]
    fn asset_version_is_part_of_candidate_identity() {
        let v1 = generate_candidate(PetSpecies::Cat, seed(), GENERATOR_VERSION, 1);
        let v2 = generate_candidate(PetSpecies::Cat, seed(), GENERATOR_VERSION, 2);
        assert_ne!(v1.generation_signature, v2.generation_signature);
        assert_eq!(v1.generator_version, v2.generator_version);
        assert_ne!(v1.asset_version, v2.asset_version);
    }

    #[test]
    fn reroll_changes_round_without_using_os_randomness() {
        let first = generate_candidates(
            PetSpecies::Jelly,
            seed(),
            0,
            GENERATOR_VERSION,
            ASSET_VERSION,
        )
        .unwrap();
        let reroll = generate_candidates(
            PetSpecies::Jelly,
            seed(),
            1,
            GENERATOR_VERSION,
            ASSET_VERSION,
        )
        .unwrap();
        assert_ne!(first, reroll);
    }

    #[test]
    fn species_templates_are_distinct() {
        let cat = generate_candidate(PetSpecies::Cat, seed(), GENERATOR_VERSION, ASSET_VERSION);
        let jelly = generate_candidate(PetSpecies::Jelly, seed(), GENERATOR_VERSION, ASSET_VERSION);
        assert_ne!(cat.species, jelly.species);
        assert_ne!(cat.name, jelly.name);
    }

    #[test]
    fn candidate_has_no_formal_pet_id() {
        let candidate =
            generate_candidate(PetSpecies::Cat, seed(), GENERATOR_VERSION, ASSET_VERSION);
        let json = serde_json::to_string(&candidate).unwrap();
        assert!(!json.contains("pet_id"));
    }
}
