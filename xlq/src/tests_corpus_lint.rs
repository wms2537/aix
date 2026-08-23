//! META-GUARD — keeps the systematic blind spot from silently returning.
//!
//! The flagship stale-cache HIGH bug survived 31 rounds because EVERY fixture shipped blank
//! `<v/>` caches. These asserts fail if a corpus fixture ever loses its populated caches or its
//! engine-faithful baseline, so the property tests built on the corpus can never quietly become
//! vacuous.

use crate::testkit;

#[test]
fn every_corpus_fixture_ships_populated_caches() {
    for case in testkit::corpus() {
        assert!(
            testkit::populated_cache_count(&case.bytes) >= 1,
            "{}: a corpus fixture MUST carry >=1 populated <v> cache — the exact blind spot that \
             hid the stale-cache HIGH bug for 31 rounds",
            case.name
        );
    }
}

#[test]
fn engine_faithful_corpus_baselines_are_clean() {
    // Each `ironcalc_faithful` fixture's stored caches must already agree with the engine — a
    // clean value baseline — so a cache-soundness failure after an edit is attributable to the
    // edit, not a pre-existing dirty cache.
    for case in testkit::corpus() {
        if case.ironcalc_faithful {
            assert!(
                testkit::cache_soundness(&case.bytes).is_ok(),
                "{}: baseline caches disagree with the engine — fixture is not a clean baseline",
                case.name
            );
        }
    }
}
