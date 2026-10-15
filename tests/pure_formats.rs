use std::collections::HashSet;
use std::num::NonZeroU16;
use tournamint::arena::{
    ArenaAwardedGameFact, ArenaConfig, ArenaPlayerFact, ArenaTerminalGameFact, ArenaTerminalResult,
};
use tournamint::elimination::{
    EliminationError, EliminationKind, EliminationNodeFact, EliminationNodeFactState,
    EliminationNodeId, EliminationNodeResolution, EliminationProjection, EliminationReleaseCadence,
    EliminationSource, EliminationStage,
};
use tournamint::round_robin::{
    RoundRobinConfig, RoundRobinCriterion, RoundRobinGameFact, RoundRobinStandingsPlan,
};
use tournamint::standings::TournamentPairingNumberOrder;
use tournamint::swiss::{
    AccelerationPolicy, BursteinConfig, DoubleSwissByePoints, DoubleSwissConfig,
    DoubleSwissPrimaryScore, DutchConfig, MatchPointSystem, PairNextRoundError, PairingError,
    RoundPairings, SingleGameSwissByePoints, SwissConfig, SwissCriterion, SwissEncounterFact,
    SwissError, SwissFacts, SwissGameId, SwissHistoryStatus, SwissLeg, SwissNextRound,
    SwissNextRoundInput, SwissPairingRestriction, SwissRoundFact, SwissStandingsPlan,
    SwissSystemConfig,
};
use tournamint::{Color, GameOutcome, Pairing, PlayedGameOutcome, PlayerId, PointSystem, Score};

fn players(count: usize) -> Vec<PlayerId> {
    (0..count).map(PlayerId::new).collect()
}

fn selected_swiss_pairings(result: Result<SwissNextRound, SwissError>) -> RoundPairings {
    match result.unwrap() {
        SwissNextRound::Pairings(pairings) => pairings,
        SwissNextRound::PairingExhausted => panic!("fixture field unexpectedly exhausted"),
    }
}

#[test]
fn round_robin_schedule_and_projection_are_deterministic() {
    let field = players(4);
    let config = RoundRobinConfig {
        primary_score: Default::default(),
        match_point_system: MatchPointSystem::STANDARD,
        repeats: 1,
        initial_color: Color::White,
        point_system: PointSystem::STANDARD,
        standings: RoundRobinStandingsPlan::new(vec![RoundRobinCriterion::PrimaryScore]),
    };
    let schedule = tournamint::round_robin::schedule(&config, &field).unwrap();
    let first = schedule.rounds[0].games[0];
    let facts = [RoundRobinGameFact {
        id: first.id,
        outcome: GameOutcome::white_win(),
    }];
    let one = tournamint::round_robin::project(&config, &field, &facts).unwrap();
    let two = tournamint::round_robin::project(&config, &field, &facts).unwrap();
    assert_eq!(one, two);
    assert_eq!(one.unresolved_games().count(), 5);
}

fn swiss_config() -> SwissConfig {
    SwissConfig {
        system: SwissSystemConfig::Dutch(DutchConfig {
            bye_points: SingleGameSwissByePoints::game_win(PointSystem::STANDARD),
        }),
        expected_rounds: 3,
        initial_color: Color::White,
        game_point_system: PointSystem::STANDARD,
        acceleration: AccelerationPolicy::None,
        standings: SwissStandingsPlan::default(),
    }
}

#[test]
fn swiss_accepts_partial_current_round_and_corrections() {
    let field = players(4);
    let empty = SwissFacts {
        config: swiss_config(),
        initial_ranking: field.clone(),
        rounds: Vec::new(),
    };
    let pairings = selected_swiss_pairings(tournamint::swiss::pair_next_round(
        &empty,
        &SwissNextRoundInput {
            eligible: field.clone(),
            ..SwissNextRoundInput::default()
        },
    ));
    let mut facts = empty;
    facts.rounds.push(SwissRoundFact {
        eligible: field.clone(),
        encounters: pairings
            .games
            .iter()
            .copied()
            .map(|pairing| SwissEncounterFact {
                pairing,
                outcomes: vec![None],
            })
            .collect(),
        pairings,
    });
    let pending = tournamint::swiss::project(&facts).unwrap();
    assert_eq!(pending.history_status, SwissHistoryStatus::AwaitingResults);
    assert_eq!(pending.rounds.len(), 1);
    assert_eq!(pending.rounds[0].encounters[0].games.len(), 1);
    assert_eq!(
        pending.rounds[0].encounters[0].games[0].id,
        SwissGameId {
            round_index: 0,
            pairing_index: 0,
            leg: SwissLeg::Single,
        }
    );
    assert!(pending.rounds[0].encounters[0].games[0].completed.is_none());
    facts.rounds[0].encounters[0].outcomes[0] = Some(GameOutcome::white_win());
    let won = tournamint::swiss::project(&facts).unwrap();
    assert_eq!(
        won.rounds[0].encounters[0].games[0]
            .completed
            .unwrap()
            .awarded_points,
        [PointSystem::STANDARD.win, PointSystem::STANDARD.loss]
    );
    assert!(won.rounds[0].encounters[0].completion.is_some());
    facts.rounds[0].encounters[0].outcomes[0] = Some(GameOutcome::black_win());
    let corrected = tournamint::swiss::project(&facts).unwrap();
    assert_ne!(won, corrected);
    assert!(
        tournamint::swiss::pair_next_round(
            &facts,
            &SwissNextRoundInput {
                eligible: field,
                ..SwissNextRoundInput::default()
            }
        )
        .is_err()
    );

    let two = players(2);
    let mut exhausted = SwissFacts {
        config: SwissConfig {
            expected_rounds: 3,
            ..swiss_config()
        },
        initial_ranking: two.clone(),
        rounds: Vec::new(),
    };
    let first = selected_swiss_pairings(tournamint::swiss::pair_next_round(
        &exhausted,
        &SwissNextRoundInput {
            eligible: two.clone(),
            ..Default::default()
        },
    ));
    exhausted.rounds.push(SwissRoundFact {
        eligible: two.clone(),
        encounters: first
            .games
            .iter()
            .copied()
            .map(|pairing| SwissEncounterFact {
                pairing,
                outcomes: vec![Some(GameOutcome::draw())],
            })
            .collect(),
        pairings: first,
    });
    assert_eq!(
        tournamint::swiss::pair_next_round(
            &exhausted,
            &SwissNextRoundInput {
                eligible: two,
                ..Default::default()
            },
        ),
        Ok(SwissNextRound::PairingExhausted)
    );
}

#[test]
fn double_swiss_preserves_leg_identity_through_partial_results_and_corrections() {
    let field = players(2);
    let mut facts = SwissFacts {
        config: SwissConfig {
            system: SwissSystemConfig::DoubleSwiss(DoubleSwissConfig {
                primary_score: DoubleSwissPrimaryScore::GamePoints,
                match_point_system: MatchPointSystem::STANDARD,
                bye_points: DoubleSwissByePoints::standard(
                    PointSystem::STANDARD,
                    MatchPointSystem::STANDARD,
                )
                .unwrap(),
            }),
            ..swiss_config()
        },
        initial_ranking: field.clone(),
        rounds: Vec::new(),
    };
    let input = SwissNextRoundInput {
        eligible: field.clone(),
        ..Default::default()
    };
    let pairings = selected_swiss_pairings(tournamint::swiss::pair_next_round(&facts, &input));
    let first = pairings.games[0];
    let second = Pairing::new(first.black(), first.white());
    facts.rounds.push(SwissRoundFact {
        eligible: field,
        encounters: vec![SwissEncounterFact {
            pairing: first,
            outcomes: vec![None, None],
        }],
        pairings,
    });

    let cases = [
        (
            [None, Some(GameOutcome::white_win())],
            [0, 2],
            [0, 0],
            false,
        ),
        (
            [None, Some(GameOutcome::black_win())],
            [2, 0],
            [0, 0],
            false,
        ),
        (
            [
                Some(GameOutcome::white_win()),
                Some(GameOutcome::black_win()),
            ],
            [4, 0],
            [2, 0],
            true,
        ),
        (
            [Some(GameOutcome::white_win()), None],
            [2, 0],
            [0, 0],
            false,
        ),
        (
            [Some(GameOutcome::draw()), Some(GameOutcome::white_win())],
            [1, 3],
            [0, 2],
            true,
        ),
    ];
    for (outcomes, game_points, match_points, complete) in cases {
        facts.rounds[0].encounters[0].outcomes = outcomes.to_vec();
        let projection = tournamint::swiss::project(&facts).unwrap();
        let encounter = &projection.rounds[0].encounters[0];
        assert_eq!(encounter.games.len(), 2);
        for (index, (leg, pairing)) in [(SwissLeg::First, first), (SwissLeg::Second, second)]
            .into_iter()
            .enumerate()
        {
            let game = &encounter.games[index];
            assert_eq!(
                game.id,
                SwissGameId {
                    round_index: 0,
                    pairing_index: 0,
                    leg
                }
            );
            assert_eq!(game.pairing, pairing);
            assert_eq!(game.completed.map(|result| result.outcome), outcomes[index]);
        }
        for (index, player) in [first.white(), first.black()].into_iter().enumerate() {
            let summary = projection
                .players
                .iter()
                .find(|summary| summary.player == player)
                .unwrap();
            assert_eq!(summary.game_points, Score::new(game_points[index]));
            assert_eq!(summary.match_points, Score::new(match_points[index]));
            assert_eq!(summary.games_played, if complete { 2 } else { 1 });
            assert_eq!(summary.matches_played, u32::from(complete));
        }
        assert_eq!(encounter.completion.is_some(), complete);
        if let Some(completion) = encounter.completion {
            assert_eq!(completion.game_points, game_points.map(Score::new));
            assert_eq!(completion.match_points, match_points.map(Score::new));
            assert_eq!(projection.history_status, SwissHistoryStatus::ReadyToPair);
        } else {
            assert_eq!(
                projection.history_status,
                SwissHistoryStatus::AwaitingResults
            );
            assert_eq!(
                tournamint::swiss::pair_next_round(&facts, &input),
                Err(SwissError::IncoherentFacts),
            );
        }
    }
}

