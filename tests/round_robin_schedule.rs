use std::collections::{HashMap, HashSet};

use tournamint::round_robin::{
    self, RoundRobinConfig, RoundRobinCriterion, RoundRobinError, RoundRobinGameFact,
    RoundRobinSchedule, RoundRobinStandingsPlan,
};
use tournamint::{Color, GameOutcome, MatchPointSystem, PlayerId, PointSystem};

fn config(repeats: u32, initial_color: Color) -> RoundRobinConfig {
    RoundRobinConfig {
        repeats,
        initial_color,
        point_system: PointSystem::STANDARD,
        primary_score: Default::default(),
        match_point_system: MatchPointSystem::STANDARD,
        standings: RoundRobinStandingsPlan::new(vec![RoundRobinCriterion::PrimaryScore]),
    }
}

fn field(count: usize) -> Vec<PlayerId> {
    (1..=count).map(PlayerId::new).collect()
}

fn boards(schedule: &RoundRobinSchedule) -> Vec<Vec<(usize, usize)>> {
    schedule
        .rounds
        .iter()
        .map(|round| {
            round
                .games
                .iter()
                .map(|game| (game.white.index(), game.black.index()))
                .collect()
        })
        .collect()
}

#[test]
fn six_player_single_pass_matches_the_berger_table() {
    // FIDE Handbook C.05 Annex 1, table for 5 or 6 players (White listed first).
    let schedule = round_robin::schedule(&config(1, Color::White), &field(6)).unwrap();
    assert_eq!(
        boards(&schedule),
        vec![
            vec![(1, 6), (2, 5), (3, 4)],
            vec![(6, 4), (5, 3), (1, 2)],
            vec![(2, 6), (3, 1), (4, 5)],
            vec![(6, 5), (1, 4), (2, 3)],
            vec![(3, 6), (4, 2), (5, 1)],
        ]
    );
}

#[test]
fn double_pass_fixture_freezes_round_order_ids_colours_and_repeat_links() {
    let schedule = round_robin::schedule(&config(2, Color::White), &field(4)).unwrap();
    assert_eq!(
        boards(&schedule),
        vec![
            vec![(1, 4), (2, 3)],
            vec![(2, 4), (3, 1)],
            vec![(4, 3), (1, 2)],
            vec![(4, 1), (3, 2)],
            vec![(3, 4), (2, 1)],
            vec![(4, 2), (1, 3)],
        ]
    );
    let identities: Vec<_> = schedule
        .rounds
        .iter()
        .flat_map(|round| &round.games)
        .map(|game| {
            (
                game.id.value(),
                game.round_index,
                game.pass_index,
                game.board_index,
                game.previous_repeat.map(|id| id.value()),
                game.next_repeat.map(|id| id.value()),
            )
        })
        .collect();
    assert_eq!(
        identities,
        vec![
            (0, 0, 0, 0, None, Some(6)),
            (1, 0, 0, 1, None, Some(7)),
            (2, 1, 0, 0, None, Some(10)),
            (3, 1, 0, 1, None, Some(11)),
            (4, 2, 0, 0, None, Some(8)),
            (5, 2, 0, 1, None, Some(9)),
            (6, 3, 1, 0, Some(0), None),
            (7, 3, 1, 1, Some(1), None),
            (8, 4, 1, 0, Some(4), None),
            (9, 4, 1, 1, Some(5), None),
            (10, 5, 1, 0, Some(2), None),
            (11, 5, 1, 1, Some(3), None),
        ]
    );
}

#[test]
fn odd_field_fixture_uses_the_highest_table_number_as_a_rest() {
    let schedule = round_robin::schedule(&config(2, Color::White), &field(3)).unwrap();
    assert_eq!(
        boards(&schedule),
        vec![
            vec![(2, 3)],
            vec![(3, 1)],
            vec![(1, 2)],
            vec![(3, 2)],
            vec![(2, 1)],
            vec![(1, 3)],
        ]
    );
    assert_eq!(
        schedule
            .rounds
            .iter()
            .map(|round| round.resting.map(PlayerId::index))
            .collect::<Vec<_>>(),
        vec![Some(1), Some(2), Some(3), Some(1), Some(3), Some(2)]
    );
}

#[test]
fn later_pass_fixture_freezes_the_nonfinal_round_swap_extension() {
    let schedule = round_robin::schedule(&config(3, Color::White), &field(4)).unwrap();
    assert_eq!(
        boards(&schedule),
        vec![
            vec![(1, 4), (2, 3)],
            vec![(2, 4), (3, 1)],
            vec![(4, 3), (1, 2)],
            vec![(4, 1), (3, 2)],
            vec![(4, 2), (1, 3)],
            vec![(3, 4), (2, 1)],
            vec![(1, 4), (2, 3)],
            vec![(4, 3), (1, 2)],
            vec![(2, 4), (3, 1)],
        ]
    );
}

#[test]
fn two_player_fixture_alternates_without_reordering_one_round_passes() {
    let schedule = round_robin::schedule(&config(3, Color::White), &field(2)).unwrap();
    assert_eq!(
        boards(&schedule),
        vec![vec![(1, 2)], vec![(2, 1)], vec![(1, 2)]]
    );
}

