use tournamint::arena::{
    self, ArenaConfig, ArenaError, ArenaGameAward, ArenaTerminalGameFact, ArenaTerminalResult,
};
use tournamint::{Pairing, PlayedGameOutcome, PlayerId, Score};

fn played(white: usize, black: usize, outcome: PlayedGameOutcome) -> ArenaTerminalGameFact {
    ArenaTerminalGameFact {
        pairing: Pairing::new(PlayerId::new(white), PlayerId::new(black)),
        white_rating: None,
        black_rating: None,
        result: ArenaTerminalResult::Played(outcome),
        plies: 40,
        white_berserked: false,
        black_berserked: false,
    }
}

fn no_start(white: usize, black: usize, absent: usize) -> ArenaTerminalGameFact {
    ArenaTerminalGameFact {
        result: ArenaTerminalResult::NoStart {
            absent: PlayerId::new(absent),
        },
        white_berserked: true,
        black_berserked: true,
        ..played(white, black, PlayedGameOutcome::Draw)
    }
}

#[test]
fn individual_awards_apply_fire_berserk_and_no_starts() {
    let config = ArenaConfig::default();
    let games = [
        played(0, 1, PlayedGameOutcome::WhiteWin),
        played(2, 0, PlayedGameOutcome::BlackWin),
        ArenaTerminalGameFact {
            white_berserked: true,
            ..played(0, 1, PlayedGameOutcome::WhiteWin)
        },
        no_start(2, 0, 2),
        ArenaTerminalGameFact {
            plies: 19,
            ..played(0, 1, PlayedGameOutcome::Draw)
        },
        no_start(0, 2, 0),
        no_start(1, 0, 1),
        played(0, 2, PlayedGameOutcome::Draw),
    ];
    let expected = [
        ([2, 0], [false, false]),
        ([0, 2], [false, false]),
        ([5, 0], [true, false]),
        ([0, 4], [false, true]),
        ([0, 0], [false, false]),
        ([0, 2], [false, false]),
        ([0, 2], [false, false]),
        ([1, 1], [false, false]),
    ];
    for (index, (game, (points, doubled))) in games.iter().zip(expected).enumerate() {
        assert_eq!(
            arena::award_game(&config, &games[..index], game).unwrap(),
            ArenaGameAward {
                awarded_points: points.map(Score::new),
                doubled,
            },
        );
    }

    let plain = ArenaConfig::plain();
    let game = no_start(0, 1, 1);
    assert_eq!(
        arena::award_game(&plain, &[], &game).unwrap(),
        ArenaGameAward {
            awarded_points: [Score::new(2), Score::new(0)],
            doubled: [false; 2],
        },
        "no-start flags never require or award a berserk policy",
    );
}

#[test]
fn individual_awards_preserve_draw_suppression_across_a_loss() {
    let config = ArenaConfig::default();
    let games = [
        played(0, 1, PlayedGameOutcome::Draw),
        played(2, 0, PlayedGameOutcome::WhiteWin),
        played(0, 2, PlayedGameOutcome::Draw),
    ];
    let award = arena::award_game(&config, &games[..2], &games[2]).unwrap();
    assert_eq!(award.awarded_points[0], Score::new(0));
    assert_eq!(award.awarded_points[1], Score::new(1));
}

#[test]
fn individual_awards_accept_sparse_ids_without_aggregating_prior_ratings() {
    let config = ArenaConfig::default();
    let first = usize::MAX;
    let second = usize::MAX - 1;
    let mut history = [
        played(first, second, PlayedGameOutcome::WhiteWin),
        played(second, first, PlayedGameOutcome::BlackWin),
        played(0, 1, PlayedGameOutcome::Draw),
    ];
    let game = played(first, second, PlayedGameOutcome::WhiteWin);
    let expected = arena::award_game(&config, &history, &game).unwrap();
    for prior in &mut history {
        prior.white_rating = Some(i32::MAX);
        prior.black_rating = Some(i32::MIN);
    }
    assert_eq!(
        arena::award_game(&config, &history, &game).unwrap(),
        expected
    );
    assert_eq!(expected.awarded_points, [Score::new(4), Score::new(0)]);
    assert_eq!(expected.doubled, [true, false]);
}

#[test]
fn individual_awards_reject_malformed_games_and_invalid_configuration() {
    let config = ArenaConfig::default();
    let game = played(0, 1, PlayedGameOutcome::WhiteWin);
    let invalid_games = [played(0, 0, PlayedGameOutcome::WhiteWin), no_start(0, 1, 2)];
    for invalid in invalid_games {
        assert_eq!(
            arena::award_game(&config, &[], &invalid),
            Err(ArenaError::IncoherentFacts),
        );
        assert_eq!(
            arena::award_game(&config, &[invalid], &game),
            Err(ArenaError::IncoherentFacts),
        );
    }
    let berserked = ArenaTerminalGameFact {
        white_berserked: true,
        ..game
    };
    assert_eq!(
        arena::award_game(&ArenaConfig::plain(), &[], &berserked),
        Err(ArenaError::IncoherentFacts),
    );
    assert_eq!(
        arena::award_game(
            &ArenaConfig {
                max_color_strike: 0,
                ..config
            },
            &[],
            &game
        ),
        Err(ArenaError::InvalidConfig),
    );
}