#[test]
fn swiss_continues_after_recorded_and_new_withdrawals() {
    let single_bye = SingleGameSwissByePoints::game_win(PointSystem::STANDARD);
    let double_config = DoubleSwissConfig {
        primary_score: DoubleSwissPrimaryScore::GamePoints,
        match_point_system: MatchPointSystem::STANDARD,
        bye_points: DoubleSwissByePoints::standard(
            PointSystem::STANDARD,
            MatchPointSystem::STANDARD,
        )
        .unwrap(),
    };
    for system in [
        SwissSystemConfig::Dutch(DutchConfig {
            bye_points: single_bye,
        }),
        SwissSystemConfig::Burstein(BursteinConfig {
            bye_points: single_bye,
        }),
        SwissSystemConfig::DoubleSwiss(double_config),
        SwissSystemConfig::DoubleSwiss(DoubleSwissConfig {
            primary_score: DoubleSwissPrimaryScore::MatchPoints,
            ..double_config
        }),
    ] {
        let field = players(8);
        let mut facts = SwissFacts {
            config: SwissConfig {
                system,
                expected_rounds: 4,
                ..swiss_config()
            },
            initial_ranking: field.clone(),
            rounds: Vec::new(),
        };
        let outcome_count = usize::from(matches!(system, SwissSystemConfig::DoubleSwiss(_))) + 1;
        for active_count in [8, 7, 6, 6] {
            let eligible = field[..active_count].to_vec();
            let pairings = selected_swiss_pairings(tournamint::swiss::pair_next_round(
                &facts,
                &SwissNextRoundInput {
                    eligible: eligible.clone(),
                    ..Default::default()
                },
            ));
            let assigned = pairings
                .games
                .iter()
                .flat_map(|pairing| [pairing.white(), pairing.black()])
                .chain(pairings.byes.iter().map(|bye_| bye_.player))
                .collect::<HashSet<_>>();
            assert_eq!(assigned, eligible.iter().copied().collect());
            facts.rounds.push(SwissRoundFact {
                eligible,
                encounters: pairings
                    .games
                    .iter()
                    .copied()
                    .map(|pairing| SwissEncounterFact {
                        pairing,
                        outcomes: vec![Some(GameOutcome::draw()); outcome_count],
                    })
                    .collect(),
                pairings,
            });
            if facts.rounds.len() == 2 {
                assert_eq!(
                    tournamint::swiss::pair_next_round(&facts, &SwissNextRoundInput::default()),
                    Ok(SwissNextRound::PairingExhausted),
                );
            }
        }
        assert_eq!(
            tournamint::swiss::project(&facts).unwrap().history_status,
            SwissHistoryStatus::ConfiguredRoundsComplete,
        );
    }
}

#[test]
fn swiss_accepted_participation_is_replayed_and_malformed_pairings_are_rejected() {
    let field: Vec<_> = (0..4).map(PlayerId::new).collect();
    let mut facts = SwissFacts {
        config: swiss_config(),
        initial_ranking: field.clone(),
        rounds: Vec::new(),
    };
    assert!(
        tournamint::swiss::pair_next_round(
            &facts,
            &SwissNextRoundInput {
                eligible: field.clone(),
                restrictions: vec![SwissPairingRestriction {
                    players: field.clone(),
                }],
                ..Default::default()
            },
        )
        .is_err(),
        "a caller restriction which eliminates every edge is not field exhaustion"
    );
    let eligible = field[..3].to_vec();
    let pairings = selected_swiss_pairings(tournamint::swiss::pair_next_round(
        &facts,
        &SwissNextRoundInput {
            eligible: eligible.clone(),
            ..Default::default()
        },
    ));
    facts.rounds.push(SwissRoundFact {
        eligible,
        encounters: pairings
            .games
            .iter()
            .copied()
            .map(|pairing| SwissEncounterFact {
                pairing,
                outcomes: vec![Some(GameOutcome::draw())],
            })
            .collect(),
        pairings,
    });
    assert!(tournamint::swiss::project(&facts).is_ok());
    assert_eq!(
        tournamint::swiss::pair_next_round(
            &facts,
            &SwissNextRoundInput {
                eligible: field.clone(),
                ..Default::default()
            }
        ),
        Err(tournamint::swiss::SwissError::InvalidInput),
    );

    facts.rounds[0].pairings.games[0] = Pairing::new(PlayerId::new(99), field[0]);
    assert_eq!(
        tournamint::swiss::project(&facts),
        Err(tournamint::swiss::SwissError::IncoherentFacts),
    );
}

#[test]
fn requested_byes_do_not_turn_a_temporary_pairing_failure_into_field_exhaustion() {
    let field = players(4);
    let mut facts = SwissFacts {
        config: swiss_config(),
        initial_ranking: field.clone(),
        rounds: Vec::new(),
    };
    let first = selected_swiss_pairings(tournamint::swiss::pair_next_round(
        &facts,
        &SwissNextRoundInput {
            eligible: field.clone(),
            ..Default::default()
        },
    ));
    facts.rounds.push(SwissRoundFact {
        eligible: field.clone(),
        encounters: first
            .games
            .iter()
            .copied()
            .map(|pairing| SwissEncounterFact {
                pairing,
                outcomes: vec![Some(GameOutcome::draw())],
            })
            .collect(),
        pairings: first.clone(),
    });

    assert!(matches!(
        tournamint::swiss::pair_next_round(
            &facts,
            &SwissNextRoundInput {
                eligible: field.clone(),
                ..Default::default()
            },
        ),
        Ok(SwissNextRound::Pairings(_))
    ));

    let repeated_pair = first.games[0];
    let repeated_players = [repeated_pair.white(), repeated_pair.black()];
    let requested_byes = field
        .iter()
        .copied()
        .filter(|player| !repeated_players.contains(player))
        .collect();
    assert_eq!(
        tournamint::swiss::pair_next_round(
            &facts,
            &SwissNextRoundInput {
                eligible: field.clone(),
                requested_byes,
                ..Default::default()
            },
        ),
        Err(SwissError::Pairing(PairNextRoundError::Pairing(
            PairingError::NoValidPairing
        )))
    );

    let all_requested = selected_swiss_pairings(tournamint::swiss::pair_next_round(
        &facts,
        &SwissNextRoundInput {
            eligible: field.clone(),
            requested_byes: field,
            ..Default::default()
        },
    ));
    assert!(all_requested.games.is_empty());
    assert_eq!(all_requested.byes.len(), 4);
}

