use super::{
    DoubleSwissInput, MatchingDiagnostics, OraclePairingFailure,
    compute_matching_oracle_with_guard, compute_matching_ordered,
    compute_matching_ordered_with_diagnostics,
};
use crate::domain::{Color, ForbiddenPairEntry, PlayerColorData, PlayerId, PointSystem, Score};
use crate::swiss::{
    PairingError, PairingFailure, RoundPairings, SwissPairingPlayerEntry, SwissPairingSnapshot,
};
use crate::swisssystems::common::PairingContext;

const ORACLE_GUARD_LIMIT: usize = 250_000;

fn input(
    tournament: &SwissPairingSnapshot,
    played_rounds: usize,
    expected_rounds: usize,
) -> DoubleSwissInput<'_> {
    DoubleSwissInput {
        pairing: PairingContext {
            tournament,
            played_rounds,
            point_system: PointSystem::STANDARD,
            initial_color: Color::White,
        },
        expected_rounds,
        points_type_max: usize::MAX,
    }
}

fn canonicalize(mut round: RoundPairings) -> RoundPairings {
    round.byes.sort_unstable_by_key(|bye| bye.player.index());
    round.games.sort_unstable_by_key(|game| {
        let white = game.white().index();
        let black = game.black().index();
        (white.min(black), white.max(black))
    });
    round
}

fn ordered_result(
    tournament: &SwissPairingSnapshot,
    played_rounds: usize,
    expected_rounds: usize,
) -> Result<RoundPairings, PairingError> {
    compute_matching_ordered(input(tournament, played_rounds, expected_rounds))
        .map(canonicalize)
        .map_err(PairingFailure::pairing_error)
}

fn oracle_result(
    tournament: &SwissPairingSnapshot,
    played_rounds: usize,
    expected_rounds: usize,
    case: &str,
) -> Result<RoundPairings, PairingError> {
    match compute_matching_oracle_with_guard(
        input(tournament, played_rounds, expected_rounds),
        ORACLE_GUARD_LIMIT,
    ) {
        Ok((round, _attempts)) => Ok(canonicalize(round)),
        Err(OraclePairingFailure::GuardExceeded) => {
            panic!("exhaustive oracle exceeded its private guard in {case}")
        }
        Err(OraclePairingFailure::Pairing(error)) => Err(error),
    }
}

fn compare_case(
    tournament: &SwissPairingSnapshot,
    played_rounds: usize,
    expected_rounds: usize,
    case: &str,
) -> Result<RoundPairings, PairingError> {
    let ordered = ordered_result(tournament, played_rounds, expected_rounds);
    let oracle = oracle_result(tournament, played_rounds, expected_rounds, case);
    assert_eq!(ordered, oracle, "ordered/oracle mismatch in {case}");
    ordered
}

fn dense_rank_indices(rank_order: &[usize], player_count: usize) -> Vec<usize> {
    assert_eq!(rank_order.len(), player_count, "rank-order length");
    let mut rank_indices = vec![None; player_count];
    for (rank_index, &player_id) in rank_order.iter().enumerate() {
        assert!(player_id < player_count, "rank-order player out of range");
        assert!(
            rank_indices[player_id].replace(rank_index).is_none(),
            "duplicate player in rank order"
        );
    }
    rank_indices
        .into_iter()
        .map(|rank_index| rank_index.expect("rank order is a dense permutation"))
        .collect()
}

