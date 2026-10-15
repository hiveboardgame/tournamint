use crate::domain::{ForbiddenPairEntry, PlayerColorData, Score};
use crate::limits::NATIVE_USIZE_MAX;
use crate::swiss::SwissCalculation;
use crate::swiss::{
    AccelerationPolicy, DutchConfig, RoundPairings, SingleGameSwissByePoints, SwissConfig,
    SwissPairingPlayerEntry, SwissPairingSnapshot, SwissStandingsPlan, SwissSystemConfig,
};
use crate::swisssystems::common::PairingContext;
use crate::{Color, GameOutcome, Pairing, PlayerId, PointSystem};

use super::DutchInput;

#[derive(Clone, Copy)]
enum HistoryShape {
    ExactThree,
    Spread,
}

fn field(player_count: usize) -> Vec<PlayerId> {
    (0..player_count).map(PlayerId::new).collect()
}

fn stored_round(field: &[PlayerId], round_index: usize) -> Vec<Pairing> {
    let half = field.len() / 2;
    (0..half)
        .map(|index| {
            let even = field[index * 2];
            let odd = field[((index + round_index) % half) * 2 + 1];
            if round_index & 1 == 0 {
                Pairing::new(even, odd)
            } else {
                Pairing::new(odd, even)
            }
        })
        .collect()
}

fn seeded_tournament(shape: HistoryShape, player_count: usize) -> (SwissCalculation, Vec<usize>) {
    let field = field(player_count);
    let point_system = PointSystem::STANDARD;
    let mut tournament = SwissCalculation::new(
        SwissConfig {
            system: SwissSystemConfig::Dutch(DutchConfig {
                bye_points: SingleGameSwissByePoints::game_win(point_system),
            }),
            expected_rounds: 9,
            initial_color: Color::White,
            game_point_system: point_system,
            acceleration: AccelerationPolicy::None,
            standings: SwissStandingsPlan::default(),
        },
        &field,
    )
    .unwrap();
    let mut half_points = vec![0usize; player_count];

    for round_index in 0..4 {
        let games = stored_round(&field, round_index);
        tournament
            .accept_round_fact(RoundPairings {
                byes: Vec::new(),
                games: games.clone(),
            })
            .unwrap();
        for (board_index, pairing) in games.into_iter().enumerate() {
            let outcome = match shape {
                HistoryShape::ExactThree if round_index < 2 => {
                    if (board_index + round_index) & 1 == 0 {
                        half_points[pairing.white().index()] += 2;
                        GameOutcome::white_win()
                    } else {
                        half_points[pairing.black().index()] += 2;
                        GameOutcome::black_win()
                    }
                }
                HistoryShape::ExactThree => {
                    half_points[pairing.white().index()] += 1;
                    half_points[pairing.black().index()] += 1;
                    GameOutcome::draw()
                }
                HistoryShape::Spread => match (board_index + round_index) % 3 {
                    0 => {
                        half_points[pairing.white().index()] += 2;
                        GameOutcome::white_win()
                    }
                    1 => {
                        half_points[pairing.black().index()] += 2;
                        GameOutcome::black_win()
                    }
                    _ => {
                        half_points[pairing.white().index()] += 1;
                        half_points[pairing.black().index()] += 1;
                        GameOutcome::draw()
                    }
                },
            };
            tournament
                .apply_outcome_fact(pairing.white(), pairing.black(), outcome)
                .unwrap();
        }
    }

    (tournament, half_points)
}

fn assert_snapshot_closed_solver_matches_global_solver(
    snapshot: &SwissPairingSnapshot,
    played_rounds: usize,
    expected_closed_path: bool,
) {
    let input = DutchInput {
        pairing: PairingContext {
            tournament: snapshot,
            played_rounds,
            point_system: PointSystem::STANDARD,
            initial_color: Color::White,
        },
        expected_rounds: 9,
        points_type_max: NATIVE_USIZE_MAX,
    };
    assert_eq!(
        super::solver::closed_score_group_path_is_available(&input).unwrap(),
        expected_closed_path
    );
    let closed = super::solver::compute_matching_impl(&input).unwrap();
    let global = super::solver::compute_matching_impl_without_closed_score_groups(&input).unwrap();
    assert_eq!(closed, global);
}

fn assert_closed_solver_matches_global_solver(
    tournament: &SwissCalculation,
    expected_closed_path: bool,
) {
    let snapshot = tournament.snapshot();
    assert_snapshot_closed_solver_matches_global_solver(
        &snapshot,
        tournament.played_rounds(),
        expected_closed_path,
    );
}