#[test]
fn elimination_withdrawal_is_a_fact_and_history_stays_fixed() {
    let field = players(4);
    let definition =
        tournamint::elimination::topology(EliminationKind::Single { bronze: true }, &field)
            .unwrap();
    let mut facts = tournamint::elimination::initial_facts(&definition);
    let initial_projection = tournamint::elimination::project(&definition, &facts, &[]).unwrap();
    let final_candidates = &initial_projection
        .nodes
        .iter()
        .find(|node| node.descriptor.stage == EliminationStage::SingleFinal)
        .unwrap()
        .possible_entrants;
    assert!(
        final_candidates[0]
            .iter()
            .all(|player| !final_candidates[1].contains(player))
    );
    assert_eq!(
        final_candidates
            .iter()
            .flatten()
            .copied()
            .collect::<HashSet<_>>(),
        field.iter().copied().collect()
    );
    let wave = tournamint::elimination::next_wave(&definition, &facts).unwrap();
    for patch in wave.patches {
        facts[patch.node.value()].state = patch.state;
    }
    let node = facts
        .iter()
        .find(|fact| matches!(fact.state, EliminationNodeFactState::Active { entrants } if entrants.contains(&field[0])))
        .unwrap()
        .node;
    let decided =
        tournamint::elimination::record_winner(&definition, &facts, node, field[0]).unwrap();
    facts[node.value()].state = decided.state;
    let historical = facts[node.value()];
    let withdrawal = tournamint::elimination::withdraw(&definition, &facts, field[0]).unwrap();
    for patch in &withdrawal {
        facts[patch.node.value()].state = patch.state;
    }
    assert_eq!(facts[node.value()], historical);
    assert!(
        withdrawal.is_empty(),
        "future withdrawal is reapplied by the host after materialization"
    );
}

#[test]
fn single_elimination_bronze_completes_with_full_exact_tiers() {
    let field = players(4);
    let definition =
        tournamint::elimination::topology(EliminationKind::Single { bronze: true }, &field)
            .unwrap();
    let mut facts = tournamint::elimination::initial_facts(&definition);
    loop {
        let active: Vec<_> = facts
            .iter()
            .filter_map(|fact| match fact.state {
                EliminationNodeFactState::Active { entrants } => Some((fact.node, entrants)),
                _ => None,
            })
            .collect();
        if !active.is_empty() {
            for (node, entrants) in active {
                let patch =
                    tournamint::elimination::record_winner(&definition, &facts, node, entrants[0])
                        .unwrap();
                facts[node.value()].state = patch.state;
            }
            continue;
        }
        match tournamint::elimination::next_wave(&definition, &facts) {
            Ok(wave) => {
                for patch in wave.patches {
                    facts[patch.node.value()].state = patch.state;
                }
            }
            Err(tournamint::elimination::EliminationError::Complete) => break,
            Err(error) => panic!("unexpected single-elimination error: {error}"),
        }
    }
    let projection = tournamint::elimination::project(&definition, &facts, &[]).unwrap();
    assert!(projection.complete);
    let mut places = projection
        .players
        .iter()
        .map(|player| player.placement.unwrap())
        .collect::<Vec<_>>();
    places.sort_unstable();
    assert_eq!(places, vec![1, 2, 3, 4]);
    assert!(
        projection
            .players
            .iter()
            .all(|player| { !player.in_contention && player.exit_stage.is_some() })
    );
}

#[test]
fn arena_aggregates_awarded_facts_and_pairs_waiters() {
    let config = ArenaConfig::default();
    let field: Vec<_> = players(4)
        .into_iter()
        .map(|player| ArenaPlayerFact {
            player,
            arena_rating: None,
            active: true,
            waiting_millis: Some(100),
        })
        .collect();
    let terminal = [ArenaTerminalGameFact {
        pairing: Pairing::new(field[0].player, field[1].player),
        white_rating: None,
        black_rating: None,
        result: ArenaTerminalResult::Played(PlayedGameOutcome::WhiteWin),
        plies: 20,
        white_berserked: false,
        black_berserked: false,
    }; 3];
    let awarded = terminal
        .iter()
        .enumerate()
        .map(|(index, game)| ArenaAwardedGameFact {
            game: *game,
            award: tournamint::arena::award_game(&config, &terminal[..index], game).unwrap(),
        })
        .collect::<Vec<_>>();
    let activated = tournamint::arena::project(&config, &field, &awarded[..2]).unwrap();
    assert!(activated.players[0].on_fire);
    let projection = tournamint::arena::project(&config, &field, &awarded).unwrap();
    assert_eq!(projection.players[0].wins, 3);
    assert!(projection.players[0].on_fire);
    assert!(!projection.players[1].on_fire);
    assert_eq!(projection.players[0].points, Score::new(8));
    assert!(
        !tournamint::arena::project(&ArenaConfig::plain(), &field, &awarded)
            .unwrap()
            .players[0]
            .on_fire
    );
    assert_eq!(
        tournamint::arena::pair(&config, &field, &awarded)
            .unwrap()
            .pairings
            .len(),
        2
    );
}

fn run_double(reset: bool) -> EliminationProjection {
    let field = players(4);
    let definition = tournamint::elimination::topology(EliminationKind::Double, &field).unwrap();
    let mut facts = tournamint::elimination::initial_facts(&definition);
    loop {
        let active: Vec<_> = facts
            .iter()
            .filter_map(|fact| match fact.state {
                EliminationNodeFactState::Active { entrants } => Some((fact.node, entrants)),
                _ => None,
            })
            .collect();
        if !active.is_empty() {
            for (node, entrants) in active {
                let stage = definition.nodes()[node.value()].stage;
                let winner =
                    if reset && stage == tournamint::elimination::EliminationStage::GrandFinal {
                        entrants[1]
                    } else {
                        entrants[0]
                    };
                let patch =
                    tournamint::elimination::record_winner(&definition, &facts, node, winner)
                        .unwrap();
                facts[node.value()].state = patch.state;
            }
            continue;
        }
        match tournamint::elimination::next_wave(&definition, &facts) {
            Ok(wave) => {
                for patch in wave.patches {
                    facts[patch.node.value()].state = patch.state;
                }
            }
            Err(tournamint::elimination::EliminationError::Complete) => break,
            Err(error) => panic!("unexpected double-elimination error: {error}"),
        }
    }
    tournamint::elimination::project(&definition, &facts, &[]).unwrap()
}

#[test]
fn double_elimination_completes_with_reset_taken_and_skipped() {
    for reset in [false, true] {
        let projection = run_double(reset);
        assert!(projection.complete);
        let reset_state = projection
            .nodes
            .iter()
            .find(|node| node.descriptor.stage == EliminationStage::Reset)
            .unwrap()
            .state;
        assert_eq!(
            matches!(reset_state, EliminationNodeFactState::Skipped),
            !reset
        );
        assert_eq!(projection.reset_required, reset);
        assert_eq!(projection.players.len(), 4);
        assert!(projection.players.iter().all(|player| {
            !player.in_contention && player.exit_stage.is_some() && player.placement.is_some()
        }));
        let terminal_stage = if reset {
            EliminationStage::Reset
        } else {
            EliminationStage::GrandFinal
        };
        for placement in [1, 2] {
            let result = projection
                .players
                .iter()
                .find(|result| result.placement == Some(placement))
                .unwrap();
            assert_eq!(result.exit_stage, Some(terminal_stage));
        }
    }
}

#[test]
fn vacancy_and_pre_materialization_withdrawal_propagate() {
    let field = players(3);
    let double = tournamint::elimination::topology(EliminationKind::Double, &field).unwrap();
    let double_facts = tournamint::elimination::initial_facts(&double);
    let double_projection = tournamint::elimination::project(&double, &double_facts, &[]).unwrap();
    let automatic = double
        .nodes()
        .iter()
        .find(|node| node.sources.contains(&EliminationSource::Vacant))
        .unwrap();
    let (loser_consumer, side) = double
        .nodes()
        .iter()
        .find_map(|node| {
            node.sources
                .iter()
                .position(|source| *source == EliminationSource::LoserOf(automatic.id))
                .map(|side| (node, side))
        })
        .unwrap();
    assert!(double_projection.nodes[loser_consumer.id.value()].possible_entrants[side].is_empty());

    let definition =
        tournamint::elimination::topology(EliminationKind::Single { bronze: false }, &field)
            .unwrap();
    let mut facts = tournamint::elimination::initial_facts(&definition);
    let patches = tournamint::elimination::withdraw(&definition, &facts, field[0]).unwrap();
    assert!(patches.is_empty());
    let wave = tournamint::elimination::next_release(
        &definition,
        &facts,
        EliminationReleaseCadence::Wave,
        &[field[0]],
    )
    .unwrap();
    for patch in wave.patches {
        facts[patch.node.value()].state = patch.state;
    }
    for patch in
        tournamint::elimination::withdraw_players(&definition, &facts, &[field[0]]).unwrap()
    {
        facts[patch.node.value()].state = patch.state;
    }
    let (active_node, entrants) = facts
        .iter()
        .find_map(|fact| match fact.state {
            EliminationNodeFactState::Active { entrants } => Some((fact.node, entrants)),
            _ => None,
        })
        .unwrap();
    let patch =
        tournamint::elimination::record_winner(&definition, &facts, active_node, entrants[0])
            .unwrap();
    facts[active_node.value()].state = patch.state;
    let final_wave = tournamint::elimination::next_wave(&definition, &facts).unwrap();
    for patch in final_wave.patches {
        facts[patch.node.value()].state = patch.state;
    }
    for patch in
        tournamint::elimination::withdraw_players(&definition, &facts, &[field[0]]).unwrap()
    {
        facts[patch.node.value()].state = patch.state;
    }
    assert!(facts.iter().any(|fact| matches!(fact.state,
        EliminationNodeFactState::Resolved {
            resolution: EliminationNodeResolution::WithdrawalWalkover { withdrawn, .. }
                | EliminationNodeResolution::WithdrawalVacancy { withdrawn }, ..
        } if withdrawn == field[0]
    )));
    assert!(facts.iter().any(|fact| matches!(
        fact.state,
        EliminationNodeFactState::Resolved {
            resolution: EliminationNodeResolution::AutomaticAdvance { .. },
            ..
        }
    )));
}

