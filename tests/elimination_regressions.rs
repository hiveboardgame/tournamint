use std::collections::HashSet;

use tournamint::PlayerId;
use tournamint::elimination::{
    self, EliminationDefinition, EliminationError, EliminationKind, EliminationNodeFact,
    EliminationNodeFactState, EliminationNodeId, EliminationNodePatch, EliminationNodeResolution,
    EliminationReleaseCadence, EliminationSource, EliminationStage,
};

fn definition(kind: EliminationKind, count: usize) -> EliminationDefinition {
    elimination::topology(kind, &(0..count).map(PlayerId::new).collect::<Vec<_>>()).unwrap()
}

fn apply(facts: &mut [EliminationNodeFact], patches: Vec<EliminationNodePatch>) {
    for patch in patches {
        facts[patch.node.value()].state = patch.state;
    }
}

fn winner_path(
    definition: &EliminationDefinition,
    mut node: EliminationNodeId,
) -> Vec<EliminationNodeId> {
    let mut path = vec![node];
    while let Some(consumer) = definition.nodes().iter().find(|candidate| {
        candidate
            .sources
            .contains(&EliminationSource::WinnerOf(node))
    }) {
        node = consumer.id;
        path.push(node);
    }
    path
}

#[test]
fn double_elimination_separates_all_previous_opponents_before_late_lower_rounds() {
    // An earlier encounter can recur in the lower bracket only when its
    // winner subsequently loses at a winners-bracket ancestor. Enumerating
    // every such pair of loss routes covers all possible match outcomes,
    // including the full brackets containing each non-power-of-two field.
    for count in 4usize..=64 {
        let definition = definition(EliminationKind::Double, count);
        let earliest_rematch = match count.next_power_of_two() {
            4 => 2,
            8 => 3,
            16 => 4,
            32 => 5,
            64 => 7,
            _ => unreachable!(),
        };
        for node in definition.nodes() {
            for source in node.sources {
                match source {
                    EliminationSource::InitialSeed { seed_index } => assert!(seed_index < count),
                    EliminationSource::WinnerOf(parent) | EliminationSource::LoserOf(parent) => {
                        assert!(parent < node.id);
                    }
                    EliminationSource::Vacant => {}
                }
            }
            if !matches!(node.stage, EliminationStage::WinnersRound { .. }) {
                continue;
            }
            let lower_path = |source| {
                let consumers = definition
                    .nodes()
                    .iter()
                    .filter(|candidate| {
                        candidate
                            .sources
                            .contains(&EliminationSource::LoserOf(source))
                    })
                    .collect::<Vec<_>>();
                assert_eq!(consumers.len(), 1, "each first loss has exactly one route");
                winner_path(&definition, consumers[0].id)
            };
            let first_loser = lower_path(node.id);
            for ancestor in winner_path(&definition, node.id).into_iter().skip(1) {
                if !matches!(
                    definition.nodes()[ancestor.value()].stage,
                    EliminationStage::WinnersRound { .. }
                ) {
                    break;
                }
                let later_loser = lower_path(ancestor);
                let reunion = first_loser
                    .iter()
                    .find(|node| later_loser.contains(node))
                    .expect("both lower paths eventually reach the grand final");
                if let Some(round) = definition.nodes()[reunion.value()]
                    .stage
                    .lower_round_ordinal()
                {
                    assert!(
                        round + 1 >= earliest_rematch,
                        "{count} entrants: opponents from {node:?} can meet again at {reunion:?} \
                         in lower round {}, after the winner loses at {ancestor:?}",
                        round + 1,
                    );
                }
            }
        }
    }
}

