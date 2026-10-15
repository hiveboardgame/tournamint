use std::cmp::Reverse;
use std::collections::HashMap;

use crate::Pairing;
use crate::matching::{BigWeight, MatchingSolver};
use crate::swiss::RoundPairings;

use super::colors::choose_dutch_player_color;
use super::forbidden::{ForbiddenPairs, build_forbidden_pairs, compatible, eligible_for_bye};
use super::input::DutchInput;
use super::scores::{compute_current_scores, sorted_pairable_players};
use super::weights::{
    DutchEdgeWeightRequest, DutchWeights, FloatHistory, build_float_history, compute_edge_weight,
};
use crate::swisssystems::bracketing::{
    BracketContext, ByeContext, InitialSolverInput, ScoreGroupShifts,
    build_initial_solver as build_bracket_initial_solver, build_pairing_result,
    build_score_group_shifts, compute_solver_matching, set_edge_weight, solve_score_groups,
    validate_initial_matching,
};
use crate::swisssystems::bracketing::{BracketPlayer, BracketStatus, build_bracket_players};
use crate::swisssystems::bracketing::{ComputeFailure, compute_failure};

struct PreparedDutchInput<'a> {
    players: Vec<BracketPlayer<'a>>,
    current_scores: Vec<usize>,
    sorted_players: Vec<usize>,
    forbidden_pairs: ForbiddenPairs,
    eligible_for_bye: Vec<bool>,
    float_history: Vec<FloatHistory>,
    score_groups: ScoreGroupShifts,
}

struct OwnedByeContext {
    bye_assignee_score: usize,
    is_single_downfloater_the_bye_assignee: bool,
    unplayed_game_ranks: HashMap<usize, usize>,
}

fn prepare_dutch_input<'a>(
    input: &DutchInput<'a>,
) -> Result<PreparedDutchInput<'a>, ComputeFailure> {
    let players = build_bracket_players(input.pairing.tournament).map_err(compute_failure)?;
    let current_scores = compute_current_scores(input, &players).map_err(compute_failure)?;
    let sorted_players =
        sorted_pairable_players(input, &players, &current_scores).map_err(compute_failure)?;
    let forbidden_pairs = if sorted_players.is_empty() {
        ForbiddenPairs::new(players.len())
    } else {
        build_forbidden_pairs(input, &players).map_err(compute_failure)?
    };
    let mut bye_eligibility = Vec::with_capacity(players.len());
    bye_eligibility.extend(
        players
            .iter()
            .map(|player| eligible_for_bye(player, input.pairing.point_system)),
    );
    let float_history = build_float_history(input, &players).map_err(compute_failure)?;
    let score_groups =
        build_score_group_shifts(&sorted_players, &current_scores).map_err(compute_failure)?;

    Ok(PreparedDutchInput {
        players,
        current_scores,
        sorted_players,
        forbidden_pairs,
        eligible_for_bye: bye_eligibility,
        float_history,
        score_groups,
    })
}

fn compute_max_edge_weight(
    prepared: &PreparedDutchInput<'_>,
    weights: &DutchWeights<'_>,
) -> Result<BigWeight, ComputeFailure> {
    let unplayed_game_ranks = HashMap::new();
    let context = weights.edge_weight_context(
        prepared.score_groups.score_group_size_bits,
        prepared.score_groups.score_groups_shift,
        &prepared.score_groups.score_group_shifts,
        false,
        &unplayed_game_ranks,
    );
    compute_edge_weight(
        &context,
        DutchEdgeWeightRequest {
            higher_player_index: prepared.sorted_players[0],
            lower_player_index: prepared.sorted_players[0],
            lower_player_in_current_bracket: true,
            lower_player_in_next_bracket: false,
            bye_assignee_score: 0,
            max: true,
        },
    )
    .map_err(compute_failure)
}

