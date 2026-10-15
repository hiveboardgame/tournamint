use crate::swisssystems::common::{
    BuildSwissPlayerProjectionsError, SwissPlayerProjection, build_swiss_player_projections,
    checked_accelerated_score, player_acceleration_at_round,
};

use super::errors::BursteinStatus;
use super::input::BursteinInput;

#[derive(Clone, Copy)]
pub(super) struct BursteinPlayer<'a> {
    pub(super) base: SwissPlayerProjection<'a>,
    pub(super) score_with_acceleration: usize,
}

pub(super) fn current_acceleration(
    input: &BursteinInput<'_>,
    player: SwissPlayerProjection<'_>,
) -> usize {
    player_acceleration_at_round(player, input.pairing.played_rounds)
}

pub(super) fn add_acceleration_to_score(
    score_without_acceleration: usize,
    acceleration: usize,
    max_points: usize,
) -> Result<usize, BursteinStatus> {
    checked_accelerated_score(score_without_acceleration, acceleration, max_points)
        .ok_or(BursteinStatus::LimitAcceleratedScore)
}

pub(super) fn build_players<'a>(
    input: &BursteinInput<'a>,
) -> Result<Vec<BursteinPlayer<'a>>, BursteinStatus> {
    let player_projections =
        build_swiss_player_projections(input.pairing.tournament).map_err(|error| match error {
            BuildSwissPlayerProjectionsError::MalformedTournament => {
                BursteinStatus::MalformedTournament
            }
        })?;
    let players_len = player_projections.len();
    let mut players = Vec::with_capacity(players_len);
    for player in player_projections {
        let acceleration = current_acceleration(input, player);
        let score_with_acceleration = add_acceleration_to_score(
            player.score_without_acceleration,
            acceleration,
            input.max_points.value() as usize,
        )?;
        players.push(BursteinPlayer {
            base: player,
            score_with_acceleration,
        });
    }
    Ok(players)
}
