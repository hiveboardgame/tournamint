use crate::domain::{Color, PlayerColorData};
use crate::swiss::{MatchDisposition, PlayerRoundRecord, SwissMatchRecord};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ComputedPlayerData {
    pub(crate) played_games: usize,
    pub(crate) played_matches: usize,
    pub(crate) color_data: PlayerColorData,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("player match-history counters overflowed")]
pub(crate) struct ComputePlayerDataError;

fn compute_player_color_data(
    games_as_white: usize,
    games_as_black: usize,
    consecutive_count: usize,
    last_color: Option<Color>,
) -> PlayerColorData {
    let lower_color = if games_as_white > games_as_black {
        Color::Black
    } else {
        Color::White
    };
    let color_imbalance = match lower_color {
        Color::White => games_as_black - games_as_white,
        Color::Black => games_as_white - games_as_black,
    };
    let color_preference = if color_imbalance > 1 {
        Some(lower_color)
    } else if consecutive_count > 1 {
        last_color.map(Color::opposite)
    } else if color_imbalance > 0 {
        Some(lower_color)
    } else if consecutive_count != 0 {
        last_color.map(Color::opposite)
    } else {
        None
    };
    let repeated_color = if consecutive_count > 1 {
        last_color
    } else {
        None
    };

    let color_data = PlayerColorData {
        color_imbalance,
        color_preference,
        repeated_color,
        strong_color_preference: false,
    };

    PlayerColorData {
        strong_color_preference: !color_data.has_absolute_preference() && color_imbalance != 0,
        ..color_data
    }
}

fn compute_player_data_from_played_match_colors(
    colors: impl IntoIterator<Item = Color>,
) -> Result<PlayerColorData, ComputePlayerDataError> {
    let mut games_as_white = 0usize;
    let mut games_as_black = 0usize;
    let mut consecutive_count = 0usize;
    let mut last_color = None;

    for color in colors {
        match color {
            Color::White => {
                games_as_white = games_as_white
                    .checked_add(1)
                    .ok_or(ComputePlayerDataError)?;
            }
            Color::Black => {
                games_as_black = games_as_black
                    .checked_add(1)
                    .ok_or(ComputePlayerDataError)?;
            }
        }
        if last_color != Some(color) {
            consecutive_count = 1;
        } else {
            consecutive_count = consecutive_count
                .checked_add(1)
                .ok_or(ComputePlayerDataError)?;
        }
        last_color = Some(color);
    }

    Ok(compute_player_color_data(
        games_as_white,
        games_as_black,
        consecutive_count,
        last_color,
    ))
}