fn build_tournament(
    scores_by_id: &[u32],
    rank_order: &[usize],
    forbidden_edges: &[(usize, usize)],
) -> SwissPairingSnapshot {
    let player_count = scores_by_id.len();
    let rank_indices = dense_rank_indices(rank_order, player_count);
    let mut tournament = SwissPairingSnapshot::new();
    for player_id in 0..player_count {
        tournament.push_player(SwissPairingPlayerEntry {
            id: PlayerId::new(player_id),
            score_without_acceleration: Score::new(scores_by_id[player_id]),
            rank_index: rank_indices[player_id],
            played_games: 0,
            played_matches: 0,
            color_data: PlayerColorData::no_played_color_history(),
            is_valid: true,
            records: &[],
            accelerations: &[],
        });
    }

    let matrix_len = player_count
        .checked_mul(player_count)
        .expect("small differential matrix length");
    let mut seen_edges = vec![false; matrix_len];
    for &(left, right) in forbidden_edges {
        assert!(left < player_count && right < player_count);
        assert_ne!(left, right, "self-forbidden edge");
        let (low, high) = (left.min(right), left.max(right));
        let edge_index = low
            .checked_mul(player_count)
            .and_then(|offset| offset.checked_add(high))
            .expect("small differential edge index");
        assert!(!seen_edges[edge_index], "duplicate forbidden edge");
        seen_edges[edge_index] = true;
        let players = [PlayerId::new(low), PlayerId::new(high)];
        tournament.push_forbidden_pair(ForbiddenPairEntry {
            round_range: 0..1,
            players: &players,
        });
    }
    tournament
}

fn complete_edges(player_count: usize) -> Vec<(usize, usize)> {
    let edge_count = player_count
        .checked_mul(player_count.saturating_sub(1))
        .and_then(|value| value.checked_div(2))
        .expect("small complete graph edge count");
    let mut edges = Vec::with_capacity(edge_count);
    for right in 1..player_count {
        for left in 0..right {
            edges.push((left, right));
        }
    }
    assert_eq!(edges.len(), edge_count);
    edges
}

fn forbidden_edges_for_mask(edges: &[(usize, usize)], mask: usize) -> Vec<(usize, usize)> {
    assert!(edges.len() < usize::BITS as usize);
    edges
        .iter()
        .copied()
        .enumerate()
        .filter_map(|(bit, edge)| ((mask >> bit) & 1 == 1).then_some(edge))
        .collect()
}

fn deterministic_rank_orders(player_count: usize) -> Vec<Vec<usize>> {
    let mut orders = Vec::new();
    let identity = (0..player_count).collect::<Vec<_>>();
    orders.push(identity);

    let reverse = (0..player_count).rev().collect::<Vec<_>>();
    if !orders.contains(&reverse) {
        orders.push(reverse);
    }

    let mut outside_in = Vec::with_capacity(player_count);
    let mut low = 0usize;
    let mut high = player_count;
    while low < high {
        outside_in.push(low);
        low = low.checked_add(1).expect("small player count");
        if low < high {
            high = high.checked_sub(1).expect("nonempty upper half");
            outside_in.push(high);
        }
    }
    if !orders.contains(&outside_in) {
        orders.push(outside_in);
    }
    orders
}

fn scores_for_boundaries(
    player_count: usize,
    boundary_mask: usize,
    rank_order: &[usize],
) -> Vec<u32> {
    let boundary_count = player_count.saturating_sub(1);
    assert!(boundary_count < usize::BITS as usize);
    assert!(boundary_mask < (1usize << boundary_count));
    let mut score = boundary_mask
        .count_ones()
        .checked_mul(10)
        .expect("small generated score");
    let mut scores = vec![0; player_count];
    for (rank_index, &player_id) in rank_order.iter().enumerate() {
        scores[player_id] = score;
        if rank_index < boundary_count && ((boundary_mask >> rank_index) & 1 == 1) {
            score = score.checked_sub(10).expect("one score unit per boundary");
        }
    }
    assert_eq!(score, 0);
    scores
}

fn unordered_pairs(round: &RoundPairings) -> Vec<(usize, usize)> {
    let mut pairs = round
        .games
        .iter()
        .map(|game| {
            let white = game.white().index();
            let black = game.black().index();
            (white.min(black), white.max(black))
        })
        .collect::<Vec<_>>();
    pairs.sort_unstable();
    pairs
}

fn opponents_of(round: &RoundPairings, players: &[usize]) -> Vec<usize> {
    let mut opponents = Vec::new();
    for game in &round.games {
        let white = game.white().index();
        let black = game.black().index();
        if players.contains(&white) && !players.contains(&black) {
            opponents.push(black);
        } else if players.contains(&black) && !players.contains(&white) {
            opponents.push(white);
        }
    }
    opponents.sort_unstable();
    opponents
}