#[test]
fn sixteen_player_first_round_opponents_do_not_reunite_in_lower_round_three() {
    let definition = definition(EliminationKind::Double, 16);
    let mut facts = elimination::initial_facts(&definition);
    let mut encounters = HashSet::new();
    let mut played_opening = false;
    let mut later_loss = false;
    // Seeds 9 and 8 meet in the first round, then seed 9 loses to seed 16.
    // Let both survive their lower-bracket matches. The previous neighboring
    // crossover brought them together in lower round three.
    let winners = [15, 8, 3, 4, 1, 6, 13, 10, 15, 3, 6, 10];
    loop {
        match elimination::next_release(
            &definition,
            &facts,
            EliminationReleaseCadence::DependencyReady,
            &[],
        ) {
            Ok(release) => apply(&mut facts, release.patches),
            Err(EliminationError::DependenciesPending) => {}
            Err(EliminationError::Complete) => break,
            Err(error) => panic!("unexpected release error: {error}"),
        }
        let (node, entrants) = facts
            .iter()
            .find_map(|fact| match fact.state {
                EliminationNodeFactState::Active { entrants } => Some((fact.node, entrants)),
                _ => None,
            })
            .expect("an unfinished full bracket has an active match");
        let stage = definition.nodes()[node.value()].stage;
        let mut pair = entrants;
        pair.sort();
        if !encounters.insert(pair)
            && let Some(round) = stage.lower_round_ordinal()
        {
            assert!(round >= 3, "early rematch between {pair:?} in {stage:?}");
        }
        let winner = if matches!(stage, EliminationStage::WinnersRound { .. })
            && node.value() < winners.len()
        {
            PlayerId::new(winners[node.value()])
        } else {
            entrants
                .into_iter()
                .find(|player| [PlayerId::new(7), PlayerId::new(8)].contains(player))
                .unwrap_or(entrants[0])
        };
        played_opening |= pair == [PlayerId::new(7), PlayerId::new(8)]
            && stage == (EliminationStage::WinnersRound { round_index: 0 })
            && winner == PlayerId::new(8);
        later_loss |= pair == [PlayerId::new(8), PlayerId::new(15)]
            && stage == (EliminationStage::WinnersRound { round_index: 1 })
            && winner == PlayerId::new(15);
        let patch = elimination::record_winner(&definition, &facts, node, winner).unwrap();
        apply(&mut facts, vec![patch]);
    }
    assert!(played_opening && later_loss);
    assert!(
        elimination::project(&definition, &facts, &[])
            .unwrap()
            .complete
    );
}

#[test]
fn withdrawing_after_a_bye_preserves_the_same_advancement_as_a_played_win() {
    for cadence in [
        EliminationReleaseCadence::Wave,
        EliminationReleaseCadence::DependencyReady,
    ] {
        let definition = definition(EliminationKind::Single { bronze: false }, 5);
        for withdrawn in [PlayerId::new(0), PlayerId::new(3)] {
            let mut facts = elimination::initial_facts(&definition);
            let opening = elimination::next_release(&definition, &facts, cadence, &[]).unwrap();
            apply(&mut facts, opening.patches);
            let bye = facts[0];
            assert!(matches!(bye.state, EliminationNodeFactState::Resolved {
                resolution: EliminationNodeResolution::AutomaticAdvance { player }, ..
            } if player == PlayerId::new(0)));

            if withdrawn == PlayerId::new(0) {
                let patches =
                    elimination::withdraw_players(&definition, &facts, &[withdrawn]).unwrap();
                apply(&mut facts, patches);
                assert_eq!(facts[0], bye, "an earned bye must not be taken back");
                assert_eq!(
                    elimination::project(&definition, &facts, &[withdrawn])
                        .unwrap()
                        .players[0]
                        .placement,
                    Some(3),
                    "the pending next match already determines the withdrawal tier",
                );
            }

            let opening_contest = facts
                .iter()
                .find_map(|fact| match fact.state {
                    EliminationNodeFactState::Active { entrants }
                        if entrants.contains(&PlayerId::new(3)) =>
                    {
                        Some(fact.node)
                    }
                    _ => None,
                })
                .unwrap();
            let patch =
                elimination::record_winner(&definition, &facts, opening_contest, PlayerId::new(3))
                    .unwrap();
            apply(&mut facts, vec![patch]);
            let patches = elimination::withdraw_players(&definition, &facts, &[withdrawn]).unwrap();
            apply(&mut facts, patches);
            let release =
                elimination::next_release(&definition, &facts, cadence, &[withdrawn]).unwrap();
            apply(&mut facts, release.patches);
            assert_eq!(facts[0], bye);
            assert!(facts.iter().any(|fact| matches!(fact.state,
                EliminationNodeFactState::Resolved {
                    resolution: EliminationNodeResolution::WithdrawalWalkover { withdrawn: player, .. }, ..
                } if player == withdrawn
            )));
            let result = elimination::project(&definition, &facts, &[withdrawn]).unwrap();
            assert_eq!(result.players[withdrawn.index()].placement, Some(3));
            assert_eq!(
                result.players[withdrawn.index()].exit_stage,
                Some(EliminationStage::SingleRound { round_index: 1 })
            );
            assert!(!result.players[withdrawn.index()].in_contention);
        }
    }
}