fn synthetic_snapshot(
    scores: &[u32],
    rank_offset: usize,
    forbidden_pairs: &[(usize, usize)],
    accelerated: bool,
    inactive_players: &[usize],
) -> SwissPairingSnapshot {
    let mut snapshot = SwissPairingSnapshot::new();
    for (player, &score) in scores.iter().enumerate() {
        let color_preference = match (player + rank_offset) % 3 {
            0 => Some(Color::White),
            1 => Some(Color::Black),
            _ => None,
        };
        let acceleration = [Score::new(score)];
        snapshot.push_player(SwissPairingPlayerEntry {
            id: PlayerId::new(player),
            is_valid: !inactive_players.contains(&player),
            rank_index: (player + rank_offset) % scores.len(),
            score_without_acceleration: Score::new(if accelerated { 0 } else { score }),
            played_games: 0,
            played_matches: 0,
            color_data: PlayerColorData {
                color_imbalance: usize::from(color_preference.is_some()),
                color_preference,
                repeated_color: None,
                strong_color_preference: color_preference.is_some(),
            },
            records: &[],
            accelerations: if accelerated { &acceleration } else { &[] },
        });
    }
    for &(left, right) in forbidden_pairs {
        let players = [PlayerId::new(left), PlayerId::new(right)];
        snapshot.push_forbidden_pair(ForbiddenPairEntry {
            round_range: 0..1,
            players: &players,
        });
    }
    snapshot
}

#[test]
fn independently_closed_scoregroups_match_the_global_solver_exactly() {
    for shape in [HistoryShape::ExactThree, HistoryShape::Spread] {
        for player_count in [16, 32] {
            let (tournament, _) = seeded_tournament(shape, player_count);
            assert_closed_solver_matches_global_solver(&tournament, true);
        }
    }
}

#[test]
fn a_scoregroup_without_an_internal_completion_falls_back_to_the_global_solver() {
    let (mut tournament, half_points) = seeded_tournament(HistoryShape::ExactThree, 16);
    let top_score = half_points.iter().copied().max().unwrap();
    let top_group = half_points
        .iter()
        .enumerate()
        .filter_map(|(player, &score)| (score == top_score).then_some(PlayerId::new(player)))
        .collect::<Vec<_>>();
    assert_eq!(top_group.len(), 4);
    tournament
        .add_calculation_restriction(&top_group, 4..9)
        .unwrap();

    assert_closed_solver_matches_global_solver(&tournament, false);
}

#[test]
fn a_later_scoregroup_failure_rejects_the_closed_path_before_accepting_results() {
    let (mut tournament, half_points) = seeded_tournament(HistoryShape::ExactThree, 16);
    let bottom_score = half_points.iter().copied().min().unwrap();
    let bottom_group = half_points
        .iter()
        .enumerate()
        .filter_map(|(player, &score)| (score == bottom_score).then_some(PlayerId::new(player)))
        .collect::<Vec<_>>();
    assert_eq!(bottom_group.len(), 4);
    tournament
        .add_calculation_restriction(&bottom_group, 4..9)
        .unwrap();

    assert_closed_solver_matches_global_solver(&tournament, false);
}

#[test]
fn closed_scoregroups_match_global_search_across_rank_color_and_forbidden_variants() {
    let score_cases = [
        vec![6, 6, 4, 4],
        vec![6, 6, 6, 6, 4, 4],
        vec![6, 6, 4, 4, 2, 2],
        vec![8, 8, 8, 8, 6, 6, 6, 6],
    ];

    for scores in score_cases {
        for rank_offset in 0..scores.len() {
            let mut forbidden_pairs = Vec::new();
            for left in 0..scores.len() {
                for right in left + 1..scores.len() {
                    let is_witness_pair = left & 1 == 0 && right == left + 1;
                    if !is_witness_pair && (left * scores.len() + right + rank_offset) % 3 == 0 {
                        forbidden_pairs.push((left, right));
                    }
                }
            }
            let snapshot = synthetic_snapshot(&scores, rank_offset, &forbidden_pairs, false, &[]);
            assert_snapshot_closed_solver_matches_global_solver(&snapshot, 0, true);
        }
    }
}

#[test]
fn odd_fields_and_odd_scoregroups_bypass_the_closed_path() {
    for scores in [vec![6, 6, 4, 4, 2], vec![6, 6, 6, 4, 4, 4]] {
        let snapshot = synthetic_snapshot(&scores, 0, &[], false, &[]);
        assert_snapshot_closed_solver_matches_global_solver(&snapshot, 0, false);
    }
}

#[test]
fn accelerated_scoregroups_and_inactive_players_use_the_closed_path() {
    let scores = [6, 6, 6, 6, 4, 4, 4, 4, 2, 2];
    let snapshot = synthetic_snapshot(&scores, 3, &[], true, &[2, 3]);

    assert_snapshot_closed_solver_matches_global_solver(&snapshot, 0, true);
}