#[test]
fn exhaustive_flat_score_forbidden_graphs_match_the_oracle() {
    for player_count in [4usize, 5] {
        let scores = vec![0; player_count];
        let rank_order = (0..player_count).collect::<Vec<_>>();
        let edges = complete_edges(player_count);
        let mask_count = 1usize
            .checked_shl(edges.len() as u32)
            .expect("small forbidden-edge powerset");
        for mask in 0..mask_count {
            let forbidden = forbidden_edges_for_mask(&edges, mask);
            let tournament = build_tournament(&scores, &rank_order, &forbidden);
            for expected_rounds in [1usize, 2] {
                let case = format!(
                    "flat forbidden graph n={player_count} mask={mask:#x} expected={expected_rounds}"
                );
                let _ = compare_case(&tournament, 0, expected_rounds, &case);
            }
        }
    }
}

#[test]
fn exhaustive_score_group_boundaries_through_eight_match_the_oracle() {
    for player_count in 2usize..=8 {
        let boundary_count = player_count.checked_sub(1).expect("nonempty field");
        let mask_count = 1usize
            .checked_shl(boundary_count as u32)
            .expect("small boundary powerset");
        for rank_order in deterministic_rank_orders(player_count) {
            for boundary_mask in 0..mask_count {
                let scores = scores_for_boundaries(player_count, boundary_mask, &rank_order);
                let tournament = build_tournament(&scores, &rank_order, &[]);
                for expected_rounds in [1usize, 2] {
                    let case = format!(
                        "score boundaries n={player_count} mask={boundary_mask:#x} ranks={rank_order:?} expected={expected_rounds}"
                    );
                    let _ = compare_case(&tournament, 0, expected_rounds, &case);
                }
            }
        }
    }
}

#[test]
fn c5_compares_the_worst_score_difference_first() {
    // With 0-1 unavailable, two upfloaters are required. The only complete
    // choices are scores [20, 0] and [10, 10]; C5 chooses [10, 10].
    let scores = [30, 30, 20, 10, 10, 0];
    let forbidden = [(0, 1), (2, 3), (2, 4), (3, 5), (4, 5)];
    let tournament = build_tournament(&scores, &[0, 1, 2, 3, 4, 5], &forbidden);

    let round = compare_case(&tournament, 0, 2, "C5 threshold direction")
        .expect("curated C5 field has a legal pairing");
    assert_eq!(opponents_of(&round, &[0, 1]), vec![3, 4]);
}

#[test]
fn c6_does_not_retarget_after_the_immediate_group_is_emptied() {
    // C4/C5 require player 2 and one player from 3-5. Removing TPN 3 makes
    // 4-5 float in their own bracket, while removing 4 or 5 does not. Since
    // the immediate group (player 2) was emptied, C6 is skipped and TPN 3
    // wins the remaining tie instead of optimizing the next group.
    let scores = [30, 30, 20, 10, 10, 10, 0, 0];
    let forbidden = [(0, 1), (4, 5)];
    let tournament = build_tournament(&scores, &[0, 1, 2, 3, 4, 5, 6, 7], &forbidden);

    let round = compare_case(&tournament, 0, 2, "C6 emptied immediate group")
        .expect("curated C6 field has a legal pairing");
    assert_eq!(opponents_of(&round, &[0, 1]), vec![2, 3]);
}

#[test]
fn arbitrary_forbidden_failures_and_pab_stranding_match_the_oracle() {
    let no_pairing = build_tournament(&[0, 0, 0, 0], &[0, 1, 2, 3], &[(0, 1), (0, 2), (0, 3)]);
    assert_eq!(
        compare_case(&no_pairing, 0, 1, "isolated player has no legal pairing"),
        Err(PairingError::NoValidPairing)
    );

    let stranded_nominal_pab = build_tournament(
        &[0, 0, 0, 0, 0],
        &[0, 1, 2, 3, 4],
        &[(0, 2), (0, 3), (1, 2), (1, 3), (2, 3)],
    );
    let round = compare_case(
        &stranded_nominal_pab,
        0,
        1,
        "nominal PAB strands the remaining graph",
    )
    .expect("a lower-priority PAB candidate leaves a legal pairing");
    assert_eq!(round.byes.len(), 1);
    assert_eq!(round.byes[0].player, PlayerId::new(3));
}

