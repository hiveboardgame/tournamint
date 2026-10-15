use crate::domain::Color;
use crate::matching::BigWeight;
use crate::swisssystems::common::{choose_player_neutral_color, first_color_difference};

use super::input::DutchInput;
use super::rules::{DutchColorBitSelector, dutch_choose_player_color, dutch_color_bit_selector};
use super::scores::accelerated_score_rank_compare;
use crate::swisssystems::bracketing::{BracketPlayer, BracketStatus};

pub(super) fn insert_color_bits(
    edge_weight: &mut BigWeight,
    player: BracketPlayer<'_>,
    opponent: BracketPlayer<'_>,
    in_current_score_group: bool,
    player_count_bits: usize,
    max: bool,
) -> Result<(), BracketStatus> {
    let color_bits = if !max && in_current_score_group {
        dutch_color_bit_selector(player.color_data, opponent.color_data)
    } else {
        DutchColorBitSelector::empty()
    };

    for bit in color_bits.in_insertion_order() {
        edge_weight.shl_assign(player_count_bits);
        edge_weight.or_assign_usize(usize::from(bit));
    }
    Ok(())
}

pub(super) fn choose_dutch_player_color(
    input: &DutchInput<'_>,
    players: &[BracketPlayer<'_>],
    current_scores: &[usize],
    player_index: usize,
    opponent_index: usize,
) -> Color {
    let player = players[player_index];
    let opponent = players[opponent_index];
    let first_color_difference = first_color_difference(player.records, opponent.records);
    let neutral_color = choose_player_neutral_color(
        player.neutral_color_input(first_color_difference.player),
        opponent.neutral_color_input(first_color_difference.opponent),
    );
    dutch_choose_player_color(
        neutral_color,
        player.color_data.color_preference,
        opponent.color_data.color_preference,
        player.rank_index,
        opponent.rank_index,
        input.pairing.initial_color,
        neutral_color.is_none()
            && accelerated_score_rank_compare(
                players,
                current_scores,
                player_index,
                opponent_index,
            ),
    )
}
