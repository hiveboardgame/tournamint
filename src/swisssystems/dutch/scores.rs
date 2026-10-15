use crate::domain::ScoreRank;
use crate::limits::{OverflowingAdd, WrappingLimit};
use crate::swiss::PlayerRoundRecord;
use crate::swisssystems::common::player_acceleration_at_round;

use super::input::DutchInput;
use crate::swisssystems::bracketing::BracketPlayer;
use crate::swisssystems::bracketing::BracketStatus;

fn normalize_points_value(value: usize, points_limit: WrappingLimit) -> usize {
    points_limit.wrap(value)
}

// Dutch score rollback mirrors legacy bounded score arithmetic when looking
// back across previous rounds.
fn wrapping_sub_points(value: usize, subtrahend: usize, points_limit: WrappingLimit) -> usize {
    points_limit.wrapping_sub(value, subtrahend)
}

fn overflowing_add_points(
    value: usize,
    addend: usize,
    points_limit: WrappingLimit,
) -> OverflowingAdd {
    points_limit.overflowing_add(value, addend)
}

pub(super) fn match_points(
    input: &DutchInput<'_>,
    records: &[PlayerRoundRecord],
    round_index: usize,
) -> Result<usize, BracketStatus> {
    let record = records
        .get(round_index)
        .copied()
        .ok_or(BracketStatus::MalformedTournament)?;
    input
        .pairing
        .tournament
        .primary_points(record, input.pairing.point_system)
        .map(|points| points.value() as usize)
        .ok_or(BracketStatus::MalformedTournament)
}

pub(super) fn player_score_with_acceleration(
    input: &DutchInput<'_>,
    player: &BracketPlayer<'_>,
    rounds_back: usize,
) -> Result<usize, BracketStatus> {
    let records = player.records;
    let played_rounds = input.pairing.played_rounds;
    if played_rounds > records.len() || rounds_back > played_rounds {
        return Err(BracketStatus::MalformedTournament);
    }

    let points_limit = WrappingLimit::from_type_max(input.points_type_max);
    let mut score = normalize_points_value(player.score_without_acceleration, points_limit);
    let mut round_index = played_rounds;
    for _ in 0..rounds_back {
        round_index = round_index
            .checked_sub(1)
            .ok_or(BracketStatus::MalformedTournament)?;
        score = wrapping_sub_points(
            score,
            match_points(input, records, round_index)?,
            points_limit,
        );
    }

    let acceleration = player_acceleration_at_round(*player, round_index);
    let result = overflowing_add_points(score, acceleration, points_limit);
    if result.overflowed {
        Err(BracketStatus::LimitAcceleratedScore)
    } else {
        Ok(result.value)
    }
}

pub(super) fn compute_current_scores(
    input: &DutchInput<'_>,
    players: &[BracketPlayer<'_>],
) -> Result<Vec<usize>, BracketStatus> {
    let mut current_scores = Vec::with_capacity(players.len());
    for player in players {
        current_scores.push(player_score_with_acceleration(input, player, 0)?);
    }
    Ok(current_scores)
}

fn score_rank(players: &[BracketPlayer<'_>], current_scores: &[usize], index: usize) -> ScoreRank {
    ScoreRank::new(current_scores[index], players[index].rank_index)
}

pub(super) fn accelerated_score_rank_compare(
    players: &[BracketPlayer<'_>],
    current_scores: &[usize],
    player0_index: usize,
    player1_index: usize,
) -> bool {
    let player0 = score_rank(players, current_scores, player0_index);
    let player1 = score_rank(players, current_scores, player1_index);
    player0.pairs_after(player1)
}

pub(super) fn sorted_pairable_players(
    input: &DutchInput<'_>,
    players: &[BracketPlayer<'_>],
    current_scores: &[usize],
) -> Result<Vec<usize>, BracketStatus> {
    let mut sorted_players = Vec::with_capacity(players.len());
    let played_rounds = input.pairing.played_rounds;
    for (player_index, player) in players.iter().enumerate() {
        if player.is_valid && player.records.len() <= played_rounds {
            sorted_players.push(player_index);
        }
    }
    sorted_players.sort_unstable_by_key(|&player| {
        (
            score_rank(players, current_scores, player),
            players[player].id,
            player,
        )
    });
    Ok(sorted_players)
}
