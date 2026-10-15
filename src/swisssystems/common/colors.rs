use crate::domain::{Color, PlayerColorData};
use crate::swiss::PlayerRoundRecord;

pub(crate) fn color_preferences_are_compatible(
    preference0: Option<Color>,
    preference1: Option<Color>,
) -> bool {
    preference0 != preference1 || preference0.is_none() || preference1.is_none()
}

pub(crate) fn initial_color_for_rank(rank_index: usize, initial_color: Color) -> Color {
    if rank_index & 1 != 0 {
        initial_color.opposite()
    } else {
        initial_color
    }
}

pub(crate) fn opposite_initial_color_for_rank(rank_index: usize, initial_color: Color) -> Color {
    initial_color_for_rank(rank_index, initial_color).opposite()
}

#[derive(Clone, Copy)]
pub(crate) struct NeutralColorInput {
    pub(crate) color_data: PlayerColorData,
    pub(crate) first_difference_color: Option<Color>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FirstColorDifference {
    pub(crate) player: Option<Color>,
    pub(crate) opponent: Option<Color>,
}

pub(crate) fn previous_played_color(
    records: &[PlayerRoundRecord],
    cursor: &mut usize,
) -> Option<Color> {
    while *cursor > 0 {
        *cursor -= 1;
        let record = records.get(*cursor)?;
        if let PlayerRoundRecord::Match {
            scheduled_first_color,
            state,
            ..
        } = record
            && state
                .games()
                .iter()
                .any(|game| game.outcome.counts_as_played())
        {
            return Some(*scheduled_first_color);
        }
    }
    None
}

pub(crate) fn first_color_difference(
    player_records: &[PlayerRoundRecord],
    opponent_records: &[PlayerRoundRecord],
) -> FirstColorDifference {
    let mut player_cursor = player_records.len();
    let mut opponent_cursor = opponent_records.len();
    let mut player_color = previous_played_color(player_records, &mut player_cursor);
    let mut opponent_color = previous_played_color(opponent_records, &mut opponent_cursor);

    while let (Some(player_value), Some(opponent_value)) = (player_color, opponent_color) {
        if player_value != opponent_value {
            break;
        }
        player_color = previous_played_color(player_records, &mut player_cursor);
        opponent_color = previous_played_color(opponent_records, &mut opponent_cursor);
    }

    FirstColorDifference {
        player: player_color,
        opponent: opponent_color,
    }
}

pub(crate) fn choose_player_neutral_color(
    player: NeutralColorInput,
    opponent: NeutralColorInput,
) -> Option<Color> {
    let player_color = player.color_data;
    let opponent_color = opponent.color_data;
    let player_has_absolute_color_preference = player_color.has_absolute_preference();
    let opponent_has_absolute_color_preference = opponent_color.has_absolute_preference();

    if color_preferences_are_compatible(
        player_color.color_preference,
        opponent_color.color_preference,
    ) {
        if player_color.color_preference.is_some() {
            player_color.color_preference
        } else if opponent_color.color_preference.is_some() {
            opponent_color.color_preference.map(Color::opposite)
        } else {
            None
        }
    } else if player_has_absolute_color_preference
        && (player_color.color_imbalance > opponent_color.color_imbalance
            || !opponent_has_absolute_color_preference)
    {
        player_color.color_preference
    } else if opponent_has_absolute_color_preference
        && (opponent_color.color_imbalance > player_color.color_imbalance
            || !player_has_absolute_color_preference)
    {
        opponent_color.color_preference.map(Color::opposite)
    } else if player_color.strong_color_preference && !opponent_color.strong_color_preference {
        player_color.color_preference
    } else if opponent_color.strong_color_preference && !player_color.strong_color_preference {
        opponent_color.color_preference.map(Color::opposite)
    } else if player.first_difference_color.is_some() && opponent.first_difference_color.is_some() {
        opponent.first_difference_color
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn neutral_input(
        color_preference: Option<Color>,
        first_difference_color: Option<Color>,
    ) -> NeutralColorInput {
        NeutralColorInput {
            color_data: PlayerColorData {
                color_imbalance: 0,
                color_preference,
                repeated_color: None,
                strong_color_preference: false,
            },
            first_difference_color,
        }
    }

    #[test]
    fn initial_color_helpers_follow_rank_parity() {
        assert_eq!(initial_color_for_rank(0, Color::White), Color::White);
        assert_eq!(initial_color_for_rank(1, Color::White), Color::Black);
        assert_eq!(
            opposite_initial_color_for_rank(0, Color::White),
            Color::Black
        );
        assert_eq!(
            opposite_initial_color_for_rank(1, Color::White),
            Color::White
        );
    }

    #[test]
    fn neutral_color_choice_keeps_absence_separate_from_board_colors() {
        assert_eq!(
            choose_player_neutral_color(neutral_input(None, None), neutral_input(None, None)),
            None
        );
        assert_eq!(
            choose_player_neutral_color(
                neutral_input(Some(Color::White), None),
                neutral_input(None, None),
            ),
            Some(Color::White)
        );
    }

    #[test]
    fn neutral_color_choice_uses_the_opponents_first_difference_on_a_tie() {
        assert_eq!(
            choose_player_neutral_color(
                neutral_input(Some(Color::White), Some(Color::White)),
                neutral_input(Some(Color::White), Some(Color::Black)),
            ),
            Some(Color::Black)
        );
    }
}
