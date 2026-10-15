use crate::domain::Color;
use crate::swiss::PlayerRoundRecord;
use crate::swisssystems::common::{
    first_color_difference, initial_color_for_rank, previous_played_color,
};

use super::input::DoubleSwissInput;
use crate::swisssystems::bracketing::BracketPlayer;

/// FIDE C.04.5 4.2: the Higher-Ranked-Player is whoever has the higher
/// configured primary score, else the smaller Tournament Pairing Number (this crate's
/// `rank_index`, which — like TPN — is fixed at the initial ranking and only
/// adjusted to skip withdrawn players).
pub(super) fn player_is_higher_ranked(
    player_primary_score: usize,
    player_rank_index: usize,
    opponent_primary_score: usize,
    opponent_rank_index: usize,
) -> bool {
    if player_primary_score != opponent_primary_score {
        player_primary_score > opponent_primary_score
    } else {
        player_rank_index < opponent_rank_index
    }
}

fn played_match_color(record: PlayerRoundRecord) -> Option<Color> {
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
        .then_some(scheduled_first_color)
}

fn total_whites(records: &[PlayerRoundRecord]) -> usize {
    records
        .iter()
        .copied()
        .filter(|record| played_match_color(*record) == Some(Color::White))
        .count()
}

fn most_recent_color(records: &[PlayerRoundRecord]) -> Option<Color> {
    let mut cursor = records.len();
    previous_played_color(records, &mut cursor)
}

fn first_match_color_difference(
    player_records: &[PlayerRoundRecord],
    opponent_records: &[PlayerRoundRecord],
) -> Option<Color> {
    let difference = first_color_difference(player_records, opponent_records);
    difference
        .player
        .zip(difference.opponent)
        .map(|(player, _)| player)
}

