mod bye;
mod colors;
mod errors;
mod forbidden;
mod input;
mod metrics;
mod player;
mod scores;
mod solver;
mod weights;

use crate::swiss::{PairingFailure, RoundPairings};

use errors::compute_matching_error_from_status;
pub(crate) use input::BursteinInput;
use solver::burstein_compute_matching;

#[cfg(test)]
use crate::domain::{
    Color, ForbiddenPairEntry, GameSideOutcome, MatchScore, PlayerColorData, PlayerId, PointSystem,
    Score,
};
#[cfg(test)]
use crate::limits::NATIVE_USIZE_MAX;
#[cfg(test)]
use crate::swiss::{
    LogicalGame, LogicalGames, MatchDisposition, PlayerRoundRecord, SwissPairingPlayerEntry,
    SwissPairingSnapshot, SwissPrimaryScoreProjection,
};
#[cfg(test)]
use errors::{BursteinComputeResult, BursteinStatus};

pub(crate) fn compute_matching(input: BursteinInput<'_>) -> Result<RoundPairings, PairingFailure> {
    burstein_compute_matching(&input)
        .map_err(|failure| compute_matching_error_from_status(failure.status))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swisssystems::common::PairingContext;

    #[derive(Clone)]
    struct TestPlayer {
        id: usize,
        score: u32,
        rank: usize,
        color_preference: Option<Color>,
        records: Vec<PlayerRoundRecord>,
    }

    struct TestForbiddenEntry {
        round_start: usize,
        round_end: usize,
        players: Vec<usize>,
    }

    struct TestInputData {
        snapshot: SwissPairingSnapshot,
        played_rounds: usize,
    }

    impl TestPlayer {
        fn new(id: usize, score: u32) -> Self {
            Self {
                id,
                score,
                rank: id,
                color_preference: None,
                records: Vec::new(),
            }
        }
    }

    impl TestInputData {
        fn new(
            players: Vec<TestPlayer>,
            forbidden_entries: Vec<TestForbiddenEntry>,
            played_rounds: usize,
        ) -> Self {
            let mut data = Self {
                snapshot: SwissPairingSnapshot::with_scoring(
                    SwissPrimaryScoreProjection::GamePoints,
                    Score::new(10),
                ),
                played_rounds,
            };

            for player in players {
                data.snapshot.push_player(SwissPairingPlayerEntry {
                    id: PlayerId::new(player.id),
                    score_without_acceleration: Score::new(player.score),
                    rank_index: player.rank,
                    played_games: player
                        .records
                        .iter()
                        .flat_map(|record| record.games().iter())
                        .filter(|game| game.outcome.counts_as_played())
                        .count(),
                    played_matches: player
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
                        .count(),
                    color_data: PlayerColorData {
                        color_imbalance: 0,
                        color_preference: player.color_preference,
                        repeated_color: None,
                        strong_color_preference: false,
                    },
                    is_valid: true,
                    records: &player.records,
                    accelerations: &[],
                });
            }

            for entry in forbidden_entries {
                let players = entry
                    .players
                    .into_iter()
                    .map(PlayerId::new)
                    .collect::<Vec<_>>();
                data.snapshot.push_forbidden_pair(ForbiddenPairEntry {
                    round_range: entry.round_start..entry.round_end,
                    players: &players,
                });
            }

            data
        }

        fn input(&self) -> BursteinInput<'_> {
            BursteinInput {
                pairing: PairingContext {
                    tournament: &self.snapshot,
                    played_rounds: self.played_rounds,
                    point_system: PointSystem {
                        win: Score::new(10),
                        draw: Score::new(5),
                        loss: Score::new(0),
                        forfeit_loss: Score::new(0),
                    },
                    initial_color: Color::White,
                },
                max_points: Score::new(1998),
                adjusted_score_type_max: NATIVE_USIZE_MAX,
                metric_type_max: NATIVE_USIZE_MAX,
            }
        }
    }

    fn played_match(opponent: usize, color: Color, score: MatchScore) -> PlayerRoundRecord {
        PlayerRoundRecord::Match {
            opponent: PlayerId::new(opponent),
            scheduled_first_color: color,
            state: crate::swiss::SwissMatchRecord::Complete {
                games: LogicalGames::single(LogicalGame {
                    color,
                    outcome: GameSideOutcome::Played(score),
                }),
                disposition: MatchDisposition::Regular,
                aggregate: score,
            },
        }
    }

    fn compute_ok(input: &BursteinInput<'_>) -> BursteinComputeResult {
        match burstein_compute_matching(input) {
            Ok(result) => result,
            Err(failure) => panic!("unexpected Burstein failure status {:?}", failure.status),
        }
    }

    fn compute_status(input: &BursteinInput<'_>) -> BursteinStatus {
        match burstein_compute_matching(input) {
            Ok(_) => panic!("expected Burstein failure"),
            Err(failure) => failure.status,
        }
    }

    fn contains_pair(result: &BursteinComputeResult, white: usize, black: usize) -> bool {
        if white == black {
            result.byes.iter().any(|bye| bye.player.index() == white)
        } else {
            result
                .games
                .iter()
                .any(|pairing| pairing.white().index() == white && pairing.black().index() == black)
        }
    }

    #[test]
    fn compute_matching_assigns_bye_to_lowest_eligible_player() {
        let data = TestInputData::new(
            vec![
                TestPlayer::new(0, 20),
                TestPlayer::new(1, 10),
                TestPlayer::new(2, 0),
            ],
            Vec::new(),
            0,
        );

        let result = compute_ok(&data.input());

        assert_eq!(result.games.len() + result.byes.len(), 2);
        assert!(contains_pair(&result, 2, 2));
    }

    #[test]
    fn compute_matching_reports_no_bye_candidate() {
        let players = (0..3)
            .map(|id| {
                let mut player = TestPlayer::new(id, 10);
                player.records.push(PlayerRoundRecord::PairingAllocatedBye);
                player
            })
            .collect();
        let data = TestInputData::new(players, Vec::new(), 1);

        assert_eq!(compute_status(&data.input()), BursteinStatus::NoBye);
    }

    #[test]
    fn compute_matching_reports_no_valid_pairing_when_all_edges_are_forbidden() {
        let data = TestInputData::new(
            vec![
                TestPlayer::new(0, 10),
                TestPlayer::new(1, 10),
                TestPlayer::new(2, 10),
                TestPlayer::new(3, 10),
            ],
            vec![TestForbiddenEntry {
                round_start: 0,
                round_end: 1,
                players: vec![0, 1, 2, 3],
            }],
            0,
        );

        assert_eq!(compute_status(&data.input()), BursteinStatus::NoMatching);
    }

    #[test]
    fn compute_matching_downfloats_when_top_group_cannot_pair_internally() {
        let data = TestInputData::new(
            vec![
                TestPlayer::new(0, 20),
                TestPlayer::new(1, 20),
                TestPlayer::new(2, 10),
                TestPlayer::new(3, 10),
            ],
            vec![TestForbiddenEntry {
                round_start: 0,
                round_end: 1,
                players: vec![0, 1],
            }],
            0,
        );

        let result = compute_ok(&data.input());

        assert_eq!(result.games.len() + result.byes.len(), 2);
        assert!(!contains_pair(&result, 0, 1));
        assert!(!contains_pair(&result, 1, 0));
    }

    #[test]
    fn burstein_bye_prefers_most_games_after_lowest_score() {
        let mut players = [10, 10, 15, 15, 20, 15, 25]
            .into_iter()
            .enumerate()
            .map(|(id, score)| TestPlayer::new(id, score))
            .collect::<Vec<_>>();
        players[0].records.extend([
            played_match(2, Color::White, MatchScore::Win),
            played_match(3, Color::White, MatchScore::Loss),
            played_match(4, Color::White, MatchScore::Loss),
        ]);
        players[1].records.extend([
            played_match(6, Color::White, MatchScore::Loss),
            PlayerRoundRecord::RequestedBye,
            played_match(5, Color::White, MatchScore::Win),
        ]);
        players[2].records.extend([
            played_match(0, Color::Black, MatchScore::Loss),
            played_match(4, Color::White, MatchScore::Win),
            played_match(3, Color::White, MatchScore::Draw),
        ]);
        players[3].records.extend([
            played_match(5, Color::White, MatchScore::Loss),
            played_match(0, Color::Black, MatchScore::Win),
            played_match(2, Color::Black, MatchScore::Draw),
        ]);
        players[4].records.extend([
            PlayerRoundRecord::PairingAllocatedBye,
            played_match(2, Color::Black, MatchScore::Loss),
            played_match(0, Color::Black, MatchScore::Win),
        ]);
        players[5].records.extend([
            played_match(3, Color::Black, MatchScore::Win),
            played_match(6, Color::White, MatchScore::Draw),
            played_match(1, Color::Black, MatchScore::Loss),
        ]);
        players[6].records.extend([
            played_match(1, Color::Black, MatchScore::Win),
            played_match(5, Color::Black, MatchScore::Draw),
            PlayerRoundRecord::PairingAllocatedBye,
        ]);
        let data = TestInputData::new(players, Vec::new(), 3);
        let input = data.input();
        let projected_players = player::build_players(&input).expect("well-formed player history");
        let adjusted_scores = scores::compute_adjusted_scores(&input, &projected_players)
            .expect("representable adjusted scores");
        let metric_scores =
            metrics::compute_metric_scores(&input, &projected_players, &adjusted_scores)
                .expect("representable Burstein metrics");

        assert_eq!(projected_players[0].base.played_games, 3);
        assert_eq!(projected_players[1].base.played_games, 2);
        assert_eq!(metric_scores[0].buchholz, metric_scores[1].buchholz);
        assert_eq!(
            metric_scores[0].sonneborn_berger,
            metric_scores[1].sonneborn_berger
        );
        assert!(metrics::metric_scores_less_than(
            metric_scores[1],
            metric_scores[0]
        ));

        let result = compute_ok(&input);

        // Players 0 and 1 have equal score, BH, and SB, and either bye leaves
        // a complete legal pairing. Player 0's extra game must therefore beat
        // player 1's later TPN in the next comparison.
        assert!(contains_pair(&result, 0, 0));
    }

    #[test]
    fn burstein_bye_skips_a_candidate_that_prevents_a_complete_pairing() {
        let forbidden_pairs = [
            (0, 2),
            (0, 3),
            (0, 4),
            (1, 2),
            (1, 3),
            (1, 4),
            (2, 3),
            (3, 4),
        ]
        .into_iter()
        .map(|(left, right)| TestForbiddenEntry {
            round_start: 0,
            round_end: 1,
            players: vec![left, right],
        })
        .collect();
        let data = TestInputData::new(
            (0..5).map(|id| TestPlayer::new(id, 0)).collect(),
            forbidden_pairs,
            0,
        );

        let result = compute_ok(&data.input());

        assert!(contains_pair(&result, 3, 3));
        assert!(contains_pair(&result, 0, 1) || contains_pair(&result, 1, 0));
        assert!(contains_pair(&result, 2, 4) || contains_pair(&result, 4, 2));
    }
}
