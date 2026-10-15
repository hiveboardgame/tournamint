use crate::domain::{Color, PlayerColorData, PointSystem, Score};
use crate::swisssystems::bracketing::FloatDirection;
use crate::swisssystems::common::{
    color_preferences_are_compatible, initial_color_for_rank, opposite_initial_color_for_rank,
};

pub(crate) enum DutchCompatibilityKind {
    Incompatible,
    Compatible,
}

pub(crate) struct DutchCompatibilityInput {
    pub(crate) forbidden_pair: bool,
    pub(crate) player0: PlayerColorData,
    pub(crate) player1: PlayerColorData,
    pub(crate) played_rounds: usize,
    pub(crate) expected_rounds: usize,
    pub(crate) point_system: PointSystem,
    pub(crate) scores: DutchCompatibilityScores,
}

pub(crate) struct DutchCompatibilityScores {
    pub(crate) player0_real: Score,
    pub(crate) player1_real: Score,
}

fn dutch_compatibility_top_score(
    player0_real_score: Score,
    player1_real_score: Score,
    top_score_threshold: Score,
) -> bool {
    player0_real_score > top_score_threshold || player1_real_score > top_score_threshold
}

fn dutch_top_score_threshold(played_rounds: usize, point_system: PointSystem) -> Option<Score> {
    u64::try_from(played_rounds)
        .ok()?
        .checked_mul(u64::from(point_system.win.max(point_system.draw).value()))
        .and_then(|value| u32::try_from(value >> 1).ok())
        .map(Score::new)
}

pub(crate) fn dutch_compatible(input: DutchCompatibilityInput) -> DutchCompatibilityKind {
    if input.forbidden_pair {
        return DutchCompatibilityKind::Incompatible;
    }
    if !input.player0.has_absolute_preference()
        || !input.player1.has_absolute_preference()
        || input.player0.color_preference != input.player1.color_preference
    {
        return DutchCompatibilityKind::Compatible;
    }
    if !input
        .expected_rounds
        .checked_sub(1)
        .is_some_and(|final_round_index| input.played_rounds >= final_round_index)
    {
        return DutchCompatibilityKind::Incompatible;
    }
    let Some(top_score_threshold) =
        dutch_top_score_threshold(input.played_rounds, input.point_system)
    else {
        return DutchCompatibilityKind::Incompatible;
    };
    if dutch_compatibility_top_score(
        input.scores.player0_real,
        input.scores.player1_real,
        top_score_threshold,
    ) {
        DutchCompatibilityKind::Compatible
    } else {
        DutchCompatibilityKind::Incompatible
    }
}

pub(crate) fn dutch_get_float(
    game_was_played: bool,
    pairing_allocated_bye: bool,
    match_points: usize,
    point_system: PointSystem,
    player_score: usize,
    opponent_score: usize,
) -> FloatDirection {
    if !game_was_played {
        // FIDE Dutch Article 1.4.3: every pairing-allocated bye is a
        // downfloat, independently of the custom numeric award. Other
        // unplayed results are downfloats only when they score above a loss.
        if pairing_allocated_bye || match_points > point_system.loss.value() as usize {
            FloatDirection::Down
        } else {
            FloatDirection::None
        }
    } else if player_score > opponent_score {
        FloatDirection::Down
    } else if player_score < opponent_score {
        FloatDirection::Up
    } else {
        FloatDirection::None
    }
}

pub(super) fn dutch_is_bye_candidate(
    eligible_for_bye: bool,
    player_score_with_acceleration: usize,
    bye_assignee_score: usize,
) -> bool {
    eligible_for_bye && player_score_with_acceleration <= bye_assignee_score
}

