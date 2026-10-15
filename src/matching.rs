//! Edmonds' blossom maximum-weight matching, the solver every pairing system
//! here reduces to.
//!
//! Adapted from NetworkX's BSD-3-Clause `max_weight_matching`, following
//! Galil's presentation of the algorithm, and modified: a stateful solver
//! over Rust-owned edge storage with integer weights only. Reached through
//! BBP Pairings (Apache-2.0, Copyright 2016-2026 Jeremy Bierema), which uses
//! the same approach.

mod blossom;
mod graph;
mod solver;
mod weight;

pub(crate) use solver::{MatchingError, MatchingSnapshot, MatchingSolver};
pub(crate) use weight::BigWeight;

#[cfg(test)]
mod tests {
    use super::{BigWeight, MatchingSolver};

    fn fixed_solver(capacity: usize) -> MatchingSolver {
        MatchingSolver::with_vertices(capacity)
    }

    fn matching_after_compute(mut solver: MatchingSolver) -> Vec<usize> {
        solver.compute_matching();
        solver
            .current_matching()
            .expect("matching was computed")
            .to_vec()
    }

    fn brute_force_maximum_weight(weights: &[Vec<usize>]) -> usize {
        fn recurse(weights: &[Vec<usize>], used: &mut [bool]) -> usize {
            let Some(vertex) = used.iter().position(|used| !*used) else {
                return 0;
            };
            used[vertex] = true;
            let mut best = recurse(weights, used);
            for neighbor in vertex + 1..weights.len() {
                if !used[neighbor] && weights[vertex][neighbor] != 0 {
                    used[neighbor] = true;
                    best = best.max(weights[vertex][neighbor] + recurse(weights, used));
                    used[neighbor] = false;
                }
            }
            used[vertex] = false;
            best
        }

        recurse(weights, &mut vec![false; weights.len()])
    }

    fn matching_weight(matching: &[usize], weights: &[Vec<usize>]) -> usize {
        let mut seen = vec![false; matching.len()];
        let mut result = 0;
        for (vertex, &neighbor) in matching.iter().enumerate() {
            assert!(neighbor < matching.len());
            assert_eq!(matching[neighbor], vertex);
            if vertex == neighbor {
                continue;
            }
            assert_ne!(weights[vertex][neighbor], 0);
            if !seen[vertex] {
                result += weights[vertex][neighbor];
                seen[vertex] = true;
                seen[neighbor] = true;
            }
        }
        result
    }