pub(crate) fn compute_player_data(
    records: &[PlayerRoundRecord],
) -> Result<ComputedPlayerData, ComputePlayerDataError> {
    let mut played_games = 0usize;
    let mut played_matches = 0usize;
    for record in records.iter().copied() {
        let PlayerRoundRecord::Match { state, .. } = record else {
            continue;
        };
        for game in state.games().iter() {
            if game.outcome.counts_as_played() {
                played_games = played_games.checked_add(1).ok_or(ComputePlayerDataError)?;
            }
        }
        if matches!(
            state,
            SwissMatchRecord::Complete {
                disposition: MatchDisposition::Regular,
                ..
            }
        ) {
            played_matches = played_matches
                .checked_add(1)
                .ok_or(ComputePlayerDataError)?;
        }
    }
    let color_data =
        compute_player_data_from_played_match_colors(records.iter().filter_map(|record| {
            let PlayerRoundRecord::Match {
                scheduled_first_color,
                state,
                ..
            } = record
            else {
                return None;
            };
            state
                .games()
                .iter()
                .any(|game| game.outcome.counts_as_played())
                .then_some(*scheduled_first_color)
        }))?;
    Ok(ComputedPlayerData {
        played_games,
        played_matches,
        color_data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AdjudicatedSideResult, GameSideOutcome, MatchScore, PlayerId};
    use crate::swiss::{LogicalGame, LogicalGames, MatchDisposition, SwissMatchRecord};

    fn projection(record: PlayerRoundRecord) -> ComputedPlayerData {
        compute_player_data(&[record]).unwrap()
    }

    fn match_record(
        games: LogicalGames,
        disposition: MatchDisposition,
        aggregate: MatchScore,
    ) -> PlayerRoundRecord {
        PlayerRoundRecord::Match {
            opponent: PlayerId::new(1),
            scheduled_first_color: Color::White,
            state: SwissMatchRecord::Complete {
                games,
                disposition,
                aggregate,
            },
        }
    }

    fn pending_match(games: LogicalGames) -> PlayerRoundRecord {
        PlayerRoundRecord::Match {
            opponent: PlayerId::new(1),
            scheduled_first_color: Color::White,
            state: SwissMatchRecord::Pending { games },
        }
    }

    fn played(color: Color, score: MatchScore) -> LogicalGame {
        LogicalGame {
            color,
            outcome: GameSideOutcome::Played(score),
        }
    }

    fn adjudicated(color: Color, outcome: AdjudicatedSideResult) -> LogicalGame {
        LogicalGame {
            color,
            outcome: GameSideOutcome::Adjudicated(outcome),
        }
    }

    #[test]
    fn double_match_game_match_and_color_counts_are_independent() {
        let complete = projection(match_record(
            LogicalGames::double(
                played(Color::White, MatchScore::Win),
                played(Color::Black, MatchScore::Draw),
            ),
            MatchDisposition::Regular,
            MatchScore::Win,
        ));
        assert_eq!((complete.played_games, complete.played_matches), (2, 1));
        assert_eq!(complete.color_data.color_imbalance, 1);

        let game_two_only = projection(pending_match(LogicalGames::single(played(
            Color::Black,
            MatchScore::Draw,
        ))));
        assert_eq!(
            (game_two_only.played_games, game_two_only.played_matches),
            (1, 0)
        );
        assert_eq!(game_two_only.color_data.color_imbalance, 1);

        let full_forfeit = projection(match_record(
            LogicalGames::double(
                adjudicated(Color::White, AdjudicatedSideResult::ForfeitWin),
                adjudicated(Color::Black, AdjudicatedSideResult::ForfeitWin),
            ),
            MatchDisposition::ForfeitWin,
            MatchScore::Win,
        ));
        assert_eq!(
            (full_forfeit.played_games, full_forfeit.played_matches),
            (0, 0)
        );
        assert_eq!(
            full_forfeit.color_data,
            PlayerColorData::no_played_color_history()
        );

        let split_adjudicated = projection(match_record(
            LogicalGames::double(
                adjudicated(Color::White, AdjudicatedSideResult::ForfeitWin),
                adjudicated(Color::Black, AdjudicatedSideResult::ForfeitLoss),
            ),
            MatchDisposition::Regular,
            MatchScore::Draw,
        ));
        assert_eq!(
            (
                split_adjudicated.played_games,
                split_adjudicated.played_matches
            ),
            (0, 1)
        );
        assert_eq!(
            split_adjudicated.color_data,
            PlayerColorData::no_played_color_history()
        );
    }

    #[test]
    fn empty_and_alternating_color_histories_use_explicit_absence() {
        let no_played_color_history = compute_player_data_from_played_match_colors([]).unwrap();
        assert_eq!(
            no_played_color_history,
            PlayerColorData::no_played_color_history()
        );

        let alternating =
            compute_player_data_from_played_match_colors([Color::White, Color::Black]).unwrap();
        assert_eq!(
            alternating,
            PlayerColorData {
                color_imbalance: 0,
                color_preference: Some(Color::White),
                repeated_color: None,
                strong_color_preference: false,
            }
        );
    }

    #[test]
    fn color_history_reports_repeated_color_as_absolute_preference() {
        let color_data =
            compute_player_data_from_played_match_colors([Color::White, Color::White]).unwrap();

        assert_eq!(color_data.color_imbalance, 2);
        assert_eq!(color_data.color_preference, Some(Color::Black));
        assert_eq!(color_data.repeated_color, Some(Color::White));
        assert!(!color_data.strong_color_preference);
    }
}