#[test]
fn pairing_id_uses_rank_index_instead_of_player_id() {
    let rank_order = [4, 1, 5, 0, 3, 2];
    let tournament = build_tournament(&[0; 6], &rank_order, &[]);

    let round = compare_case(&tournament, 0, 1, "pairing ID with permuted player IDs")
        .expect("flat six-player field has a legal pairing");
    assert_eq!(unordered_pairs(&round), vec![(0, 4), (1, 3), (2, 5)]);
}

#[test]
fn ordered_and_oracle_are_repeatably_deterministic() {
    let scores = [30, 30, 20, 10, 10, 10, 0, 0];
    let tournament = build_tournament(
        &scores,
        &[7, 0, 5, 2, 6, 1, 4, 3],
        &[(0, 1), (4, 5), (2, 6)],
    );
    let first = compare_case(&tournament, 0, 2, "determinism baseline");
    for repetition in 1..=8 {
        let case = format!("determinism repetition {repetition}");
        assert_eq!(compare_case(&tournament, 0, 2, &case), first, "{case}");
    }
}

fn assert_complete_scaling_round(round: &RoundPairings, player_count: usize) {
    assert!(round.byes.is_empty(), "the scaling fields are even");
    assert_eq!(round.games.len(), player_count / 2);
    let mut seen = vec![false; player_count];
    for game in &round.games {
        let white = game.white().index();
        let black = game.black().index();
        assert!(white < player_count && black < player_count);
        assert_ne!(white, black);
        assert!(!seen[white], "player {white} occurs twice");
        assert!(!seen[black], "player {black} occurs twice");
        seen[white] = true;
        seen[black] = true;
    }
    assert!(seen.into_iter().all(|paired| paired));
}

fn scaling_checksum(round: &RoundPairings) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut checksum = FNV_OFFSET;
    for game in &round.games {
        for value in [game.white().index(), game.black().index()] {
            checksum ^= value as u64;
            checksum = checksum.wrapping_mul(FNV_PRIME);
        }
    }
    for bye in &round.byes {
        checksum ^= bye.player.index() as u64;
        checksum = checksum.wrapping_mul(FNV_PRIME);
    }
    checksum
}

fn linearithmic_weight_limb_ceiling(player_count: usize) -> usize {
    assert!(player_count > 0);
    let player_bits = (usize::BITS - player_count.leading_zeros()) as usize;
    // The widest ordered objective has at most one log(n)-width field per
    // player, one one-bit membership field per player, and four scalar fields.
    // Extra slack covers the leading cardinality digit and storage rounding.
    let bit_ceiling = player_count
        .checked_mul(player_bits.checked_add(2).expect("small bit width"))
        .and_then(|bits| bits.checked_add(player_bits.checked_mul(4)?))
        .and_then(|bits| bits.checked_add(16))
        .expect("release fixture dimensions fit");
    bit_ceiling
        .div_ceil(usize::BITS as usize)
        .checked_add(1)
        .expect("release fixture limb ceiling fits")
}

