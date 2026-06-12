//! The Memory benchmark fixture corpus — 100 deterministic test cases.
//!
//! Authored across phases:
//!   * P2 (this file, fixtures 1–41): 25 ingest + 16 recall-current
//!   * P3 (fixtures 42–75): 12 recall_at + 12 recall_as_of + 10 contradiction
//!   * P4 (fixtures 76–100): 10 procedural + 10 feedback + 5 determinism
//!
//! Compounding fixtures carry `requires_state_from: &[predecessor_ids]`. The
//! harness runs fixtures in `id`-sorted order so predecessors complete first.
//!
//! Every fixture has at least one Pathology tag and one PublicBench mapping.
//! Coverage gates are checked by `tests/coverage.rs`.

use crate::fixture::{Expected, Fixture, Setup, SetupEvent};
use crate::scorer;
use crate::{Domain, FixtureBlock, Pathology, PublicBench, TemporalLens};

// Fixtures are entirely const-constructible. Queries are encoded as
// (query_text, query_intent, query_mentions) and materialized into a
// `Query` struct at run time by `bin/bench.rs`. See fixture.rs for the
// field semantics.

use std::sync::LazyLock;

// The corpus is split into small module chunks so no authored file crosses
// the shape threshold. The public API remains the same `FIXTURES` slice.
pub static FIXTURES: LazyLock<Vec<Fixture>> = LazyLock::new(build_fixtures);

fn build_fixtures() -> Vec<Fixture> {
    let mut fixtures = Vec::with_capacity(100);
    part01::extend(&mut fixtures);
    part02::extend(&mut fixtures);
    part11::extend(&mut fixtures);
    part03::extend(&mut fixtures);
    part04::extend(&mut fixtures);
    part05::extend(&mut fixtures);
    part06::extend(&mut fixtures);
    part07::extend(&mut fixtures);
    part08::extend(&mut fixtures);
    part09::extend(&mut fixtures);
    part10::extend(&mut fixtures);
    fixtures
}

pub fn all() -> &'static [Fixture] {
    FIXTURES.as_slice()
}

mod part01;
mod part02;
mod part11;
mod part03;
mod part04;
mod part05;
mod part06;
mod part07;
mod part08;
mod part09;
mod part10;
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_count_is_100() {
        assert_eq!(FIXTURES.len(), 100);
    }

    #[test]
    fn ids_are_sequential_and_unique() {
        for (i, f) in FIXTURES.iter().enumerate() {
            assert_eq!(f.id as usize, i + 1, "fixture id {} at index {}", f.id, i);
        }
    }

    #[test]
    fn predecessors_exist_and_come_before() {
        let max_id = FIXTURES.len() as u8;
        for f in FIXTURES.iter() {
            for &pred in f.requires_state_from {
                assert!(
                    pred < f.id,
                    "fixture {} predecessor {} must come before",
                    f.id,
                    pred
                );
                assert!(
                    pred >= 1 && pred <= max_id,
                    "fixture {} predecessor {} out of range",
                    f.id,
                    pred
                );
            }
        }
    }

    #[test]
    fn block_distribution_matches_spec() {
        let counts = |b: FixtureBlock| FIXTURES.iter().filter(|f| f.block == b).count();
        assert_eq!(counts(FixtureBlock::Ingest), 25, "ingest block");
        assert_eq!(
            counts(FixtureBlock::RecallCurrent),
            16,
            "recall-current block"
        );
        assert_eq!(counts(FixtureBlock::RecallAt), 12, "recall_at block");
        assert_eq!(counts(FixtureBlock::RecallAsOf), 12, "recall_as_of block");
        assert_eq!(
            counts(FixtureBlock::Contradiction),
            10,
            "contradiction block"
        );
        assert_eq!(counts(FixtureBlock::Procedural), 10, "procedural block");
        assert_eq!(counts(FixtureBlock::Feedback), 10, "feedback block");
        assert_eq!(counts(FixtureBlock::Determinism), 5, "determinism block");
    }

    #[test]
    fn every_pathology_appears_at_least_three_times() {
        for p in crate::Pathology::ALL {
            let count = FIXTURES
                .iter()
                .filter(|f| f.pathologies.iter().any(|x| x == p))
                .count();
            assert!(
                count >= 3,
                "pathology {:?} should appear in ≥ 3 fixtures, found {}",
                p,
                count
            );
        }
    }

    #[test]
    fn every_domain_pathology_cell_has_a_fixture() {
        // Coverage matrix: every (domain × pathology) cell must have ≥ 1 fixture.
        // Documented in docs/ADVANCED_MEMORY_CHALLENGE.md §4.
        let mut missing = Vec::new();
        for d in crate::Domain::ALL {
            for p in crate::Pathology::ALL {
                let hit = FIXTURES
                    .iter()
                    .any(|f| f.domain == *d && f.pathologies.iter().any(|x| x == p));
                if !hit {
                    missing.push((d.name(), p.name()));
                }
            }
        }
        assert!(
            missing.is_empty(),
            "empty (domain × pathology) cells: {:?}",
            missing
        );
    }

    #[test]
    fn compounding_fixtures_meet_target() {
        let compounding = FIXTURES
            .iter()
            .filter(|f| !f.requires_state_from.is_empty())
            .count();
        assert!(
            compounding >= 20,
            "expected ≥ 20 compounding fixtures (Innovation D), got {}",
            compounding
        );
    }

    #[test]
    fn every_fixture_has_a_public_bench_mapping() {
        for f in FIXTURES.iter() {
            assert!(
                !f.public_bench.is_empty(),
                "fixture {} has no public-bench mapping",
                f.id
            );
        }
    }
}
