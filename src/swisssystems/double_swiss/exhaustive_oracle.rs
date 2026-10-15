use std::cmp::Ordering;
use std::collections::HashMap;

use crate::domain::{Pairing, PlayerId};
use crate::limits::bits_to_represent;
use crate::matching::{BigWeight, MatchingSolver};
use crate::swiss::{ByeAssignment, ByeKind, MatchDisposition, RoundPairings};
use crate::swisssystems::bracketing::{
    BracketPlayer, BracketStatus, ComputeFailure, build_bracket_players, compute_failure,
    set_edge_weight,
};
use crate::swisssystems::common::pairing_from_player_color;

use super::colors::choose_double_swiss_player_color;
use super::forbidden::{
    ForbiddenPairs, build_forbidden_pairs, compatible, eligible_for_bye, empty_forbidden_pairs,
};
use super::input::DoubleSwissInput;
use super::scores::{
    compute_current_scores, player_score_with_acceleration, sorted_pairable_players,
};

// The exhaustive solver is test-only. One guard unit is one maximum-matching
// problem; the guard bounds oracle exploration without becoming a runtime
// pairing contract.
struct OracleGuard {
    remaining_matching_attempts: usize,
    max_matching_attempts: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum OracleError {
    Pairing(BracketStatus),
    GuardExceeded,
}

impl From<BracketStatus> for OracleError {
    fn from(status: BracketStatus) -> Self {
        Self::Pairing(status)
    }
}

impl OracleGuard {
    fn new(max_matching_attempts: usize) -> Self {
        Self {
            remaining_matching_attempts: max_matching_attempts,
            max_matching_attempts,
        }
    }

    fn consume_matching_attempt(&mut self) -> Result<(), OracleError> {
        let Some(remaining) = self.remaining_matching_attempts.checked_sub(1) else {
            return Err(OracleError::GuardExceeded);
        };
        self.remaining_matching_attempts = remaining;
        Ok(())
    }