fn build_initial_solver(
    input: &DutchInput<'_>,
    prepared: &PreparedDutchInput,
    weights: &DutchWeights<'_>,
    sorted_players: &[usize],
) -> Result<MatchingSolver, ComputeFailure> {
    let unplayed_game_ranks = HashMap::new();
    let context = weights.edge_weight_context(
        prepared.score_groups.score_group_size_bits,
        prepared.score_groups.score_groups_shift,
        &prepared.score_groups.score_group_shifts,
        false,
        &unplayed_game_ranks,
    );
    build_bracket_initial_solver(
        InitialSolverInput {
            sorted_players,
            current_scores: &prepared.current_scores,
            eligible_for_bye: &prepared.eligible_for_bye,
            score_groups: &prepared.score_groups,
        },
        |player_index, opponent_index| {
            compatible(
                input,
                &prepared.players,
                &prepared.forbidden_pairs,
                player_index,
                opponent_index,
            )
        },
        |higher_player_index, lower_player_index| {
            compute_edge_weight(
                &context,
                DutchEdgeWeightRequest {
                    higher_player_index,
                    lower_player_index,
                    lower_player_in_current_bracket: false,
                    lower_player_in_next_bracket: false,
                    bye_assignee_score: 0,
                    max: false,
                },
            )
        },
    )
    .map_err(compute_failure)
}

fn matching_is_perfect(matching: &[usize]) -> bool {
    matching
        .iter()
        .enumerate()
        .all(|(vertex, &matched_vertex)| vertex != matched_vertex)
}

fn closed_score_group_ranges(
    sorted_players: &[usize],
    current_scores: &[usize],
) -> Result<Option<Vec<std::ops::Range<usize>>>, BracketStatus> {
    if sorted_players.len() & 1 != 0 {
        return Ok(None);
    }

    let mut ranges = Vec::with_capacity(sorted_players.len() / 2);
    let mut begin = 0usize;
    while begin < sorted_players.len() {
        let score = current_scores[sorted_players[begin]];
        let mut end = begin + 1;
        while end < sorted_players.len() && current_scores[sorted_players[end]] == score {
            end += 1;
        }
        if (end - begin) & 1 != 0 {
            return Ok(None);
        }
        ranges.push(begin..end);
        begin = end;
    }

    Ok((ranges.len() > 1).then_some(ranges))
}

fn try_solve_closed_score_groups(
    input: &DutchInput<'_>,
    prepared: &PreparedDutchInput<'_>,
    weights: &DutchWeights<'_>,
    max_edge_weight: &BigWeight,
    max_edge_weight_limbs: usize,
) -> Result<Option<Vec<Option<usize>>>, ComputeFailure> {
    let Some(score_group_ranges) =
        closed_score_group_ranges(&prepared.sorted_players, &prepared.current_scores)
            .map_err(compute_failure)?
    else {
        return Ok(None);
    };

    // C6 has priority over every bracket-quality criterion: when every
    // scoregroup is even and has an internal C1-C3-compatible perfect
    // matching, zero downfloaters is attainable in every bracket. C7-C9 and
    // C14-C21 are then vacuous, while C10-C13 and the Article 4 ordering are
    // local to each homogeneous bracket. C4 is preserved by certifying every
    // later scoregroup before accepting any isolated result.
    //
    // Keep the global solver as the fallback whenever even one group needs a
    // floater. Its cross-scoregroup graph is required for those cases.
    let mut certified_solvers = Vec::with_capacity(score_group_ranges.len());
    for score_group_range in &score_group_ranges {
        let sorted_players = &prepared.sorted_players[score_group_range.clone()];
        let mut solver = build_initial_solver(input, prepared, weights, sorted_players)?;
        let matching = compute_solver_matching(&mut solver).map_err(compute_failure)?;
        if !matching_is_perfect(matching) {
            return Ok(None);
        }
        certified_solvers.push((score_group_range.clone(), solver));
    }

    let empty_unplayed_game_ranks = HashMap::new();
    let mut combined_matching = vec![None; prepared.players.len()];
    for (score_group_range, mut solver) in certified_solvers {
        let sorted_players = &prepared.sorted_players[score_group_range];
        let score_group_matching = solve_score_groups(
            &mut solver,
            BracketContext {
                weights,
                players: &prepared.players,
                current_scores: &prepared.current_scores,
                sorted_players,
                score_group_size_bits: prepared.score_groups.score_group_size_bits,
                score_groups_shift: prepared.score_groups.score_groups_shift,
                score_group_shifts: &prepared.score_groups.score_group_shifts,
                max_edge_weight,
                max_edge_weight_limbs,
            },
            ByeContext {
                bye_assignee_score: 0,
                is_single_downfloater_the_bye_assignee: false,
                unplayed_game_ranks: &empty_unplayed_game_ranks,
            },
        )
        .map_err(compute_failure)?;
        if sorted_players
            .iter()
            .any(|&player_index| score_group_matching[prepared.players[player_index].id].is_none())
        {
            return Err(compute_failure(BracketStatus::MalformedTournament));
        }
        for (player_id, opponent) in score_group_matching.into_iter().enumerate() {
            if opponent.is_some() {
                combined_matching[player_id] = opponent;
            }
        }
    }

    Ok(Some(combined_matching))
}

