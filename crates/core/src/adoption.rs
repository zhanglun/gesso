//! 首次领养流程的纯状态机。
//!
//! 这里管理临时候选和流程转移，不创建正式 `pet_id`，也不执行存档或缓存 I/O。
//! 正式身份由后续的领养事务在 `Persisting` 成功后创建。

use crate::pet::{generate_candidates, Candidate, PetSeed, PetSpecies};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateSet {
    pub session_seed: PetSeed,
    pub species: PetSpecies,
    pub reroll_generation: u64,
    pub candidates: [Candidate; 3],
    pub selected: Option<usize>,
    pub generator_version: u16,
    pub asset_version: u16,
}

impl CandidateSet {
    /// 会话内临时候选身份；不是正式 `pet_id`，也不写入存档。
    pub fn candidate_id(&self, index: usize) -> Option<PetSeed> {
        self.candidates.get(index).and_then(|_| {
            let counter = self
                .reroll_generation
                .checked_mul(3)?
                .checked_add(index as u64)?;
            Some(self.session_seed.derive("adoption/candidate-id", counter))
        })
    }

    fn with_selection(&self, selected: Option<usize>) -> Self {
        let mut next = self.clone();
        next.selected = selected;
        next
    }

    fn rerolled(&self) -> Option<Self> {
        let next_generation = self.reroll_generation.checked_add(1)?;
        let candidates = generate_candidates(
            self.species,
            self.session_seed,
            next_generation,
            self.generator_version,
            self.asset_version,
        )
        .ok()?;
        Some(Self {
            session_seed: self.session_seed,
            species: self.species,
            reroll_generation: next_generation,
            candidates,
            selected: None,
            generator_version: self.generator_version,
            asset_version: self.asset_version,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdoptionState {
    NotAdopted,
    CandidatesVisible(CandidateSet),
    CandidateSelected(CandidateSet),
    Persisting { candidates: CandidateSet },
    Adopted,
    ResetConfirm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdoptionEvent {
    /// 从托盘主动开始领养；`session_seed` 由 app 层使用系统熵源创建。
    Start {
        species: PetSpecies,
        session_seed: PetSeed,
        generator_version: u16,
        asset_version: u16,
    },
    Reroll,
    SelectCandidate(usize),
    ConfirmAdoption,
    PersistSucceeded,
    PersistFailed,
    BackToSpecies,
    Close,
    RequestReset,
    CancelReset,
    ConfirmReset,
}

/// 纯状态转移。
///
/// 无效事件保持原状态；候选生成失败、索引越界和计数溢出都不会 panic。
pub fn transition(state: AdoptionState, event: AdoptionEvent) -> AdoptionState {
    use AdoptionEvent as E;
    use AdoptionState as S;

    match (state, event) {
        (
            S::NotAdopted,
            E::Start {
                species,
                session_seed,
                generator_version,
                asset_version,
            },
        ) => {
            let Ok(candidates) =
                generate_candidates(species, session_seed, 0, generator_version, asset_version)
            else {
                return S::NotAdopted;
            };
            S::CandidatesVisible(CandidateSet {
                session_seed,
                species,
                reroll_generation: 0,
                candidates,
                selected: None,
                generator_version,
                asset_version,
            })
        }
        (S::CandidatesVisible(set), E::Reroll) => set
            .rerolled()
            .map(S::CandidatesVisible)
            .unwrap_or(S::CandidatesVisible(set)),
        (S::CandidatesVisible(set), E::SelectCandidate(index)) if index < set.candidates.len() => {
            S::CandidateSelected(set.with_selection(Some(index)))
        }
        (S::CandidateSelected(set), E::SelectCandidate(index)) if index < set.candidates.len() => {
            S::CandidateSelected(set.with_selection(Some(index)))
        }
        (S::CandidateSelected(set), E::Reroll) => set
            .rerolled()
            .map(S::CandidatesVisible)
            .unwrap_or(S::CandidateSelected(set)),
        (S::CandidateSelected(set), E::ConfirmAdoption) => {
            if set.selected.is_none() {
                return S::CandidateSelected(set);
            }
            S::Persisting { candidates: set }
        }
        (S::Persisting { candidates: _ }, E::PersistSucceeded) => S::Adopted,
        (S::Persisting { candidates }, E::PersistFailed) => S::CandidateSelected(candidates),
        (S::CandidatesVisible(_), E::BackToSpecies)
        | (S::CandidateSelected(_), E::BackToSpecies)
        | (S::CandidatesVisible(_), E::Close)
        | (S::CandidateSelected(_), E::Close) => S::NotAdopted,
        (S::Adopted, E::RequestReset) => S::ResetConfirm,
        (S::ResetConfirm, E::CancelReset) => S::Adopted,
        (S::ResetConfirm, E::ConfirmReset) => S::NotAdopted,
        (state, _) => state,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pet::{ASSET_VERSION, GENERATOR_VERSION};

    fn seed() -> PetSeed {
        PetSeed::from_bytes([7; 16])
    }

    fn start() -> AdoptionState {
        transition(
            AdoptionState::NotAdopted,
            AdoptionEvent::Start {
                species: PetSpecies::Cat,
                session_seed: seed(),
                generator_version: GENERATOR_VERSION,
                asset_version: ASSET_VERSION,
            },
        )
    }

    #[test]
    fn first_adoption_happy_path() {
        let state = start();
        let state = transition(state, AdoptionEvent::SelectCandidate(1));
        let state = transition(state, AdoptionEvent::ConfirmAdoption);
        assert!(matches!(state, AdoptionState::Persisting { .. }));
        assert!(matches!(
            transition(state, AdoptionEvent::PersistSucceeded),
            AdoptionState::Adopted
        ));
    }

    #[test]
    fn reroll_replaces_candidates_and_clears_selection() {
        let state = transition(start(), AdoptionEvent::SelectCandidate(0));
        let old = match &state {
            AdoptionState::CandidateSelected(set) => set.candidates.clone(),
            _ => panic!("expected selected candidate"),
        };
        let next = transition(state, AdoptionEvent::Reroll);
        match next {
            AdoptionState::CandidatesVisible(set) => {
                assert_eq!(set.reroll_generation, 1);
                assert_eq!(set.selected, None);
                assert_ne!(set.candidates, old);
            }
            _ => panic!("expected visible candidates"),
        }
    }

    #[test]
    fn invalid_selection_and_confirm_are_noops() {
        let state = start();
        let state = transition(state, AdoptionEvent::SelectCandidate(3));
        assert!(matches!(state, AdoptionState::CandidatesVisible(_)));
        let state = transition(state, AdoptionEvent::ConfirmAdoption);
        assert!(matches!(state, AdoptionState::CandidatesVisible(_)));
    }

    #[test]
    fn close_and_back_discard_temporary_candidates() {
        assert!(matches!(
            transition(start(), AdoptionEvent::Close),
            AdoptionState::NotAdopted
        ));
        assert!(matches!(
            transition(start(), AdoptionEvent::BackToSpecies),
            AdoptionState::NotAdopted
        ));
    }

    #[test]
    fn reset_requires_confirmation() {
        let state = transition(AdoptionState::Adopted, AdoptionEvent::RequestReset);
        assert!(matches!(state, AdoptionState::ResetConfirm));
        assert!(matches!(
            transition(state.clone(), AdoptionEvent::CancelReset),
            AdoptionState::Adopted
        ));
        assert!(matches!(
            transition(state, AdoptionEvent::ConfirmReset),
            AdoptionState::NotAdopted
        ));
    }

    #[test]
    fn persist_failure_keeps_selected_candidates_and_versions() {
        let state = transition(
            transition(start(), AdoptionEvent::SelectCandidate(1)),
            AdoptionEvent::ConfirmAdoption,
        );
        let failed = transition(state, AdoptionEvent::PersistFailed);
        match failed {
            AdoptionState::CandidateSelected(set) => {
                assert_eq!(set.selected, Some(1));
                assert_eq!(set.generator_version, GENERATOR_VERSION);
                assert_eq!(set.asset_version, ASSET_VERSION);
            }
            _ => panic!("expected selected candidates after failure"),
        }
    }

    #[test]
    fn candidate_ids_are_temporary_and_round_scoped() {
        let state = start();
        let set = match state {
            AdoptionState::CandidatesVisible(set) => set,
            _ => panic!("expected candidates"),
        };
        assert_ne!(set.candidate_id(0), set.candidate_id(1));
        assert!(set.candidate_id(3).is_none());
    }
}