    fn consumed_matching_attempts(&self) -> usize {
        self.max_matching_attempts - self.remaining_matching_attempts
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PairingPlan {
    pairs: Vec<(usize, usize)>,
    first_bracket_upfloaters: usize,
}

struct PreparedDoubleSwissInput<'a> {
    players: Vec<BracketPlayer<'a>>,
    current_scores: Vec<usize>,
    sorted_players: Vec<usize>,
    forbidden_pairs: ForbiddenPairs,
    eligible_for_bye: Vec<bool>,
    previous_floaters: Vec<bool>,
    is_final_round: bool,
}

struct UpfloaterCandidate {
    score_profile: Vec<usize>,
    following_group_upfloaters: usize,
    previous_floater_count: usize,
    tpn_key: Vec<usize>,
    plan: PairingPlan,
}

fn copied_vec<T: Copy>(values: &[T]) -> Result<Vec<T>, BracketStatus> {
    Ok(values.to_vec())
}

fn build_player_indices_by_id(
    players: &[BracketPlayer<'_>],
) -> Result<Vec<Option<usize>>, BracketStatus> {
    let mut player_indices = Vec::with_capacity(players.len());
    player_indices.resize(players.len(), None);
    for (player_index, player) in players.iter().enumerate() {
        let index = player_indices
            .get_mut(player.id)
            .ok_or(BracketStatus::MalformedTournament)?;
        if index.is_none() {
            *index = Some(player_index);
        }
    }
    Ok(player_indices)
}

fn player_index_by_id(
    player_indices: &[Option<usize>],
    player_id: usize,
) -> Result<usize, BracketStatus> {
    player_indices
        .get(player_id)
        .copied()
        .flatten()
        .ok_or(BracketStatus::MalformedTournament)
}

fn cloned_plan(plan: &PairingPlan) -> Result<PairingPlan, BracketStatus> {
    Ok(PairingPlan {
        pairs: copied_vec(&plan.pairs)?,
        first_bracket_upfloaters: plan.first_bracket_upfloaters,
    })
}

fn is_final_round(input: &DoubleSwissInput<'_>) -> Result<bool, BracketStatus> {
    let current_round = input
        .pairing
        .played_rounds
        .checked_add(1)
        .ok_or(BracketStatus::MalformedTournament)?;
    if current_round > input.expected_rounds {
        return Err(BracketStatus::MalformedTournament);
    }
    Ok(current_round == input.expected_rounds)
}

fn build_previous_floaters(
    input: &DoubleSwissInput<'_>,
    players: &[BracketPlayer<'_>],
    final_round: bool,
) -> Result<Vec<bool>, BracketStatus> {
    let mut result = Vec::with_capacity(players.len());
    if final_round || input.pairing.played_rounds == 0 {
        result.resize(players.len(), false);
        return Ok(result);
    }

    let player_indices = build_player_indices_by_id(players)?;

    let previous_round = input
        .pairing
        .played_rounds
        .checked_sub(1)
        .ok_or(BracketStatus::MalformedTournament)?;
    for player in players {
        let Some(record) = player.records.get(previous_round).copied() else {
            result.push(false);
            continue;
        };
        let crate::swiss::PlayerRoundRecord::Match {
            opponent,
            state:
                crate::swiss::SwissMatchRecord::Complete {
                    disposition: MatchDisposition::Regular,
                    ..
                },
            ..
        } = record
        else {
            result.push(false);
            continue;
        };
        let opponent_id = opponent.index();
        let opponent_index = player_index_by_id(&player_indices, opponent_id)?;
        let opponent = players
            .get(opponent_index)
            .ok_or(BracketStatus::MalformedTournament)?;
        let player_score = player_score_with_acceleration(input, player, 1)?;
        let opponent_score = player_score_with_acceleration(input, opponent, 1)?;
        result.push(player_score != opponent_score);
    }
    Ok(result)
}

fn prepare_double_swiss_input<'a>(
    input: &DoubleSwissInput<'a>,
) -> Result<PreparedDoubleSwissInput<'a>, ComputeFailure> {
    let players = build_bracket_players(input.pairing.tournament).map_err(compute_failure)?;
    let current_scores = compute_current_scores(input, &players).map_err(compute_failure)?;
    let sorted_players =
        sorted_pairable_players(input, &players, &current_scores).map_err(compute_failure)?;
    let forbidden_pairs = if sorted_players.is_empty() {
        empty_forbidden_pairs(players.len()).map_err(compute_failure)?
    } else {
        build_forbidden_pairs(input, &players).map_err(compute_failure)?
    };
    let mut eligible_for_bye_players = Vec::with_capacity(players.len());
    eligible_for_bye_players.extend(players.iter().map(eligible_for_bye));
    let final_round = is_final_round(input).map_err(compute_failure)?;
    let previous_floaters =
        build_previous_floaters(input, &players, final_round).map_err(compute_failure)?;

    Ok(PreparedDoubleSwissInput {
        players,
        current_scores,
        sorted_players,
        forbidden_pairs,
        eligible_for_bye: eligible_for_bye_players,
        previous_floaters,
        is_final_round: final_round,
    })
}

fn perfect_legal_matching_exists(
    prepared: &PreparedDoubleSwissInput<'_>,
    players: &[usize],
    oracle_guard: &mut OracleGuard,
) -> Result<bool, OracleError> {
    if players.len() & 1 != 0 {
        return Ok(false);
    }
    oracle_guard.consume_matching_attempt()?;
    let mut solver = MatchingSolver::with_vertices(players.len());
    for right in 0..players.len() {
        for left in 0..right {
            if compatible(
                &prepared.players,
                &prepared.forbidden_pairs,
                players[left],
                players[right],
            ) {
                set_edge_weight(&mut solver, left, right, BigWeight::from_usize(1))?;
            }
        }
    }
    solver.compute_matching();
    let matching = solver
        .current_matching()
        .ok_or(BracketStatus::MalformedTournament)?;
    Ok(matching
        .iter()
        .enumerate()
        .all(|(vertex, opponent)| vertex != *opponent))
}

fn choose_bye(
    prepared: &PreparedDoubleSwissInput<'_>,
    oracle_guard: &mut OracleGuard,
) -> Result<Option<usize>, OracleError> {
    if prepared.sorted_players.len() & 1 == 0 {
        return Ok(None);
    }
    let mut candidates = Vec::with_capacity(prepared.sorted_players.len());
    candidates.extend(
        prepared
            .sorted_players
            .iter()
            .copied()
            .filter(|player| prepared.eligible_for_bye[*player]),
    );
    candidates.sort_by(|left, right| {
        prepared.current_scores[*left]
            .cmp(&prepared.current_scores[*right])
            .then_with(|| {
                prepared.players[*right]
                    .played_matches
                    .cmp(&prepared.players[*left].played_matches)
            })
            .then_with(|| {
                prepared.players[*right]
                    .rank_index
                    .cmp(&prepared.players[*left].rank_index)
            })
    });

    let mut remaining = Vec::with_capacity(
        prepared
            .sorted_players
            .len()
            .checked_sub(1)
            .ok_or(BracketStatus::MalformedTournament)?,
    );
    for candidate in candidates {
        remaining.clear();
        remaining.extend(
            prepared
                .sorted_players
                .iter()
                .copied()
                .filter(|player| *player != candidate),
        );
        if perfect_legal_matching_exists(prepared, &remaining, oracle_guard)? {
            return Ok(Some(candidate));
        }
    }
    Err(BracketStatus::NoValidPairing.into())
}

fn visit_combinations<F>(
    items: &[usize],
    count: usize,
    begin: usize,
    selected: &mut Vec<usize>,
    visitor: &mut F,
) -> Result<(), OracleError>
where
    F: FnMut(&[usize]) -> Result<(), OracleError>,
{
    if selected.len() == count {
        return visitor(selected);
    }
    let needed = count
        .checked_sub(selected.len())
        .ok_or(BracketStatus::MalformedTournament)?;
    let last_begin = items
        .len()
        .checked_sub(needed)
        .ok_or(BracketStatus::MalformedTournament)?;
    for index in begin..=last_begin {
        selected.push(items[index]);
        visit_combinations(items, count, index + 1, selected, visitor)?;
        selected.pop();
    }
    Ok(())
}

fn for_each_combination<F>(items: &[usize], count: usize, mut visitor: F) -> Result<(), OracleError>
where
    F: FnMut(&[usize]) -> Result<(), OracleError>,
{
    if count > items.len() {
        return Ok(());
    }
    let mut selected = Vec::with_capacity(count);
    visit_combinations(items, count, 0, &mut selected, &mut visitor)
}

fn shifted_bit(bit: usize) -> Result<BigWeight, BracketStatus> {
    let mut result = BigWeight::from_usize(1);
    result.shl_assign(bit);
    Ok(result)
}

fn bracket_edge_weight(
    prepared: &PreparedDoubleSwissInput<'_>,
    ordered_bracket: &[usize],
    upfloaters: &[usize],
    left: usize,
    right: usize,
) -> Result<BigWeight, BracketStatus> {
    let player_count = ordered_bracket.len();
    let bottom_bits = player_count
        .checked_mul(player_count)
        .expect("oracle player-count square fits usize");
    let c8_bits = bits_to_represent(player_count);
    let lower_width = c8_bits
        .checked_add(player_count)
        .and_then(|width| width.checked_add(bottom_bits))
        .expect("oracle weight width fits usize");

    let left_player = ordered_bracket[left];
    let right_player = ordered_bracket[right];
    let c8_cost =
        usize::from(upfloaters.contains(&left_player) && prepared.previous_floaters[right_player])
            + usize::from(
                upfloaters.contains(&right_player) && prepared.previous_floaters[left_player],
            );
    let c8_value = 2usize
        .checked_sub(c8_cost)
        .ok_or(BracketStatus::MalformedTournament)?;
    let mut result = BigWeight::from_usize(c8_value);

    result.shl_assign(player_count);
    result.add_assign(&shifted_bit(
        player_count
            .checked_sub(left)
            .and_then(|value| value.checked_sub(1))
            .ok_or(BracketStatus::MalformedTournament)?,
    )?);

    result.shl_assign(bottom_bits);
    let top_band = player_count
        .checked_sub(left)
        .and_then(|value| value.checked_sub(1))
        .and_then(|value| value.checked_mul(player_count))
        .expect("oracle top preference band fits usize");
    let bottom_preference = player_count
        .checked_sub(right)
        .and_then(|value| value.checked_sub(1))
        .ok_or(BracketStatus::MalformedTournament)?;
    result.add_assign(&shifted_bit(
        top_band
            .checked_add(bottom_preference)
            .expect("oracle preference bit fits usize"),
    )?);

    let mut cardinality = shifted_bit(lower_width)?;
    cardinality.add_assign(&result);
    Ok(cardinality)
}

fn pair_bracket(
    prepared: &PreparedDoubleSwissInput<'_>,
    bracket: &[usize],
    upfloaters: &[usize],
    oracle_guard: &mut OracleGuard,
) -> Result<Option<Vec<(usize, usize)>>, OracleError> {
    if bracket.len() & 1 != 0 {
        return Ok(None);
    }
    oracle_guard.consume_matching_attempt()?;
    let mut ordered = copied_vec(bracket)?;
    ordered.sort_by_key(|player| prepared.players[*player].rank_index);
    let mut solver = MatchingSolver::with_vertices(ordered.len());
    for right in 0..ordered.len() {
        for left in 0..right {
            if compatible(
                &prepared.players,
                &prepared.forbidden_pairs,
                ordered[left],
                ordered[right],
            ) {
                let weight = bracket_edge_weight(prepared, &ordered, upfloaters, left, right)?;
                set_edge_weight(&mut solver, left, right, weight)?;
            }
        }
    }
    solver.compute_matching();
    let matching = solver
        .current_matching()
        .ok_or(BracketStatus::MalformedTournament)?;
    if matching
        .iter()
        .enumerate()
        .any(|(vertex, opponent)| vertex == *opponent)
    {
        return Ok(None);
    }
    let mut pairs = Vec::with_capacity(ordered.len() / 2);
    for (left, right) in matching.iter().copied().enumerate() {
        if left < right {
            pairs.push((ordered[left], ordered[right]));
        }
    }
    Ok(Some(pairs))
}

fn upfloater_score_profile(
    prepared: &PreparedDoubleSwissInput<'_>,
    upfloaters: &[usize],
) -> Result<Vec<usize>, BracketStatus> {
    let mut profile = Vec::with_capacity(upfloaters.len());
    profile.extend(
        upfloaters
            .iter()
            .map(|player| prepared.current_scores[*player]),
    );
    profile.sort_unstable();
    Ok(profile)
}

fn upfloater_tpn_key(
    prepared: &PreparedDoubleSwissInput<'_>,
    upfloaters: &[usize],
) -> Result<Vec<usize>, BracketStatus> {
    let mut ordered = copied_vec(upfloaters)?;
    ordered.sort_by(|left, right| {
        prepared.current_scores[*right]
            .cmp(&prepared.current_scores[*left])
            .then_with(|| {
                prepared.players[*left]
                    .rank_index
                    .cmp(&prepared.players[*right].rank_index)
            })
    });
    let mut result = Vec::with_capacity(ordered.len());
    result.extend(
        ordered
            .into_iter()
            .map(|player| prepared.players[player].rank_index),
    );
    Ok(result)
}

fn candidate_order(left: &UpfloaterCandidate, right: &UpfloaterCandidate) -> Ordering {
    right
        .score_profile
        .cmp(&left.score_profile)
        .then_with(|| {
            left.following_group_upfloaters
                .cmp(&right.following_group_upfloaters)
        })
        .then_with(|| {
            left.previous_floater_count
                .cmp(&right.previous_floater_count)
        })
        .then_with(|| left.tpn_key.cmp(&right.tpn_key))
}

fn pair_remaining(
    prepared: &PreparedDoubleSwissInput<'_>,
    cache: &mut HashMap<Vec<usize>, Option<PairingPlan>>,
    remaining: &[usize],
    oracle_guard: &mut OracleGuard,
) -> Result<Option<PairingPlan>, OracleError> {
    if let Some(plan) = cache.get(remaining) {
        return Ok(plan.as_ref().map(cloned_plan).transpose()?);
    }
    let plan = pair_remaining_uncached(prepared, cache, remaining, oracle_guard)?;
    let key = remaining.to_vec();
    let cached_plan = plan.as_ref().map(cloned_plan).transpose()?;
    cache.insert(key, cached_plan);
    Ok(plan)
}

fn pair_remaining_uncached(
    prepared: &PreparedDoubleSwissInput<'_>,
    cache: &mut HashMap<Vec<usize>, Option<PairingPlan>>,
    remaining: &[usize],
    oracle_guard: &mut OracleGuard,
) -> Result<Option<PairingPlan>, OracleError> {
    if remaining.is_empty() {
        return Ok(Some(PairingPlan {
            pairs: Vec::new(),
            first_bracket_upfloaters: 0,
        }));
    }
    if remaining.len() & 1 != 0 {
        return Ok(None);
    }

    let resident_score = prepared.current_scores[remaining[0]];
    let resident_count = remaining
        .iter()
        .take_while(|player| prepared.current_scores[**player] == resident_score)
        .count();
    let residents = &remaining[..resident_count];
    let potential_upfloaters = &remaining[resident_count..];
    let first_count = resident_count & 1;

    for upfloater_count in (first_count..=potential_upfloaters.len()).step_by(2) {
        let mut best_candidate: Option<UpfloaterCandidate> = None;
        for_each_combination(potential_upfloaters, upfloater_count, |upfloaters| {
            let bracket_len = resident_count
                .checked_add(upfloater_count)
                .expect("oracle bracket is a subset of the remaining field");
            let mut bracket = Vec::with_capacity(bracket_len);
            bracket.extend_from_slice(residents);
            bracket.extend_from_slice(upfloaters);
            let Some(bracket_pairs) = pair_bracket(prepared, &bracket, upfloaters, oracle_guard)?
            else {
                return Ok(());
            };

            let mut lower_remaining = Vec::with_capacity(
                potential_upfloaters
                    .len()
                    .checked_sub(upfloater_count)
                    .ok_or(BracketStatus::MalformedTournament)?,
            );
            lower_remaining.extend(
                potential_upfloaters
                    .iter()
                    .copied()
                    .filter(|player| !upfloaters.contains(player)),
            );
            let Some(mut lower_plan) =
                pair_remaining(prepared, cache, &lower_remaining, oracle_guard)?
            else {
                return Ok(());
            };

            let following_group_score = potential_upfloaters
                .first()
                .map(|player| prepared.current_scores[*player]);
            let following_group_was_emptied = following_group_score.is_none_or(|score| {
                !lower_remaining
                    .iter()
                    .any(|player| prepared.current_scores[*player] == score)
            });
            let following_group_upfloaters = if following_group_was_emptied {
                0
            } else {
                lower_plan.first_bracket_upfloaters
            };
            let previous_floater_count = if prepared.is_final_round {
                0
            } else {
                upfloaters
                    .iter()
                    .filter(|player| prepared.previous_floaters[**player])
                    .count()
            };

            let mut pairs = bracket_pairs;
            pairs.reserve(lower_plan.pairs.len());
            pairs.append(&mut lower_plan.pairs);
            let candidate = UpfloaterCandidate {
                score_profile: upfloater_score_profile(prepared, upfloaters)?,
                following_group_upfloaters,
                previous_floater_count,
                tpn_key: upfloater_tpn_key(prepared, upfloaters)?,
                plan: PairingPlan {
                    pairs,
                    first_bracket_upfloaters: upfloater_count,
                },
            };
            if best_candidate
                .as_ref()
                .is_none_or(|current| candidate_order(&candidate, current) == Ordering::Less)
            {
                best_candidate = Some(candidate);
            }
            Ok(())
        })?;

        if let Some(candidate) = best_candidate {
            return Ok(Some(candidate.plan));
        }
    }
    Ok(None)
}

fn build_result(
    input: &DoubleSwissInput<'_>,
    prepared: &PreparedDoubleSwissInput<'_>,
    plan: PairingPlan,
    bye: Option<usize>,
) -> RoundPairings {
    let mut result = RoundPairings::default();
    for (player, opponent) in plan.pairs {
        let color = choose_double_swiss_player_color(
            input,
            &prepared.players,
            &prepared.current_scores,
            player,
            opponent,
        );
        let players = pairing_from_player_color(
            prepared.players[player].id,
            prepared.players[opponent].id,
            color,
        );
        result.games.push(Pairing::new(
            PlayerId::new(players.white),
            PlayerId::new(players.black),
        ));
    }
    if let Some(bye_player) = bye {
        result.byes.push(ByeAssignment {
            player: PlayerId::new(prepared.players[bye_player].id),
            kind: ByeKind::PairingAllocated,
        });
    }
    result
}

fn run_with_guard(
    input: &DoubleSwissInput<'_>,
    oracle_guard: &mut OracleGuard,
) -> Result<RoundPairings, OracleError> {
    let prepared = prepare_double_swiss_input(input)
        .map_err(|failure| OracleError::Pairing(failure.status))?;
    if prepared.sorted_players.is_empty() {
        return Ok(RoundPairings::default());
    }

    let bye = choose_bye(&prepared, oracle_guard)?;
    let mut remaining = copied_vec(&prepared.sorted_players)?;
    if let Some(bye_player) = bye {
        remaining.retain(|player| *player != bye_player);
    }
    let mut cache = HashMap::new();
    let plan = pair_remaining(&prepared, &mut cache, &remaining, oracle_guard)?
        .ok_or(BracketStatus::NoValidPairing)?;
    Ok(build_result(input, &prepared, plan, bye))
}

#[cfg(test)]
pub(super) fn compute_matching_impl_with_guard(
    input: &DoubleSwissInput<'_>,
    max_matching_attempts: usize,
) -> Result<(RoundPairings, usize), OracleError> {
    let mut oracle_guard = OracleGuard::new(max_matching_attempts);
    let result = run_with_guard(input, &mut oracle_guard)?;
    Ok((result, oracle_guard.consumed_matching_attempts()))
}

#[cfg(test)]
mod tests {
    use super::{
        OracleError, OracleGuard, build_player_indices_by_id, build_previous_floaters,
        for_each_combination, pair_bracket, prepare_double_swiss_input,
    };
    use crate::domain::{
        AdjudicatedSideResult, Color, GameSideOutcome, MatchScore, PlayerColorData, PlayerId,
        PointSystem, Score,
    };
    use crate::swiss::{
        LogicalGame, LogicalGames, MatchDisposition, PlayerRoundRecord, SwissPairingPlayerEntry,
        SwissPairingSnapshot,
    };
    use crate::swisssystems::bracketing::build_bracket_players;
    use crate::swisssystems::common::PairingContext;
    use crate::swisssystems::double_swiss::DoubleSwissInput;