fn build_bye_context(
    prepared: &PreparedDutchInput,
    weights: &DutchWeights<'_>,
    solver: &mut MatchingSolver,
) -> Result<OwnedByeContext, ComputeFailure> {
    let players = &prepared.players;
    let current_scores = &prepared.current_scores;
    let sorted_players = &prepared.sorted_players;
    let score_groups = &prepared.score_groups;
    let matching = compute_solver_matching(solver).map_err(compute_failure)?;
    let bye_assignee_score = validate_initial_matching(
        sorted_players,
        current_scores,
        &prepared.eligible_for_bye,
        matching,
    )
    .map_err(compute_failure)?;
    let mut is_single_downfloater_the_bye_assignee = false;
    let mut unplayed_game_ranks = HashMap::new();

    if sorted_players.len() & 1 != 0 {
        let top_score = current_scores[sorted_players[0]];
        if bye_assignee_score >= top_score {
            is_single_downfloater_the_bye_assignee = true;
            for (player_vertex, &player_index) in sorted_players.iter().enumerate() {
                if current_scores[player_index] < top_score {
                    break;
                }
                if current_scores[sorted_players[matching[player_vertex]]] < top_score {
                    is_single_downfloater_the_bye_assignee = false;
                    break;
                }
            }
        }

        let mut played_game_counts = Vec::with_capacity(sorted_players.len());
        for &player_index in sorted_players {
            if current_scores[player_index] == bye_assignee_score {
                played_game_counts.push(players[player_index].played_games);
            }
        }
        played_game_counts.sort_unstable_by(|lhs, rhs| rhs.cmp(lhs));
        unplayed_game_ranks.reserve(played_game_counts.len());
        for (rank, played_games) in played_game_counts.into_iter().enumerate() {
            unplayed_game_ranks.insert(played_games, rank);
        }

        let context = weights.edge_weight_context(
            score_groups.score_group_size_bits,
            score_groups.score_groups_shift,
            &score_groups.score_group_shifts,
            is_single_downfloater_the_bye_assignee,
            &unplayed_game_ranks,
        );
        for (player_vertex, &player_index) in sorted_players.iter().enumerate() {
            for (opponent_vertex, &opponent_index) in
                sorted_players.iter().enumerate().take(player_vertex)
            {
                let edge_weight = compute_edge_weight(
                    &context,
                    DutchEdgeWeightRequest {
                        higher_player_index: opponent_index,
                        lower_player_index: player_index,
                        lower_player_in_current_bracket: false,
                        lower_player_in_next_bracket: false,
                        bye_assignee_score,
                        max: false,
                    },
                )
                .map_err(compute_failure)?;
                set_edge_weight(solver, player_vertex, opponent_vertex, edge_weight)
                    .map_err(compute_failure)?;
            }
        }
    }

    Ok(OwnedByeContext {
        bye_assignee_score,
        is_single_downfloater_the_bye_assignee,
        unplayed_game_ranks,
    })
}