pub(super) fn choose_double_swiss_player_color(
    input: &DoubleSwissInput<'_>,
    players: &[BracketPlayer<'_>],
    current_primary_scores: &[usize],
    player_index: usize,
    opponent_index: usize,
) -> Color {
    let player = players[player_index];
    let opponent = players[opponent_index];

    // 4.3.1: neither player has played a match yet.
    if player.played_matches == 0 && opponent.played_matches == 0 {
        let player_is_hrp = player_is_higher_ranked(
            current_primary_scores[player_index],
            player.rank_index,
            current_primary_scores[opponent_index],
            opponent.rank_index,
        );
        let hrp_rank_index = if player_is_hrp {
            player.rank_index
        } else {
            opponent.rank_index
        };
        let hrp_color = initial_color_for_rank(hrp_rank_index, input.pairing.initial_color);
        return if player_is_hrp {
            hrp_color
        } else {
            hrp_color.opposite()
        };
    }

    // 4.3.2: fewer total Whites so far (whole-tournament count).
    let player_whites = total_whites(player.records);
    let opponent_whites = total_whites(opponent.records);
    if player_whites != opponent_whites {
        return if player_whites < opponent_whites {
            Color::White
        } else {
            Color::Black
        };
    }

    // 4.3.3: alternate from the most recent round where colours differed.
    if let Some(difference) = first_match_color_difference(player.records, opponent.records) {
        return difference.opposite();
    }

    // 4.3.4 / 4.3.5: alternate the HRP's own last colour, else the HRP's
    // opponent's last colour (which leaves the complementary colour for HRP).
    let player_is_hrp = player_is_higher_ranked(
        current_primary_scores[player_index],
        player.rank_index,
        current_primary_scores[opponent_index],
        opponent.rank_index,
    );
    let (hrp, hrp_opponent) = if player_is_hrp {
        (player, opponent)
    } else {
        (opponent, player)
    };
    let hrp_color = if let Some(hrp_last_color) = most_recent_color(hrp.records) {
        hrp_last_color.opposite()
    } else if let Some(opponent_last_color) = most_recent_color(hrp_opponent.records) {
        opponent_last_color
    } else {
        // Project gap-fill: a technically regular match can make 4.3.1
        // inapplicable even though no game began, leaving 4.3.2-4.3.5 without
        // colour-bearing history. Reuse 4.3.1's HRP-TPN parity rule only after
        // those criteria are exhausted.
        initial_color_for_rank(hrp.rank_index, input.pairing.initial_color)
    };
    if player_is_hrp {
        hrp_color
    } else {
        hrp_color.opposite()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        AdjudicatedSideResult, GameSideOutcome, MatchScore, PlayerColorData, PlayerId, Score,
    };
    use crate::swiss::{
        LogicalGame, LogicalGames, MatchDisposition, SwissPairingPlayerEntry, SwissPairingSnapshot,
    };
    use crate::swisssystems::common::PairingContext;

    use crate::swisssystems::common::build_swiss_player_projections;

    fn base_input(tournament: &SwissPairingSnapshot) -> DoubleSwissInput<'_> {
        DoubleSwissInput {
            pairing: PairingContext {
                played_rounds: 1,
                point_system: crate::domain::PointSystem {
                    win: crate::domain::Score::new(20),
                    draw: crate::domain::Score::new(10),
                    loss: crate::domain::Score::new(0),
                    forfeit_loss: crate::domain::Score::new(0),
                },
                initial_color: Color::White,
                tournament,
            },
            expected_rounds: 3,
            points_type_max: 9999,
        }
    }

    fn tournament_with_no_history(player_count: usize) -> SwissPairingSnapshot {
        let mut tournament = SwissPairingSnapshot::new();
        for index in 0..player_count {
            tournament.push_player(SwissPairingPlayerEntry {
                id: PlayerId::new(index),
                score_without_acceleration: Score::new(0),
                rank_index: index,
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

    #[test]
    fn round_one_gives_white_to_hrp_when_tpn_is_odd() {
        let tournament = tournament_with_no_history(2);
        let input = base_input(&tournament);
        let players = build_swiss_player_projections(&tournament).unwrap();
        let current_match_points = [0usize, 0usize];

        // Player 0 has rank_index 0 (TPN 1, odd) and equal score to player 1,
        // so player 0 is HRP by the smaller-TPN tiebreak and gets White
        // (initial_color) per rule 4.3.1.
        assert_eq!(
            choose_double_swiss_player_color(&input, &players, &current_match_points, 0, 1),
            Color::White
        );
        assert_eq!(
            choose_double_swiss_player_color(&input, &players, &current_match_points, 1, 0),
            Color::Black
        );
    }

    #[test]
    fn fewer_whites_so_far_overrides_tpn_parity() {
        let matches0 = [record(
            PlayerId::new(9),
            Color::Black,
            LogicalGames::single(played(Color::Black, MatchScore::Win)),
            MatchScore::Win,
        )];
        let matches1 = [record(
            PlayerId::new(9),
            Color::White,
            LogicalGames::single(played(Color::White, MatchScore::Win)),
            MatchScore::Win,
        )];
        let mut tournament = SwissPairingSnapshot::new();
        tournament.push_player(SwissPairingPlayerEntry {
            id: PlayerId::new(0),
            score_without_acceleration: crate::domain::Score::new(20),
            rank_index: 0,
            played_games: 1,
            played_matches: 1,
            color_data: PlayerColorData {
                color_imbalance: 1,
                color_preference: Some(Color::White),
                repeated_color: None,
                strong_color_preference: true,
            },
            is_valid: true,
            records: &matches0,
            accelerations: &[],
        });
        tournament.push_player(SwissPairingPlayerEntry {
            id: PlayerId::new(1),
            score_without_acceleration: crate::domain::Score::new(20),
            rank_index: 1,
            played_games: 1,
            played_matches: 1,
            color_data: PlayerColorData {
                color_imbalance: 1,
                color_preference: Some(Color::Black),
                repeated_color: None,
                strong_color_preference: true,
            },
            is_valid: true,
            records: &matches1,
            accelerations: &[],
        });
        let input = base_input(&tournament);
        let players = build_swiss_player_projections(&tournament).unwrap();
        let current_match_points = [20usize, 20usize];

        // Player 0 played Black last time (0 Whites), player 1 played White
        // (1 White) — player 0 has fewer Whites so far and should get White
        // now, even though player 1 would otherwise be neither HRP-favoured
        // nor disfavoured by TPN alone.
        assert_eq!(
            choose_double_swiss_player_color(&input, &players, &current_match_points, 0, 1),
            Color::White
        );
    }

    #[test]
    fn double_history_counts_one_scheduled_match_color_when_any_game_was_played() {
        let matches = [
            record(
                PlayerId::new(1),
                Color::White,
                LogicalGames::double(
                    played(Color::White, MatchScore::Win),
                    played(Color::Black, MatchScore::Draw),
                ),
                MatchScore::Win,
            ),
            record(
                PlayerId::new(2),
                Color::Black,
                LogicalGames::double(
                    adjudicated(Color::Black, AdjudicatedSideResult::ForfeitLoss),
                    played(Color::White, MatchScore::Win),
                ),
                MatchScore::Draw,
            ),
            record(
                PlayerId::new(3),
                Color::White,
                LogicalGames::double(
                    adjudicated(Color::White, AdjudicatedSideResult::ForfeitWin),
                    adjudicated(Color::Black, AdjudicatedSideResult::ForfeitLoss),
                ),
                MatchScore::Draw,
            ),
        ];

        assert_eq!(total_whites(&matches), 1);
        assert_eq!(most_recent_color(&matches), Some(Color::Black));
    }

    #[test]
    fn color_difference_requires_comparable_history_on_both_sides() {
        let player_matches = [
            record(
                PlayerId::new(2),
                Color::Black,
                LogicalGames::single(played(Color::Black, MatchScore::Win)),
                MatchScore::Win,
            ),
            record(
                PlayerId::new(3),
                Color::White,
                LogicalGames::single(played(Color::White, MatchScore::Win)),
                MatchScore::Win,
            ),
        ];
        let opponent_matches = [record(
            PlayerId::new(4),
            Color::White,
            LogicalGames::single(played(Color::White, MatchScore::Win)),
            MatchScore::Win,
        )];

        assert_eq!(
            first_match_color_difference(&player_matches, &opponent_matches),
            None
        );
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

    fn record(
        opponent: PlayerId,
        scheduled_first_color: Color,
        games: LogicalGames,
        aggregate: MatchScore,
    ) -> PlayerRoundRecord {
        PlayerRoundRecord::Match {
            opponent,
            scheduled_first_color,
            state: crate::swiss::SwissMatchRecord::Complete {
                games,
                disposition: MatchDisposition::Regular,
                aggregate,
            },
        }
    }
}
