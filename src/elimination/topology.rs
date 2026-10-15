use super::seeding::{bracket_size_for, standard_seed_order};
use super::{EliminationNodeDescriptor, EliminationNodeId, EliminationSource, EliminationStage};

/// One immutable node in an elimination bracket's complete topology.
///
/// Nodes are stored in deterministic scheduling order: waves first, then the
/// documented stage order inside a shared wave, then stage ordinal. The node
/// identifier is therefore also its index in [`TopologyPlan::nodes`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TopologyNode {
    pub(crate) descriptor: EliminationNodeDescriptor,
    pub(crate) conditional: bool,
}

impl core::ops::Deref for TopologyNode {
    type Target = EliminationNodeDescriptor;

    fn deref(&self) -> &Self::Target {
        &self.descriptor
    }
}

/// The single immutable source graph used by scheduling and projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TopologyPlan {
    nodes: Vec<TopologyNode>,
    /// Start index of every wave in `nodes`. Every planned wave has at least
    /// one node, so adjacent starts delimit non-empty slices.
    wave_starts: Vec<usize>,
}

impl TopologyPlan {
    /// Builds a complete single-elimination topology for a validated field.
    ///
    /// The final is followed by the bronze node in their shared scheduling
    /// wave. With only two seeds there are no semifinal losers, so a requested
    /// bronze node has two immutable vacant sources.
    pub(crate) fn single(seed_count: usize, third_place: bool) -> Self {
        assert!(
            seed_count >= 2,
            "an elimination topology requires at least two seeds"
        );

        let bracket_size = bracket_size_for(seed_count);
        let round_count = bracket_size.trailing_zeros() as usize;
        let initial_sources = initial_sources(seed_count, bracket_size);
        let mut builder = TopologyBuilder::default();
        let mut previous_round = Vec::new();

        for round_index in 0..round_count {
            let wave_index = builder.start_wave();
            debug_assert_eq!(wave_index, round_index);
            let node_count = bracket_size >> (round_index + 1);
            let is_final = round_index + 1 == round_count;
            let mut round = Vec::with_capacity(node_count);

            for ordinal in 0..node_count {
                let sources = if round_index == 0 {
                    source_pair(&initial_sources, ordinal)
                } else {
                    winner_pair(&previous_round, ordinal)
                };
                let stage = if is_final {
                    EliminationStage::SingleFinal
                } else {
                    EliminationStage::SingleRound { round_index }
                };
                round.push(builder.push_node(stage, ordinal, sources, false));
            }

            if is_final && third_place {
                let sources = if previous_round.len() == 2 {
                    [
                        EliminationSource::LoserOf(previous_round[0]),
                        EliminationSource::LoserOf(previous_round[1]),
                    ]
                } else {
                    [EliminationSource::Vacant, EliminationSource::Vacant]
                };
                builder.push_node(EliminationStage::Bronze, 0, sources, false);
            }

            previous_round = round;
        }

        builder.finish()
    }