fn build_result(
    input: &DutchInput<'_>,
    prepared: &PreparedDutchInput,
    matching_by_id: &[Option<usize>],
) -> Result<RoundPairings, ComputeFailure> {
    let players = &prepared.players;
    let current_scores = &prepared.current_scores;
    let sorted_players = &prepared.sorted_players;
    let mut result = build_pairing_result(
        players,
        sorted_players,
        matching_by_id,
        |player_index, match_index| {
            choose_dutch_player_color(input, players, current_scores, player_index, match_index)
        },
    )
    .map_err(compute_failure)?;
    let mut score_and_rank_by_id = vec![(0, 0); players.len()];
    for (index, player) in players.iter().enumerate() {
        score_and_rank_by_id[player.id] = (current_scores[index], player.rank_index);
    }
    sort_games_for_publication(&mut result.games, &score_and_rank_by_id);
    Ok(result)
}

fn sort_games_for_publication(games: &mut [Pairing], score_and_rank_by_id: &[(usize, usize)]) {
    // FIDE C.04.2 Article 3.6: higher score, combined score, then the higher
    // ranked participant's initial TPN. C.04.7 includes acceleration points
    // in board ordering. Only new selector output passes through this sort.
    games.sort_by_key(|pairing| {
        let white = score_and_rank_by_id[pairing.white().index()];
        let black = score_and_rank_by_id[pairing.black().index()];
        let higher = if (Reverse(white.0), white.1) < (Reverse(black.0), black.1) {
            white
        } else {
            black
        };
        (
            Reverse(higher.0),
            Reverse(white.0 as u128 + black.0 as u128),
            higher.1,
        )
    });
}

fn compute_matching_impl_with_closed_score_groups(
    input: &DutchInput<'_>,
    use_closed_score_groups: bool,
) -> Result<RoundPairings, ComputeFailure> {
    let prepared = prepare_dutch_input(input)?;
    if prepared.sorted_players.is_empty() {
        return Ok(RoundPairings::default());
    }

    let weights = DutchWeights {
        input,
        players: &prepared.players,
        current_scores: &prepared.current_scores,
        forbidden_pairs: &prepared.forbidden_pairs,
        eligible_for_bye: &prepared.eligible_for_bye,
        float_history: &prepared.float_history,
    };
    let max_edge_weight = compute_max_edge_weight(&prepared, &weights)?;
    let max_edge_weight_limbs = max_edge_weight.limb_len();
    if use_closed_score_groups
        && let Some(matching_by_id) = try_solve_closed_score_groups(
            input,
            &prepared,
            &weights,
            &max_edge_weight,
            max_edge_weight_limbs,
        )?
    {
        return build_result(input, &prepared, &matching_by_id);
    }

    let mut solver = build_initial_solver(input, &prepared, &weights, &prepared.sorted_players)?;
    let bye_context = build_bye_context(&prepared, &weights, &mut solver)?;
    let matching_by_id = solve_score_groups(
        &mut solver,
        BracketContext {
            weights: &weights,
            players: &prepared.players,
            current_scores: &prepared.current_scores,
            sorted_players: &prepared.sorted_players,
            score_group_size_bits: prepared.score_groups.score_group_size_bits,
            score_groups_shift: prepared.score_groups.score_groups_shift,
            score_group_shifts: &prepared.score_groups.score_group_shifts,
            max_edge_weight: &max_edge_weight,
            max_edge_weight_limbs,
        },
        ByeContext {
            bye_assignee_score: bye_context.bye_assignee_score,
            is_single_downfloater_the_bye_assignee: bye_context
                .is_single_downfloater_the_bye_assignee,
            unplayed_game_ranks: &bye_context.unplayed_game_ranks,
        },
    )
    .map_err(compute_failure)?;

    build_result(input, &prepared, &matching_by_id)
}