pub(super) fn dutch_choose_player_color(
    neutral_color: Option<Color>,
    player_color_preference: Option<Color>,
    opponent_color_preference: Option<Color>,
    player_rank_index: usize,
    opponent_rank_index: usize,
    initial_color: Color,
    player_accelerated_score_rank_compare: bool,
) -> Color {
    if let Some(neutral_color) = neutral_color {
        neutral_color
    } else if let Some(player_color_preference) = player_color_preference {
        if player_accelerated_score_rank_compare {
            opponent_color_preference
                .map(Color::opposite)
                .expect("a neutral conflict has two concrete color preferences")
        } else {
            player_color_preference
        }
    } else if player_accelerated_score_rank_compare {
        opposite_initial_color_for_rank(opponent_rank_index, initial_color)
    } else {
        initial_color_for_rank(player_rank_index, initial_color)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct DutchColorBitSelector {
    absolute_imbalance_pair_allowed: bool,
    absolute_preference_pair_allowed: bool,
    color_preferences_compatible: bool,
    strong_preference_pair_allowed: bool,
}

impl DutchColorBitSelector {
    pub(super) fn empty() -> Self {
        Self {
            absolute_imbalance_pair_allowed: false,
            absolute_preference_pair_allowed: false,
            color_preferences_compatible: false,
            strong_preference_pair_allowed: false,
        }
    }

    pub(super) fn in_insertion_order(self) -> [bool; 4] {
        [
            self.absolute_imbalance_pair_allowed,
            self.absolute_preference_pair_allowed,
            self.color_preferences_compatible,
            self.strong_preference_pair_allowed,
        ]
    }
}

pub(super) fn dutch_color_bit_selector(
    player: PlayerColorData,
    opponent: PlayerColorData,
) -> DutchColorBitSelector {
    let player_has_absolute_color_imbalance = player.has_absolute_imbalance();
    let opponent_has_absolute_color_imbalance = opponent.has_absolute_imbalance();
    let player_has_absolute_color_preference = player.has_absolute_preference();
    let opponent_has_absolute_color_preference = opponent.has_absolute_preference();
    let preferences_differ = player.color_preference != opponent.color_preference;

    let lesser_imbalance_repeated_color = if player.color_imbalance > opponent.color_imbalance {
        opponent.repeated_color
    } else {
        player.repeated_color
    };
    let color_repetition_is_compatible = if player.color_imbalance == opponent.color_imbalance {
        player.repeated_color.is_none() || player.repeated_color != opponent.repeated_color
    } else {
        lesser_imbalance_repeated_color != player.color_preference.map(Color::opposite)
    };

    DutchColorBitSelector {
        absolute_imbalance_pair_allowed: !player_has_absolute_color_imbalance
            || !opponent_has_absolute_color_imbalance
            || preferences_differ,
        absolute_preference_pair_allowed: !player_has_absolute_color_preference
            || !opponent_has_absolute_color_preference
            || preferences_differ
            || color_repetition_is_compatible,
        color_preferences_compatible: color_preferences_are_compatible(
            player.color_preference,
            opponent.color_preference,
        ),
        strong_preference_pair_allowed: (!player.strong_color_preference
            && !player_has_absolute_color_preference)
            || (!opponent.strong_color_preference && !opponent_has_absolute_color_preference)
            || (player_has_absolute_color_preference && opponent_has_absolute_color_preference)
            || preferences_differ,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DutchColorBitSelector, DutchCompatibilityInput, DutchCompatibilityKind,
        DutchCompatibilityScores, FloatDirection, PlayerColorData, dutch_color_bit_selector,
        dutch_compatible, dutch_get_float, dutch_top_score_threshold,
    };
    use crate::domain::{Color, PointSystem, Score};

    fn absolute_white_player() -> PlayerColorData {
        PlayerColorData {
            color_imbalance: 2,
            repeated_color: None,
            color_preference: Some(Color::White),
            strong_color_preference: false,
        }
    }

    fn strong_white_player() -> PlayerColorData {
        PlayerColorData {
            color_imbalance: 1,
            repeated_color: None,
            color_preference: Some(Color::White),
            strong_color_preference: true,
        }
    }

    #[test]
    fn dutch_color_bit_selector_reports_ordered_compatible_bits() {
        let selector = dutch_color_bit_selector(
            PlayerColorData {
                color_imbalance: 0,
                repeated_color: None,
                color_preference: Some(Color::White),
                strong_color_preference: false,
            },
            PlayerColorData {
                color_imbalance: 0,
                repeated_color: None,
                color_preference: Some(Color::Black),
                strong_color_preference: false,
            },
        );

        assert_eq!(selector.in_insertion_order(), [true, true, true, true]);
    }

    #[test]
    fn dutch_color_bit_selector_can_report_disallowed_strong_preference_pair() {
        let selector = dutch_color_bit_selector(strong_white_player(), strong_white_player());

        assert_eq!(
            selector,
            DutchColorBitSelector {
                absolute_imbalance_pair_allowed: true,
                absolute_preference_pair_allowed: true,
                color_preferences_compatible: false,
                strong_preference_pair_allowed: false,
            }
        );
        assert_eq!(selector.in_insertion_order(), [true, true, false, false]);
    }

    #[test]
    fn dutch_top_score_threshold_uses_larger_win_or_draw_value() {
        assert_eq!(
            dutch_top_score_threshold(
                3,
                PointSystem {
                    win: Score::new(2),
                    draw: Score::new(5),
                    loss: Score::new(0),
                    forfeit_loss: Score::new(0),
                }
            ),
            Some(Score::new(7))
        );
    }

    #[test]
    fn dutch_top_score_threshold_returns_none_on_overflow() {
        assert_eq!(
            dutch_top_score_threshold(
                usize::MAX,
                PointSystem {
                    win: Score::new(2),
                    draw: Score::new(1),
                    loss: Score::new(0),
                    forfeit_loss: Score::new(0),
                }
            ),
            None
        );
    }

    #[test]
    fn dutch_get_float_uses_point_system_loss_value() {
        assert!(matches!(
            dutch_get_float(
                false,
                false,
                5,
                PointSystem {
                    win: Score::new(10),
                    draw: Score::new(5),
                    loss: Score::new(4),
                    forfeit_loss: Score::new(0),
                },
                0,
                0
            ),
            FloatDirection::Down
        ));
        assert!(matches!(
            dutch_get_float(
                false,
                false,
                4,
                PointSystem {
                    win: Score::new(10),
                    draw: Score::new(5),
                    loss: Score::new(4),
                    forfeit_loss: Score::new(0),
                },
                0,
                0
            ),
            FloatDirection::None
        ));
    }

    #[test]
    fn a_zero_value_pairing_allocated_bye_is_still_a_downfloat() {
        assert!(matches!(
            dutch_get_float(false, true, 0, PointSystem::STANDARD, 0, 0),
            FloatDirection::Down
        ));
    }

    #[test]
    fn dutch_compatible_rejects_top_score_exception_when_threshold_overflows() {
        assert!(matches!(
            dutch_compatible(DutchCompatibilityInput {
                forbidden_pair: false,
                player0: absolute_white_player(),
                player1: absolute_white_player(),
                played_rounds: usize::MAX,
                expected_rounds: usize::MAX,
                point_system: PointSystem {
                    win: Score::new(2),
                    draw: Score::new(1),
                    loss: Score::new(0),
                    forfeit_loss: Score::new(0),
                },
                scores: DutchCompatibilityScores {
                    player0_real: Score::new(u32::MAX),
                    player1_real: Score::new(0),
                },
            }),
            DutchCompatibilityKind::Incompatible
        ));
    }
}