    fn empty_tournament(player_count: usize) -> SwissPairingSnapshot {
        let mut tournament = SwissPairingSnapshot::new();
        for id in 0..player_count {
            tournament.push_player(SwissPairingPlayerEntry {
                id: PlayerId::new(id),
                score_without_acceleration: Score::new(0),
                rank_index: id,
                played_games: 0,
                played_matches: 0,
                color_data: PlayerColorData::no_played_color_history(),
                is_valid: true,
                records: &[],
                accelerations: &[],
            });
        }
        tournament
    }

    fn test_input(
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

    fn opponent_in_pairs(pairs: &[(usize, usize)], player: usize) -> usize {
        pairs
            .iter()
            .find_map(|(left, right)| {
                if *left == player {
                    Some(*right)
                } else if *right == player {
                    Some(*left)
                } else {
                    None
                }
            })
            .unwrap()
    }

    #[test]
    fn combination_generation_includes_the_empty_set() {
        let mut visited = Vec::new();
        for_each_combination(&[1, 2, 3], 0, |combination| {
            visited.push(combination.to_vec());
            Ok(())
        })
        .unwrap();
        assert_eq!(visited, vec![Vec::<usize>::new()]);
    }

    #[test]
    fn combination_generation_is_lexicographic() {
        let mut visited = Vec::new();
        for_each_combination(&[1, 2, 3], 2, |combination| {
            visited.push(combination.to_vec());
            Ok(())
        })
        .unwrap();
        assert_eq!(visited, vec![vec![1, 2], vec![1, 3], vec![2, 3]]);
    }

    #[test]
    fn oracle_guard_bounds_exhaustive_matching_attempts() {
        let mut guard = OracleGuard::new(1);

        assert_eq!(guard.consume_matching_attempt(), Ok(()));
        assert_eq!(guard.consumed_matching_attempts(), 1);
        assert_eq!(
            guard.consume_matching_attempt(),
            Err(OracleError::GuardExceeded)
        );
        assert_eq!(guard.consumed_matching_attempts(), 1);
    }

    #[test]
    fn dense_player_index_lookup_follows_projection_ids() {
        let mut tournament = empty_tournament(3);
        *tournament.player_mut(0).unwrap().id = PlayerId::new(2);
        *tournament.player_mut(1).unwrap().id = PlayerId::new(0);
        *tournament.player_mut(2).unwrap().id = PlayerId::new(1);
        let players = build_bracket_players(&tournament).unwrap();

        assert_eq!(
            build_player_indices_by_id(&players).unwrap(),
            vec![Some(1), Some(2), Some(0)]
        );
    }

    #[test]
    fn c8_avoids_a_previous_floaters_opponent_except_in_the_final_round() {
        let tournament = empty_tournament(4);
        let nonfinal_input = test_input(&tournament, 0, 2);
        let mut prepared = prepare_double_swiss_input(&nonfinal_input).unwrap();
        prepared.previous_floaters[1] = true;
        let mut oracle_guard = OracleGuard::new(2);
        let nonfinal_pairs = pair_bracket(&prepared, &[0, 1, 2, 3], &[3], &mut oracle_guard)
            .unwrap()
            .unwrap();
        assert_ne!(opponent_in_pairs(&nonfinal_pairs, 3), 1);

        let final_input = test_input(&tournament, 0, 1);
        let final_prepared = prepare_double_swiss_input(&final_input).unwrap();
        let final_pairs = pair_bracket(&final_prepared, &[0, 1, 2, 3], &[3], &mut oracle_guard)
            .unwrap()
            .unwrap();
        assert_eq!(opponent_in_pairs(&final_pairs, 3), 1);
        assert_eq!(oracle_guard.consumed_matching_attempts(), 2);
    }

    #[test]
    fn pab_and_full_forfeits_do_not_float_but_regular_adjudicated_split_does() {
        let full_win = adjudicated_match(
            1,
            Color::White,
            [
                AdjudicatedSideResult::ForfeitWin,
                AdjudicatedSideResult::ForfeitWin,
            ],
            MatchDisposition::ForfeitWin,
            MatchScore::Win,
        );
        let full_loss = adjudicated_match(
            0,
            Color::Black,
            [
                AdjudicatedSideResult::ForfeitLoss,
                AdjudicatedSideResult::ForfeitLoss,
            ],
            MatchDisposition::ForfeitLoss,
            MatchScore::Loss,
        );
        let pab = PlayerRoundRecord::PairingAllocatedBye;
        let split_white = adjudicated_match(
            4,
            Color::White,
            [
                AdjudicatedSideResult::ForfeitWin,
                AdjudicatedSideResult::ForfeitLoss,
            ],
            MatchDisposition::Regular,
            MatchScore::Draw,
        );
        let split_black = adjudicated_match(
            3,
            Color::Black,
            [
                AdjudicatedSideResult::ForfeitLoss,
                AdjudicatedSideResult::ForfeitWin,
            ],
            MatchDisposition::Regular,
            MatchScore::Draw,
        );
        let histories = [
            vec![full_win],
            vec![full_loss],
            vec![pab],
            vec![split_white],
            vec![split_black],
        ];
        let scores = [20, 0, 20, 20, 10];
        let mut tournament = SwissPairingSnapshot::new();
        for id in 0..histories.len() {
            tournament.push_player(SwissPairingPlayerEntry {
                id: PlayerId::new(id),
                score_without_acceleration: Score::new(scores[id]),
                rank_index: id,
                played_games: 0,
                played_matches: usize::from(id >= 3),
                color_data: PlayerColorData::no_played_color_history(),
                is_valid: true,
                records: &histories[id],
                accelerations: &[],
            });
        }
        let pairing_input = test_input(&tournament, 1, 3);
        let players = build_bracket_players(&tournament).unwrap();

        assert_eq!(
            build_previous_floaters(&pairing_input, &players, false).unwrap(),
            vec![false, false, false, true, true]
        );
    }

    fn adjudicated_match(
        opponent: usize,
        first_color: Color,
        outcomes: [AdjudicatedSideResult; 2],
        disposition: MatchDisposition,
        aggregate: MatchScore,
    ) -> PlayerRoundRecord {
        PlayerRoundRecord::Match {
            opponent: PlayerId::new(opponent),
            scheduled_first_color: first_color,
            state: crate::swiss::SwissMatchRecord::Complete {
                games: LogicalGames::double(
                    LogicalGame {
                        color: first_color,
                        outcome: GameSideOutcome::Adjudicated(outcomes[0]),
                    },
                    LogicalGame {
                        color: first_color.opposite(),
                        outcome: GameSideOutcome::Adjudicated(outcomes[1]),
                    },
                ),
                disposition,
                aggregate,
            },
        }
    }
}