fn adversarial_scaling_input(player_count: usize) -> (Vec<u32>, Vec<usize>, Vec<(usize, usize)>) {
    assert_eq!(player_count % 16, 0);
    let quarter = player_count / 4;
    let group_sizes = [
        3,
        quarter.checked_add(1).expect("fixture group size fits"),
        quarter,
        player_count
            .checked_sub(quarter.checked_mul(2).expect("fixture group size fits"))
            .and_then(|remaining| remaining.checked_sub(4))
            .expect("fixture tail group exists"),
    ];
    assert_eq!(group_sizes.iter().sum::<usize>(), player_count);
    assert!(group_sizes.iter().all(|size| *size >= 3));

    let mut scores = vec![0; player_count];
    let mut forbidden = Vec::new();
    let mut group_starts = Vec::new();
    let mut begin = 0usize;
    for (group_index, group_size) in group_sizes.into_iter().enumerate() {
        group_starts.push(begin);
        let end = begin
            .checked_add(group_size)
            .expect("fixture group end fits");
        let score_group_count = u32::try_from(group_sizes.len() - group_index)
            .expect("fixture score-group count fits u32");
        let score = score_group_count
            .checked_mul(10)
            .expect("fixture score fits");
        scores[begin..end].fill(score);
        // Removing one edge defeats the complete-bracket fast path without
        // threatening existence of a perfect matching in this dense group.
        forbidden.push((begin, begin + 1));
        begin = end;
    }
    assert_eq!(begin, player_count);
    for adjacent in group_starts.windows(2) {
        forbidden.push((adjacent[0], adjacent[1]));
    }

    // Five is coprime to every power of two in this fixture, so this is a
    // deterministic dense permutation rather than player-id order.
    let rank_order = (0..player_count)
        .map(|rank| (rank * 5 + 3) % player_count)
        .collect::<Vec<_>>();
    (scores, rank_order, forbidden)
}

fn run_release_scaling_case(
    case: &str,
    tournament: &SwissPairingSnapshot,
    player_count: usize,
) -> (RoundPairings, MatchingDiagnostics) {
    use std::time::Instant;

    let started = Instant::now();
    let (round, diagnostics) = compute_matching_ordered_with_diagnostics(input(tournament, 0, 2))
        .unwrap_or_else(|failure| panic!("{case} n={player_count} failed: {failure}"));
    let elapsed = started.elapsed();
    assert_complete_scaling_round(&round, player_count);
    let weight_limb_ceiling = linearithmic_weight_limb_ceiling(player_count);
    assert!(
        diagnostics.max_weight_limb_len <= weight_limb_ceiling,
        "{case} n={player_count} rebuilt a quadratic-width objective: {diagnostics:?}, ceiling={weight_limb_ceiling}"
    );

    let (repeated_round, repeated_diagnostics) =
        compute_matching_ordered_with_diagnostics(input(tournament, 0, 2))
            .unwrap_or_else(|failure| panic!("repeated {case} n={player_count} failed: {failure}"));
    assert_eq!(repeated_round, round, "{case} n={player_count} changed");
    assert_eq!(
        repeated_diagnostics, diagnostics,
        "{case} n={player_count} work changed"
    );
    let checksum = scaling_checksum(&round);
    assert_eq!(scaling_checksum(&repeated_round), checksum);
    eprintln!(
        "Double-Swiss {case}: players={player_count} elapsed={elapsed:?} checksum={checksum:#018x} diagnostics={diagnostics:?} weight_limb_ceiling={weight_limb_ceiling}"
    );
    std::hint::black_box((&round, diagnostics));
    (round, diagnostics)
}

#[test]
#[ignore = "release-only Double-Swiss ordered-solver scaling fixture"]
fn ordered_solver_release_scaling_through_five_hundred_twelve_players() {
    assert!(
        !std::hint::black_box(cfg!(debug_assertions)),
        "run this ignored scaling fixture with cargo test --release"
    );

    for player_count in [16usize, 32, 64, 128, 256, 512] {
        let flat_scores = vec![0; player_count];
        let identity = (0..player_count).collect::<Vec<_>>();
        let flat = build_tournament(&flat_scores, &identity, &[]);
        let (_, flat_diagnostics) = run_release_scaling_case("flat", &flat, player_count);
        assert_eq!(
            flat_diagnostics.matching_calls, 0,
            "flat fast path regressed"
        );
        assert_eq!(flat_diagnostics.max_weight_limb_len, 0);

        let (scores, rank_order, forbidden) = adversarial_scaling_input(player_count);
        let adversarial = build_tournament(&scores, &rank_order, &forbidden);
        let (_, adversarial_diagnostics) =
            run_release_scaling_case("multi-score-restricted", &adversarial, player_count);
        assert!(adversarial_diagnostics.matching_calls > 0);
        assert!(adversarial_diagnostics.submitted_compatible_edges > 0);
    }
}
