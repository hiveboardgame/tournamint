use tournamint::arena::{
    self, ArenaAwardedGameFact, ArenaConfig, ArenaError, ArenaPairingPolicy, ArenaPlayerFact,
    ArenaSetupError, ArenaTerminalGameFact, ArenaTerminalResult, MAX_REMATCH_WAIT_SECS,
    MIN_REMATCH_WAIT_SECS,
};
use tournamint::{Pairing, PlayedGameOutcome, PlayerId};

fn player(id: usize, active: bool, waiting_millis: Option<u64>) -> ArenaPlayerFact {
    ArenaPlayerFact {
        player: PlayerId::new(id),
        arena_rating: None,
        active,
        waiting_millis,
    }
}

fn history(config: &ArenaConfig, pairings: &[(usize, usize)]) -> Vec<ArenaAwardedGameFact> {
    let games = pairings
        .iter()
        .map(|&(white, black)| ArenaTerminalGameFact {
            pairing: Pairing::new(PlayerId::new(white), PlayerId::new(black)),
            white_rating: None,
            black_rating: None,
            result: ArenaTerminalResult::Played(PlayedGameOutcome::Draw),
            plies: 60,
            white_berserked: false,
            black_berserked: false,
        })
        .collect::<Vec<_>>();
    games
        .iter()
        .enumerate()
        .map(|(index, game)| ArenaAwardedGameFact {
            game: *game,
            award: arena::award_game(config, &games[..index], game).unwrap(),
        })
        .collect()
}

#[test]
fn a_duel_rematch_requires_both_waiters_to_reach_the_saved_delay() {
    for pairing_policy in [ArenaPairingPolicy::Grouped, ArenaPairingPolicy::Greedy] {
        for rematch_wait_secs in [MIN_REMATCH_WAIT_SECS, 15, MAX_REMATCH_WAIT_SECS] {
            let config = ArenaConfig {
                pairing_policy,
                rematch_wait_secs,
                ..ArenaConfig::plain()
            };
            let games = history(&config, &[(0, 1)]);
            let threshold = u64::from(rematch_wait_secs) * 1000;
            for waits in [
                [0, threshold],
                [threshold - 1, threshold],
                [threshold, threshold - 1],
            ] {
                let players = [
                    player(0, true, Some(waits[0])),
                    player(1, true, Some(waits[1])),
                ];
                assert!(
                    arena::pair(&config, &players, &games)
                        .unwrap()
                        .pairings
                        .is_empty()
                );
            }
            let players = [
                player(0, true, Some(threshold)),
                player(1, true, Some(threshold)),
            ];
            let paired = arena::pair(&config, &players, &games).unwrap();
            assert_eq!(paired.pairings.len(), 1);
            assert_eq!(
                paired.pairings[0],
                Pairing::new(PlayerId::new(1), PlayerId::new(0))
            );
        }
    }
}

#[test]
fn nonwaiting_active_entrants_prevent_duels_and_inactive_history_is_retained() {
    for pairing_policy in [ArenaPairingPolicy::Grouped, ArenaPairingPolicy::Greedy] {
        let config = ArenaConfig {
            pairing_policy,
            ..ArenaConfig::plain()
        };
        let games = history(&config, &[(0, 1), (2, 3)]);
        let mut players = [
            player(0, true, Some(u64::MAX)),
            player(1, true, Some(u64::MAX)),
            player(2, true, None),
            player(3, true, None),
        ];
        let all_active = arena::project(&config, &players, &games).unwrap();
        assert!(
            arena::pair(&config, &players, &games)
                .unwrap()
                .pairings
                .is_empty()
        );

        players[3].active = false;
        assert!(
            arena::pair(&config, &players, &games)
                .unwrap()
                .pairings
                .is_empty()
        );

        players[2].active = false;
        let paired = arena::pair(&config, &players, &games).unwrap();
        assert_eq!(paired.pairings.len(), 1);
        assert_eq!(paired.ranks.len(), players.len());
        assert_eq!(
            arena::project(&config, &players, &games).unwrap(),
            all_active
        );
        assert_eq!(
            arena::player_metrics(&config, &players, &games).unwrap(),
            all_active.players
        );
    }
}

