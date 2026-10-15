use crate::domain::Color;
use crate::swisssystems::common::{
    choose_player_neutral_color, first_color_difference, initial_color_for_rank,
    opposite_initial_color_for_rank,
};

use super::input::BursteinInput;
use super::metrics::{BursteinMetricScores, metric_scores_less_than};
use super::player::BursteinPlayer;

pub(super) fn burstein_choose_player_color(
    neutral_color: Option<Color>,
    player_color_preference: Option<Color>,
    opponent_color_preference: Option<Color>,
    player_rank_index: usize,
    opponent_rank_index: usize,
    initial_color: Color,
    player_metric_less_than_opponent: bool,
) -> Color {
    if let Some(neutral_color) = neutral_color {
        neutral_color
    } else if let Some(player_color_preference) = player_color_preference {
        if player_metric_less_than_opponent {
            opponent_color_preference
                .map(Color::opposite)
                .expect("a neutral conflict has two concrete color preferences")
        } else {
            player_color_preference
        }
    } else if player_rank_index < opponent_rank_index {
        initial_color_for_rank(player_rank_index, initial_color)
    } else {
        opposite_initial_color_for_rank(opponent_rank_index, initial_color)
    }
}

pub(super) fn choose_burstein_player_color(
    input: &BursteinInput<'_>,
    players: &[BursteinPlayer<'_>],
    metric_scores: &[BursteinMetricScores],
    player_index: usize,
    opponent_index: usize,
) -> Color {
    let player = players[player_index];
    let opponent = players[opponent_index];
    let first_color_difference = first_color_difference(player.base.records, opponent.base.records);
    let neutral_color = choose_player_neutral_color(
        player
            .base
            .neutral_color_input(first_color_difference.player),
        opponent
            .base
            .neutral_color_input(first_color_difference.opponent),
    );
    burstein_choose_player_color(
        neutral_color,
        player.base.color_data.color_preference,
        opponent.base.color_data.color_preference,
        player.base.rank_index,
        opponent.base.rank_index,
        input.pairing.initial_color,
        neutral_color.is_none()
            && player.base.color_data.color_preference.is_some()
            && metric_scores_less_than(
                metric_scores[player.base.id],
                metric_scores[opponent.base.id],
            ),
    )
}
