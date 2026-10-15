use crate::domain::{Color, Pairing, PlayerId};
use crate::limits::bits_to_represent;
use crate::matching::{BigWeight, MatchingError, MatchingSolver};
use crate::swisssystems::common::{ForbiddenPairMatrix, pairing_from_player_color};

use super::bye::choose_bye;
use super::colors::choose_burstein_player_color;
use super::errors::{
    BursteinComputeFailure, BursteinComputeResult, BursteinStatus, compute_failure,
};
use super::forbidden::build_forbidden_pairs;
use super::input::BursteinInput;
use super::metrics::{
    BursteinMetricScores, compute_metric_scores, metric_scores_order, sorted_pairable_players,
};
use super::player::{BursteinPlayer, build_players};
use super::scores::compute_adjusted_scores;
use super::weights::{
    burstein_pair_grants_both_color_preferences, burstein_players_are_compatible,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct OriginalScoreGroup {
    begin: usize,
    end: usize,
}

fn append_field(
    weight: &mut BigWeight,
    matching_sum_bound: usize,
    edge_digit: usize,
) -> Result<(), BursteinStatus> {
    if edge_digit > matching_sum_bound {
        return Err(BursteinStatus::MalformedTournament);
    }
    weight.shl_assign(bits_to_represent(matching_sum_bound));
    weight.add_assign_usize(edge_digit);
    Ok(())
}

fn set_big_edge_weight(
    solver: &mut MatchingSolver,
    left: usize,
    right: usize,
    weight: BigWeight,
) -> Result<(), BursteinStatus> {
    match solver.set_big_edge_weight(left, right, weight) {
        Ok(()) => Ok(()),
        Err(MatchingError::InvalidEdge) => Err(BursteinStatus::MalformedTournament),
    }
}

fn build_original_score_groups(
    players: &[BursteinPlayer<'_>],
    vertex_labels: &[usize],
) -> Result<Vec<OriginalScoreGroup>, BursteinStatus> {
    let mut groups = Vec::with_capacity(vertex_labels.len());

    let mut begin = 0usize;
    while begin < vertex_labels.len() {
        let first_player = *vertex_labels
            .get(begin)
            .ok_or(BursteinStatus::MalformedTournament)?;
        let score = players
            .get(first_player)
            .ok_or(BursteinStatus::MalformedTournament)?
            .score_with_acceleration;
        let mut end = begin
            .checked_add(1)
            .ok_or(BursteinStatus::MalformedTournament)?;
        while end < vertex_labels.len()
            && players
                .get(vertex_labels[end])
                .ok_or(BursteinStatus::MalformedTournament)?
                .score_with_acceleration
                == score
        {
            end = end
                .checked_add(1)
                .ok_or(BursteinStatus::MalformedTournament)?;
        }
        groups.push(OriginalScoreGroup { begin, end });
        begin = end;
    }

    Ok(groups)
}

fn distinct_descending_scores(
    players: &[BursteinPlayer<'_>],
    vertex_labels: &[usize],
    remaining: &[usize],
    end: usize,
) -> Result<Vec<usize>, BursteinStatus> {
    if end > remaining.len() {
        return Err(BursteinStatus::MalformedTournament);
    }
    let mut scores = Vec::with_capacity(end);
    for &vertex in &remaining[..end] {
        let player = *vertex_labels
            .get(vertex)
            .ok_or(BursteinStatus::MalformedTournament)?;
        scores.push(
            players
                .get(player)
                .ok_or(BursteinStatus::MalformedTournament)?
                .score_with_acceleration,
        );
    }
    scores.sort_unstable_by(|left, right| right.cmp(left));
    scores.dedup();
    Ok(scores)
}

fn bracket_bsn_by_local_vertex(
    players: &[BursteinPlayer<'_>],
    metric_scores: &[BursteinMetricScores],
    vertex_labels: &[usize],
    remaining: &[usize],
    current_end: usize,
) -> Result<(Vec<usize>, Vec<usize>), BursteinStatus> {
    if current_end > remaining.len() {
        return Err(BursteinStatus::MalformedTournament);
    }
    let mut bracket_order = Vec::with_capacity(current_end);
    bracket_order.extend(0..current_end);
    bracket_order.sort_unstable_by(|&left, &right| {
        let left_player = vertex_labels[remaining[left]];
        let right_player = vertex_labels[remaining[right]];
        metric_scores_order(
            metric_scores[players[right_player].base.id],
            metric_scores[players[left_player].base.id],
        )
        .then_with(|| {
            players[left_player]
                .base
                .id
                .cmp(&players[right_player].base.id)
        })
    });

    let mut bsn_by_local = vec![0; current_end];
    for (position, &local_vertex) in bracket_order.iter().enumerate() {
        bsn_by_local[local_vertex] = position
            .checked_add(1)
            .ok_or(BursteinStatus::MalformedTournament)?;
    }
    Ok((bracket_order, bsn_by_local))
}

struct BracketSelectionContext<'a, 'input> {
    players: &'a [BursteinPlayer<'input>],
    forbidden_pairs: &'a ForbiddenPairMatrix,
    vertex_labels: &'a [usize],
    remaining: &'a [usize],
    current_end: usize,
    following_end: Option<usize>,
    current_scores: &'a [usize],
    following_scores: &'a [usize],
    bsn_by_local: &'a [usize],
}

fn player_at(
    context: &BracketSelectionContext<'_, '_>,
    local_vertex: usize,
) -> Result<usize, BursteinStatus> {
    let original_vertex = *context
        .remaining
        .get(local_vertex)
        .ok_or(BursteinStatus::MalformedTournament)?;
    context
        .vertex_labels
        .get(original_vertex)
        .copied()
        .ok_or(BursteinStatus::MalformedTournament)
}

fn retained_score_digit(
    context: &BracketSelectionContext<'_, '_>,
    left: usize,
    right: usize,
    bracket_end: usize,
    score: usize,
) -> Result<usize, BursteinStatus> {
    if right >= bracket_end {
        return Ok(0);
    }
    let left_player = player_at(context, left)?;
    let right_player = player_at(context, right)?;
    Ok(
        usize::from(context.players[left_player].score_with_acceleration == score)
            + usize::from(context.players[right_player].score_with_acceleration == score),
    )
}

fn selection_edge_weight(
    context: &BracketSelectionContext<'_, '_>,
    left: usize,
    right: usize,
) -> Result<BigWeight, BursteinStatus> {
    let current_internal = right < context.current_end;

    // The leading one first requires a maximum-cardinality matching. A
    // perfect matching is C4's witness that every choice made for this
    // bracket leaves a completable round-pairing.
    let mut weight = BigWeight::from_usize(1);

    // C5: maximize pairs made in the current bracket.
    append_field(
        &mut weight,
        context.current_end / 2,
        usize::from(current_internal),
    )?;

    // C6: with the outgoing count fixed by C5, retaining as many players as
    // possible at each descending score minimizes the sorted score vector of
    // all outgoing floaters.
    for &score in context.current_scores {
        append_field(
            &mut weight,
            context.current_end,
            retained_score_digit(context, left, right, context.current_end, score)?,
        )?;
    }

    // C7 looks ahead exactly one bracket and applies its C5 and C6. The
    // current bracket's internal-edge and per-score contributions are already
    // fixed by the preceding fields, so counting edges/endpoints in B + Gnext
    // differs from the actual next bracket only by those fixed constants.
    if let Some(following_end) = context.following_end {
        let following_internal = right < following_end;
        append_field(
            &mut weight,
            following_end / 2,
            usize::from(following_internal),
        )?;
        for &score in context.following_scores {
            append_field(
                &mut weight,
                following_end,
                retained_score_digit(context, left, right, following_end, score)?,
            )?;
        }
    }

    // C8: the number of current pairs is already fixed. An opposite or absent
    // preference grants every preference in the pair; equal non-absolute
    // preferences necessarily deny one player.
    let c8_digit = if current_internal
        && burstein_pair_grants_both_color_preferences(
            context.players,
            player_at(context, left)?,
            player_at(context, right)?,
        ) {
        1
    } else {
        0
    };
    append_field(&mut weight, context.current_end / 2, c8_digit)?;

    // Reserve one low field for the currently evaluated Article 4 BSN. The
    // final lexicographic ordering is applied incrementally below, so every
    // blossom solve carries only this one small field instead of an O(n)-limb
    // opponent vector on every edge.
    append_field(&mut weight, context.current_end, 0)?;

    Ok(weight)
}

fn isolate_internal_pair(
    solver: &mut MatchingSolver,
    left: usize,
    right: usize,
) -> Result<(), BursteinStatus> {
    for neighbor in 0..solver.size() {
        if neighbor != left && neighbor != right {
            set_big_edge_weight(solver, left, neighbor, BigWeight::zero())?;
            set_big_edge_weight(solver, right, neighbor, BigWeight::zero())?;
        }
    }
    solver.preserve_current_matching_for_isolated_pair(left, right);
    Ok(())
}

fn lock_article_four_floater(
    solver: &mut MatchingSolver,
    candidate: usize,
    current_end: usize,
) -> Result<(), BursteinStatus> {
    for neighbor in 0..current_end {
        if neighbor != candidate {
            set_big_edge_weight(solver, candidate, neighbor, BigWeight::zero())?;
        }
    }
    Ok(())
}

fn apply_article_four_order(
    solver: &mut MatchingSolver,
    context: &BracketSelectionContext<'_, '_>,
    bracket_order: &[usize],
) -> Result<(), BursteinStatus> {
    let mut locked = Vec::with_capacity(context.current_end);
    locked.resize(context.current_end, false);

    // Article 4 compares BSN #1's opponent, then BSN #2's, and so on. Once a
    // player's best value is found among all higher-priority optima, fixing
    // that pair (or fixing virtual opponent zero) lets the next solve reuse a
    // single low field without weakening C5-C8.
    for &candidate in bracket_order {
        if candidate >= context.current_end {
            return Err(BursteinStatus::MalformedTournament);
        }
        if locked[candidate] {
            continue;
        }

        let candidate_player = player_at(context, candidate)?;
        for (neighbor, &neighbor_locked) in locked.iter().enumerate() {
            if neighbor == candidate || neighbor_locked {
                continue;
            }
            let neighbor_player = player_at(context, neighbor)?;
            if !burstein_players_are_compatible(
                context.players,
                context.forbidden_pairs,
                candidate_player,
                neighbor_player,
            ) {
                continue;
            }
            let mut weight =
                selection_edge_weight(context, candidate.min(neighbor), candidate.max(neighbor))?;
            weight.add_assign_usize(
                *context
                    .bsn_by_local
                    .get(neighbor)
                    .ok_or(BursteinStatus::MalformedTournament)?,
            );
            set_big_edge_weight(solver, candidate, neighbor, weight)?;
        }

        solver.compute_matching();
        let matching = solver
            .current_matching()
            .ok_or(BursteinStatus::MalformedTournament)?;
        if matching
            .iter()
            .enumerate()
            .any(|(vertex, opponent)| vertex == *opponent)
        {
            return Err(BursteinStatus::NoMatching);
        }
        let opponent = *matching
            .get(candidate)
            .ok_or(BursteinStatus::MalformedTournament)?;
        if opponent == candidate {
            return Err(BursteinStatus::NoMatching);
        }
        locked[candidate] = true;
        if opponent < context.current_end {
            if locked[opponent] {
                return Err(BursteinStatus::MalformedTournament);
            }
            locked[opponent] = true;
            isolate_internal_pair(solver, candidate, opponent)?;
        } else {
            lock_article_four_floater(solver, candidate, context.current_end)?;
        }
    }
    Ok(())
}

fn select_bracket_completion(
    players: &[BursteinPlayer<'_>],
    metric_scores: &[BursteinMetricScores],
    vertex_labels: &[usize],
    forbidden_pairs: &ForbiddenPairMatrix,
    remaining: &[usize],
    current_end: usize,
    following_end: Option<usize>,
) -> Result<(Vec<usize>, Vec<usize>), BursteinStatus> {
    if remaining.len() & 1 != 0
        || current_end > remaining.len()
        || following_end.is_some_and(|end| end < current_end || end > remaining.len())
    {
        return Err(BursteinStatus::MalformedTournament);
    }

    let current_scores =
        distinct_descending_scores(players, vertex_labels, remaining, current_end)?;
    let following_scores = if let Some(end) = following_end {
        distinct_descending_scores(players, vertex_labels, remaining, end)?
    } else {
        Vec::new()
    };
    let (bracket_order, bsn_by_local) = bracket_bsn_by_local_vertex(
        players,
        metric_scores,
        vertex_labels,
        remaining,
        current_end,
    )?;
    let context = BracketSelectionContext {
        players,
        forbidden_pairs,
        vertex_labels,
        remaining,
        current_end,
        following_end,
        current_scores: &current_scores,
        following_scores: &following_scores,
        bsn_by_local: &bsn_by_local,
    };

    let mut solver = MatchingSolver::with_vertices(remaining.len());
    for right in 0..remaining.len() {
        for left in 0..right {
            let left_player = player_at(&context, left)?;
            let right_player = player_at(&context, right)?;
            if burstein_players_are_compatible(players, forbidden_pairs, left_player, right_player)
            {
                set_big_edge_weight(
                    &mut solver,
                    left,
                    right,
                    selection_edge_weight(&context, left, right)?,
                )?;
            }
        }
    }
    apply_article_four_order(&mut solver, &context, &bracket_order)?;
    solver.compute_matching();
    let matching = solver
        .current_matching()
        .ok_or(BursteinStatus::MalformedTournament)?;
    if matching
        .iter()
        .enumerate()
        .any(|(vertex, opponent)| vertex == *opponent)
    {
        return Err(BursteinStatus::NoMatching);
    }
    let mut copied_matching = Vec::with_capacity(matching.len());
    copied_matching.extend_from_slice(matching);
    Ok((copied_matching, bracket_order))
}

fn append_pair(
    result: &mut BursteinComputeResult,
    player0: usize,
    player1: usize,
    player0_color: Color,
) {
    let pairing_players = pairing_from_player_color(player0, player1, player0_color);
    result.games.push(Pairing::new(
        PlayerId::new(pairing_players.white),
        PlayerId::new(pairing_players.black),
    ));
}

struct FreezeBracketContext<'a, 'input> {
    input: &'a BursteinInput<'input>,
    players: &'a [BursteinPlayer<'input>],
    metric_scores: &'a [BursteinMetricScores],
    vertex_labels: &'a [usize],
}

fn freeze_current_bracket(
    context: &FreezeBracketContext<'_, '_>,
    remaining: Vec<usize>,
    current_end: usize,
    matching: &[usize],
    bracket_order: &[usize],
    result: &mut BursteinComputeResult,
) -> Result<Vec<usize>, BursteinStatus> {
    if matching.len() != remaining.len()
        || bracket_order.len() != current_end
        || current_end > remaining.len()
    {
        return Err(BursteinStatus::MalformedTournament);
    }

    let mut remove = Vec::with_capacity(remaining.len());
    remove.resize(remaining.len(), false);

    for &local_vertex in bracket_order {
        if local_vertex >= current_end || remove[local_vertex] {
            continue;
        }
        let opponent = *matching
            .get(local_vertex)
            .ok_or(BursteinStatus::MalformedTournament)?;
        if opponent >= current_end {
            continue;
        }
        if opponent == local_vertex || remove[opponent] {
            return Err(BursteinStatus::MalformedTournament);
        }
        remove[local_vertex] = true;
        remove[opponent] = true;

        let player0 = *context
            .vertex_labels
            .get(remaining[local_vertex])
            .ok_or(BursteinStatus::MalformedTournament)?;
        let player1 = *context
            .vertex_labels
            .get(remaining[opponent])
            .ok_or(BursteinStatus::MalformedTournament)?;
        let player0_color = choose_burstein_player_color(
            context.input,
            context.players,
            context.metric_scores,
            player0,
            player1,
        );
        append_pair(
            result,
            context.players[player0].base.id,
            context.players[player1].base.id,
            player0_color,
        );
    }

    let mut next_remaining = Vec::with_capacity(remaining.len());
    for (local_vertex, original_vertex) in remaining.into_iter().enumerate() {
        if !remove[local_vertex] {
            next_remaining.push(original_vertex);
        }
    }
    Ok(next_remaining)
}

fn pair_original_score_groups(
    input: &BursteinInput<'_>,
    players: &[BursteinPlayer<'_>],
    metric_scores: &[BursteinMetricScores],
    vertex_labels: &[usize],
    forbidden_pairs: &ForbiddenPairMatrix,
    score_groups: &[OriginalScoreGroup],
    result: &mut BursteinComputeResult,
) -> Result<(), BursteinStatus> {
    let mut remaining = Vec::with_capacity(vertex_labels.len());
    remaining.extend(0..vertex_labels.len());
    let freeze_context = FreezeBracketContext {
        input,
        players,
        metric_scores,
        vertex_labels,
    };

    for (group_index, group) in score_groups.iter().enumerate() {
        if group.begin >= group.end || group.end > vertex_labels.len() {
            return Err(BursteinStatus::MalformedTournament);
        }
        let current_end = remaining.partition_point(|vertex| *vertex < group.end);
        let following_end = score_groups
            .get(group_index + 1)
            .map(|following| remaining.partition_point(|vertex| *vertex < following.end));
        let (matching, bracket_order) = select_bracket_completion(
            players,
            metric_scores,
            vertex_labels,
            forbidden_pairs,
            &remaining,
            current_end,
            following_end,
        )?;
        remaining = freeze_current_bracket(
            &freeze_context,
            remaining,
            current_end,
            &matching,
            &bracket_order,
            result,
        )?;
    }

    if !remaining.is_empty()
        || result.games.len()
            != vertex_labels
                .len()
                .checked_div(2)
                .ok_or(BursteinStatus::MalformedTournament)?
    {
        return Err(BursteinStatus::MalformedTournament);
    }
    Ok(())
}

pub(super) fn burstein_compute_matching(
    input: &BursteinInput<'_>,
) -> Result<BursteinComputeResult, BursteinComputeFailure> {
    let players = build_players(input).map_err(compute_failure)?;
    let forbidden_pairs = build_forbidden_pairs(input, &players).map_err(compute_failure)?;
    let adjusted_scores = compute_adjusted_scores(input, &players).map_err(compute_failure)?;
    let metric_scores =
        compute_metric_scores(input, &players, &adjusted_scores).map_err(compute_failure)?;
    let mut sorted_players =
        sorted_pairable_players(input, &players, &metric_scores).map_err(compute_failure)?;

    let mut result = BursteinComputeResult::default();
    result
        .byes
        .reserve_exact(usize::from(sorted_players.len() & 1 != 0));
    result.games.reserve_exact(sorted_players.len() / 2);
    let bye_player = choose_bye(
        input,
        &players,
        &metric_scores,
        &forbidden_pairs,
        &mut sorted_players,
    )
    .map_err(compute_failure)?;
    if let Some(bye_player) = bye_player {
        result.byes.push(crate::swiss::ByeAssignment {
            player: PlayerId::new(players[bye_player].base.id),
            kind: crate::swiss::ByeKind::PairingAllocated,
        });
    } else if sorted_players.len() & 1 != 0 {
        return Err(compute_failure(BursteinStatus::NoBye));
    }

    let vertex_labels = sorted_players;
    let score_groups =
        build_original_score_groups(&players, &vertex_labels).map_err(compute_failure)?;
    pair_original_score_groups(
        input,
        &players,
        &metric_scores,
        &vertex_labels,
        &forbidden_pairs,
        &score_groups,
        &mut result,
    )
    .map_err(compute_failure)?;

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Color, PlayerColorData};
    use crate::swisssystems::common::SwissPlayerProjection;

    fn test_players(
        scores: &[usize],
        case_index: usize,
    ) -> (Vec<BursteinPlayer<'static>>, Vec<BursteinMetricScores>) {
        let colors = [
            Some(Color::White),
            Some(Color::White),
            Some(Color::Black),
            Some(Color::Black),
            None,
            Some(Color::White),
            Some(Color::Black),
            None,
        ];
        let mut players = Vec::new();
        let mut metrics = Vec::new();
        for (id, &score) in scores.iter().enumerate() {
            players.push(BursteinPlayer {
                base: SwissPlayerProjection {
                    id,
                    rank_index: (id * 5 + case_index) % scores.len(),
                    score_without_acceleration: score,
                    played_games: 0,
                    played_matches: 0,
                    color_data: PlayerColorData {
                        color_imbalance: 0,
                        color_preference: colors[id],
                        repeated_color: None,
                        strong_color_preference: false,
                    },
                    is_valid: true,
                    records: &[],
                    accelerations: &[],
                },
                score_with_acceleration: score,
            });
            metrics.push(BursteinMetricScores {
                buchholz: (id * 3 + case_index) % 5,
                sonneborn_berger: (id * 7 + case_index / 3) % 6,
                rank_index: (id * 5 + case_index) % scores.len(),
            });
        }
        (players, metrics)
    }

    fn matching_key(
        players: &[BursteinPlayer<'_>],
        matching: &[usize],
        bracket_order: &[usize],
        current_end: usize,
        following_end: Option<usize>,
    ) -> Vec<usize> {
        let mut key = Vec::new();
        let current_scores = {
            let mut scores = players[..current_end]
                .iter()
                .map(|player| player.score_with_acceleration)
                .collect::<Vec<_>>();
            scores.sort_unstable_by(|left, right| right.cmp(left));
            scores.dedup();
            scores
        };
        key.push(
            (0..current_end)
                .filter(|&vertex| matching[vertex] < current_end && vertex < matching[vertex])
                .count(),
        );
        for score in current_scores {
            key.push(
                (0..current_end)
                    .filter(|&vertex| {
                        matching[vertex] < current_end
                            && players[vertex].score_with_acceleration == score
                    })
                    .count(),
            );
        }

        if let Some(end) = following_end {
            let mut scores = players[..end]
                .iter()
                .map(|player| player.score_with_acceleration)
                .collect::<Vec<_>>();
            scores.sort_unstable_by(|left, right| right.cmp(left));
            scores.dedup();
            key.push(
                (0..end)
                    .filter(|&vertex| matching[vertex] < end && vertex < matching[vertex])
                    .count(),
            );
            for score in scores {
                key.push(
                    (0..end)
                        .filter(|&vertex| {
                            matching[vertex] < end
                                && players[vertex].score_with_acceleration == score
                        })
                        .count(),
                );
            }
        }

        key.push(
            (0..current_end)
                .filter(|&vertex| {
                    let opponent = matching[vertex];
                    vertex < opponent
                        && opponent < current_end
                        && burstein_pair_grants_both_color_preferences(players, vertex, opponent)
                })
                .count(),
        );
        let mut bsn_by_vertex = vec![0usize; current_end];
        for (position, &vertex) in bracket_order.iter().enumerate() {
            bsn_by_vertex[vertex] = position + 1;
        }
        for &vertex in bracket_order {
            let opponent = matching[vertex];
            key.push(if opponent < current_end {
                bsn_by_vertex[opponent]
            } else {
                0
            });
        }
        key
    }

    fn visit_perfect_matchings(
        compatible: &[Vec<bool>],
        matching: &mut [usize],
        visit: &mut impl FnMut(&[usize]),
    ) {
        let Some(left) = matching.iter().position(|opponent| *opponent == usize::MAX) else {
            visit(matching);
            return;
        };
        for right in left + 1..matching.len() {
            if matching[right] == usize::MAX && compatible[left][right] {
                matching[left] = right;
                matching[right] = left;
                visit_perfect_matchings(compatible, matching, visit);
                matching[left] = usize::MAX;
                matching[right] = usize::MAX;
            }
        }
    }

    #[test]
    fn cumulative_c5_to_article_four_order_matches_a_small_exhaustive_oracle() {
        let profiles: [([usize; 8], usize, Option<usize>); 4] = [
            ([30, 30, 20, 20, 10, 10, 0, 0], 2, Some(4)),
            ([30, 20, 20, 10, 10, 0, 0, 0], 3, Some(5)),
            ([30, 20, 10, 10, 10, 0, 0, 0], 5, Some(8)),
            ([30, 20, 20, 10, 10, 0, 0, 0], 8, None),
        ];

        for case_index in 0..256usize {
            let (scores, current_end, following_end) = profiles[case_index % profiles.len()];
            let (players, metric_scores) = test_players(&scores, case_index);
            let vertex_labels = (0..scores.len()).collect::<Vec<_>>();
            let remaining = vertex_labels.clone();
            let mut forbidden_pairs = ForbiddenPairMatrix::new(scores.len());
            let mut compatible = vec![vec![false; scores.len()]; scores.len()];
            let mut random = (case_index as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15);
            for (left, neighbors) in compatible.iter_mut().enumerate() {
                for (right, allowed_cell) in neighbors.iter_mut().enumerate().skip(left + 1) {
                    random = random
                        .wrapping_mul(6_364_136_223_846_793_005)
                        .wrapping_add(1_442_695_040_888_963_407);
                    // Retain one known perfect matching while varying every
                    // other edge, so the oracle always has a C4 completion.
                    let allowed = right == left + 1 && left & 1 == 0 || random >> 63 == 0;
                    *allowed_cell = allowed;
                    if !allowed {
                        assert!(forbidden_pairs.insert(left, right));
                        assert!(forbidden_pairs.insert(right, left));
                    }
                }
            }

            let (selected, bracket_order) = select_bracket_completion(
                &players,
                &metric_scores,
                &vertex_labels,
                &forbidden_pairs,
                &remaining,
                current_end,
                following_end,
            )
            .unwrap();
            let selected_key = matching_key(
                &players,
                &selected,
                &bracket_order,
                current_end,
                following_end,
            );

            let mut oracle_key = None;
            let mut candidate = vec![usize::MAX; scores.len()];
            visit_perfect_matchings(&compatible, &mut candidate, &mut |matching| {
                let key = matching_key(
                    &players,
                    matching,
                    &bracket_order,
                    current_end,
                    following_end,
                );
                if oracle_key.as_ref().is_none_or(|best| key > *best) {
                    oracle_key = Some(key);
                }
            });

            assert_eq!(selected_key, oracle_key.unwrap(), "case {case_index}");
        }
    }
}
