use crate::domain::ScoreRank;
use crate::limits::WrappingLimit;
use crate::swiss::PlayerRoundRecord;
use crate::swisssystems::common::{checked_accelerated_score, player_acceleration_at_round};

use super::input::DoubleSwissInput;
use crate::swisssystems::bracketing::BracketPlayer;
use crate::swisssystems::bracketing::BracketStatus;

pub(super) fn match_points(
    input: &DoubleSwissInput<'_>,
    _player: &BracketPlayer<'_>,
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
    input: &DoubleSwissInput<'_>,
    player: &BracketPlayer<'_>,
    rounds_back: usize,
) -> Result<usize, BracketStatus> {
    let records = player.records;
    let played_rounds = input.pairing.played_rounds;
    if played_rounds > records.len() || rounds_back > played_rounds {
        return Err(BracketStatus::MalformedTournament);
    }

    let points_limit = WrappingLimit::from_type_max(input.points_type_max);
    let mut score = points_limit
        .checked_add(player.score_without_acceleration, 0)
        .ok_or(BracketStatus::LimitAcceleratedScore)?;
    let mut round_index = played_rounds;
    for _ in 0..rounds_back {
        round_index = round_index
            .checked_sub(1)
            .ok_or(BracketStatus::MalformedTournament)?;
        score = score
            .checked_sub(match_points(input, player, records, round_index)?)
            .ok_or(BracketStatus::MalformedTournament)?;
    }

    let acceleration = player_acceleration_at_round(*player, round_index);
    checked_accelerated_score(score, acceleration, input.points_type_max)
        .ok_or(BracketStatus::LimitAcceleratedScore)
}

pub(super) fn compute_current_scores(
    input: &DoubleSwissInput<'_>,
    players: &[BracketPlayer<'_>],
) -> Result<Vec<usize>, BracketStatus> {
    let mut current_scores = Vec::with_capacity(players.len());
    for player in players {
        current_scores.push(player_score_with_acceleration(input, player, 0)?);
    }
    Ok(current_scores)
}

pub(super) fn sorted_pairable_players(
    input: &DoubleSwissInput<'_>,
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
            ScoreRank::new(current_scores[player], players[player].rank_index),
            players[player].id,
            player,
        )
    });
    Ok(sorted_players)
}