#[test]
fn planned_match_with_a_vacant_side_cannot_project_a_loser() {
    let field = players(4);
    let definition = tournamint::elimination::topology(EliminationKind::Double, &field).unwrap();
    let mut facts = tournamint::elimination::initial_facts(&definition);
    let opening = tournamint::elimination::next_wave(&definition, &facts).unwrap();
    for patch in opening.patches {
        facts[patch.node.value()].state = patch.state;
    }
    let opening_nodes = facts
        .iter()
        .filter_map(|fact| match fact.state {
            EliminationNodeFactState::Active { entrants } => Some((fact.node, entrants)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(opening_nodes.len(), 2);
    for patch in
        tournamint::elimination::withdraw_players(&definition, &facts, &opening_nodes[0].1).unwrap()
    {
        facts[patch.node.value()].state = patch.state;
    }

    let source_match = definition
        .nodes()
        .iter()
        .find(|node| {
            opening_nodes.iter().all(|(opening, _)| {
                node.sources
                    .contains(&EliminationSource::WinnerOf(*opening))
            })
        })
        .unwrap();
    let (loser_consumer, loser_side) = definition
        .nodes()
        .iter()
        .find_map(|node| {
            node.sources
                .iter()
                .position(|source| *source == EliminationSource::LoserOf(source_match.id))
                .map(|side| (node, side))
        })
        .unwrap();
    let projection = tournamint::elimination::project(&definition, &facts, &[]).unwrap();
    let source_sides = &projection.nodes[source_match.id.value()].possible_entrants;
    assert!(source_sides.iter().any(Vec::is_empty));
    assert!(source_sides.iter().any(|side| side.len() > 1));
    assert!(projection.nodes[loser_consumer.id.value()].possible_entrants[loser_side].is_empty());
}

#[test]
fn routed_withdrawal_waits_for_sibling_then_completes_single_and_double() {
    for kind in [
        EliminationKind::Single { bronze: false },
        EliminationKind::Double,
    ] {
        let field = players(4);
        let definition = tournamint::elimination::topology(kind, &field).unwrap();
        let mut facts = tournamint::elimination::initial_facts(&definition);
        let wave = tournamint::elimination::next_wave(&definition, &facts).unwrap();
        for patch in wave.patches {
            facts[patch.node.value()].state = patch.state;
        }
        let active = facts
            .iter()
            .filter_map(|fact| match fact.state {
                EliminationNodeFactState::Active { entrants } => Some((fact.node, entrants)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(active.len() >= 2);
        let routed = active[0].1[0];
        let decided =
            tournamint::elimination::record_winner(&definition, &facts, active[0].0, routed)
                .unwrap();
        facts[active[0].0.value()].state = decided.state;
        assert!(
            tournamint::elimination::withdraw(&definition, &facts, routed)
                .unwrap()
                .is_empty()
        );
        let pending_projection =
            tournamint::elimination::project(&definition, &facts, &[routed]).unwrap();
        let pending_result = pending_projection
            .players
            .iter()
            .find(|result| result.player == routed)
            .unwrap();
        assert!(!pending_result.in_contention);
        assert!(pending_result.placement.is_some());
        assert!(pending_result.exit_stage.is_some());
        assert!(
            !pending_projection
                .nodes
                .iter()
                .flat_map(|node| node.possible_entrants.iter().flatten())
                .any(|player| *player == routed)
        );
        let sibling = active[1];
        let decided =
            tournamint::elimination::record_winner(&definition, &facts, sibling.0, sibling.1[0])
                .unwrap();
        facts[sibling.0.value()].state = decided.state;

        loop {
            let active = facts.iter().find_map(|fact| match fact.state {
                EliminationNodeFactState::Active { entrants } => Some((fact.node, entrants)),
                _ => None,
            });
            if let Some((node, entrants)) = active {
                let winner = entrants
                    .into_iter()
                    .find(|player| *player != routed)
                    .unwrap_or(entrants[0]);
                let patch =
                    tournamint::elimination::record_winner(&definition, &facts, node, winner)
                        .unwrap();
                facts[node.value()].state = patch.state;
                continue;
            }
            match tournamint::elimination::next_wave(&definition, &facts) {
                Ok(wave) => {
                    for patch in wave.patches {
                        facts[patch.node.value()].state = patch.state;
                    }
                    for patch in
                        tournamint::elimination::withdraw_players(&definition, &facts, &[routed])
                            .unwrap()
                    {
                        facts[patch.node.value()].state = patch.state;
                    }
                }
                Err(tournamint::elimination::EliminationError::Complete) => break,
                Err(error) => {
                    panic!("unexpected {kind:?} elimination error: {error}; facts: {facts:?}")
                }
            }
        }
        let projection = tournamint::elimination::project(&definition, &facts, &[routed]).unwrap();
        assert!(projection.complete);
        assert_eq!(projection.players.len(), field.len());
        assert!(projection.players.iter().all(|player| {
            !player.in_contention && player.exit_stage.is_some() && player.placement.is_some()
        }));
    }
}

#[test]
fn wave_release_does_not_award_a_new_bye_to_an_already_withdrawn_winner() {
    for kind in [
        EliminationKind::Single { bronze: false },
        EliminationKind::Double,
    ] {
        let field = players(4);
        let definition = tournamint::elimination::topology(kind, &field).unwrap();
        let mut facts = tournamint::elimination::initial_facts(&definition);
        let wave = tournamint::elimination::next_wave(&definition, &facts).unwrap();
        for patch in wave.patches {
            facts[patch.node.value()].state = patch.state;
        }
        let active = facts
            .iter()
            .filter_map(|fact| match fact.state {
                EliminationNodeFactState::Active { entrants } => Some((fact.node, entrants)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(active.len(), 2);

        let future_withdrawn = active[0].1[0];
        let decided = tournamint::elimination::record_winner(
            &definition,
            &facts,
            active[0].0,
            future_withdrawn,
        )
        .unwrap();
        facts[active[0].0.value()].state = decided.state;
        assert!(
            tournamint::elimination::withdraw(&definition, &facts, future_withdrawn)
                .unwrap()
                .is_empty()
        );

        let sibling_withdrawn = active[1].1;
        for patch in
            tournamint::elimination::withdraw_players(&definition, &facts, &sibling_withdrawn)
                .unwrap()
        {
            facts[patch.node.value()].state = patch.state;
        }
        let withdrawn = [future_withdrawn, sibling_withdrawn[0], sibling_withdrawn[1]];
        let wave = tournamint::elimination::next_release(
            &definition,
            &facts,
            EliminationReleaseCadence::Wave,
            &withdrawn,
        )
        .unwrap();
        for patch in wave.patches {
            facts[patch.node.value()].state = patch.state;
        }
        assert!(!facts.iter().any(|fact| matches!(
            fact.state,
            EliminationNodeFactState::Resolved {
                resolution: EliminationNodeResolution::AutomaticAdvance { player },
                ..
            } if player == future_withdrawn
        )));
        assert!(facts.iter().any(|fact| matches!(
            fact.state,
            EliminationNodeFactState::Resolved {
                resolution: EliminationNodeResolution::WithdrawalVacancy {
                    withdrawn,
                },
                ..
            } if withdrawn == future_withdrawn
        )));

        loop {
            if let Some((node, entrants)) = facts.iter().find_map(|fact| match fact.state {
                EliminationNodeFactState::Active { entrants } => Some((fact.node, entrants)),
                _ => None,
            }) {
                let winner = entrants
                    .into_iter()
                    .find(|entrant| !withdrawn.contains(entrant))
                    .unwrap();
                let patch =
                    tournamint::elimination::record_winner(&definition, &facts, node, winner)
                        .unwrap();
                facts[node.value()].state = patch.state;
                continue;
            }
            match tournamint::elimination::next_wave(&definition, &facts) {
                Ok(wave) => {
                    for patch in wave.patches {
                        facts[patch.node.value()].state = patch.state;
                    }
                    for patch in
                        tournamint::elimination::withdraw_players(&definition, &facts, &withdrawn)
                            .unwrap()
                    {
                        facts[patch.node.value()].state = patch.state;
                    }
                }
                Err(tournamint::elimination::EliminationError::Complete) => break,
                Err(error) => panic!("unexpected {kind:?} elimination error: {error}"),
            }
        }
        let projection = tournamint::elimination::project(&definition, &facts, &withdrawn).unwrap();
        assert!(projection.complete);
        assert_eq!(projection.players.len(), field.len());
        assert!(projection.players.iter().all(|player| {
            !player.in_contention && player.exit_stage.is_some() && player.placement.is_some()
        }));
    }
}

#[test]
fn two_routed_withdrawals_are_both_retained_in_single_and_double() {
    for kind in [
        EliminationKind::Single { bronze: false },
        EliminationKind::Double,
    ] {
        let field = players(4);
        let definition = tournamint::elimination::topology(kind, &field).unwrap();
        let mut facts = tournamint::elimination::initial_facts(&definition);
        let wave = tournamint::elimination::next_wave(&definition, &facts).unwrap();
        for patch in wave.patches {
            facts[patch.node.value()].state = patch.state;
        }
        let target = definition
            .nodes()
            .iter()
            .find(|node| {
                node.sources.iter().all(|source| match source {
                    tournamint::elimination::EliminationSource::WinnerOf(source)
                    | tournamint::elimination::EliminationSource::LoserOf(source) => {
                        matches!(
                            facts[source.value()].state,
                            EliminationNodeFactState::Active { .. }
                        )
                    }
                    _ => false,
                })
            })
            .unwrap();
        let route = |source, facts: &mut Vec<EliminationNodeFact>| {
            let (source_node, take_winner) = match source {
                tournamint::elimination::EliminationSource::WinnerOf(node) => (node, true),
                tournamint::elimination::EliminationSource::LoserOf(node) => (node, false),
                _ => unreachable!(),
            };
            let EliminationNodeFactState::Active { entrants } = facts[source_node.value()].state
            else {
                unreachable!()
            };
            let routed = entrants[0];
            let winner = if take_winner { routed } else { entrants[1] };
            let patch =
                tournamint::elimination::record_winner(&definition, facts, source_node, winner)
                    .unwrap();
            facts[source_node.value()].state = patch.state;
            routed
        };
        let first = route(target.sources[0], &mut facts);
        assert!(
            tournamint::elimination::withdraw(&definition, &facts, first)
                .unwrap()
                .is_empty()
        );
        let second = route(target.sources[1], &mut facts);
        let routed = [first, second];
        assert!(
            tournamint::elimination::withdraw(&definition, &facts, routed[1])
                .unwrap()
                .is_empty()
        );
        let wave = tournamint::elimination::next_wave(&definition, &facts).unwrap();
        for patch in wave.patches {
            facts[patch.node.value()].state = patch.state;
        }
        for patch in
            tournamint::elimination::withdraw_players(&definition, &facts, &routed).unwrap()
        {
            facts[patch.node.value()].state = patch.state;
        }
        assert!(matches!(facts[target.id.value()].state,
            EliminationNodeFactState::Resolved {
                resolution: EliminationNodeResolution::MutualWithdrawalVacancy { withdrawn }, ..
            } if withdrawn.into_iter().collect::<std::collections::HashSet<_>>() == routed.into_iter().collect()
        ));
        tournamint::elimination::project(&definition, &facts, &routed).unwrap();
    }
}

#[test]
fn elimination_rejects_fabricated_and_unreachable_node_facts() {
    let field = players(4);
    let definition =
        tournamint::elimination::topology(EliminationKind::Single { bronze: false }, &field)
            .unwrap();
    let initial = tournamint::elimination::initial_facts(&definition);

    let mut fabricated = initial.clone();
    fabricated[0].state = EliminationNodeFactState::Active {
        entrants: [PlayerId::new(99), field[0]],
    };
    assert_eq!(
        tournamint::elimination::project(&definition, &fabricated, &[]),
        Err(tournamint::elimination::EliminationError::IncoherentFacts),
    );

    let mut premature = initial.clone();
    let final_node = *definition.nodes().last().unwrap();
    premature[final_node.id.value()].state = EliminationNodeFactState::Resolved {
        entrants: [None, None],
        resolution: EliminationNodeResolution::Vacant,
    };
    assert_eq!(
        tournamint::elimination::next_wave(&definition, &premature),
        Err(tournamint::elimination::EliminationError::IncoherentFacts),
    );

    let mut invalid_pending = initial;
    invalid_pending[0].state = EliminationNodeFactState::Resolved {
        entrants: [Some(PlayerId::new(99)), None],
        resolution: EliminationNodeResolution::WithdrawalVacancy {
            withdrawn: PlayerId::new(99),
        },
    };
    assert_eq!(
        tournamint::elimination::withdraw(&definition, &invalid_pending, field[0]),
        Err(tournamint::elimination::EliminationError::IncoherentFacts),
    );
}

#[test]
fn elimination_accepts_causal_materialization_beyond_the_global_wave_frontier() {
    let field = players(8);
    let definition =
        tournamint::elimination::topology(EliminationKind::Single { bronze: false }, &field)
            .unwrap();
    let mut facts = tournamint::elimination::initial_facts(&definition);
    let wave = tournamint::elimination::next_wave(&definition, &facts).unwrap();
    for patch in wave.patches {
        facts[patch.node.value()].state = patch.state;
    }
    let target = definition
        .nodes()
        .iter()
        .find(|node| {
            node.scheduled_wave_index > 0
                && node.sources.iter().all(|source| match source {
                    tournamint::elimination::EliminationSource::WinnerOf(source) => matches!(
                        facts[source.value()].state,
                        EliminationNodeFactState::Active { .. }
                    ),
                    _ => false,
                })
        })
        .unwrap();
    let mut routed = [PlayerId::new(0); 2];
    for (index, source) in target.sources.into_iter().enumerate() {
        let tournamint::elimination::EliminationSource::WinnerOf(node) = source else {
            unreachable!()
        };
        let EliminationNodeFactState::Active { entrants } = facts[node.value()].state else {
            unreachable!()
        };
        routed[index] = entrants[0];
        let patch =
            tournamint::elimination::record_winner(&definition, &facts, node, entrants[0]).unwrap();
        facts[node.value()].state = patch.state;
    }
    facts[target.id.value()].state = EliminationNodeFactState::Active { entrants: routed };
    assert!(tournamint::elimination::project(&definition, &facts, &[]).is_ok());
    assert_eq!(
        tournamint::elimination::next_wave(&definition, &facts),
        Err(EliminationError::IncoherentFacts),
    );
}

#[test]
fn dependency_ready_releases_a_ready_branch_from_wave_facts() {
    let field = players(8);
    let definition =
        tournamint::elimination::topology(EliminationKind::Single { bronze: false }, &field)
            .unwrap();
    let mut facts = tournamint::elimination::initial_facts(&definition);
    let opening = tournamint::elimination::next_wave(&definition, &facts).unwrap();
    assert_eq!(opening.wave_index, 0);
    assert!(
        opening
            .patches
            .iter()
            .all(|patch| { definition.nodes()[patch.node.value()].scheduled_wave_index == 0 })
    );
    for patch in opening.patches {
        facts[patch.node.value()].state = patch.state;
    }

    let target = definition
        .nodes()
        .iter()
        .find(|node| {
            node.scheduled_wave_index == 1
                && node.sources.iter().all(|source| {
                    source_node(*source).is_some_and(|source| {
                        matches!(
                            facts[source.value()].state,
                            EliminationNodeFactState::Active { .. }
                        )
                    })
                })
        })
        .unwrap();
    for source in target.sources {
        let source = source_node(source).unwrap();
        let EliminationNodeFactState::Active { entrants } = facts[source.value()].state else {
            unreachable!()
        };
        facts[source.value()].state =
            tournamint::elimination::record_winner(&definition, &facts, source, entrants[0])
                .unwrap()
                .state;
    }

    let release = tournamint::elimination::next_release(
        &definition,
        &facts,
        EliminationReleaseCadence::DependencyReady,
        &[],
    )
    .unwrap();
    assert_eq!(release.patches.len(), 1);
    assert_eq!(release.patches[0].node, target.id);
    assert!(matches!(
        release.patches[0].state,
        EliminationNodeFactState::Active { .. }
    ));
    facts[target.id.value()].state = release.patches[0].state;
    assert!(facts.iter().any(|fact| {
        definition.nodes()[fact.node.value()].scheduled_wave_index == 0
            && matches!(fact.state, EliminationNodeFactState::Active { .. })
    }));
    assert!(tournamint::elimination::project(&definition, &facts, &[]).is_ok());
    assert_eq!(
        tournamint::elimination::next_release(
            &definition,
            &facts,
            EliminationReleaseCadence::DependencyReady,
            &[],
        ),
        Err(EliminationError::DependenciesPending),
    );
}

#[test]
fn one_double_elimination_result_can_release_different_scheduled_waves() {
    let field = players(8);
    let definition = tournamint::elimination::topology(EliminationKind::Double, &field).unwrap();
    let mut facts = tournamint::elimination::initial_facts(&definition);
    let opening = tournamint::elimination::next_release(
        &definition,
        &facts,
        EliminationReleaseCadence::DependencyReady,
        &[],
    )
    .unwrap();
    for patch in opening.patches {
        facts[patch.node.value()].state = patch.state;
    }
    let opening_active = facts
        .iter()
        .filter_map(|fact| match fact.state {
            EliminationNodeFactState::Active { entrants } => Some((fact.node, entrants)),
            _ => None,
        })
        .collect::<Vec<_>>();
    for (node, entrants) in opening_active {
        facts[node.value()].state =
            tournamint::elimination::record_winner(&definition, &facts, node, entrants[0])
                .unwrap()
                .state;
    }
    let second = tournamint::elimination::next_release(
        &definition,
        &facts,
        EliminationReleaseCadence::DependencyReady,
        &[],
    )
    .unwrap();
    for patch in second.patches {
        facts[patch.node.value()].state = patch.state;
    }
    let winners_round = facts
        .iter()
        .filter_map(|fact| {
            (matches!(
                definition.nodes()[fact.node.value()].stage,
                EliminationStage::WinnersRound { round_index: 1 }
            ) && matches!(fact.state, EliminationNodeFactState::Active { .. }))
            .then_some(fact.node)
        })
        .collect::<Vec<_>>();
    let held = winners_round[0];
    let readying = facts
        .iter()
        .filter_map(|fact| {
            (fact.node != held
                && matches!(
                    definition.nodes()[fact.node.value()].stage,
                    EliminationStage::WinnersRound { round_index: 1 }
                        | EliminationStage::LosersMinor { round_index: 0 }
                ))
            .then_some(match fact.state {
                EliminationNodeFactState::Active { entrants } => Some((fact.node, entrants)),
                _ => None,
            })
            .flatten()
        })
        .collect::<Vec<_>>();
    let lower_losers = readying
        .iter()
        .filter_map(|(node, entrants)| {
            matches!(
                definition.nodes()[node.value()].stage,
                EliminationStage::LosersMinor { round_index: 0 }
            )
            .then_some(entrants[1])
        })
        .collect::<Vec<_>>();
    let (first_lower, first_lower_entrants) = readying
        .iter()
        .find(|(node, _)| {
            matches!(
                definition.nodes()[node.value()].stage,
                EliminationStage::LosersMinor { round_index: 0 }
            )
        })
        .copied()
        .unwrap();
    let mut partial_facts = facts.clone();
    partial_facts[first_lower.value()].state = tournamint::elimination::record_winner(
        &definition,
        &partial_facts,
        first_lower,
        first_lower_entrants[0],
    )
    .unwrap()
    .state;
    let partial_projection =
        tournamint::elimination::project(&definition, &partial_facts, &[]).unwrap();
    let first_lower_loser = partial_projection
        .players
        .iter()
        .find(|result| result.player == first_lower_entrants[1])
        .unwrap();
    assert!(!first_lower_loser.in_contention);
    assert_eq!(first_lower_loser.placement, Some(7));

    let mut mixed_facts = facts.clone();
    let EliminationNodeFactState::Active {
        entrants: held_entrants,
    } = mixed_facts[held.value()].state
    else {
        unreachable!()
    };
    let upper_withdrawn = held_entrants[0];
    for patch in
        tournamint::elimination::withdraw_players(&definition, &mixed_facts, &[upper_withdrawn])
            .unwrap()
    {
        mixed_facts[patch.node.value()].state = patch.state;
    }
    for (node, entrants) in readying.iter().filter(|(node, _)| {
        matches!(
            definition.nodes()[node.value()].stage,
            EliminationStage::LosersMinor { round_index: 0 }
        )
    }) {
        mixed_facts[node.value()].state =
            tournamint::elimination::record_winner(&definition, &mixed_facts, *node, entrants[0])
                .unwrap()
                .state;
    }
    let mixed_projection =
        tournamint::elimination::project(&definition, &mixed_facts, &[upper_withdrawn]).unwrap();
    let upper_result = mixed_projection
        .players
        .iter()
        .find(|result| result.player == upper_withdrawn)
        .unwrap();
    assert_eq!(upper_result.placement, Some(5));
    assert_eq!(
        upper_result.exit_stage,
        Some(EliminationStage::WinnersRound { round_index: 1 })
    );
    assert!(
        mixed_projection.players.iter().all(|result| {
            !lower_losers.contains(&result.player) || result.placement == Some(7)
        })
    );
    for (node, entrants) in readying {
        facts[node.value()].state =
            tournamint::elimination::record_winner(&definition, &facts, node, entrants[0])
                .unwrap()
                .state;
    }
    let projection = tournamint::elimination::project(&definition, &facts, &[]).unwrap();
    assert_eq!(lower_losers.len(), 2);
    for result in &projection.players {
        if lower_losers.contains(&result.player) {
            assert!(!result.in_contention);
            assert_eq!(result.placement, Some(7));
            assert_eq!(
                result.exit_stage,
                Some(EliminationStage::LosersMinor { round_index: 0 })
            );
        } else {
            assert!(result.in_contention);
            assert_eq!(result.placement, None);
            assert_eq!(result.exit_stage, None);
        }
    }
    let partial = tournamint::elimination::next_release(
        &definition,
        &facts,
        EliminationReleaseCadence::DependencyReady,
        &[],
    )
    .unwrap();
    assert_eq!(partial.patches.len(), 1);
    for patch in partial.patches {
        facts[patch.node.value()].state = patch.state;
    }
    let EliminationNodeFactState::Active { entrants } = facts[held.value()].state else {
        unreachable!()
    };
    facts[held.value()].state =
        tournamint::elimination::record_winner(&definition, &facts, held, entrants[0])
            .unwrap()
            .state;
    let release = tournamint::elimination::next_release(
        &definition,
        &facts,
        EliminationReleaseCadence::DependencyReady,
        &[],
    )
    .unwrap();
    assert_eq!(
        release
            .patches
            .iter()
            .map(|patch| definition.nodes()[patch.node.value()].scheduled_wave_index)
            .collect::<std::collections::BTreeSet<_>>(),
        [2, 3].into_iter().collect()
    );
}

#[test]
fn dependency_ready_follows_withdrawal_and_reset_resolutions_to_a_fixed_point() {
    for withdraw_upper in [false, true] {
        let field = players(2);
        let definition =
            tournamint::elimination::topology(EliminationKind::Double, &field).unwrap();
        let mut facts = tournamint::elimination::initial_facts(&definition);
        let opening = tournamint::elimination::next_release(
            &definition,
            &facts,
            EliminationReleaseCadence::DependencyReady,
            &[],
        )
        .unwrap();
        assert_eq!(opening.patches.len(), 1);
        for patch in opening.patches {
            facts[patch.node.value()].state = patch.state;
        }
        let (opening_node, entrants) = facts
            .iter()
            .find_map(|fact| match fact.state {
                EliminationNodeFactState::Active { entrants } => Some((fact.node, entrants)),
                _ => None,
            })
            .unwrap();
        facts[opening_node.value()].state =
            tournamint::elimination::record_winner(&definition, &facts, opening_node, entrants[0])
                .unwrap()
                .state;

        let withdrawn = if withdraw_upper {
            entrants[0]
        } else {
            entrants[1]
        };
        let release = tournamint::elimination::next_release(
            &definition,
            &facts,
            EliminationReleaseCadence::DependencyReady,
            &[withdrawn],
        )
        .unwrap();
        assert_eq!(release.patches.len(), 2);
        assert_eq!(
            release
                .patches
                .iter()
                .map(|patch| definition.nodes()[patch.node.value()].stage)
                .collect::<Vec<_>>(),
            vec![EliminationStage::GrandFinal, EliminationStage::Reset]
        );
        assert!(matches!(
            release.patches[0].state,
            EliminationNodeFactState::Resolved {
                resolution: EliminationNodeResolution::WithdrawalWalkover { .. },
                ..
            }
        ));
        assert_eq!(
            matches!(release.patches[1].state, EliminationNodeFactState::Skipped),
            !withdraw_upper
        );
        assert_eq!(
            matches!(
                release.patches[1].state,
                EliminationNodeFactState::Resolved {
                    resolution: EliminationNodeResolution::AutomaticAdvance { .. },
                    ..
                }
            ),
            withdraw_upper
        );
        for patch in release.patches {
            facts[patch.node.value()].state = patch.state;
        }
        let projection =
            tournamint::elimination::project(&definition, &facts, &[withdrawn]).unwrap();
        assert!(projection.complete);
        let withdrawn_result = projection
            .players
            .iter()
            .find(|result| result.player == withdrawn)
            .unwrap();
        assert!(!withdrawn_result.in_contention);
        assert_eq!(withdrawn_result.placement, Some(2));
        assert_eq!(
            withdrawn_result.exit_stage,
            Some(EliminationStage::GrandFinal)
        );
        let champion_result = projection
            .players
            .iter()
            .find(|result| result.placement == Some(1))
            .unwrap();
        assert!(!champion_result.in_contention);
        assert_eq!(
            champion_result.exit_stage,
            Some(if withdraw_upper {
                EliminationStage::Reset
            } else {
                EliminationStage::GrandFinal
            })
        );
        assert_eq!(
            tournamint::elimination::next_release(
                &definition,
                &facts,
                EliminationReleaseCadence::DependencyReady,
                &[withdrawn],
            ),
            Err(EliminationError::Complete),
        );
    }
}

#[test]
fn dependency_ready_follows_vacancies_across_waves_in_topology_order() {
    let field = players(5);
    let definition =
        tournamint::elimination::topology(EliminationKind::Single { bronze: false }, &field)
            .unwrap();
    let facts = tournamint::elimination::initial_facts(&definition);
    let release = tournamint::elimination::next_release(
        &definition,
        &facts,
        EliminationReleaseCadence::DependencyReady,
        &field,
    )
    .unwrap();
    assert_eq!(release.patches.len(), definition.nodes().len());
    assert!(
        release
            .patches
            .windows(2)
            .all(|patches| patches[0].node < patches[1].node)
    );
    assert!(release.patches.iter().any(|patch| matches!(
        patch.state,
        EliminationNodeFactState::Resolved {
            resolution: EliminationNodeResolution::Vacant,
            ..
        }
    )));
    assert!(
        release
            .patches
            .iter()
            .any(|patch| { definition.nodes()[patch.node.value()].scheduled_wave_index > 0 })
    );
    let mut materialized = facts;
    for patch in release.patches {
        materialized[patch.node.value()].state = patch.state;
    }
    assert!(
        tournamint::elimination::project(&definition, &materialized, &[])
            .unwrap()
            .complete
    );
}

#[test]
fn cadence_wave_matches_next_wave_with_durable_withdrawals_applied() {
    let field = players(4);
    let definition =
        tournamint::elimination::topology(EliminationKind::Single { bronze: false }, &field)
            .unwrap();
    let initial = tournamint::elimination::initial_facts(&definition);
    let release = tournamint::elimination::next_release(
        &definition,
        &initial,
        EliminationReleaseCadence::Wave,
        &[field[0]],
    )
    .unwrap();
    let mut direct = initial.clone();
    for patch in release.patches {
        direct[patch.node.value()].state = patch.state;
    }

    let wave = tournamint::elimination::next_wave(&definition, &initial).unwrap();
    let mut two_step = initial;
    for patch in wave.patches {
        two_step[patch.node.value()].state = patch.state;
    }
    for patch in
        tournamint::elimination::withdraw_players(&definition, &two_step, &[field[0]]).unwrap()
    {
        two_step[patch.node.value()].state = patch.state;
    }
    assert_eq!(direct, two_step);
}

#[test]
fn dependency_ready_records_only_the_new_withdrawal_at_a_convergence() {
    let field = players(2);
    let definition = tournamint::elimination::topology(EliminationKind::Double, &field).unwrap();
    let mut facts = tournamint::elimination::initial_facts(&definition);
    let opening = tournamint::elimination::next_release(
        &definition,
        &facts,
        EliminationReleaseCadence::DependencyReady,
        &[],
    )
    .unwrap();
    for patch in opening.patches {
        facts[patch.node.value()].state = patch.state;
    }
    let (_opening_node, entrants) = facts
        .iter()
        .find_map(|fact| match fact.state {
            EliminationNodeFactState::Active { entrants } => Some((fact.node, entrants)),
            _ => None,
        })
        .unwrap();
    let first_withdrawn = entrants[1];
    for patch in
        tournamint::elimination::withdraw_players(&definition, &facts, &[first_withdrawn]).unwrap()
    {
        facts[patch.node.value()].state = patch.state;
    }
    let grand_final = definition
        .nodes()
        .iter()
        .find(|node| node.stage == EliminationStage::GrandFinal)
        .unwrap();
    let mut repeated_withdrawal = facts.clone();
    repeated_withdrawal[grand_final.id.value()].state = EliminationNodeFactState::Resolved {
        entrants: [Some(entrants[0]), Some(first_withdrawn)],
        resolution: EliminationNodeResolution::WithdrawalWalkover {
            winner: entrants[0],
            withdrawn: first_withdrawn,
        },
    };
    assert_eq!(
        tournamint::elimination::project(&definition, &repeated_withdrawal, &[first_withdrawn]),
        Err(EliminationError::IncoherentFacts),
    );
    let newly_withdrawn = entrants[0];
    let release = tournamint::elimination::next_release(
        &definition,
        &facts,
        EliminationReleaseCadence::DependencyReady,
        &[first_withdrawn, newly_withdrawn],
    )
    .unwrap();
    assert!(matches!(
        release.patches[0].state,
        EliminationNodeFactState::Resolved {
            resolution: EliminationNodeResolution::WithdrawalVacancy { withdrawn },
            ..
        } if withdrawn == newly_withdrawn
    ));
}

fn source_node(source: EliminationSource) -> Option<EliminationNodeId> {
    match source {
        EliminationSource::WinnerOf(node) | EliminationSource::LoserOf(node) => Some(node),
        EliminationSource::InitialSeed { .. } | EliminationSource::Vacant => None,
    }
}

#[test]
fn typed_stages_support_distinct_final_series_plans() {
    use tournamint::series::{ClinchPolicy, EntrantSide, SeriesPhase, SeriesPlan, SetLimit};

    let default = SeriesPlan::balanced();
    let grand_final = SeriesPlan {
        phases: vec![SeriesPhase {
            games_per_set: NonZeroU16::new(1).unwrap(),
            color_order: vec![EntrantSide::Second],
            set_limit: SetLimit::UntilDecisive,
            clinch: ClinchPolicy::PlayAll,
        }],
    };
    let reset = SeriesPlan {
        phases: vec![SeriesPhase {
            games_per_set: NonZeroU16::new(3).unwrap(),
            color_order: vec![EntrantSide::First, EntrantSide::Second, EntrantSide::First],
            set_limit: SetLimit::UntilDecisive,
            clinch: ClinchPolicy::EarlyClinch,
        }],
    };
    let definition =
        tournamint::elimination::topology(EliminationKind::Double, &players(4)).unwrap();
    for node in definition.nodes() {
        let effective = match node.stage {
            tournamint::elimination::EliminationStage::GrandFinal => &grand_final,
            tournamint::elimination::EliminationStage::Reset => &reset,
            _ => &default,
        };
        effective.validate().unwrap();
        if node.stage == tournamint::elimination::EliminationStage::GrandFinal {
            assert_eq!(effective, &grand_final);
        }
        if node.stage == tournamint::elimination::EliminationStage::Reset {
            assert_eq!(effective, &reset);
        }
    }

    let definition =
        tournamint::elimination::topology(EliminationKind::Double, &players(2)).unwrap();
    let mut facts = tournamint::elimination::initial_facts(&definition);
    let opening = tournamint::elimination::next_wave(&definition, &facts).unwrap();
    for patch in opening.patches {
        facts[patch.node.value()].state = patch.state;
    }
    let (opening_node, opening_entrants) = facts
        .iter()
        .find_map(|fact| match fact.state {
            EliminationNodeFactState::Active { entrants } => Some((fact.node, entrants)),
            _ => None,
        })
        .unwrap();
    facts[opening_node.value()].state = tournamint::elimination::record_winner(
        &definition,
        &facts,
        opening_node,
        opening_entrants[0],
    )
    .unwrap()
    .state;
    let final_wave = tournamint::elimination::next_wave(&definition, &facts).unwrap();
    for patch in final_wave.patches {
        facts[patch.node.value()].state = patch.state;
    }
    let (final_node, entrants) = facts
        .iter()
        .find_map(|fact| match fact.state {
            EliminationNodeFactState::Active { entrants } => Some((fact.node, entrants)),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        definition.nodes()[final_node.value()].stage,
        tournamint::elimination::EliminationStage::GrandFinal
    );
    let Some(tournamint::series::SeriesAction::CreateSet { games, .. }) =
        tournamint::series::next_action(&grand_final, entrants, &[]).unwrap()
    else {
        panic!("stage plan creates set")
    };
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].white, entrants[1]);
}

#[test]
fn all_swiss_systems_pair_and_validate_fact_dimensions() {
    let systems = [
        SwissSystemConfig::Dutch(DutchConfig {
            bye_points: SingleGameSwissByePoints::game_win(PointSystem::STANDARD),
        }),
        SwissSystemConfig::Burstein(BursteinConfig {
            bye_points: SingleGameSwissByePoints::game_win(PointSystem::STANDARD),
        }),
        SwissSystemConfig::DoubleSwiss(DoubleSwissConfig {
            primary_score: DoubleSwissPrimaryScore::GamePoints,
            match_point_system: MatchPointSystem::STANDARD,
            bye_points: DoubleSwissByePoints::standard(
                PointSystem::STANDARD,
                MatchPointSystem::STANDARD,
            )
            .unwrap(),
        }),
    ];
    for system in systems {
        let field = players(4);
        let mut facts = SwissFacts {
            config: SwissConfig {
                system,
                expected_rounds: 2,
                initial_color: Color::White,
                game_point_system: PointSystem::STANDARD,
                acceleration: AccelerationPolicy::None,
                standings: SwissStandingsPlan::default(),
            },
            initial_ranking: field.clone(),
            rounds: Vec::new(),
        };
        let pairings = selected_swiss_pairings(tournamint::swiss::pair_next_round(
            &facts,
            &SwissNextRoundInput {
                eligible: field.clone(),
                ..Default::default()
            },
        ));
        let outcome_count = usize::from(matches!(system, SwissSystemConfig::DoubleSwiss(_))) + 1;
        facts.rounds.push(SwissRoundFact {
            eligible: field.clone(),
            encounters: pairings
                .games
                .iter()
                .copied()
                .map(|pairing| SwissEncounterFact {
                    pairing,
                    outcomes: vec![Some(GameOutcome::draw()); outcome_count],
                })
                .collect(),
            pairings,
        });
        assert_eq!(tournamint::swiss::project(&facts).unwrap().players.len(), 4);
        for count in 0..=3 {
            facts.rounds[0].encounters[0].outcomes = vec![None; count];
            let projection = tournamint::swiss::project(&facts);
            if count == outcome_count {
                assert_eq!(
                    projection.unwrap().history_status,
                    SwissHistoryStatus::AwaitingResults
                );
            } else {
                assert_eq!(projection, Err(SwissError::IncoherentFacts));
            }
        }
    }
}

#[test]
fn vacant_grand_final_does_not_require_a_reset() {
    let field = players(5);
    let definition = tournamint::elimination::topology(EliminationKind::Double, &field).unwrap();
    let mut facts = tournamint::elimination::initial_facts(&definition);
    let release = tournamint::elimination::next_release(
        &definition,
        &facts,
        EliminationReleaseCadence::DependencyReady,
        &field,
    )
    .unwrap();
    for patch in release.patches {
        facts[patch.node.value()].state = patch.state;
    }
    let projection = tournamint::elimination::project(&definition, &facts, &field).unwrap();
    assert!(projection.complete);
    assert!(!projection.reset_required);
    let reset = projection
        .nodes
        .iter()
        .find(|node| node.descriptor.stage == EliminationStage::Reset)
        .unwrap();
    assert_eq!(reset.state, EliminationNodeFactState::Skipped);
}

#[test]
fn seed_tiebreaks_use_initial_ranking_in_both_directions() {
    let field = [3, 0, 2, 1].map(PlayerId::new).to_vec();
    for order in [
        TournamentPairingNumberOrder::Ascending,
        TournamentPairingNumberOrder::Descending,
    ] {
        let expected = if order == TournamentPairingNumberOrder::Ascending {
            field.clone()
        } else {
            field.iter().copied().rev().collect()
        };
        let rr = RoundRobinConfig {
            primary_score: Default::default(),
            match_point_system: MatchPointSystem::STANDARD,
            repeats: 1,
            initial_color: Color::White,
            point_system: PointSystem::STANDARD,
            standings: RoundRobinStandingsPlan::new(vec![
                RoundRobinCriterion::PrimaryScore,
                RoundRobinCriterion::TournamentPairingNumber(order),
            ]),
        };
        let rr = tournamint::round_robin::project(&rr, &field, &[]).unwrap();
        assert!(
            rr.standings
                .groups
                .iter()
                .all(|group| group.players.len() == 1)
        );
        assert_eq!(
            rr.standings
                .groups
                .iter()
                .map(|group| group.players[0].player)
                .collect::<Vec<_>>(),
            expected
        );
        let swiss = SwissFacts {
            config: SwissConfig {
                standings: SwissStandingsPlan::new(vec![
                    SwissCriterion::PrimaryScore,
                    SwissCriterion::TournamentPairingNumber(order),
                ]),
                ..swiss_config()
            },
            initial_ranking: field.clone(),
            rounds: Vec::new(),
        };
        let swiss = tournamint::swiss::project(&swiss).unwrap();
        assert!(
            swiss
                .standings
                .groups
                .iter()
                .all(|group| group.players.len() == 1)
        );
        assert_eq!(
            swiss
                .standings
                .groups
                .iter()
                .map(|group| group.players[0].player)
                .collect::<Vec<_>>(),
            expected
        );
    }
}

#[test]
fn double_swiss_alternate_score_separates_primary_ties_without_changing_pairings() {
    let field = players(4);
    let wins = [GameOutcome::white_win(), GameOutcome::black_win()];
    let narrow_win = [GameOutcome::white_win(), GameOutcome::draw()];
    let draws = [GameOutcome::draw(), GameOutcome::draw()];
    for (primary, results, alternate) in [
        (
            DoubleSwissPrimaryScore::MatchPoints,
            [[wins, narrow_win], [draws, draws]],
            SwissCriterion::GamePoints,
        ),
        (
            DoubleSwissPrimaryScore::GamePoints,
            [[narrow_win, wins], [narrow_win, draws]],
            SwissCriterion::MatchPoints,
        ),
    ] {
        let mut facts = SwissFacts {
            config: SwissConfig {
                system: SwissSystemConfig::DoubleSwiss(DoubleSwissConfig {
                    primary_score: primary,
                    match_point_system: MatchPointSystem::STANDARD,
                    bye_points: DoubleSwissByePoints::standard(
                        PointSystem::STANDARD,
                        MatchPointSystem::STANDARD,
                    )
                    .unwrap(),
                }),
                standings: SwissStandingsPlan::new(vec![SwissCriterion::PrimaryScore]),
                ..swiss_config()
            },
            initial_ranking: field.clone(),
            rounds: Vec::new(),
        };
        for (opponents, outcomes) in [[[0, 2], [1, 3]], [[0, 3], [1, 2]]]
            .into_iter()
            .zip(results)
        {
            let games = opponents
                .map(|[white, black]| Pairing::new(field[white], field[black]))
                .to_vec();
            facts.rounds.push(SwissRoundFact {
                eligible: field.clone(),
                encounters: games
                    .iter()
                    .copied()
                    .zip(outcomes)
                    .map(|(pairing, outcomes)| SwissEncounterFact {
                        pairing,
                        outcomes: outcomes.into_iter().map(Some).collect(),
                    })
                    .collect(),
                pairings: RoundPairings {
                    games,
                    byes: Vec::new(),
                },
            });
        }
        let tied = tournamint::swiss::project(&facts).unwrap();
        assert!(tied.standings.groups.iter().any(|group| {
            [field[0], field[1]]
                .iter()
                .all(|player| group.players.iter().any(|row| row.player == *player))
        }));
        let input = SwissNextRoundInput {
            eligible: field.clone(),
            ..Default::default()
        };
        let pairings = tournamint::swiss::pair_next_round(&facts, &input);
        facts.config.standings =
            SwissStandingsPlan::new(vec![SwissCriterion::PrimaryScore, alternate]);
        let separated = tournamint::swiss::project(&facts).unwrap();
        let place = |player| {
            separated
                .standings
                .groups
                .iter()
                .find(|group| group.players.iter().any(|row| row.player == player))
                .unwrap()
                .rank
        };
        assert!(place(field[0]) < place(field[1]));
        assert_eq!(tournamint::swiss::pair_next_round(&facts, &input), pairings);
    }
}