pub(super) fn compute_matching_impl(
    input: &DutchInput<'_>,
) -> Result<RoundPairings, ComputeFailure> {
    compute_matching_impl_with_closed_score_groups(input, true)
}

#[cfg(test)]
pub(super) fn compute_matching_impl_without_closed_score_groups(
    input: &DutchInput<'_>,
) -> Result<RoundPairings, ComputeFailure> {
    compute_matching_impl_with_closed_score_groups(input, false)
}

#[cfg(test)]
pub(super) fn closed_score_group_path_is_available(
    input: &DutchInput<'_>,
) -> Result<bool, ComputeFailure> {
    let prepared = prepare_dutch_input(input)?;
    if prepared.sorted_players.is_empty() {
        return Ok(false);
    }
    let weights = DutchWeights {
        input,
        players: &prepared.players,
        current_scores: &prepared.current_scores,
        forbidden_pairs: &prepared.forbidden_pairs,
        eligible_for_bye: &prepared.eligible_for_bye,
        float_history: &prepared.float_history,
    };
    let max_edge_weight = compute_max_edge_weight(&prepared, &weights)?;
    let max_edge_weight_limbs = max_edge_weight.limb_len();

    try_solve_closed_score_groups(
        input,
        &prepared,
        &weights,
        &max_edge_weight,
        max_edge_weight_limbs,
    )
    .map(|matching| matching.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::PlayerColorData;
    use crate::swiss::{SwissPairingPlayerEntry, SwissPairingSnapshot};
    use crate::swisssystems::common::PairingContext;
    use crate::{Color, PlayerId, PointSystem, Score};

    #[test]
    fn board_order_uses_both_scores_before_the_higher_players_initial_rank() {
        let pairs = [(0, 1), (2, 3), (4, 5), (6, 7)]
            .map(|(white, black)| Pairing::new(PlayerId::new(white), PlayerId::new(black)));
        // Highest scores: 6, 6, 4, 6; sums: 6, 10, 8, 10. The final tie is
        // broken by initial rank 2 before 4, despite player IDs 7 and 2.
        let scores_and_ranks = [
            (6, 0),
            (0, 7),
            (6, 4),
            (4, 3),
            (4, 1),
            (4, 6),
            (4, 5),
            (6, 2),
        ];
        let mut games = pairs.to_vec();
        sort_games_for_publication(&mut games, &scores_and_ranks);
        assert_eq!(games, [pairs[3], pairs[1], pairs[0], pairs[2]]);
    }

    #[test]
    fn generated_board_order_includes_acceleration_points() {
        let mut snapshot = SwissPairingSnapshot::new();
        let bonus = [Score::new(4)];
        for id in 0..4 {
            snapshot.push_player(SwissPairingPlayerEntry {
                id: PlayerId::new(id),
                is_valid: true,
                rank_index: id,
                score_without_acceleration: Score::new(if id < 2 { 0 } else { 2 }),
                played_games: 0,
                played_matches: 0,
                color_data: PlayerColorData::no_played_color_history(),
                records: &[],
                accelerations: if id < 2 { &bonus } else { &[] },
            });
        }
        let input = DutchInput {
            pairing: PairingContext {
                tournament: &snapshot,
                played_rounds: 0,
                point_system: PointSystem::STANDARD,
                initial_color: Color::White,
            },
            expected_rounds: 4,
            points_type_max: crate::limits::NATIVE_USIZE_MAX,
        };
        let prepared = prepare_dutch_input(&input).unwrap();
        let round = build_result(&input, &prepared, &[Some(1), Some(0), Some(3), Some(2)]).unwrap();
        assert_eq!(
            round.games,
            [
                Pairing::new(PlayerId::new(0), PlayerId::new(1)),
                Pairing::new(PlayerId::new(2), PlayerId::new(3)),
            ],
        );
    }
}