    /// Builds the complete double-elimination topology for a
    /// validated field, including the conditional reset.
    ///
    /// Winners rounds precede losers-minor nodes in their shared wave. Each
    /// losers-major node receives its same-ordinal minor survivor and a loser
    /// crossed from another section of the winners bracket. Successive
    /// crossings separate paths from earlier encounters to delay rematches.
    pub(crate) fn double(seed_count: usize) -> Self {
        assert!(
            seed_count >= 2,
            "an elimination topology requires at least two seeds"
        );

        let bracket_size = bracket_size_for(seed_count);
        let winners_round_count = bracket_size.trailing_zeros() as usize;
        let initial_sources = initial_sources(seed_count, bracket_size);
        let mut builder = TopologyBuilder::default();

        let first_wave = builder.start_wave();
        debug_assert_eq!(first_wave, 0);
        let mut winners_round = Vec::with_capacity(bracket_size / 2);
        for ordinal in 0..(bracket_size / 2) {
            winners_round.push(builder.push_node(
                EliminationStage::WinnersRound { round_index: 0 },
                ordinal,
                source_pair(&initial_sources, ordinal),
                false,
            ));
        }

        // A two-player bracket has no separate losers-bracket stage. The
        // winners-round loser is the lower-side grand-final entrant.
        let losers_champion_source = if winners_round_count == 1 {
            EliminationSource::LoserOf(winners_round[0])
        } else {
            let mut previous_major: Vec<EliminationNodeId> = Vec::new();

            for winners_round_index in 1..winners_round_count {
                let shared_wave = builder.start_wave();
                debug_assert_eq!(shared_wave, 2 * winners_round_index - 1);

                let winners_node_count = bracket_size >> (winners_round_index + 1);
                let mut next_winners_round = Vec::with_capacity(winners_node_count);
                for ordinal in 0..winners_node_count {
                    next_winners_round.push(builder.push_node(
                        EliminationStage::WinnersRound {
                            round_index: winners_round_index,
                        },
                        ordinal,
                        winner_pair(&winners_round, ordinal),
                        false,
                    ));
                }

                let minor_index = winners_round_index - 1;
                let minor_sources: Vec<[EliminationSource; 2]> = if winners_round_index == 1 {
                    (0..winners_node_count)
                        .map(|ordinal| loser_pair(&winners_round, ordinal))
                        .collect()
                } else {
                    (0..winners_node_count)
                        .map(|ordinal| winner_pair(&previous_major, ordinal))
                        .collect()
                };
                let mut minor_round = Vec::with_capacity(winners_node_count);
                for (ordinal, sources) in minor_sources.into_iter().enumerate() {
                    minor_round.push(builder.push_node(
                        EliminationStage::LosersMinor {
                            round_index: minor_index,
                        },
                        ordinal,
                        sources,
                        false,
                    ));
                }

                let major_wave = builder.start_wave();
                debug_assert_eq!(major_wave, 2 * winners_round_index);
                let mut major_round = Vec::with_capacity(winners_node_count);
                let crossing = drop_in_crossing(winners_round_index, winners_node_count);
                for (ordinal, &minor_node) in minor_round.iter().enumerate() {
                    let crossed_winners_ordinal = ordinal ^ crossing;
                    major_round.push(builder.push_node(
                        EliminationStage::LosersMajor {
                            round_index: minor_index,
                        },
                        ordinal,
                        [
                            EliminationSource::WinnerOf(minor_node),
                            EliminationSource::LoserOf(next_winners_round[crossed_winners_ordinal]),
                        ],
                        false,
                    ));
                }

                winners_round = next_winners_round;
                previous_major = major_round;
            }

            EliminationSource::WinnerOf(previous_major[0])
        };

        let grand_final_wave = builder.start_wave();
        debug_assert_eq!(grand_final_wave, 2 * winners_round_count - 1);
        let grand_final = builder.push_node(
            EliminationStage::GrandFinal,
            0,
            [
                EliminationSource::WinnerOf(winners_round[0]),
                losers_champion_source,
            ],
            false,
        );

        let reset_wave = builder.start_wave();
        debug_assert_eq!(reset_wave, 2 * winners_round_count);
        builder.push_node(
            EliminationStage::Reset,
            0,
            [
                EliminationSource::WinnerOf(grand_final),
                EliminationSource::LoserOf(grand_final),
            ],
            true,
        );

        builder.finish()
    }

    pub(crate) fn nodes(&self) -> &[TopologyNode] {
        &self.nodes
    }

    #[cfg(test)]
    pub(crate) fn node(&self, id: EliminationNodeId) -> Option<&TopologyNode> {
        self.nodes.get(id.value()).filter(|node| node.id == id)
    }

    #[cfg(test)]
    pub(crate) fn wave(&self, wave_index: usize) -> Option<&[TopologyNode]> {
        let &start = self.wave_starts.get(wave_index)?;
        let end = self
            .wave_starts
            .get(wave_index + 1)
            .copied()
            .unwrap_or(self.nodes.len());
        Some(&self.nodes[start..end])
    }

    #[cfg(test)]
    pub(crate) fn wave_count(&self) -> usize {
        self.wave_starts.len()
    }
}

#[derive(Default)]
struct TopologyBuilder {
    nodes: Vec<TopologyNode>,
    wave_starts: Vec<usize>,
}

impl TopologyBuilder {
    fn start_wave(&mut self) -> usize {
        let wave_index = self.wave_starts.len();
        self.wave_starts.push(self.nodes.len());
        wave_index
    }

