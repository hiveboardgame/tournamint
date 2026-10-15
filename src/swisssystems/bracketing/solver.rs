use crate::domain::{Color, Pairing, PlayerId};
use crate::matching::{BigWeight, MatchingSolver};
use crate::swiss::{ByeAssignment, ByeKind, RoundPairings};
use crate::swisssystems::common::pairing_from_player_color;

use super::{BracketPlayer, BracketStatus, ScoreGroupShifts, set_edge_weight};

pub(crate) struct InitialSolverInput<'a> {
    pub(crate) sorted_players: &'a [usize],
    pub(crate) current_scores: &'a [usize],
    pub(crate) eligible_for_bye: &'a [bool],
    pub(crate) score_groups: &'a ScoreGroupShifts,
}

/// Builds the provisional graph the Dutch bracket solver uses to identify the
/// bye assignee. Compatibility and the normal even-field edge weight remain
/// caller-supplied so graph construction stays independent of Dutch rules.
pub(crate) fn build_initial_solver(
    input: InitialSolverInput<'_>,
    mut compatible: impl FnMut(usize, usize) -> bool,
    mut regular_edge_weight: impl FnMut(usize, usize) -> Result<BigWeight, BracketStatus>,
) -> Result<MatchingSolver, BracketStatus> {
    let sorted_players = input.sorted_players;
    let mut solver = MatchingSolver::with_vertices(sorted_players.len());

    for player_vertex in 0..sorted_players.len() {
        let player_index = sorted_players[player_vertex];
        let player_score = input.current_scores[player_index];
        for opponent_vertex in 0..player_vertex {
            let opponent_index = sorted_players[opponent_vertex];
            let edge_weight = if sorted_players.len() & 1 != 0 {
                let mut edge_weight = BigWeight::zero();
                if compatible(player_index, opponent_index) {
                    edge_weight.or_assign_usize(
                        1 + usize::from(!input.eligible_for_bye[player_index])
                            + usize::from(!input.eligible_for_bye[opponent_index]),
                    );
                    edge_weight.shl_assign(input.score_groups.score_groups_shift);
                    edge_weight.or_assign_usize(
                        input
                            .score_groups
                            .score_group_shifts
                            .get(&player_score)
                            .copied()
                            .ok_or(BracketStatus::MalformedTournament)?
                            + input
                                .score_groups
                                .score_group_shifts
                                .get(&input.current_scores[opponent_index])
                                .copied()
                                .ok_or(BracketStatus::MalformedTournament)?,
                    );
                    edge_weight.shl_assign(input.score_groups.score_group_size_bits);
                    edge_weight.or_assign_usize(usize::from(
                        player_score >= input.current_scores[sorted_players[0]],
                    ));
                }
                edge_weight
            } else {
                regular_edge_weight(opponent_index, player_index)?
            };
            set_edge_weight(&mut solver, player_vertex, opponent_vertex, edge_weight)?;
        }
    }

    Ok(solver)
}

/// Checks that a provisional matching is perfect, or has exactly one
/// unmatched player who may receive the bye, and returns that player's score.
pub(crate) fn validate_initial_matching(
    sorted_players: &[usize],
    current_scores: &[usize],
    eligible_for_bye: &[bool],
    matching: &[usize],
) -> Result<usize, BracketStatus> {
    if matching.len() != sorted_players.len() {
        return Err(BracketStatus::MalformedTournament);
    }

    let mut bye_assignee_score = None;
    for (vertex, &matched_vertex) in matching.iter().enumerate() {
        if matched_vertex != vertex {
            continue;
        }
        let player_index = *sorted_players
            .get(vertex)
            .ok_or(BracketStatus::MalformedTournament)?;
        if bye_assignee_score.is_some()
            || !eligible_for_bye
                .get(player_index)
                .copied()
                .ok_or(BracketStatus::MalformedTournament)?
        {
            return Err(BracketStatus::NoValidPairing);
        }
        bye_assignee_score = Some(
            *current_scores
                .get(player_index)
                .ok_or(BracketStatus::MalformedTournament)?,
        );
    }

    Ok(bye_assignee_score.unwrap_or(0))
}

/// Converts a matching indexed by player id into public pairings. Colour
/// selection remains a system hook.
pub(crate) fn build_pairing_result(
    players: &[BracketPlayer<'_>],
    sorted_players: &[usize],
    matching_by_id: &[Option<usize>],
    mut player_color: impl FnMut(usize, usize) -> Color,
) -> Result<RoundPairings, BracketStatus> {
    let mut result = RoundPairings::default();
    result.games.reserve(sorted_players.len() / 2);
    result
        .byes
        .reserve(usize::from(sorted_players.len() & 1 != 0));
    for &player_index in sorted_players {
        let player = players[player_index];
        if let Some(match_index) = matching_by_id[player.id] {
            let opponent = players[match_index];
            if player.id < opponent.id {
                let color = player_color(player_index, match_index);
                let pairing_players = pairing_from_player_color(player.id, opponent.id, color);
                result.games.push(Pairing::new(
                    PlayerId::new(pairing_players.white),
                    PlayerId::new(pairing_players.black),
                ));
            }
        } else {
            result.byes.push(ByeAssignment {
                player: PlayerId::new(player.id),
                kind: ByeKind::PairingAllocated,
            });
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::validate_initial_matching;
    use crate::swisssystems::bracketing::BracketStatus;

    #[test]
    fn initial_matching_accepts_a_perfect_matching() {
        assert_eq!(
            validate_initial_matching(&[0, 1, 2, 3], &[30, 20, 10, 0], &[false; 4], &[1, 0, 3, 2],),
            Ok(0)
        );
    }

    #[test]
    fn initial_matching_maps_an_eligible_bye_through_sorted_players() {
        assert_eq!(
            validate_initial_matching(
                &[3, 1, 4],
                &[50, 40, 30, 20, 10],
                &[false, false, false, false, true],
                &[1, 0, 2],
            ),
            Ok(10)
        );
    }

    #[test]
    fn initial_matching_rejects_an_ineligible_bye() {
        assert_eq!(
            validate_initial_matching(&[2, 0, 1], &[30, 20, 10], &[true, false, false], &[1, 0, 2],),
            Err(BracketStatus::NoValidPairing)
        );
    }

    #[test]
    fn initial_matching_rejects_multiple_unmatched_players() {
        assert_eq!(
            validate_initial_matching(&[1, 0], &[20, 10], &[true, true], &[0, 1]),
            Err(BracketStatus::NoValidPairing)
        );
    }

    #[test]
    fn initial_matching_rejects_a_malformed_matching_length() {
        assert_eq!(
            validate_initial_matching(&[0, 1], &[20, 10], &[true, true], &[0]),
            Err(BracketStatus::MalformedTournament)
        );
    }

    #[test]
    fn initial_matching_rejects_out_of_bounds_player_data() {
        assert_eq!(
            validate_initial_matching(&[2], &[20, 10], &[true, true], &[0]),
            Err(BracketStatus::MalformedTournament)
        );
    }
}
