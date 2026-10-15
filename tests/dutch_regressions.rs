use tournamint::swiss::{
    AccelerationPolicy, ByeKind, DutchConfig, RoundPairings, SingleGameSwissByePoints, SwissConfig,
    SwissEncounterFact, SwissFacts, SwissHistoryStatus, SwissNextRound, SwissNextRoundInput,
    SwissRoundFact, SwissStandingsPlan, SwissSystemConfig,
};
use tournamint::{Color, GameOutcome, Pairing, PlayerId, PointSystem};

fn facts(ranking: Vec<PlayerId>, expected_rounds: u32) -> SwissFacts {
    SwissFacts {
        config: SwissConfig {
            system: SwissSystemConfig::Dutch(DutchConfig {
                bye_points: SingleGameSwissByePoints::game_win(PointSystem::STANDARD),
            }),
            expected_rounds,
            initial_color: Color::White,
            game_point_system: PointSystem::STANDARD,
            acceleration: AccelerationPolicy::None,
            standings: SwissStandingsPlan::default(),
        },
        initial_ranking: ranking,
        rounds: Vec::new(),
    }
}

fn next_pairings(facts: &SwissFacts, input: &SwissNextRoundInput) -> RoundPairings {
    match tournamint::swiss::pair_next_round(facts, input).unwrap() {
        SwissNextRound::Pairings(pairings) => pairings,
        SwissNextRound::PairingExhausted => panic!("fixture exhausted before its expected round"),
    }
}

#[test]
fn dutch_six_players_can_exhaust_before_four_configured_rounds() {
    let field = (0..6).map(PlayerId::new).collect::<Vec<_>>();
    let mut facts = facts(field.clone(), 4);
    let input = SwissNextRoundInput {
        eligible: field.clone(),
        ..Default::default()
    };
    let draw = GameOutcome::draw();
    let win = GameOutcome::white_win();
    // These are actual generated Dutch pairings. Results leave two disjoint
    // triangles of unmet opponents: {0, 2, 4} and {1, 3, 5}. Every player has
    // two unmet opponents, but neither odd component has a perfect matching.
    let rounds = [
        [(0, 3, draw), (4, 1, draw), (2, 5, draw)],
        [(5, 0, draw), (3, 4, draw), (1, 2, win)],
        [(0, 1, draw), (4, 5, win), (2, 3, win)],
    ];
    for expected in rounds {
        let pairings = next_pairings(&facts, &input);
        assert!(pairings.byes.is_empty());
        assert_eq!(pairings.games.len(), expected.len());
        let encounters = pairings
            .games
            .iter()
            .copied()
            .map(|pairing| {
                let (_, _, outcome) = expected
                    .iter()
                    .find(|&&(white, black, _)| pairing == Pairing::new(field[white], field[black]))
                    .expect("the expected opponents and colours are preserved");
                SwissEncounterFact {
                    pairing,
                    outcomes: vec![Some(*outcome)],
                }
            })
            .collect();
        facts.rounds.push(SwissRoundFact {
            eligible: field.clone(),
            pairings,
            encounters,
        });
    }
    assert_eq!(
        tournamint::swiss::pair_next_round(&facts, &input).unwrap(),
        SwissNextRound::PairingExhausted,
    );
    let projection = tournamint::swiss::project(&facts).unwrap();
    assert_eq!(projection.rounds.len(), 3);
    assert_eq!(projection.history_status, SwissHistoryStatus::ReadyToPair);
}

#[test]
fn dutch_board_order_uses_initial_ranking_and_replay_keeps_accepted_order() {
    let ranking = (0..6).rev().map(PlayerId::new).collect::<Vec<_>>();
    let mut facts = facts(ranking.clone(), 4);
    let input = SwissNextRoundInput {
        eligible: ranking.clone(),
        ..Default::default()
    };
    let mut pairings = next_pairings(&facts, &input);
    assert_eq!(
        pairings.games,
        [
            Pairing::new(PlayerId::new(5), PlayerId::new(2)),
            Pairing::new(PlayerId::new(1), PlayerId::new(4)),
            Pairing::new(PlayerId::new(3), PlayerId::new(0)),
        ],
    );
    // Historical board identities follow the host's accepted facts, even if
    // those facts predate the publication-order rule.
    pairings.games.reverse();
    facts.rounds.push(SwissRoundFact {
        eligible: ranking,
        encounters: pairings
            .games
            .iter()
            .copied()
            .map(|pairing| SwissEncounterFact {
                pairing,
                outcomes: vec![Some(GameOutcome::draw())],
            })
            .collect(),
        pairings: pairings.clone(),
    });
    let projection = tournamint::swiss::project(&facts).unwrap();
    for (index, encounter) in projection.rounds[0].encounters.iter().enumerate() {
        assert_eq!(encounter.pairing_index, index as u32);
        assert_eq!(encounter.pairing, pairings.games[index]);
    }
}

#[test]
fn a_lone_remaining_eligible_player_still_receives_one_bye() {
    let field = (0..6).map(PlayerId::new).collect::<Vec<_>>();
    let mut facts = facts(field, 4);
    let remaining = PlayerId::new(5);
    let input = SwissNextRoundInput {
        eligible: vec![remaining],
        ..Default::default()
    };
    let pairings = next_pairings(&facts, &input);
    assert!(pairings.games.is_empty());
    assert_eq!(pairings.byes.len(), 1);
    assert_eq!(pairings.byes[0].player, remaining);
    assert_eq!(pairings.byes[0].kind, ByeKind::PairingAllocated);
    facts.rounds.push(SwissRoundFact {
        eligible: input.eligible.clone(),
        pairings,
        encounters: Vec::new(),
    });
    assert_eq!(
        tournamint::swiss::pair_next_round(&facts, &input).unwrap(),
        SwissNextRound::PairingExhausted,
    );
}