    fn push_node(
        &mut self,
        stage: EliminationStage,
        ordinal: usize,
        sources: [EliminationSource; 2],
        conditional: bool,
    ) -> EliminationNodeId {
        let scheduled_wave_index = self
            .wave_starts
            .len()
            .checked_sub(1)
            .expect("a topology node must belong to a scheduling wave");
        let id = EliminationNodeId::new(self.nodes.len());
        self.nodes.push(TopologyNode {
            descriptor: EliminationNodeDescriptor {
                id,
                scheduled_wave_index,
                stage,
                stage_ordinal: ordinal,
                sources,
            },
            conditional,
        });
        id
    }

    fn finish(self) -> TopologyPlan {
        debug_assert!(
            self.wave_starts
                .windows(2)
                .all(|starts| starts[0] < starts[1])
        );
        debug_assert!(
            self.wave_starts
                .last()
                .is_some_and(|start| *start < self.nodes.len())
        );
        TopologyPlan {
            nodes: self.nodes,
            wave_starts: self.wave_starts,
        }
    }
}

fn drop_in_crossing(winners_round_index: usize, node_count: usize) -> usize {
    if node_count == 1 {
        return 0;
    }
    if winners_round_index == 1 {
        return node_count - 1;
    }
    // Reverse the first drop-in round, then spread later loss rounds among
    // halves, quarters, and smaller sections. Reversing the round counter's
    // bits changes the widest sections first; swapping only neighbours lets
    // earlier opponents converge again at the next losers-minor round.
    (winners_round_index - 1).reverse_bits() >> (usize::BITS - node_count.ilog2())
}

fn initial_sources(seed_count: usize, bracket_size: usize) -> Vec<EliminationSource> {
    standard_seed_order(bracket_size)
        .into_iter()
        .map(|seed_number| initial_source(seed_number, seed_count))
        .collect()
}

fn initial_source(seed_number: usize, seed_count: usize) -> EliminationSource {
    if seed_number <= seed_count {
        EliminationSource::InitialSeed {
            seed_index: seed_number - 1,
        }
    } else {
        EliminationSource::Vacant
    }
}

fn source_pair(sources: &[EliminationSource], ordinal: usize) -> [EliminationSource; 2] {
    [sources[ordinal * 2], sources[ordinal * 2 + 1]]
}

fn winner_pair(round: &[EliminationNodeId], ordinal: usize) -> [EliminationSource; 2] {
    [
        EliminationSource::WinnerOf(round[ordinal * 2]),
        EliminationSource::WinnerOf(round[ordinal * 2 + 1]),
    ]
}

