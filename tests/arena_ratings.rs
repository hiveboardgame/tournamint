use std::collections::HashSet;

use tournamint::arena::{
    self, ArenaAwardedGameFact, ArenaConfig, ArenaPlayerFact, ArenaTerminalGameFact,
    ArenaTerminalResult,
};
use tournamint::{Pairing, PlayedGameOutcome, PlayerId};

fn player(id: usize, arena_rating: i32) -> ArenaPlayerFact {
    ArenaPlayerFact {
        player: PlayerId::new(id),
        arena_rating: Some(arena_rating),
        active: true,
        waiting_millis: Some(100),
    }
}

fn award_games(config: &ArenaConfig, games: &[ArenaTerminalGameFact]) -> Vec<ArenaAwardedGameFact> {
    games
        .iter()
        .enumerate()
        .map(|(index, game)| ArenaAwardedGameFact {
            game: *game,
            award: arena::award_game(config, &games[..index], game).unwrap(),
        })
        .collect()
}

fn uncolored_pairs(pairings: &[Pairing]) -> HashSet<[usize; 2]> {
    pairings
        .iter()
        .map(|pairing| {
            let mut players = [pairing.white().index(), pairing.black().index()];
            players.sort_unstable();
            players
        })
        .collect()
}

#[test]
fn performance_uses_each_terminal_opponent_snapshot_not_current_arena_rating() {
    let config = ArenaConfig::plain();
    let players = vec![player(0, 1612), player(1, 900), player(2, 1000)];
    let terminal_games = [
        ArenaTerminalGameFact {
            pairing: Pairing::new(PlayerId::new(0), PlayerId::new(1)),
            white_rating: Some(1600),
            black_rating: Some(1700),
            result: ArenaTerminalResult::Played(PlayedGameOutcome::WhiteWin),
            plies: 40,
            white_berserked: false,
            black_berserked: false,
        },
        ArenaTerminalGameFact {
            pairing: Pairing::new(PlayerId::new(2), PlayerId::new(0)),
            white_rating: Some(1801),
            black_rating: Some(1612),
            result: ArenaTerminalResult::Played(PlayedGameOutcome::Draw),
            plies: 40,
            white_berserked: false,
            black_berserked: false,
        },
    ];

    let terminal_games = award_games(&config, &terminal_games);
    let metrics = arena::player_metrics(&config, &players, &terminal_games).unwrap();
    let performance = metrics[0].performance.unwrap();
    assert_eq!(performance.numerator(), 4001);
    assert_eq!(performance.denominator(), 2);

    let changed_current_ratings = vec![player(0, 2200), player(1, 2500), player(2, 2600)];
    let changed =
        arena::player_metrics(&config, &changed_current_ratings, &terminal_games).unwrap();
    assert_eq!(changed[0].performance, Some(performance));
}

#[test]
fn pre_performance_standings_fallback_uses_current_arena_rating() {
    let config = ArenaConfig::plain();
    let non_waiter = ArenaPlayerFact {
        waiting_millis: None,
        ..player(2, 1700)
    };
    let players = vec![player(0, 1400), player(1, 1700), non_waiter];

    let projection = tournamint::arena::project(&config, &players, &[]).unwrap();

    assert_eq!(projection.players[0].arena_rating, Some(1400));
    assert_eq!(projection.players[1].arena_rating, Some(1700));
    assert_eq!(projection.players[0].performance, None);
    assert_eq!(projection.players[1].performance, None);
    assert_eq!(
        projection.standings.groups[0].players[0].player,
        PlayerId::new(1)
    );
    assert_eq!(
        arena::pair(&config, &players, &[]).unwrap().ranks,
        vec![3, 1, 1]
    );

    let changed = vec![player(0, 1800), player(1, 1700), non_waiter];
    let changed_projection = tournamint::arena::project(&config, &changed, &[]).unwrap();
    assert_eq!(
        changed_projection.standings.groups[0].players[0].player,
        PlayerId::new(0)
    );
    assert_eq!(
        arena::pair(&config, &changed, &[]).unwrap().ranks,
        vec![1, 2, 2]
    );
}

#[test]
fn pairing_proximity_uses_current_arena_rating() {
    let mut config = ArenaConfig::plain();
    config.rematch_cooldown = 0;
    let initial = vec![
        player(0, 1000),
        player(1, 1010),
        player(2, 2000),
        player(3, 2010),
    ];
    let changed = vec![
        player(0, 1000),
        player(1, 2000),
        player(2, 1010),
        player(3, 2010),
    ];
    let terminal_games = [
        ArenaTerminalGameFact {
            pairing: Pairing::new(PlayerId::new(0), PlayerId::new(3)),
            white_rating: Some(1500),
            black_rating: Some(1500),
            result: ArenaTerminalResult::Played(PlayedGameOutcome::Draw),
            plies: 40,
            white_berserked: false,
            black_berserked: false,
        },
        ArenaTerminalGameFact {
            pairing: Pairing::new(PlayerId::new(1), PlayerId::new(2)),
            white_rating: Some(1500),
            black_rating: Some(1500),
            result: ArenaTerminalResult::Played(PlayedGameOutcome::Draw),
            plies: 40,
            white_berserked: false,
            black_berserked: false,
        },
    ];

    let terminal_games = award_games(&config, &terminal_games);
    let initial_projection =
        tournamint::arena::project(&config, &initial, &terminal_games).unwrap();
    assert!(
        initial_projection
            .players
            .iter()
            .all(|metrics| metrics.performance == initial_projection.players[0].performance)
    );
    let initial_pairs = tournamint::arena::pair(&config, &initial, &terminal_games).unwrap();
    let changed_pairs = tournamint::arena::pair(&config, &changed, &terminal_games).unwrap();

    assert_eq!(
        uncolored_pairs(&initial_pairs.pairings),
        HashSet::from([[0, 1], [2, 3]])
    );
    assert_eq!(
        uncolored_pairs(&changed_pairs.pairings),
        HashSet::from([[0, 2], [1, 3]])
    );
}

#[test]
fn persisted_awards_survive_scoring_changes_and_feed_new_awards() {
    let players = vec![player(0, 1600), player(1, 1600)];
    let mut config = ArenaConfig::default();
    let games = vec![
        ArenaTerminalGameFact {
            pairing: Pairing::new(PlayerId::new(0), PlayerId::new(1)),
            white_rating: Some(1600),
            black_rating: Some(1600),
            result: ArenaTerminalResult::Played(PlayedGameOutcome::WhiteWin),
            plies: 40,
            white_berserked: true,
            black_berserked: false,
        };
        3
    ];
    let mut awarded_games = award_games(&config, &games);
    let awarded = arena::project(&config, &players, &awarded_games).unwrap();
    config.scoring.berserk.as_mut().unwrap().win_bonus = tournamint::Score::new(7);
    let retained = arena::project(&config, &players, &awarded_games).unwrap();
    assert_eq!(retained, awarded);
    let next_award = arena::award_game(&config, &games, &games[0]).unwrap();
    assert_eq!(next_award.awarded_points[0].value(), 11);
    assert!(next_award.doubled[0]);
    awarded_games.push(ArenaAwardedGameFact {
        game: games[0],
        award: next_award,
    });
    let next = arena::project(&config, &players, &awarded_games).unwrap();
    assert_eq!(
        next.players[0].points.value(),
        awarded.players[0].points.value() + 11
    );
    assert_eq!(next.players[0].current_streak, 4);
}