#[test]
fn fresh_opponents_can_pair_without_waiting_including_a_newly_available_third_player() {
    for pairing_policy in [ArenaPairingPolicy::Grouped, ArenaPairingPolicy::Greedy] {
        let config = ArenaConfig {
            pairing_policy,
            ..ArenaConfig::plain()
        };
        let players = [player(0, true, Some(0)), player(1, true, Some(0))];
        assert_eq!(
            arena::pair(&config, &players, &[]).unwrap().pairings.len(),
            1
        );

        let games = history(&config, &[(0, 1)]);
        let players = [
            player(0, true, Some(100)),
            player(1, true, Some(100)),
            player(2, true, Some(0)),
        ];
        let paired = arena::pair(&config, &players, &games).unwrap();
        assert_eq!(paired.pairings.len(), 1);
        let pairing = paired.pairings[0];
        assert!(pairing.white() == PlayerId::new(2) || pairing.black() == PlayerId::new(2));

        let games = history(&config, &[(0, 1), (0, 2), (1, 3)]);
        let players = [
            player(0, true, Some(0)),
            player(1, true, Some(0)),
            player(2, false, None),
            player(3, false, None),
        ];
        assert_eq!(
            arena::pair(&config, &players, &games)
                .unwrap()
                .pairings
                .len(),
            1
        );
    }
}

#[test]
fn a_waited_duel_rematch_still_respects_color_constraints() {
    for pairing_policy in [ArenaPairingPolicy::Grouped, ArenaPairingPolicy::Greedy] {
        let config = ArenaConfig {
            pairing_policy,
            rematch_cooldown: 2,
            max_color_strike: 1,
            ..ArenaConfig::plain()
        };
        let games = history(&config, &[(0, 1), (0, 2), (1, 2)]);
        let players = [
            player(0, true, Some(u64::MAX)),
            player(1, true, Some(u64::MAX)),
            player(2, false, None),
        ];
        assert!(
            arena::pair(&config, &players, &games)
                .unwrap()
                .pairings
                .is_empty()
        );
    }
}

#[test]
fn inactive_players_cannot_be_offered_as_waiters() {
    let config = ArenaConfig::plain();
    let players = [player(0, false, Some(0)), player(1, true, Some(0))];
    assert_eq!(
        arena::pair(&config, &players, &[]),
        Err(ArenaError::IncoherentFacts)
    );
    assert_eq!(
        arena::project(&config, &players, &[]),
        Err(ArenaError::IncoherentFacts)
    );
    assert_eq!(
        arena::player_metrics(&config, &players, &[]),
        Err(ArenaError::IncoherentFacts)
    );
}

#[test]
fn rematch_wait_validation_enforces_both_inclusive_bounds() {
    for rematch_wait_secs in [MIN_REMATCH_WAIT_SECS, MAX_REMATCH_WAIT_SECS] {
        assert_eq!(
            ArenaConfig {
                rematch_wait_secs,
                ..ArenaConfig::plain()
            }
            .validate(),
            Ok(())
        );
    }
    for rematch_wait_secs in [
        0,
        MIN_REMATCH_WAIT_SECS - 1,
        MAX_REMATCH_WAIT_SECS + 1,
        u32::MAX,
    ] {
        let config = ArenaConfig {
            rematch_wait_secs,
            ..ArenaConfig::plain()
        };
        assert_eq!(
            config.validate(),
            Err(ArenaSetupError::InvalidRematchWaitSecs)
        );
        assert_eq!(
            arena::pair(&config, &[], &[]),
            Err(ArenaError::InvalidConfig)
        );
    }
}