#[test]
fn schedules_cover_each_matchup_and_preserve_identity_for_the_hive_field_limits() {
    for count in 2..=16 {
        // Participant IDs need not be dense or sorted; table numbers follow input order.
        let players: Vec<_> = (0..count)
            .map(|index| PlayerId::new(100 + (count - index) * 7))
            .collect();
        for repeats in 1..=6 {
            let config = config(repeats, Color::White);
            let schedule = round_robin::schedule(&config, &players).unwrap();
            assert_eq!(schedule, round_robin::schedule(&config, &players).unwrap());
            let rounds_per_pass = count - usize::from(count.is_multiple_of(2));
            assert_eq!(schedule.rounds.len(), rounds_per_pass * repeats as usize);
            let all_games: Vec<_> = schedule
                .rounds
                .iter()
                .flat_map(|round| &round.games)
                .collect();
            assert_eq!(all_games.len(), count * (count - 1) / 2 * repeats as usize);
            let mut encounters: HashMap<_, Vec<_>> = HashMap::new();
            let mut colours: HashMap<_, Vec<_>> = HashMap::new();
            let mut pass_pairs = vec![HashSet::new(); repeats as usize];
            let mut pass_rests = vec![HashSet::new(); repeats as usize];

            for (round_index, round) in schedule.rounds.iter().enumerate() {
                let pass_index = round_index / rounds_per_pass;
                assert_eq!(round.round_index, round_index);
                assert_eq!(round.pass_index, pass_index);
                let mut present = HashSet::new();
                for (board_index, game) in round.games.iter().enumerate() {
                    assert_eq!(game.round_index, round_index);
                    assert_eq!(game.pass_index, pass_index);
                    assert_eq!(game.board_index, board_index);
                    assert!(present.insert(game.white));
                    assert!(present.insert(game.black));
                    let pair = (game.white.min(game.black), game.white.max(game.black));
                    assert!(pass_pairs[pass_index].insert(pair));
                    encounters.entry(pair).or_default().push(*game);
                    colours.entry(game.white).or_default().push(Color::White);
                    colours.entry(game.black).or_default().push(Color::Black);
                }
                if let Some(resting) = round.resting {
                    assert!(present.insert(resting));
                    assert!(pass_rests[pass_index].insert(resting));
                }
                assert_eq!(present, players.iter().copied().collect());
                assert_eq!(round.resting.is_some(), !count.is_multiple_of(2));
            }
            for (id, game) in all_games.iter().enumerate() {
                assert_eq!(game.id.value(), id);
            }
            for pass in 0..repeats as usize {
                assert_eq!(pass_pairs[pass].len(), count * (count - 1) / 2);
                assert_eq!(
                    pass_rests[pass].len(),
                    if count.is_multiple_of(2) { 0 } else { count }
                );
            }
            for games in encounters.values() {
                assert_eq!(games.len(), repeats as usize);
                for (pass, game) in games.iter().enumerate() {
                    assert_eq!(
                        game.previous_repeat,
                        pass.checked_sub(1).map(|i| games[i].id)
                    );
                    assert_eq!(game.next_repeat, games.get(pass + 1).map(|next| next.id));
                    if let Some(next) = games.get(pass + 1) {
                        assert_eq!((game.white, game.black), (next.black, next.white));
                    }
                }
            }
            for history in colours.values() {
                assert_eq!(history.len(), (count - 1) * repeats as usize);
                let whites = history
                    .iter()
                    .filter(|&&colour| colour == Color::White)
                    .count();
                let blacks = history.len() - whites;
                assert!(whites.abs_diff(blacks) <= 1);
                let mut longest = 0;
                let mut streak = 0;
                let mut previous = None;
                for &colour in history {
                    streak = if previous == Some(colour) {
                        streak + 1
                    } else {
                        1
                    };
                    longest = longest.max(streak);
                    previous = Some(colour);
                }
                // Repeated four-player fields can reach three; there is no universal max-two promise.
                assert!(
                    longest <= 3,
                    "{count} players, {repeats} passes: {history:?}"
                );
                if count == 6 && repeats == 2 {
                    assert!(
                        longest <= 2,
                        "six-player double-pass regression: {history:?}"
                    );
                }
            }

            let mut reversed_config = config.clone();
            reversed_config.initial_color = Color::Black;
            let reversed = round_robin::schedule(&reversed_config, &players).unwrap();
            let mut expected = schedule.clone();
            for round in &mut expected.rounds {
                for game in &mut round.games {
                    std::mem::swap(&mut game.white, &mut game.black);
                }
            }
            assert_eq!(reversed, expected);

            let facts: Vec<_> = all_games
                .iter()
                .map(|game| RoundRobinGameFact {
                    id: game.id,
                    outcome: if game.id.value().is_multiple_of(2) {
                        GameOutcome::white_win()
                    } else {
                        GameOutcome::black_win()
                    },
                })
                .collect();
            let projection = round_robin::project(&config, &players, &facts).unwrap();
            for (game, fact) in all_games.iter().zip(facts) {
                let projected = &projection.rounds[game.round_index].games[game.board_index];
                assert_eq!(projected.scheduled_game, **game);
                assert_eq!(projected.completed.unwrap().outcome, fact.outcome);
            }
        }
    }
}

#[test]
fn zero_repeats_remain_a_domain_validation_error() {
    let config = config(0, Color::White);
    assert_eq!(
        round_robin::schedule(&config, &field(2)),
        Err(RoundRobinError::ZeroRepeats)
    );
}
