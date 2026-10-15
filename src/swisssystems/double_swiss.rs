mod colors;
#[cfg(test)]
mod differential;
#[cfg(test)]
#[allow(dead_code)]
mod exhaustive_oracle;
mod forbidden;
#[cfg(test)]
mod history_differential;
mod input;
mod ordered_solver;
mod scores;

#[cfg(test)]
use crate::swiss::PairingError;
use crate::swiss::{PairingFailure, RoundPairings};

use crate::swisssystems::bracketing::compute_matching_error_from_status;
pub(crate) use input::DoubleSwissInput;

pub(crate) fn compute_matching(
    input: DoubleSwissInput<'_>,
) -> Result<RoundPairings, PairingFailure> {
    ordered_solver::compute_matching_impl(&input)
        .map_err(|failure| compute_matching_error_from_status(failure.status))
}

#[cfg(test)]
pub(crate) use ordered_solver::MatchingDiagnostics;

#[cfg(test)]
pub(crate) fn compute_matching_ordered_with_diagnostics(
    input: DoubleSwissInput<'_>,
) -> Result<(RoundPairings, MatchingDiagnostics), PairingFailure> {
    ordered_solver::compute_matching_impl_with_diagnostics(&input)
        .map_err(|failure| compute_matching_error_from_status(failure.status))
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OraclePairingFailure {
    GuardExceeded,
    Pairing(PairingError),
}

#[cfg(test)]
pub(crate) fn compute_matching_oracle_with_guard(
    input: DoubleSwissInput<'_>,
    max_matching_attempts: usize,
) -> Result<(RoundPairings, usize), OraclePairingFailure> {
    exhaustive_oracle::compute_matching_impl_with_guard(&input, max_matching_attempts).map_err(
        |failure| match failure {
            exhaustive_oracle::OracleError::GuardExceeded => OraclePairingFailure::GuardExceeded,
            exhaustive_oracle::OracleError::Pairing(status) => OraclePairingFailure::Pairing(
                compute_matching_error_from_status(status).pairing_error(),
            ),
        },
    )
}

#[cfg(test)]
pub(crate) fn compute_matching_ordered(
    input: DoubleSwissInput<'_>,
) -> Result<RoundPairings, PairingFailure> {
    ordered_solver::compute_matching_impl(&input)
        .map_err(|failure| compute_matching_error_from_status(failure.status))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Pairing;
    use crate::domain::{
        AdjudicatedSideResult, Color, ForbiddenPairEntry, GameSideOutcome, MatchScore,
        PlayerColorData, PlayerId, PointSystem, Score,
    };
    use crate::swiss::{
        LogicalGame, LogicalGames, MatchDisposition, PlayerRoundRecord, RoundPairings,
        SwissPairingPlayerEntry, SwissPairingSnapshot,
    };
    use crate::swisssystems::common::PairingContext;

    struct TestPlayer {
        id: usize,
        score: u32,
        records: Vec<PlayerRoundRecord>,
    }

    impl TestPlayer {
        fn new(id: usize, score: u32) -> Self {
            Self {
                id,
                score,
                records: Vec::new(),
            }
        }
    }

    fn played_match(opponent: usize, color: Color, score: MatchScore) -> PlayerRoundRecord {
        let second_score = match score {
            MatchScore::Win | MatchScore::Loss => MatchScore::Draw,
            MatchScore::Draw => MatchScore::Loss,
        };
        let first_score = match score {
            MatchScore::Draw => MatchScore::Win,
            score => score,
        };
        PlayerRoundRecord::Match {
            opponent: PlayerId::new(opponent),
            scheduled_first_color: color,
            state: crate::swiss::SwissMatchRecord::Complete {
                games: LogicalGames::double(
                    LogicalGame {
                        color,
                        outcome: GameSideOutcome::Played(first_score),
                    },
                    LogicalGame {
                        color: color.opposite(),
                        outcome: GameSideOutcome::Played(second_score),
                    },
                ),
                disposition: MatchDisposition::Regular,
                aggregate: score,
            },
        }
    }

    fn double_forfeit_match(opponent: usize, color: Color) -> PlayerRoundRecord {
        PlayerRoundRecord::Match {
            opponent: PlayerId::new(opponent),
            scheduled_first_color: color,
            state: crate::swiss::SwissMatchRecord::Complete {
                games: LogicalGames::double(
                    LogicalGame {
                        color,
                        outcome: GameSideOutcome::Adjudicated(AdjudicatedSideResult::DoubleForfeit),
                    },
                    LogicalGame {
                        color: color.opposite(),
                        outcome: GameSideOutcome::Adjudicated(AdjudicatedSideResult::DoubleForfeit),
                    },
                ),
                disposition: MatchDisposition::DoubleForfeit,
                aggregate: MatchScore::Draw,
            },
        }
    }

    fn played_sweep(opponent: usize, color: Color, score: MatchScore) -> PlayerRoundRecord {
        PlayerRoundRecord::Match {
            opponent: PlayerId::new(opponent),
            scheduled_first_color: color,
            state: crate::swiss::SwissMatchRecord::Complete {
                games: LogicalGames::double(
                    LogicalGame {
                        color,
                        outcome: GameSideOutcome::Played(score),
                    },
                    LogicalGame {
                        color: color.opposite(),
                        outcome: GameSideOutcome::Played(score),
                    },
                ),
                disposition: MatchDisposition::Regular,
                aggregate: score,
            },
        }
    }

    fn build_tournament(players: Vec<TestPlayer>) -> SwissPairingSnapshot {
        let mut tournament = SwissPairingSnapshot::new();
        for player in players {
            let played_matches = player
                .records
                .iter()
                .filter(|record| {
                    matches!(
                        record,
                        PlayerRoundRecord::Match {
                            state: crate::swiss::SwissMatchRecord::Complete {
                                disposition: MatchDisposition::Regular,
                                ..
                            },
                            ..
                        }
                    )
                })
                .count();
            tournament.push_player(SwissPairingPlayerEntry {
                id: PlayerId::new(player.id),
                score_without_acceleration: Score::new(player.score),
                rank_index: player.id,
                played_games: player
                    .records
                    .iter()
                    .flat_map(|record| record.games().iter())
                    .filter(|game| game.outcome.counts_as_played())
                    .count(),
                played_matches,
                color_data: PlayerColorData::no_played_color_history(),
                is_valid: true,
                records: &player.records,
                accelerations: &[],
            });
        }
        tournament
    }

    fn input(tournament: &SwissPairingSnapshot, played_rounds: usize) -> DoubleSwissInput<'_> {
        DoubleSwissInput {
            pairing: PairingContext {
                played_rounds,
                point_system: PointSystem {
                    win: Score::new(20),
                    draw: Score::new(10),
                    loss: Score::new(0),
                    forfeit_loss: Score::new(0),
                },
                initial_color: Color::White,
                tournament,
            },
            expected_rounds: played_rounds.checked_add(2).unwrap(),
            points_type_max: 9999,
        }
    }

    fn pairing_opponent(result: &RoundPairings, player: usize) -> usize {
        result
            .games
            .iter()
            .find_map(|pairing| {
                let white = pairing.white().index();
                let black = pairing.black().index();
                if white == player {
                    Some(black)
                } else if black == player {
                    Some(white)
                } else {
                    None
                }
            })
            .expect("player occurs in the pairing result")
    }

    fn bye_player(result: &RoundPairings) -> usize {
        result
            .byes
            .first()
            .expect("pairing result contains a bye")
            .player
            .index()
    }

    fn push_history_player(
        tournament: &mut SwissPairingSnapshot,
        id: usize,
        score: u32,
        is_valid: bool,
        records: &[PlayerRoundRecord],
    ) {
        let played_games = records
            .iter()
            .flat_map(|record| record.games().iter())
            .filter(|game| game.outcome.counts_as_played())
            .count();
        let played_matches = records
            .iter()
            .filter(|record| {
                matches!(
                    record,
                    PlayerRoundRecord::Match {
                        state: crate::swiss::SwissMatchRecord::Complete {
                            disposition: MatchDisposition::Regular,
                            ..
                        },
                        ..
                    }
                )
            })
            .count();
        tournament.push_player(SwissPairingPlayerEntry {
            id: PlayerId::new(id),
            score_without_acceleration: Score::new(score),
            rank_index: id,
            played_games,
            played_matches,
            color_data: PlayerColorData::no_played_color_history(),
            is_valid,
            records,
            accelerations: &[],
        });
    }

    fn c7_history_tournament() -> SwissPairingSnapshot {
        let split = |opponent, first_color| PlayerRoundRecord::Match {
            opponent: PlayerId::new(opponent),
            scheduled_first_color: first_color,
            state: crate::swiss::SwissMatchRecord::Complete {
                games: LogicalGames::double(
                    LogicalGame {
                        color: first_color,
                        outcome: GameSideOutcome::Adjudicated(AdjudicatedSideResult::ForfeitWin),
                    },
                    LogicalGame {
                        color: first_color.opposite(),
                        outcome: GameSideOutcome::Adjudicated(AdjudicatedSideResult::ForfeitLoss),
                    },
                ),
                disposition: MatchDisposition::Regular,
                aggregate: MatchScore::Draw,
            },
        };
        let unplayed = || PlayerRoundRecord::RequestedBye;
        let histories = [
            vec![split(3, Color::White)],
            vec![unplayed()],
            vec![unplayed()],
            vec![split(0, Color::Black)],
            vec![split(6, Color::White)],
            vec![split(7, Color::White)],
            vec![split(4, Color::Black)],
            vec![split(5, Color::Black)],
        ];
        let scores = [40, 40, 40, 31, 31, 31, 31, 31];
        let mut tournament = SwissPairingSnapshot::new();
        for id in 0..histories.len() {
            push_history_player(&mut tournament, id, scores[id], id < 6, &histories[id]);
        }
        tournament
    }

    #[test]
    fn even_round_one_pairs_every_player_exactly_once() {
        let tournament = build_tournament(vec![
            TestPlayer::new(0, 0),
            TestPlayer::new(1, 0),
            TestPlayer::new(2, 0),
            TestPlayer::new(3, 0),
        ]);

        let result = compute_matching(input(&tournament, 0)).expect("valid Double-Swiss pairing");

        assert_eq!(result.games.len(), 2);
        assert!(result.byes.is_empty());
        let mut paired = [false; 4];
        for pairing in &result.games {
            let white = pairing.white().index();
            let black = pairing.black().index();
            assert_ne!(
                white, black,
                "no player should be self-paired in an even round"
            );
            assert!(!paired[white], "player {white} paired more than once");
            assert!(!paired[black], "player {black} paired more than once");
            paired[white] = true;
            paired[black] = true;
        }
        assert!(
            paired.iter().all(|&value| value),
            "every player must be paired"
        );
    }

    #[test]
    fn exhaustive_oracle_small_field_fingerprint_is_frozen() {
        let tournament = build_tournament(vec![
            TestPlayer::new(0, 20),
            TestPlayer::new(1, 20),
            TestPlayer::new(2, 20),
            TestPlayer::new(3, 10),
            TestPlayer::new(4, 10),
            TestPlayer::new(5, 10),
        ]);

        let (result, consumed_matching_attempts) =
            compute_matching_oracle_with_guard(input(&tournament, 0), usize::MAX).unwrap();

        assert_eq!(consumed_matching_attempts, 6);
        assert!(result.byes.is_empty());
        assert_eq!(
            result.games,
            vec![
                Pairing::new(PlayerId::new(0), PlayerId::new(2)),
                Pairing::new(PlayerId::new(3), PlayerId::new(1)),
                Pairing::new(PlayerId::new(4), PlayerId::new(5)),
            ]
        );
    }

    #[test]
    fn odd_round_one_produces_exactly_one_bye() {
        let tournament = build_tournament(vec![
            TestPlayer::new(0, 0),
            TestPlayer::new(1, 0),
            TestPlayer::new(2, 0),
        ]);

        let result = compute_matching(input(&tournament, 0)).expect("valid Double-Swiss pairing");

        assert_eq!(result.byes.len(), 1);
        assert_eq!(result.games.len(), 1);
    }

    #[test]
    fn round_two_avoids_rematching_players_who_already_played() {
        let mut players = [
            TestPlayer::new(0, 30),
            TestPlayer::new(1, 10),
            TestPlayer::new(2, 30),
            TestPlayer::new(3, 10),
        ];
        players[0]
            .records
            .push(played_match(1, Color::White, MatchScore::Win));
        players[1]
            .records
            .push(played_match(0, Color::Black, MatchScore::Loss));
        players[2]
            .records
            .push(played_match(3, Color::White, MatchScore::Win));
        players[3]
            .records
            .push(played_match(2, Color::Black, MatchScore::Loss));
        let tournament = build_tournament(players.into());

        let result = compute_matching(input(&tournament, 1)).expect("valid Double-Swiss pairing");

        for pairing in &result.games {
            let white = pairing.white().index();
            let black = pairing.black().index();
            let is_prior_pairing = (white == 0 && black == 1)
                || (white == 1 && black == 0)
                || (white == 2 && black == 3)
                || (white == 3 && black == 2);
            assert!(
                !is_prior_pairing,
                "must not repeat a prior pairing: {white} vs {black}"
            );
        }
    }

    #[test]
    fn forbidden_pair_prevents_rematch_in_first_round() {
        let mut tournament = build_tournament((0..4).map(|id| TestPlayer::new(id, 0)).collect());
        let forbidden_players = [PlayerId::new(0), PlayerId::new(1)];
        tournament.push_forbidden_pair(ForbiddenPairEntry {
            round_range: 0..1,
            players: &forbidden_players,
        });

        let result = compute_matching(input(&tournament, 0)).expect("valid Double-Swiss pairing");

        for pairing in &result.games {
            let white = pairing.white().index();
            let black = pairing.black().index();
            let is_forbidden = (white == 0 && black == 1) || (white == 1 && black == 0);
            assert!(!is_forbidden, "forbidden pair 0 vs 1 must not be paired");
        }
    }

    #[test]
    fn c4_uses_the_minimum_number_of_upfloaters() {
        let tournament = build_tournament(vec![
            TestPlayer::new(0, 20),
            TestPlayer::new(1, 20),
            TestPlayer::new(2, 20),
            TestPlayer::new(3, 10),
            TestPlayer::new(4, 10),
            TestPlayer::new(5, 10),
        ]);

        let result = compute_matching(input(&tournament, 0)).unwrap();
        let cross_score_pairs = result
            .games
            .iter()
            .filter(|pairing| {
                let white = pairing.white().index();
                let black = pairing.black().index();
                (white < 3) != (black < 3)
            })
            .count();

        assert_eq!(cross_score_pairs, 1);
    }

    #[test]
    fn c5_chooses_the_highest_scoring_available_upfloater() {
        let tournament = build_tournament(vec![
            TestPlayer::new(0, 30),
            TestPlayer::new(1, 30),
            TestPlayer::new(2, 30),
            TestPlayer::new(3, 20),
            TestPlayer::new(4, 10),
            TestPlayer::new(5, 10),
            TestPlayer::new(6, 10),
            TestPlayer::new(7, 10),
        ]);

        let result = compute_matching(input(&tournament, 0)).unwrap();

        assert!(pairing_opponent(&result, 3) < 3);
        assert!((4..8).all(|player| pairing_opponent(&result, player) >= 4));
    }

    #[test]
    fn c6_preserves_minimum_upfloaters_in_the_following_nonempty_group() {
        let mut tournament = build_tournament(vec![
            TestPlayer::new(0, 30),
            TestPlayer::new(1, 30),
            TestPlayer::new(2, 30),
            TestPlayer::new(3, 20),
            TestPlayer::new(4, 20),
            TestPlayer::new(5, 20),
            TestPlayer::new(6, 10),
            TestPlayer::new(7, 10),
        ]);
        let forbidden_following_pair = [PlayerId::new(4), PlayerId::new(5)];
        tournament.push_forbidden_pair(ForbiddenPairEntry {
            round_range: 0..1,
            players: &forbidden_following_pair,
        });

        let result = compute_matching(input(&tournament, 0)).unwrap();

        // TPN order would choose player 3. Doing so strands forbidden pair
        // 4-5 and makes the following scoregroup require two upfloaters.
        assert!(pairing_opponent(&result, 4) < 3);
        assert_eq!(pairing_opponent(&result, 3), 5);
    }

    #[test]
    fn c7_avoids_a_previous_regular_split_floater_before_the_final_round() {
        let tournament = c7_history_tournament();
        let mut pairing_input = input(&tournament, 1);
        pairing_input.expected_rounds = 3;

        let result = compute_matching(pairing_input).unwrap();

        assert!(pairing_opponent(&result, 4) < 3);
        assert!(pairing_opponent(&result, 3) >= 3);
    }

    #[test]
    fn final_round_disables_c7_and_restores_upfloater_set_tpn_order() {
        let tournament = c7_history_tournament();
        let mut pairing_input = input(&tournament, 1);
        pairing_input.expected_rounds = 2;

        let result = compute_matching(pairing_input).unwrap();

        assert!(pairing_opponent(&result, 3) < 3);
        assert_eq!(pairing_opponent(&result, 4), 5);
    }

    #[test]
    fn pab_prefers_lowest_primary_score() {
        let tournament = build_tournament(vec![
            TestPlayer::new(0, 10),
            TestPlayer::new(1, 10),
            TestPlayer::new(2, 10),
            TestPlayer::new(3, 10),
            TestPlayer::new(4, 0),
        ]);

        assert_eq!(
            bye_player(&compute_matching(input(&tournament, 0)).unwrap()),
            4
        );
    }

    #[test]
    fn pab_prefers_most_played_matches_before_tpn() {
        let mut players = (0..7).map(|id| TestPlayer::new(id, 0)).collect::<Vec<_>>();
        players[0].records.extend([
            played_sweep(1, Color::White, MatchScore::Loss),
            played_sweep(2, Color::Black, MatchScore::Loss),
            played_sweep(3, Color::White, MatchScore::Loss),
        ]);
        players[1].score = 40;
        players[1].records.extend([
            played_sweep(0, Color::Black, MatchScore::Win),
            double_forfeit_match(4, Color::White),
            double_forfeit_match(5, Color::Black),
        ]);
        players[2].score = 40;
        players[2].records.extend([
            double_forfeit_match(3, Color::White),
            played_sweep(0, Color::White, MatchScore::Win),
            double_forfeit_match(6, Color::Black),
        ]);
        players[3].score = 40;
        players[3].records.extend([
            double_forfeit_match(2, Color::Black),
            double_forfeit_match(6, Color::White),
            played_sweep(0, Color::Black, MatchScore::Win),
        ]);
        players[4].records.extend([
            double_forfeit_match(5, Color::White),
            double_forfeit_match(1, Color::Black),
            PlayerRoundRecord::RequestedBye,
        ]);
        players[5].records.extend([
            double_forfeit_match(4, Color::Black),
            PlayerRoundRecord::RequestedBye,
            double_forfeit_match(1, Color::White),
        ]);
        players[6].records.extend([
            PlayerRoundRecord::RequestedBye,
            double_forfeit_match(3, Color::Black),
            double_forfeit_match(2, Color::White),
        ]);
        let tournament = build_tournament(players);

        assert_eq!(
            bye_player(&compute_matching(input(&tournament, 3)).unwrap()),
            0
        );
    }

    #[test]
    fn pab_prefers_largest_tpn_after_score_and_match_count() {
        let tournament = build_tournament((0..5).map(|id| TestPlayer::new(id, 0)).collect());

        assert_eq!(
            bye_player(&compute_matching(input(&tournament, 0)).unwrap()),
            4
        );
    }

    #[test]
    fn pab_skips_a_candidate_that_does_not_leave_a_complete_pairing() {
        let mut tournament = build_tournament((0..5).map(|id| TestPlayer::new(id, 0)).collect());
        let forbidden_02 = [PlayerId::new(0), PlayerId::new(2)];
        let forbidden_03 = [PlayerId::new(0), PlayerId::new(3)];
        let forbidden_12 = [PlayerId::new(1), PlayerId::new(2)];
        let forbidden_13 = [PlayerId::new(1), PlayerId::new(3)];
        let forbidden_23 = [PlayerId::new(2), PlayerId::new(3)];
        for players in [
            &forbidden_02,
            &forbidden_03,
            &forbidden_12,
            &forbidden_13,
            &forbidden_23,
        ] {
            tournament.push_forbidden_pair(ForbiddenPairEntry {
                round_range: 0..1,
                players,
            });
        }

        // Player 4 is the nominal TPN choice, but removing player 4 leaves
        // only edge 0-1 among players 0-3. Removing player 3 leaves 0-1, 2-4.
        assert_eq!(
            bye_player(&compute_matching(input(&tournament, 0)).unwrap()),
            3
        );
    }
}