fn loser_pair(round: &[EliminationNodeId], ordinal: usize) -> [EliminationSource; 2] {
    [
        EliminationSource::LoserOf(round[ordinal * 2]),
        EliminationSource::LoserOf(round[ordinal * 2 + 1]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: usize) -> EliminationNodeId {
        EliminationNodeId::new(value)
    }

    fn wave_ids(plan: &TopologyPlan, wave_index: usize) -> Vec<usize> {
        plan.wave(wave_index)
            .expect("the fixture wave exists")
            .iter()
            .map(|node| node.id.value())
            .collect()
    }

    #[test]
    fn two_seed_single_plan_has_a_fixed_final_and_optional_vacant_bronze() {
        let without_bronze = TopologyPlan::single(2, false);
        assert_eq!(without_bronze.wave_count(), 1);
        assert_eq!(wave_ids(&without_bronze, 0), vec![0]);
        assert_eq!(
            without_bronze.nodes()[0].stage,
            EliminationStage::SingleFinal
        );
        assert_eq!(
            without_bronze.nodes()[0].sources,
            [
                EliminationSource::InitialSeed { seed_index: 0 },
                EliminationSource::InitialSeed { seed_index: 1 },
            ]
        );

        let with_bronze = TopologyPlan::single(2, true);
        assert_eq!(wave_ids(&with_bronze, 0), vec![0, 1]);
        assert_eq!(with_bronze.nodes()[1].stage, EliminationStage::Bronze);
        assert_eq!(
            with_bronze.nodes()[1].sources,
            [EliminationSource::Vacant, EliminationSource::Vacant]
        );
    }

    #[test]
    fn three_seed_single_plan_retains_bye_edges_and_semifinal_loser_edges() {
        let plan = TopologyPlan::single(3, true);
        assert_eq!(plan.wave_count(), 2);
        assert_eq!(wave_ids(&plan, 0), vec![0, 1]);
        assert_eq!(wave_ids(&plan, 1), vec![2, 3]);
        assert_eq!(
            plan.nodes()[0].sources,
            [
                EliminationSource::InitialSeed { seed_index: 0 },
                EliminationSource::Vacant,
            ]
        );
        assert_eq!(
            plan.nodes()[1].sources,
            [
                EliminationSource::InitialSeed { seed_index: 1 },
                EliminationSource::InitialSeed { seed_index: 2 },
            ]
        );
        assert_eq!(
            plan.nodes()[2].sources,
            [
                EliminationSource::WinnerOf(id(0)),
                EliminationSource::WinnerOf(id(1))
            ]
        );
        assert_eq!(
            plan.nodes()[3].sources,
            [
                EliminationSource::LoserOf(id(0)),
                EliminationSource::LoserOf(id(1))
            ]
        );
    }

    #[test]
    fn five_seed_single_plan_is_wave_major_with_stable_source_edges() {
        let plan = TopologyPlan::single(5, true);
        assert_eq!(plan.wave_count(), 3);
        assert_eq!(wave_ids(&plan, 0), vec![0, 1, 2, 3]);
        assert_eq!(wave_ids(&plan, 1), vec![4, 5]);
        assert_eq!(wave_ids(&plan, 2), vec![6, 7]);
        assert_eq!(
            plan.nodes()[0].sources,
            [
                EliminationSource::InitialSeed { seed_index: 0 },
                EliminationSource::Vacant,
            ]
        );
        assert_eq!(
            plan.nodes()[1].sources,
            [
                EliminationSource::InitialSeed { seed_index: 3 },
                EliminationSource::InitialSeed { seed_index: 4 },
            ]
        );
        assert_eq!(
            plan.nodes()[6].sources,
            [
                EliminationSource::WinnerOf(id(4)),
                EliminationSource::WinnerOf(id(5))
            ]
        );
        assert_eq!(
            plan.nodes()[7].sources,
            [
                EliminationSource::LoserOf(id(4)),
                EliminationSource::LoserOf(id(5))
            ]
        );
        assert_eq!(plan.nodes()[6].stage, EliminationStage::SingleFinal);
        assert_eq!(plan.nodes()[7].stage, EliminationStage::Bronze);
    }

    #[test]
    fn two_seed_double_plan_routes_wb_loser_to_grand_final_and_plans_reset() {
        let plan = TopologyPlan::double(2);
        assert_eq!(plan.wave_count(), 3);
        assert_eq!(wave_ids(&plan, 0), vec![0]);
        assert_eq!(wave_ids(&plan, 1), vec![1]);
        assert_eq!(wave_ids(&plan, 2), vec![2]);
        assert_eq!(
            plan.nodes()[1].sources,
            [
                EliminationSource::WinnerOf(id(0)),
                EliminationSource::LoserOf(id(0))
            ]
        );
        assert_eq!(plan.nodes()[2].stage, EliminationStage::Reset);
        assert!(plan.nodes()[2].conditional);
        assert_eq!(
            plan.nodes()[2].sources,
            [
                EliminationSource::WinnerOf(id(1)),
                EliminationSource::LoserOf(id(1))
            ]
        );
    }

    #[test]
    fn three_seed_double_plan_has_complete_minor_major_and_final_graph() {
        let plan = TopologyPlan::double(3);
        assert_eq!(plan.wave_count(), 5);
        assert_eq!(wave_ids(&plan, 0), vec![0, 1]);
        assert_eq!(wave_ids(&plan, 1), vec![2, 3]);
        assert_eq!(wave_ids(&plan, 2), vec![4]);
        assert_eq!(wave_ids(&plan, 3), vec![5]);
        assert_eq!(wave_ids(&plan, 4), vec![6]);
        assert_eq!(
            plan.nodes()[3].stage,
            EliminationStage::LosersMinor { round_index: 0 }
        );
        assert_eq!(
            plan.nodes()[3].sources,
            [
                EliminationSource::LoserOf(id(0)),
                EliminationSource::LoserOf(id(1))
            ]
        );
        assert_eq!(
            plan.nodes()[4].stage,
            EliminationStage::LosersMajor { round_index: 0 }
        );
        assert_eq!(
            plan.nodes()[4].sources,
            [
                EliminationSource::WinnerOf(id(3)),
                EliminationSource::LoserOf(id(2))
            ]
        );
        assert_eq!(
            plan.nodes()[5].sources,
            [
                EliminationSource::WinnerOf(id(2)),
                EliminationSource::WinnerOf(id(4))
            ]
        );
    }

    #[test]
    fn five_seed_double_plan_crosses_winners_drop_ins() {
        let plan = TopologyPlan::double(5);
        assert_eq!(plan.wave_count(), 7);
        assert_eq!(wave_ids(&plan, 0), vec![0, 1, 2, 3]);
        assert_eq!(wave_ids(&plan, 1), vec![4, 5, 6, 7]);
        assert_eq!(wave_ids(&plan, 2), vec![8, 9]);
        assert_eq!(wave_ids(&plan, 3), vec![10, 11]);
        assert_eq!(wave_ids(&plan, 4), vec![12]);
        assert_eq!(wave_ids(&plan, 5), vec![13]);
        assert_eq!(wave_ids(&plan, 6), vec![14]);

        assert_eq!(
            plan.nodes()[8].sources,
            [
                EliminationSource::WinnerOf(id(6)),
                EliminationSource::LoserOf(id(5))
            ]
        );
        assert_eq!(
            plan.nodes()[9].sources,
            [
                EliminationSource::WinnerOf(id(7)),
                EliminationSource::LoserOf(id(4))
            ]
        );
        assert_eq!(
            plan.nodes()[10].stage,
            EliminationStage::WinnersRound { round_index: 2 }
        );
        assert_eq!(
            plan.nodes()[11].stage,
            EliminationStage::LosersMinor { round_index: 1 }
        );
        assert_eq!(
            plan.nodes()[12].stage,
            EliminationStage::LosersMajor { round_index: 1 }
        );
        assert_eq!(
            plan.nodes()
                .iter()
                .filter_map(|node| node.stage.lower_round_ordinal())
                .collect::<Vec<_>>(),
            vec![0, 0, 1, 1, 2, 3]
        );
        assert_eq!(
            EliminationStage::LosersMinor {
                round_index: usize::MAX,
            }
            .lower_round_ordinal(),
            None
        );
        assert_eq!(
            plan.nodes()[13].sources,
            [
                EliminationSource::WinnerOf(id(10)),
                EliminationSource::WinnerOf(id(12))
            ]
        );
        assert_eq!(
            plan.nodes()[14].sources,
            [
                EliminationSource::WinnerOf(id(13)),
                EliminationSource::LoserOf(id(13))
            ]
        );
        assert!(plan.nodes()[14].conditional);
        assert!(plan.nodes()[..14].iter().all(|node| !node.conditional));
    }

    #[test]
    fn nine_seed_double_plan_routes_first_drop_ins_across_distant_sections() {
        let plan = TopologyPlan::double(9);
        assert_eq!(plan.wave_count(), 9);

        assert_eq!(
            plan.nodes()[16].sources,
            [
                EliminationSource::WinnerOf(id(12)),
                EliminationSource::LoserOf(id(11))
            ]
        );
        assert_eq!(
            plan.nodes()[17].sources,
            [
                EliminationSource::WinnerOf(id(13)),
                EliminationSource::LoserOf(id(10))
            ]
        );
        assert_eq!(
            plan.nodes()[18].sources,
            [
                EliminationSource::WinnerOf(id(14)),
                EliminationSource::LoserOf(id(9))
            ]
        );
        assert_eq!(
            plan.nodes()[19].sources,
            [
                EliminationSource::WinnerOf(id(15)),
                EliminationSource::LoserOf(id(8))
            ]
        );
        assert_eq!(
            plan.nodes()[24].sources,
            [
                EliminationSource::WinnerOf(id(22)),
                EliminationSource::LoserOf(id(21))
            ]
        );
        assert_eq!(
            plan.nodes()[25].sources,
            [
                EliminationSource::WinnerOf(id(23)),
                EliminationSource::LoserOf(id(20))
            ]
        );
    }

    #[test]
    fn identical_inputs_recreate_identical_ids_waves_and_sources() {
        assert_eq!(TopologyPlan::single(5, true), TopologyPlan::single(5, true));
        assert_eq!(TopologyPlan::double(5), TopologyPlan::double(5));

        let plan = TopologyPlan::double(5);
        assert!(
            plan.nodes()
                .iter()
                .enumerate()
                .all(|(index, node)| node.id.value() == index)
        );
        assert!(plan.node(id(14)).is_some());
        assert!(plan.node(id(15)).is_none());
        assert!(plan.wave(plan.wave_count()).is_none());
    }
}