    fn next_random(seed: &mut u64) -> u64 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        *seed
    }

    fn vertex_pairs(size: usize) -> impl Iterator<Item = (usize, usize)> {
        (0..size).flat_map(|vertex0| (0..vertex0).map(move |vertex1| (vertex0, vertex1)))
    }

    #[test]
    fn unique_maximum_matching() {
        let mut solver = fixed_solver(4);
        assert!(solver.set_edge_weight(0, 1, BigWeight::from_usize(10)));
        assert!(solver.set_edge_weight(2, 3, BigWeight::from_usize(9)));
        assert!(solver.set_edge_weight(0, 2, BigWeight::from_usize(1)));
        assert!(solver.set_edge_weight(1, 3, BigWeight::from_usize(1)));
        assert_eq!(matching_after_compute(solver), vec![1, 0, 3, 2]);
    }

    #[test]
    fn zero_weight_is_a_non_edge_and_unmatched_vertices_match_themselves() {
        let mut solver = fixed_solver(3);
        assert!(solver.set_edge_weight(0, 1, BigWeight::from_usize(5)));
        assert!(solver.set_edge_weight(1, 2, BigWeight::zero()));
        assert_eq!(matching_after_compute(solver), vec![1, 0, 2]);
    }

    #[test]
    fn repeated_updates_recompute_from_current_edges() {
        let mut solver = fixed_solver(4);
        assert!(solver.set_edge_weight(0, 1, BigWeight::from_usize(10)));
        assert!(solver.set_edge_weight(2, 3, BigWeight::from_usize(10)));
        assert!(solver.set_edge_weight(0, 2, BigWeight::from_usize(7)));
        assert!(solver.set_edge_weight(1, 3, BigWeight::from_usize(7)));
        solver.compute_matching();
        assert_eq!(
            solver.current_matching().expect("matching was computed"),
            &[1, 0, 3, 2]
        );

        assert!(solver.set_edge_weight(0, 1, BigWeight::zero()));
        solver.compute_matching();
        assert_eq!(
            solver.current_matching().expect("matching was computed"),
            &[2, 3, 0, 1]
        );
    }

    #[test]
    fn repeated_compute_without_changes_keeps_cached_state_current() {
        let mut solver = fixed_solver(2);
        assert!(!solver.solved_graph_state_is_current_for_test());
        assert!(solver.set_edge_weight(0, 1, BigWeight::from_usize(5)));

        solver.compute_matching();
        let first_matching = solver
            .current_matching()
            .expect("matching was computed")
            .to_vec();
        assert!(solver.solved_graph_state_is_current_for_test());

        solver.compute_matching();
        assert_eq!(
            solver.current_matching().expect("matching was computed"),
            first_matching
        );
        assert!(solver.solved_graph_state_is_current_for_test());
    }

    #[test]
    fn graph_generation_rollover_cannot_expose_a_stale_matching() {
        let mut solver = fixed_solver(2);
        solver.compute_matching();
        assert_eq!(solver.current_matching(), Some([0, 1].as_slice()));

        // Reproduce the otherwise impractical final mutation before the u64
        // generation counter rolls back to the cached value zero.
        solver.set_dirty_edge_count_for_test(u64::MAX);
        assert!(solver.set_edge_weight(0, 1, BigWeight::from_usize(1)));

        assert_eq!(solver.current_matching(), None);
        solver.compute_matching();
        assert_eq!(solver.current_matching(), Some([1, 0].as_slice()));
    }

    #[test]
    fn matching_is_unavailable_until_current_graph_is_solved() {
        let mut solver = fixed_solver(2);

        assert_eq!(solver.current_matching(), None);
        solver.compute_matching();
        assert_eq!(solver.current_matching(), Some([0, 1].as_slice()));

        assert!(solver.set_edge_weight(0, 1, BigWeight::from_usize(1)));

        assert_eq!(solver.current_matching(), None);
        assert_eq!(
            solver
                .last_matching_snapshot()
                .expect("last matching was computed")
                .as_slice(),
            &[0, 1]
        );
    }

    #[test]
    fn odd_blossom_contraction_case() {
        let mut solver = fixed_solver(5);
        assert!(solver.set_edge_weight(0, 1, BigWeight::from_usize(10)));
        assert!(solver.set_edge_weight(1, 2, BigWeight::from_usize(10)));
        assert!(solver.set_edge_weight(2, 0, BigWeight::from_usize(10)));
        assert!(solver.set_edge_weight(0, 3, BigWeight::from_usize(11)));
        assert!(solver.set_edge_weight(1, 4, BigWeight::from_usize(11)));
        assert_eq!(matching_after_compute(solver), vec![3, 4, 2, 0, 1]);
    }

    #[test]
    fn dynamic_weight_above_native_priority_decides_matching() {
        let mut solver = fixed_solver(4);
        let high = BigWeight::from_limbs(&[0, 1]);
        assert!(solver.set_edge_weight(0, 1, high));
        assert!(solver.set_edge_weight(2, 3, BigWeight::from_usize(1)));
        assert!(solver.set_edge_weight(0, 2, BigWeight::from_usize(100)));
        assert!(solver.set_edge_weight(1, 3, BigWeight::from_usize(100)));
        assert_eq!(matching_after_compute(solver), vec![1, 0, 3, 2]);
    }

    #[test]
    fn tie_output_is_deterministic() {
        let mut solver = fixed_solver(4);
        for vertex0 in 0..4 {
            for vertex1 in 0..vertex0 {
                assert!(solver.set_edge_weight(vertex0, vertex1, BigWeight::from_usize(1)));
            }
        }
        let first = matching_after_compute(solver.clone());
        let second = matching_after_compute(solver);
        assert_eq!(first, second);
    }

    #[test]
    fn matches_bruteforce_for_small_graphs() {
        let mut seed = 0x4d41544348494e47;
        for size in 0..=7 {
            for _ in 0..100 {
                let mut solver = fixed_solver(size);
                let mut weights = vec![vec![0; size]; size];
                for (vertex0, vertex1) in vertex_pairs(size) {
                    let weight = (next_random(&mut seed) % 17) as usize;
                    weights[vertex0][vertex1] = weight;
                    weights[vertex1][vertex0] = weight;
                    if weight != 0 {
                        assert!(solver.set_edge_weight(
                            vertex0,
                            vertex1,
                            BigWeight::from_usize(weight),
                        ));
                    }
                }

                let matching = matching_after_compute(solver);
                assert_eq!(
                    matching_weight(&matching, &weights),
                    brute_force_maximum_weight(&weights),
                );
            }
        }
    }
}
