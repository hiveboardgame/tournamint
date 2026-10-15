use super::{
    DoubleSwissInput, OraclePairingFailure, compute_matching_oracle_with_guard,
    compute_matching_ordered,
};
use crate::domain::{
    AdjudicatedSideResult, Color, ForbiddenPairEntry, GameSideOutcome, MatchScore, PlayerColorData,
    PlayerId, PointSystem, Score,
};
use crate::swiss::{
    ByeKind, LogicalGame, LogicalGames, MatchDisposition, MatchPointSystem, PairingError,
    PairingFailure, PlayerRoundRecord, RoundPairings, SwissPairingPlayerEntry,
    SwissPairingSnapshot, SwissPrimaryScoreProjection,
};
use crate::swisssystems::common::PairingContext;

const ORACLE_GUARD_LIMIT: usize = 500_000;
const GAME_POINTS: PointSystem = PointSystem::STANDARD;

#[derive(Clone, Copy, Debug)]
enum PrimaryContext {
    GamePoints,
    MatchPoints,
}

impl PrimaryContext {
    fn scoring(self) -> (SwissPrimaryScoreProjection, Score) {
        match self {
            Self::GamePoints => (SwissPrimaryScoreProjection::GamePoints, Score::new(3)),
            Self::MatchPoints => (
                SwissPrimaryScoreProjection::MatchPoints(MatchPointSystem::STANDARD),
                Score::new(2),
            ),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum CompletedMatch {
    Played([MatchScore; 2]),
    SplitForfeit,
    FullForfeit,
    DoubleForfeit,
}

#[derive(Debug, Eq, PartialEq)]
struct CanonicalRound {
    pab: Option<usize>,
    unordered_pairs: Vec<(usize, usize)>,
    white_by_pair: Vec<((usize, usize), usize)>,
}

fn input(
    tournament: &SwissPairingSnapshot,
    played_rounds: usize,
    expected_rounds: usize,
) -> DoubleSwissInput<'_> {
    DoubleSwissInput {
        pairing: PairingContext {
            tournament,
            played_rounds,
            point_system: GAME_POINTS,
            initial_color: Color::White,
        },
        expected_rounds,
        points_type_max: usize::MAX,
    }
}

fn canonical_round(round: RoundPairings) -> CanonicalRound {
    assert!(round.byes.len() <= 1, "Double-Swiss has at most one PAB");
    let pab = round.byes.first().map(|bye| {
        assert_eq!(bye.kind, ByeKind::PairingAllocated);
        bye.player.index()
    });
    let mut white_by_pair = round
        .games
        .into_iter()
        .map(|game| {
            let white = game.white().index();
            let black = game.black().index();
            ((white.min(black), white.max(black)), white)
        })
        .collect::<Vec<_>>();
    white_by_pair.sort_unstable_by_key(|(pair, _white)| *pair);
    let unordered_pairs = white_by_pair.iter().map(|(pair, _white)| *pair).collect();
    CanonicalRound {
        pab,
        unordered_pairs,
        white_by_pair,
    }
}

fn ordered_result(
    tournament: &SwissPairingSnapshot,
    played_rounds: usize,
    expected_rounds: usize,
) -> Result<CanonicalRound, PairingError> {
    compute_matching_ordered(input(tournament, played_rounds, expected_rounds))
        .map(canonical_round)
        .map_err(PairingFailure::pairing_error)
}

fn oracle_result(
    tournament: &SwissPairingSnapshot,
    played_rounds: usize,
    expected_rounds: usize,
    case: &str,
) -> Result<CanonicalRound, PairingError> {
    match compute_matching_oracle_with_guard(
        input(tournament, played_rounds, expected_rounds),
        ORACLE_GUARD_LIMIT,
    ) {
        Ok((round, _attempts)) => Ok(canonical_round(round)),
        Err(OraclePairingFailure::GuardExceeded) => {
            panic!("exhaustive oracle exceeded its {ORACLE_GUARD_LIMIT}-attempt guard in {case}")
        }
        Err(OraclePairingFailure::Pairing(error)) => Err(error),
    }
}

fn compare_case(
    tournament: &SwissPairingSnapshot,
    played_rounds: usize,
    expected_rounds: usize,
    case: &str,
) -> Result<CanonicalRound, PairingError> {
    let ordered = ordered_result(tournament, played_rounds, expected_rounds);
    let oracle = oracle_result(tournament, played_rounds, expected_rounds, case);
    assert_eq!(ordered, oracle, "ordered/oracle mismatch in {case}");
    ordered
}

fn inverse_outcome(outcome: GameSideOutcome) -> GameSideOutcome {
    match outcome {
        GameSideOutcome::Played(score) => GameSideOutcome::Played(score.invert()),
        GameSideOutcome::Adjudicated(AdjudicatedSideResult::ForfeitWin) => {
            GameSideOutcome::Adjudicated(AdjudicatedSideResult::ForfeitLoss)
        }
        GameSideOutcome::Adjudicated(AdjudicatedSideResult::ForfeitLoss) => {
            GameSideOutcome::Adjudicated(AdjudicatedSideResult::ForfeitWin)
        }
        GameSideOutcome::Adjudicated(AdjudicatedSideResult::Draw) => {
            GameSideOutcome::Adjudicated(AdjudicatedSideResult::Draw)
        }
        GameSideOutcome::Adjudicated(AdjudicatedSideResult::DoubleForfeit) => {
            GameSideOutcome::Adjudicated(AdjudicatedSideResult::DoubleForfeit)
        }
    }
}

fn inverse_disposition(disposition: MatchDisposition) -> MatchDisposition {
    match disposition {
        MatchDisposition::Regular => MatchDisposition::Regular,
        MatchDisposition::ForfeitWin => MatchDisposition::ForfeitLoss,
        MatchDisposition::ForfeitLoss => MatchDisposition::ForfeitWin,
        MatchDisposition::DoubleForfeit => MatchDisposition::DoubleForfeit,
    }
}

fn match_semantics(
    completed: CompletedMatch,
) -> ([GameSideOutcome; 2], MatchDisposition, MatchScore) {
    match completed {
        CompletedMatch::Played(scores) => (
            scores.map(GameSideOutcome::Played),
            MatchDisposition::Regular,
            MatchScore::from_ordering(
                GAME_POINTS
                    .points_for(GameSideOutcome::Played(scores[0]))
                    .value()
                    .checked_add(
                        GAME_POINTS
                            .points_for(GameSideOutcome::Played(scores[1]))
                            .value(),
                    )
                    .expect("two game scores fit")
                    .cmp(
                        &GAME_POINTS
                            .points_for(GameSideOutcome::Played(scores[0].invert()))
                            .value()
                            .checked_add(
                                GAME_POINTS
                                    .points_for(GameSideOutcome::Played(scores[1].invert()))
                                    .value(),
                            )
                            .expect("two game scores fit"),
                    ),
            ),
        ),
        CompletedMatch::SplitForfeit => (
            [
                GameSideOutcome::Adjudicated(AdjudicatedSideResult::ForfeitWin),
                GameSideOutcome::Adjudicated(AdjudicatedSideResult::ForfeitLoss),
            ],
            MatchDisposition::Regular,
            MatchScore::Draw,
        ),
        CompletedMatch::FullForfeit => (
            [
                GameSideOutcome::Adjudicated(AdjudicatedSideResult::ForfeitWin),
                GameSideOutcome::Adjudicated(AdjudicatedSideResult::ForfeitWin),
            ],
            MatchDisposition::ForfeitWin,
            MatchScore::Win,
        ),
        CompletedMatch::DoubleForfeit => (
            [
                GameSideOutcome::Adjudicated(AdjudicatedSideResult::DoubleForfeit),
                GameSideOutcome::Adjudicated(AdjudicatedSideResult::DoubleForfeit),
            ],
            MatchDisposition::DoubleForfeit,
            MatchScore::Draw,
        ),
    }
}

fn completed_record(
    opponent: usize,
    first_color: Color,
    outcomes: [GameSideOutcome; 2],
    disposition: MatchDisposition,
    aggregate: MatchScore,
) -> PlayerRoundRecord {
    PlayerRoundRecord::Match {
        opponent: PlayerId::new(opponent),
        scheduled_first_color: first_color,
        state: crate::swiss::SwissMatchRecord::Complete {
            games: LogicalGames::double(
                LogicalGame {
                    color: first_color,
                    outcome: outcomes[0],
                },
                LogicalGame {
                    color: first_color.opposite(),
                    outcome: outcomes[1],
                },
            ),
            disposition,
            aggregate,
        },
    }
}

fn append_completed_match(
    round: &mut [Option<PlayerRoundRecord>],
    player: usize,
    opponent: usize,
    first_color: Color,
    completed: CompletedMatch,
) {
    assert_ne!(player, opponent);
    let (outcomes, disposition, aggregate) = match_semantics(completed);
    assert!(round[player].is_none() && round[opponent].is_none());
    round[player] = Some(completed_record(
        opponent,
        first_color,
        outcomes,
        disposition,
        aggregate,
    ));
    round[opponent] = Some(completed_record(
        player,
        first_color.opposite(),
        outcomes.map(inverse_outcome),
        inverse_disposition(disposition),
        aggregate.invert(),
    ));
}

fn push_round(histories: &mut [Vec<PlayerRoundRecord>], round: Vec<Option<PlayerRoundRecord>>) {
    assert_eq!(histories.len(), round.len());
    for (history, record) in histories.iter_mut().zip(round) {
        history.push(record.expect("every player receives one round record"));
    }
}

fn reciprocal_record_is_consistent(
    player: usize,
    record: PlayerRoundRecord,
    opponent_record: PlayerRoundRecord,
) -> bool {
    let (
        PlayerRoundRecord::Match {
            opponent,
            scheduled_first_color,
            state:
                crate::swiss::SwissMatchRecord::Complete {
                    games,
                    disposition,
                    aggregate,
                },
        },
        PlayerRoundRecord::Match {
            opponent: reciprocal_opponent,
            scheduled_first_color: reciprocal_first_color,
            state:
                crate::swiss::SwissMatchRecord::Complete {
                    games: reciprocal_games,
                    disposition: reciprocal_disposition,
                    aggregate: reciprocal_aggregate,
                },
        },
    ) = (record, opponent_record)
    else {
        return false;
    };
    let games = games.iter().collect::<Vec<_>>();
    let reciprocal_games = reciprocal_games.iter().collect::<Vec<_>>();
    opponent.index() != player
        && reciprocal_opponent == PlayerId::new(player)
        && reciprocal_first_color == scheduled_first_color.opposite()
        && reciprocal_disposition == inverse_disposition(disposition)
        && reciprocal_aggregate == aggregate.invert()
        && games.len() == reciprocal_games.len()
        && games
            .iter()
            .zip(reciprocal_games)
            .all(|(game, reciprocal)| {
                reciprocal.color == game.color.opposite()
                    && reciprocal.outcome == inverse_outcome(game.outcome)
            })
}

fn validate_histories(histories: &[Vec<PlayerRoundRecord>]) {
    let round_count = histories.first().map_or(0, Vec::len);
    assert!(histories.iter().all(|history| history.len() == round_count));
    for (player, history) in histories.iter().enumerate() {
        let mut opponents = vec![false; histories.len()];
        for (round, record) in history.iter().copied().enumerate() {
            let PlayerRoundRecord::Match { opponent, .. } = record else {
                continue;
            };
            let opponent = opponent.index();
            assert!(opponent < histories.len() && opponent != player);
            assert!(!opponents[opponent], "fixture repeats {player}-{opponent}");
            opponents[opponent] = true;
            assert!(reciprocal_record_is_consistent(
                player,
                record,
                histories[opponent][round]
            ));
        }
    }
}

fn derived_color_data(records: &[PlayerRoundRecord]) -> PlayerColorData {
    let colors = records
        .iter()
        .filter_map(|record| match record {
            PlayerRoundRecord::Match {
                scheduled_first_color,
                state,
                ..
            } if state
                .games()
                .iter()
                .any(|game| game.outcome.counts_as_played()) =>
            {
                Some(*scheduled_first_color)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let games_as_white = colors
        .iter()
        .filter(|color| **color == Color::White)
        .count();
    let games_as_black = colors
        .iter()
        .filter(|color| **color == Color::Black)
        .count();
    let color_imbalance = games_as_white.abs_diff(games_as_black);
    let lower_color = if games_as_white > games_as_black {
        Color::Black
    } else {
        Color::White
    };
    let mut consecutive_count = 0usize;
    let mut last_color = None;
    for color in colors {
        if last_color != Some(color) {
            consecutive_count = 1;
        } else {
            consecutive_count += 1;
        }
        last_color = Some(color);
    }
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
    PlayerColorData {
        color_imbalance,
        color_preference,
        repeated_color,
        strong_color_preference: repeated_color.is_none() && color_imbalance == 1,
    }
}

fn build_snapshot(
    histories: &[Vec<PlayerRoundRecord>],
    rank_order: &[usize],
    primary: PrimaryContext,
    forbidden_pairs: &[(usize, usize)],
) -> SwissPairingSnapshot {
    validate_histories(histories);
    assert_eq!(histories.len(), rank_order.len());
    let (projection, pab_points) = primary.scoring();
    let mut tournament = SwissPairingSnapshot::with_scoring(projection, pab_points);
    let mut rank_indices = vec![None; histories.len()];
    for (rank, player) in rank_order.iter().copied().enumerate() {
        assert!(player < histories.len());
        assert!(rank_indices[player].replace(rank).is_none());
    }
    for (player, records) in histories.iter().enumerate() {
        let score = records
            .iter()
            .copied()
            .try_fold(0u32, |total, record| {
                total.checked_add(
                    tournament
                        .primary_points(record, GAME_POINTS)
                        .expect("completed fixture record has primary points")
                        .value(),
                )
            })
            .expect("small fixture score fits");
        tournament.push_player(SwissPairingPlayerEntry {
            id: PlayerId::new(player),
            is_valid: true,
            rank_index: rank_indices[player].expect("rank order is a permutation"),
            score_without_acceleration: Score::new(score),
            played_games: records
                .iter()
                .flat_map(|record| record.games().iter())
                .filter(|game| game.outcome.counts_as_played())
                .count(),
            played_matches: records
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
            color_data: derived_color_data(records),
            records,
            accelerations: &[],
        });
    }
    let played_rounds = histories.first().map_or(0, Vec::len);
    for &(left, right) in forbidden_pairs {
        assert!(left < histories.len() && right < histories.len() && left != right);
        let players = [PlayerId::new(left), PlayerId::new(right)];
        tournament.push_forbidden_pair(ForbiddenPairEntry {
            round_range: played_rounds..played_rounds + 1,
            players: &players,
        });
    }
    tournament
}

fn semantic_histories() -> Vec<Vec<PlayerRoundRecord>> {
    let mut histories = vec![Vec::new(); 8];
    let mut first = vec![None; 8];
    append_completed_match(
        &mut first,
        0,
        1,
        Color::White,
        CompletedMatch::Played([MatchScore::Win, MatchScore::Draw]),
    );
    append_completed_match(&mut first, 2, 3, Color::Black, CompletedMatch::SplitForfeit);
    append_completed_match(&mut first, 4, 5, Color::White, CompletedMatch::FullForfeit);
    append_completed_match(
        &mut first,
        6,
        7,
        Color::Black,
        CompletedMatch::DoubleForfeit,
    );
    push_round(&mut histories, first);

    let mut second = vec![None; 8];
    append_completed_match(
        &mut second,
        0,
        2,
        Color::Black,
        CompletedMatch::Played([MatchScore::Draw, MatchScore::Draw]),
    );
    append_completed_match(
        &mut second,
        1,
        3,
        Color::White,
        CompletedMatch::SplitForfeit,
    );
    append_completed_match(
        &mut second,
        4,
        6,
        Color::Black,
        CompletedMatch::Played([MatchScore::Loss, MatchScore::Win]),
    );
    append_completed_match(&mut second, 5, 7, Color::White, CompletedMatch::FullForfeit);
    push_round(&mut histories, second);
    histories
}

fn stranded_pab_histories() -> Vec<Vec<PlayerRoundRecord>> {
    let mut histories = vec![Vec::new(); 5];
    let mut round = vec![None; 5];
    append_completed_match(
        &mut round,
        0,
        1,
        Color::White,
        CompletedMatch::Played([MatchScore::Draw, MatchScore::Draw]),
    );
    append_completed_match(
        &mut round,
        2,
        3,
        Color::Black,
        CompletedMatch::Played([MatchScore::Draw, MatchScore::Draw]),
    );
    round[4] = Some(PlayerRoundRecord::RequestedBye);
    push_round(&mut histories, round);
    histories
}

#[derive(Clone, Copy)]
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }

    fn index(&mut self, len: usize) -> usize {
        (self.next() as usize) % len
    }
}

fn generated_histories(
    player_count: usize,
    played_rounds: usize,
    seed: u64,
) -> Vec<Vec<PlayerRoundRecord>> {
    assert!((4..=8).contains(&player_count));
    let field_size = player_count + (player_count & 1);
    let rotating_count = field_size - 1;
    assert!(played_rounds < rotating_count);
    let mut histories = vec![Vec::new(); player_count];
    let mut random = Lcg(seed);
    for round_index in 0..played_rounds {
        let mut round = vec![None; player_count];
        let first_pair = (field_size - 1, round_index % rotating_count);
        let pairs = core::iter::once(first_pair).chain((1..field_size / 2).map(|offset| {
            (
                (round_index + offset) % rotating_count,
                (round_index + rotating_count - offset) % rotating_count,
            )
        }));
        for (left, right) in pairs {
            if left >= player_count || right >= player_count {
                let player = left.min(right);
                round[player] = Some(PlayerRoundRecord::PairingAllocatedBye);
                continue;
            }
            let completed = match random.index(6) {
                0 => CompletedMatch::Played([MatchScore::Win, MatchScore::Draw]),
                1 => CompletedMatch::Played([MatchScore::Draw, MatchScore::Loss]),
                2 => CompletedMatch::Played([MatchScore::Draw, MatchScore::Draw]),
                3 => CompletedMatch::SplitForfeit,
                4 => CompletedMatch::FullForfeit,
                _ => CompletedMatch::DoubleForfeit,
            };
            let first_color = if random.next() & 1 == 0 {
                Color::White
            } else {
                Color::Black
            };
            append_completed_match(&mut round, left, right, first_color, completed);
        }
        push_round(&mut histories, round);
    }
    histories
}

#[test]
fn completed_history_semantics_match_in_both_primary_contexts() {
    let histories = semantic_histories();
    let ranks = [4, 1, 6, 0, 7, 2, 5, 3];
    for primary in [PrimaryContext::GamePoints, PrimaryContext::MatchPoints] {
        let tournament = build_snapshot(&histories, &ranks, primary, &[]);
        let scores = tournament
            .players()
            .map(|player| player.score_without_acceleration.value())
            .collect::<Vec<_>>();
        let expected_scores = match primary {
            PrimaryContext::GamePoints => vec![5, 3, 4, 4, 6, 4, 2, 0],
            PrimaryContext::MatchPoints => vec![3, 1, 2, 2, 3, 2, 1, 0],
        };
        assert_eq!(scores, expected_scores, "{primary:?} recurrence");
        let nonfinal = format!("{primary:?} semantic history, nonfinal");
        let final_round = format!("{primary:?} semantic history, final");
        let _ = compare_case(&tournament, 2, 4, &nonfinal);
        let _ = compare_case(&tournament, 2, 3, &final_round);
    }
}

#[test]
fn preferred_eligible_pab_that_strands_the_remainder_is_skipped() {
    let histories = stranded_pab_histories();
    // Alongside the historical 0-1 and 2-3 edges, these leave only 0-2,
    // 1-4, and 3-4 available. PAB candidate 4 is preferred on score but
    // strands 0-3; candidate 3 leaves 0-2 plus 1-4.
    let forbidden = [(0, 3), (0, 4), (1, 2), (1, 3), (2, 4)];
    for primary in [PrimaryContext::GamePoints, PrimaryContext::MatchPoints] {
        let tournament = build_snapshot(&histories, &[0, 1, 2, 3, 4], primary, &forbidden);
        let case = format!("{primary:?} stranded preferred PAB");
        let result = compare_case(&tournament, 1, 3, &case)
            .expect("the second PAB candidate leaves a legal remainder");
        assert_eq!(result.pab, Some(3));
        assert_eq!(result.unordered_pairs, vec![(0, 2), (1, 4)]);
    }
}

#[test]
fn valid_history_no_pairing_errors_match_exactly() {
    let mut histories = vec![Vec::new(); 4];
    let mut round = vec![None; 4];
    append_completed_match(
        &mut round,
        0,
        1,
        Color::White,
        CompletedMatch::Played([MatchScore::Draw, MatchScore::Draw]),
    );
    append_completed_match(
        &mut round,
        2,
        3,
        Color::Black,
        CompletedMatch::Played([MatchScore::Draw, MatchScore::Draw]),
    );
    push_round(&mut histories, round);
    for primary in [PrimaryContext::GamePoints, PrimaryContext::MatchPoints] {
        let tournament = build_snapshot(&histories, &[0, 1, 2, 3], primary, &[(0, 2), (0, 3)]);
        let case = format!("{primary:?} isolated after valid history");
        assert_eq!(
            compare_case(&tournament, 1, 3, &case),
            Err(PairingError::NoValidPairing)
        );
    }
}

#[test]
fn fixed_lcg_valid_histories_match_the_oracle() {
    for case_index in 0usize..12 {
        let player_count = 4 + case_index % 5;
        let played_rounds = 1 + case_index % 2;
        let seed = 0x6a09_e667_f3bc_c909u64 ^ (case_index as u64).wrapping_mul(0x9e37_79b9);
        let histories = generated_histories(player_count, played_rounds, seed);
        let mut ranks = (0..player_count).collect::<Vec<_>>();
        ranks.rotate_left(case_index % player_count);
        if case_index & 1 != 0 {
            ranks.reverse();
        }
        for primary in [PrimaryContext::GamePoints, PrimaryContext::MatchPoints] {
            let tournament = build_snapshot(&histories, &ranks, primary, &[]);
            let case = format!(
                "fixed LCG case={case_index} n={player_count} rounds={played_rounds} {primary:?}"
            );
            let _ = compare_case(&tournament, played_rounds, played_rounds + 2, &case);
        }
    }
}

fn previous_floater_flags(
    histories: &[Vec<PlayerRoundRecord>],
    primary: PrimaryContext,
) -> Vec<bool> {
    let previous_round = histories
        .first()
        .and_then(|history| history.len().checked_sub(1))
        .expect("floater fixture has history");
    let (projection, pab_points) = primary.scoring();
    let scoring = SwissPairingSnapshot::with_scoring(projection, pab_points);
    let score_before_previous_round = |player: usize| {
        histories[player][..previous_round]
            .iter()
            .copied()
            .map(|record| {
                scoring
                    .primary_points(record, GAME_POINTS)
                    .expect("completed fixture record has primary points")
                    .value()
            })
            .sum::<u32>()
    };
    histories
        .iter()
        .enumerate()
        .map(|(player, history)| {
            let PlayerRoundRecord::Match {
                opponent,
                state:
                    crate::swiss::SwissMatchRecord::Complete {
                        disposition: MatchDisposition::Regular,
                        ..
                    },
                ..
            } = history[previous_round]
            else {
                return false;
            };
            score_before_previous_round(player) != score_before_previous_round(opponent.index())
        })
        .collect()
}

#[test]
fn final_round_c7_exception_can_select_a_previous_floater() {
    let histories = generated_histories(5, 3, 81);
    assert_eq!(
        previous_floater_flags(&histories, PrimaryContext::GamePoints),
        vec![false, true, false, true, false]
    );
    let tournament = build_snapshot(
        &histories,
        &[0, 1, 2, 3, 4],
        PrimaryContext::GamePoints,
        &[],
    );
    assert_eq!(
        tournament
            .players()
            .map(|player| player.score_without_acceleration.value())
            .collect::<Vec<_>>(),
        vec![3, 7, 10, 6, 7]
    );

    let nonfinal = compare_case(&tournament, 3, 5, "C7 valid history, nonfinal").unwrap();
    let final_round = compare_case(&tournament, 3, 4, "C7 valid history, final").unwrap();
    assert_eq!(nonfinal.pab, Some(3));
    assert_eq!(final_round.pab, Some(3));
    // Player 2's odd top group takes non-floater 4 in a nonfinal round;
    // without C7 in the final round, TPN selects prior floater 1 instead.
    assert_eq!(nonfinal.unordered_pairs, vec![(0, 1), (2, 4)]);
    assert_eq!(final_round.unordered_pairs, vec![(0, 4), (1, 2)]);
    assert_eq!(nonfinal.white_by_pair, vec![((0, 1), 0), ((2, 4), 2)]);
    assert_eq!(final_round.white_by_pair, vec![((0, 4), 0), ((1, 2), 2)]);
}

#[test]
fn final_round_c8_exception_can_pair_an_upfloater_with_a_previous_floater() {
    let histories = generated_histories(5, 3, 96);
    assert_eq!(
        previous_floater_flags(&histories, PrimaryContext::MatchPoints),
        vec![false, true, false, true, false]
    );
    let tournament = build_snapshot(
        &histories,
        &[0, 1, 2, 3, 4],
        PrimaryContext::MatchPoints,
        &[],
    );
    assert_eq!(
        tournament
            .players()
            .map(|player| player.score_without_acceleration.value())
            .collect::<Vec<_>>(),
        vec![4, 4, 3, 1, 4]
    );

    let nonfinal = compare_case(&tournament, 3, 5, "C8 valid history, nonfinal").unwrap();
    let final_round = compare_case(&tournament, 3, 4, "C8 valid history, final").unwrap();
    assert_eq!(nonfinal.pab, Some(3));
    assert_eq!(final_round.pab, Some(3));
    // Player 2 is the sole lower-score upfloater in both rounds. C8 pairs it
    // with non-floater 4 outside the final round; the final exception permits
    // TPN-preferred prior floater 1 as its opponent.
    assert_eq!(nonfinal.unordered_pairs, vec![(0, 1), (2, 4)]);
    assert_eq!(final_round.unordered_pairs, vec![(0, 4), (1, 2)]);
    assert_eq!(nonfinal.white_by_pair, vec![((0, 1), 0), ((2, 4), 4)]);
    assert_eq!(final_round.white_by_pair, vec![((0, 4), 0), ((1, 2), 1)]);
}
