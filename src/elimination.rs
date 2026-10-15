//! Pure single- and double-elimination bracket calculations.
//!
//! Elimination brackets own a fixed graph of logical contests. They do not
//! own games, series, colours, scoring, or tiebreak rules. A caller may use
//! any external procedure to decide a contest and then records only the
//! winning participant for its stable [`EliminationNodeId`].

mod seeding;
mod topology;

use std::collections::{HashMap, HashSet};

use crate::domain::PlayerId;

/// Stable identity of one logical contest in an elimination bracket.
///
/// Every identifier is assigned from the complete topology at construction.
/// Numeric values are native to the deterministic topology created from one
/// bracket type, seed order, and topology option set.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(transparent)
)]
pub struct EliminationNodeId(usize);

impl EliminationNodeId {
    /// Recreates a stored logical-node identifier.
    ///
    /// Match submission validates membership in a particular bracket. Direct
    /// topology lookup requires an identifier originating from that bracket.
    pub const fn new(value: usize) -> Self {
        Self(value)
    }

    /// The deterministic numeric value of this identifier.
    pub const fn value(self) -> usize {
        self.0
    }
}

/// The topology stage containing a logical elimination node.
///
/// Every round index is zero-based. Scheduling waves are deliberately
/// separate: independent winners- and losers-bracket stages can share one
/// wave.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case", deny_unknown_fields)
)]
pub enum EliminationStage {
    /// A non-final single-elimination round.
    SingleRound {
        /// Zero-based round index, starting at the first seeded round.
        round_index: usize,
    },
    /// The single-elimination championship contest.
    SingleFinal,
    /// The optional single-elimination third-place contest.
    Bronze,
    /// A winners-bracket round in double elimination.
    WinnersRound {
        /// Zero-based winners-bracket round index.
        round_index: usize,
    },
    /// A losers-bracket consolidation round.
    LosersMinor {
        /// Zero-based minor-round index.
        round_index: usize,
    },
    /// A losers-bracket round receiving winners-bracket drop-ins.
    LosersMajor {
        /// Zero-based major-round index.
        round_index: usize,
    },
    /// The first double-elimination championship contest.
    GrandFinal,
    /// The conditional double-elimination championship reset.
    Reset,
}

impl EliminationStage {
    /// Returns the round index for stages which have one.
    pub const fn round_index(self) -> Option<usize> {
        match self {
            Self::SingleRound { round_index }
            | Self::WinnersRound { round_index }
            | Self::LosersMinor { round_index }
            | Self::LosersMajor { round_index } => Some(round_index),
            Self::SingleFinal | Self::Bronze | Self::GrandFinal | Self::Reset => None,
        }
    }

    /// Replaces the round index for stages which have one.
    pub const fn with_round_index(self, round_index: usize) -> Option<Self> {
        match self {
            Self::SingleRound { .. } => Some(Self::SingleRound { round_index }),
            Self::WinnersRound { .. } => Some(Self::WinnersRound { round_index }),
            Self::LosersMinor { .. } => Some(Self::LosersMinor { round_index }),
            Self::LosersMajor { .. } => Some(Self::LosersMajor { round_index }),
            Self::SingleFinal | Self::Bronze | Self::GrandFinal | Self::Reset => None,
        }
    }

    /// Zero-based ordinal across consecutive lower-bracket rounds.
    ///
    /// Minor and major rounds with the same native round index are consecutive,
    /// not aliases: minor `k` is `2k`, and major `k` is `2k + 1`.
    pub fn lower_round_ordinal(self) -> Option<u32> {
        let (round_index, offset) = match self {
            Self::LosersMinor { round_index } => (round_index, 0),
            Self::LosersMajor { round_index } => (round_index, 1),
            Self::SingleRound { .. }
            | Self::SingleFinal
            | Self::Bronze
            | Self::WinnersRound { .. }
            | Self::GrandFinal
            | Self::Reset => return None,
        };
        u32::try_from(round_index)
            .ok()?
            .checked_mul(2)?
            .checked_add(offset)
    }
}

/// Supported immutable bracket topology.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case", deny_unknown_fields)
)]
pub enum EliminationKind {
    /// Single elimination, optionally with a bronze contest.
    Single {
        /// Whether semifinal losers play for third place.
        bronze: bool,
    },
    /// Double elimination with a conditional reset final.
    Double,
}

/// Policy controlling when causally available elimination nodes materialize.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case", deny_unknown_fields)
)]
pub enum EliminationReleaseCadence {
    /// Materialize a complete scheduling wave only after the prior wave ends.
    Wave,
    /// Materialize each node as soon as all of its own sources have resolved.
    DependencyReady,
}

impl EliminationKind {
    /// Whether this topology can contain the supplied native stage.
    ///
    /// A bronze stage additionally requires `bronze: true`; round ordinals
    /// are validated when a concrete topology is constructed.
    pub const fn supports_stage(self, stage: EliminationStage) -> bool {
        matches!(
            (self, stage),
            (
                Self::Single { .. },
                EliminationStage::SingleRound { .. } | EliminationStage::SingleFinal,
            ) | (Self::Single { bronze: true }, EliminationStage::Bronze)
                | (
                    Self::Double,
                    EliminationStage::WinnersRound { .. }
                        | EliminationStage::LosersMinor { .. }
                        | EliminationStage::LosersMajor { .. }
                        | EliminationStage::GrandFinal
                        | EliminationStage::Reset,
                )
        )
    }
}

/// Immutable source of one logical node slot.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case", deny_unknown_fields)
)]
pub enum EliminationSource {
    /// A participant from the immutable constructor order.
    InitialSeed {
        /// Zero-based index in the slice supplied to the constructor.
        seed_index: usize,
    },
    /// The winner produced by an earlier logical node.
    WinnerOf(EliminationNodeId),
    /// The loser produced by an earlier logical node.
    LoserOf(EliminationNodeId),
    /// A fixed empty seed position.
    Vacant,
}

/// How a materialized logical elimination node resolved.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case", deny_unknown_fields)
)]
pub enum EliminationNodeResolution {
    /// The caller supplied the winner of a two-entrant contest.
    MatchDecided {
        /// Contest winner.
        winner: PlayerId,
        /// The other entrant, derived by the bracket.
        loser: PlayerId,
    },
    /// One active entrant withdrew and the other won without a decision.
    WithdrawalWalkover {
        /// Entrant advancing by walkover.
        winner: PlayerId,
        /// Entrant whose withdrawal caused the walkover.
        withdrawn: PlayerId,
    },
    /// A participant withdrew from a materialized slot with no eligible
    /// opponent, leaving the node vacant while retaining the withdrawal fact.
    WithdrawalVacancy {
        /// Participant removed from this branch.
        withdrawn: PlayerId,
    },
    /// Both routed participants withdrew before the node materialized.
    MutualWithdrawalVacancy {
        /// The two withdrawn participants, aligned with the node sources.
        withdrawn: [PlayerId; 2],
    },
    /// Exactly one eligible participant reached this materialized node.
    AutomaticAdvance {
        /// Participant advancing through the node.
        player: PlayerId,
    },
    /// No eligible participant reached this materialized node.
    Vacant,
}

/// Immutable topology for one logical elimination contest.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(deny_unknown_fields)
)]
pub struct EliminationNodeDescriptor {
    /// Stable logical identity.
    pub id: EliminationNodeId,
    /// Zero-based nominal wave used by wave scheduling and presentation.
    pub scheduled_wave_index: usize,
    /// Typed topology stage.
    pub stage: EliminationStage,
    /// Zero-based ordinal within this exact stage.
    pub stage_ordinal: usize,
    /// Immutable predecessor or seed sources.
    pub sources: [EliminationSource; 2],
}

/// Complete immutable elimination definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EliminationDefinition {
    kind: EliminationKind,
    seeds: Vec<PlayerId>,
    nodes: Vec<EliminationNodeDescriptor>,
    conditional: Vec<bool>,
}

impl EliminationDefinition {
    /// Bracket kind used to construct this definition.
    pub const fn kind(&self) -> EliminationKind {
        self.kind
    }

    /// Immutable constructor order used to fill the standard seed positions.
    pub fn seeds(&self) -> &[PlayerId] {
        &self.seeds
    }

    /// Complete node topology in deterministic identity order.
    pub fn nodes(&self) -> &[EliminationNodeDescriptor] {
        &self.nodes
    }
}

/// Host-owned state of one elimination node.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case", deny_unknown_fields)
)]
pub enum EliminationNodeFactState {
    /// The node has not materialized.
    Planned,
    /// A materialized contest awaits its winner.
    Active {
        /// Entrants fixed at materialization.
        entrants: [PlayerId; 2],
    },
    /// The materialized node has resolved.
    Resolved {
        /// Entrants fixed when the node materialized; vacancies are retained.
        entrants: [Option<PlayerId>; 2],
        /// Terminal logical resolution.
        resolution: EliminationNodeResolution,
    },
    /// A conditional node was unnecessary.
    Skipped,
}

/// One authoritative node fact.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(deny_unknown_fields)
)]
pub struct EliminationNodeFact {
    /// Stable topology node.
    pub node: EliminationNodeId,
    /// Current state.
    pub state: EliminationNodeFactState,
}

/// Replacement for one node fact returned by a pure operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(deny_unknown_fields)
)]
pub struct EliminationNodePatch {
    /// Stable topology node.
    pub node: EliminationNodeId,
    /// Entire replacement state.
    pub state: EliminationNodeFactState,
}

/// A wave of node replacements which the host persists atomically.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(deny_unknown_fields)
)]
pub struct EliminationWavePatch {
    /// Zero-based topology wave.
    pub wave_index: usize,
    /// Every node in this wave, including automatic and vacant resolutions.
    pub patches: Vec<EliminationNodePatch>,
}

/// One deterministic atomic batch of causally materialized node replacements.
///
/// Unlike [`EliminationWavePatch`], a release can contain nodes from multiple
/// scheduling waves when automatic resolutions unlock later nodes.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(deny_unknown_fields)
)]
pub struct EliminationReleasePatch {
    /// Node replacements in deterministic topology order.
    pub patches: Vec<EliminationNodePatch>,
}

/// Pure bracket projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EliminationProjection {
    /// Topology nodes, host facts, and side-specific possible entrants.
    pub nodes: Vec<EliminationNodeProjection>,
    /// Whether every topology node is resolved or skipped.
    pub complete: bool,
    /// Participation and terminal result in immutable constructor order.
    pub players: Vec<EliminationPlayerProjection>,
    /// Whether the resolved grand final requires the conditional reset.
    pub reset_required: bool,
}

/// One participant's native elimination state and terminal result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EliminationPlayerProjection {
    /// Participant owning this result.
    pub player: PlayerId,
    /// Whether this participant still has a path through an unresolved node.
    pub in_contention: bool,
    /// Final competition place once the participant's complete path is terminal.
    pub placement: Option<u32>,
    /// Native stage whose resolution fixed the terminal placement.
    pub exit_stage: Option<EliminationStage>,
}

/// One elimination node with engine-owned entrant provenance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EliminationNodeProjection {
    /// Immutable topology descriptor.
    pub descriptor: EliminationNodeDescriptor,
    /// Current host-owned node fact.
    pub state: EliminationNodeFactState,
    /// Players who can still arrive from each corresponding source.
    pub possible_entrants: [Vec<PlayerId>; 2],
}

/// Invalid elimination facts or operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum EliminationError {
    /// Topology construction failed.
    #[error(transparent)]
    Setup(#[from] BracketSetupError),
    /// Facts must contain exactly one correctly identified state per node.
    #[error("elimination node facts do not match the topology")]
    IncoherentFacts,
    /// An active node still blocks the next wave.
    #[error("an elimination wave is already in progress")]
    WaveInProgress,
    /// Every topology node is resolved or skipped.
    #[error("the elimination bracket is complete")]
    Complete,
    /// The bracket is unfinished, but no node can currently be released.
    #[error("no elimination nodes are dependency-ready")]
    DependenciesPending,
    /// The requested node is outside the topology.
    #[error("the elimination node is unknown")]
    UnknownNode,
    /// The requested node is not an active contest.
    #[error("the elimination node is not an active contest")]
    NotActive,
    /// The supplied winner is not an entrant.
    #[error("the supplied winner is not an entrant of the elimination node")]
    WinnerNotEntrant,
    /// The participant is not seeded in this bracket.
    #[error("the participant is not seeded in this elimination bracket")]
    UnknownParticipant,
}

/// Builds an immutable topology and its stable node descriptors.
///
/// `seeds` fills the bracket's standard seed positions. Supply rating order for
/// ordinary seeding, or a player permutation for custom placement. Bye positions
/// and source connections are fixed by the field size and `kind`; permuting
/// players changes who receives a bye without moving the bye itself. Hosts may
/// retain a separate rating order for seed labels and standings presentation.
pub fn topology(
    kind: EliminationKind,
    seeds: &[PlayerId],
) -> Result<EliminationDefinition, BracketSetupError> {
    validate_seeds(seeds)?;
    let plan = match kind {
        EliminationKind::Single { bronze } => topology::TopologyPlan::single(seeds.len(), bronze),
        EliminationKind::Double => topology::TopologyPlan::double(seeds.len()),
    };
    Ok(EliminationDefinition {
        kind,
        seeds: seeds.to_vec(),
        nodes: plan.nodes().iter().map(|node| node.descriptor).collect(),
        conditional: plan.nodes().iter().map(|node| node.conditional).collect(),
    })
}

/// Returns the canonical initial planned facts for a definition.
pub fn initial_facts(definition: &EliminationDefinition) -> Vec<EliminationNodeFact> {
    definition
        .nodes
        .iter()
        .map(|descriptor| EliminationNodeFact {
            node: descriptor.id,
            state: EliminationNodeFactState::Planned,
        })
        .collect()
}

/// Materializes the next complete scheduling wave as pure node replacements.
///
/// This convenience function knows only withdrawals already recorded in node
/// facts. Hosts with durable withdrawal intent must use [`next_release`] with
/// [`EliminationReleaseCadence::Wave`] and the complete withdrawn set, so a
/// withdrawn entrant is never granted a new automatic advance.
pub fn next_wave(
    definition: &EliminationDefinition,
    facts: &[EliminationNodeFact],
) -> Result<EliminationWavePatch, EliminationError> {
    validate_facts(definition, facts)?;
    validate_wave_progression(definition, facts)?;
    if facts
        .iter()
        .any(|fact| matches!(fact.state, EliminationNodeFactState::Active { .. }))
    {
        return Err(EliminationError::WaveInProgress);
    }
    let wave = definition
        .nodes
        .iter()
        .zip(facts)
        .filter(|(_, fact)| matches!(fact.state, EliminationNodeFactState::Planned))
        .map(|(node, _)| node.scheduled_wave_index)
        .min()
        .ok_or(EliminationError::Complete)?;
    let withdrawn = withdrawn_players(facts);
    let patches = release_nodes(
        definition,
        facts,
        EliminationReleaseCadence::Wave,
        &withdrawn,
    )?;
    Ok(EliminationWavePatch {
        wave_index: wave,
        patches: patches.patches,
    })
}

/// Materializes the next atomic batch allowed by `cadence`.
///
/// `withdrawn` is the host's complete durable withdrawn-player set and is
/// applied to nodes materialized by this call. It does not rewrite an already
/// materialized node: when a withdrawal becomes durable, first apply
/// [`withdraw_players`] to the current facts, then call this function with the
/// resulting facts and complete set. Wave cadence enforces a complete-wave
/// barrier, while dependency-ready cadence permits unrelated active contests
/// and follows automatic, vacant, and conditional-reset resolutions to a
/// fixed point. [`EliminationError::Complete`] means every node is terminal;
/// [`EliminationError::DependenciesPending`] means the bracket is unfinished
/// but no node can currently be released.
pub fn next_release(
    definition: &EliminationDefinition,
    facts: &[EliminationNodeFact],
    cadence: EliminationReleaseCadence,
    withdrawn: &[PlayerId],
) -> Result<EliminationReleasePatch, EliminationError> {
    validate_facts(definition, facts)?;
    validate_withdrawn(definition, withdrawn)?;
    if cadence == EliminationReleaseCadence::Wave {
        validate_wave_progression(definition, facts)?;
        if facts
            .iter()
            .any(|fact| matches!(fact.state, EliminationNodeFactState::Active { .. }))
        {
            return Err(EliminationError::WaveInProgress);
        }
    }
    let withdrawn = withdrawn.iter().copied().collect();
    release_nodes(definition, facts, cadence, &withdrawn)
}

fn release_nodes(
    definition: &EliminationDefinition,
    facts: &[EliminationNodeFact],
    cadence: EliminationReleaseCadence,
    durable_withdrawn: &HashSet<PlayerId>,
) -> Result<EliminationReleasePatch, EliminationError> {
    let selected_wave = (cadence == EliminationReleaseCadence::Wave)
        .then(|| {
            definition
                .nodes
                .iter()
                .zip(facts)
                .filter(|(_, fact)| matches!(fact.state, EliminationNodeFactState::Planned))
                .map(|(node, _)| node.scheduled_wave_index)
                .min()
        })
        .flatten();
    let mut working = facts.to_vec();
    let mut known_withdrawn = withdrawn_players(facts);
    let complete_withdrawn = durable_withdrawn
        .union(&known_withdrawn)
        .copied()
        .collect::<HashSet<_>>();
    let mut patches = Vec::new();

    for descriptor in &definition.nodes {
        if !matches!(
            working[descriptor.id.value()].state,
            EliminationNodeFactState::Planned
        ) || selected_wave.is_some_and(|wave| descriptor.scheduled_wave_index != wave)
            || !descriptor
                .sources
                .iter()
                .all(|source| source_ready(&working, *source))
        {
            continue;
        }
        let state = materialize_node(
            definition,
            &working,
            *descriptor,
            &known_withdrawn,
            &complete_withdrawn,
        );
        working[descriptor.id.value()].state = state;
        remember_withdrawals(state, &mut known_withdrawn);
        patches.push(EliminationNodePatch {
            node: descriptor.id,
            state,
        });
    }

    if patches.is_empty() {
        if facts.iter().all(|fact| {
            matches!(
                fact.state,
                EliminationNodeFactState::Resolved { .. } | EliminationNodeFactState::Skipped
            )
        }) {
            Err(EliminationError::Complete)
        } else {
            Err(EliminationError::DependenciesPending)
        }
    } else {
        Ok(EliminationReleasePatch { patches })
    }
}

fn materialize_node(
    definition: &EliminationDefinition,
    facts: &[EliminationNodeFact],
    descriptor: EliminationNodeDescriptor,
    known_withdrawn: &HashSet<PlayerId>,
    complete_withdrawn: &HashSet<PlayerId>,
) -> EliminationNodeFactState {
    if definition.conditional[descriptor.id.value()]
        && descriptor.stage == EliminationStage::Reset
        && !reset_required(definition, facts)
    {
        return EliminationNodeFactState::Skipped;
    }
    let source_entrants = descriptor
        .sources
        .map(|source| source_player(definition, facts, source));
    let newly_withdrawn = source_entrants
        .into_iter()
        .flatten()
        .any(|player| complete_withdrawn.contains(&player) && !known_withdrawn.contains(&player));
    if newly_withdrawn {
        return withdrawal_state(source_entrants, known_withdrawn, complete_withdrawn);
    }
    let entrants =
        source_entrants.map(|entrant| entrant.filter(|player| !known_withdrawn.contains(player)));
    match entrants {
        [Some(first), Some(second)] => EliminationNodeFactState::Active {
            entrants: [first, second],
        },
        [Some(player), None] | [None, Some(player)] => EliminationNodeFactState::Resolved {
            entrants,
            resolution: EliminationNodeResolution::AutomaticAdvance { player },
        },
        [None, None] => EliminationNodeFactState::Resolved {
            entrants,
            resolution: EliminationNodeResolution::Vacant,
        },
    }
}

fn withdrawal_state(
    entrants: [Option<PlayerId>; 2],
    known_withdrawn: &HashSet<PlayerId>,
    complete_withdrawn: &HashSet<PlayerId>,
) -> EliminationNodeFactState {
    let removed: Vec<_> = entrants
        .into_iter()
        .flatten()
        .filter(|entrant| {
            complete_withdrawn.contains(entrant) && !known_withdrawn.contains(entrant)
        })
        .collect();
    let survivors: Vec<_> = entrants
        .into_iter()
        .flatten()
        .filter(|entrant| !complete_withdrawn.contains(entrant))
        .collect();
    let resolution = match (survivors.as_slice(), removed.as_slice()) {
        ([winner], [withdrawn]) => EliminationNodeResolution::WithdrawalWalkover {
            winner: *winner,
            withdrawn: *withdrawn,
        },
        ([], [withdrawn]) => EliminationNodeResolution::WithdrawalVacancy {
            withdrawn: *withdrawn,
        },
        ([], [first, second]) => EliminationNodeResolution::MutualWithdrawalVacancy {
            withdrawn: [*first, *second],
        },
        _ => unreachable!("a newly withdrawn elimination entrant must be removed"),
    };
    EliminationNodeFactState::Resolved {
        entrants,
        resolution,
    }
}

fn remember_withdrawals(state: EliminationNodeFactState, withdrawn: &mut HashSet<PlayerId>) {
    if let EliminationNodeFactState::Resolved { resolution, .. } = state {
        match resolution {
            EliminationNodeResolution::WithdrawalWalkover {
                withdrawn: player, ..
            }
            | EliminationNodeResolution::WithdrawalVacancy { withdrawn: player } => {
                withdrawn.insert(player);
            }
            EliminationNodeResolution::MutualWithdrawalVacancy { withdrawn: players } => {
                withdrawn.extend(players);
            }
            EliminationNodeResolution::MatchDecided { .. }
            | EliminationNodeResolution::AutomaticAdvance { .. }
            | EliminationNodeResolution::Vacant => {}
        }
    }
}

/// Resolves one active contest and returns its node replacement.
pub fn record_winner(
    definition: &EliminationDefinition,
    facts: &[EliminationNodeFact],
    node: EliminationNodeId,
    winner: PlayerId,
) -> Result<EliminationNodePatch, EliminationError> {
    validate_facts(definition, facts)?;
    let Some(fact) = facts.get(node.value()).filter(|fact| fact.node == node) else {
        return Err(EliminationError::UnknownNode);
    };
    let EliminationNodeFactState::Active { entrants } = fact.state else {
        return Err(EliminationError::NotActive);
    };
    let loser = if entrants[0] == winner {
        entrants[1]
    } else if entrants[1] == winner {
        entrants[0]
    } else {
        return Err(EliminationError::WinnerNotEntrant);
    };
    Ok(EliminationNodePatch {
        node,
        state: EliminationNodeFactState::Resolved {
            entrants: entrants.map(Some),
            resolution: EliminationNodeResolution::MatchDecided { winner, loser },
        },
    })
}

/// Applies a withdrawal to currently active affected nodes.
///
/// Hosts persist withdrawal intent, apply these patches, then call
/// [`next_release`] with the complete durable withdrawn set.
pub fn withdraw(
    definition: &EliminationDefinition,
    facts: &[EliminationNodeFact],
    player: PlayerId,
) -> Result<Vec<EliminationNodePatch>, EliminationError> {
    withdraw_players(definition, facts, &[player])
}

/// Applies the host's complete withdrawn set to active nodes atomically.
///
/// Persist these patches before calling [`next_release`] with the resulting
/// facts and complete durable withdrawn set. That release propagates the
/// withdrawal to newly available matches in either cadence.
///
/// Resolved contests, including already-awarded automatic advances, remain
/// unchanged. A player withdrawing after a bye exits at their next match;
/// an empty patch list does not replace the host's durable withdrawal intent.
pub fn withdraw_players(
    definition: &EliminationDefinition,
    facts: &[EliminationNodeFact],
    players: &[PlayerId],
) -> Result<Vec<EliminationNodePatch>, EliminationError> {
    validate_facts(definition, facts)?;
    validate_withdrawn(definition, players)?;
    let withdrawn: HashSet<_> = players.iter().copied().collect();
    let known_withdrawn = withdrawn_players(facts);
    let complete_withdrawn: HashSet<_> = withdrawn.union(&known_withdrawn).copied().collect();
    let newly_withdrawn: HashSet<_> = withdrawn.difference(&known_withdrawn).copied().collect();
    let mut patches = Vec::new();
    for fact in facts {
        let replacement = match fact.state {
            EliminationNodeFactState::Active { entrants }
                if entrants
                    .iter()
                    .any(|entrant| newly_withdrawn.contains(entrant)) =>
            {
                Some(withdrawal_state(
                    entrants.map(Some),
                    &known_withdrawn,
                    &complete_withdrawn,
                ))
            }
            _ => None,
        };
        if let Some(state) = replacement {
            patches.push(EliminationNodePatch {
                node: fact.node,
                state,
            });
        }
    }
    Ok(patches)
}

/// Projects the current bracket and native participant results.
pub fn project(
    definition: &EliminationDefinition,
    facts: &[EliminationNodeFact],
    withdrawn: &[PlayerId],
) -> Result<EliminationProjection, EliminationError> {
    validate_facts(definition, facts)?;
    validate_withdrawn(definition, withdrawn)?;
    let terminal = match definition.kind {
        EliminationKind::Single { .. } => definition
            .nodes
            .iter()
            .find(|node| node.stage == EliminationStage::SingleFinal),
        EliminationKind::Double => {
            let terminal_stage = if reset_required(definition, facts) {
                EliminationStage::Reset
            } else {
                EliminationStage::GrandFinal
            };
            definition
                .nodes
                .iter()
                .find(|node| node.stage == terminal_stage)
        }
    };
    let (champion, runner_up) = terminal
        .map(|node| championship(facts[node.id.value()].state))
        .unwrap_or((None, None));
    let mut terminal_results = HashMap::new();
    let terminal_stage = terminal.map(|node| node.stage);
    for (descriptor, fact) in definition.nodes.iter().zip(facts) {
        let eliminated_players = match fact.state {
            EliminationNodeFactState::Resolved { resolution, .. } => match resolution {
                EliminationNodeResolution::MatchDecided { loser, .. } => match definition.kind {
                    EliminationKind::Single { bronze } => {
                        let feeds_bronze = bronze
                            && definition.nodes.iter().any(|node| {
                                node.stage == EliminationStage::Bronze
                                    && node
                                        .sources
                                        .contains(&EliminationSource::LoserOf(descriptor.id))
                            });
                        (!feeds_bronze && descriptor.stage != EliminationStage::SingleFinal)
                            .then_some(loser)
                            .or_else(|| {
                                (descriptor.stage == EliminationStage::Bronze).then_some(loser)
                            })
                    }
                    EliminationKind::Double => matches!(
                        descriptor.stage,
                        EliminationStage::LosersMinor { .. }
                            | EliminationStage::LosersMajor { .. }
                            | EliminationStage::Reset
                    )
                    .then_some(loser),
                },
                EliminationNodeResolution::WithdrawalWalkover { withdrawn, .. }
                | EliminationNodeResolution::WithdrawalVacancy { withdrawn } => Some(withdrawn),
                EliminationNodeResolution::MutualWithdrawalVacancy { withdrawn } => {
                    for player in withdrawn {
                        terminal_results.entry(player).or_insert_with(|| {
                            (nominal_placement(definition, descriptor), descriptor.stage)
                        });
                    }
                    None
                }
                EliminationNodeResolution::AutomaticAdvance { .. }
                | EliminationNodeResolution::Vacant => None,
            },
            _ => None,
        };
        if let Some(player) = eliminated_players {
            terminal_results
                .entry(player)
                .or_insert_with(|| (nominal_placement(definition, descriptor), descriptor.stage));
        }
    }
    let mut possible_entrants = possible_entrants(definition, facts);
    for player in withdrawn {
        if terminal_results.contains_key(player) {
            continue;
        }
        let pending_stage = definition
            .nodes
            .iter()
            .zip(facts)
            .zip(&possible_entrants)
            .find_map(|((descriptor, fact), possible)| {
                (!matches!(
                    fact.state,
                    EliminationNodeFactState::Resolved { .. } | EliminationNodeFactState::Skipped
                ) && possible
                    .iter()
                    .flatten()
                    .any(|candidate| candidate == player))
                .then_some(descriptor)
            });
        if let Some(descriptor) = pending_stage {
            terminal_results.insert(
                *player,
                (nominal_placement(definition, descriptor), descriptor.stage),
            );
        }
    }
    let durable_withdrawn: HashSet<_> = withdrawn.iter().copied().collect();
    for side in possible_entrants
        .iter_mut()
        .flat_map(|sides| sides.iter_mut())
    {
        side.retain(|player| !durable_withdrawn.contains(player));
    }
    if let Some(champion) = champion {
        terminal_results
            .entry(champion)
            .or_insert((1, terminal_stage.expect("a champion has a terminal stage")));
    }
    if let Some(runner_up) = runner_up {
        terminal_results
            .entry(runner_up)
            .or_insert((2, terminal_stage.expect("a runner-up has a terminal stage")));
    }
    if let EliminationKind::Single { bronze: true } = definition.kind
        && let Some(bronze) = definition
            .nodes
            .iter()
            .find(|node| node.stage == EliminationStage::Bronze)
    {
        let (winner, _) = resolution_players(facts[bronze.id.value()].state);
        if let Some(winner) = winner {
            terminal_results
                .entry(winner)
                .or_insert((3, EliminationStage::Bronze));
        }
    }
    let complete = facts.iter().all(|fact| {
        matches!(
            fact.state,
            EliminationNodeFactState::Resolved { .. } | EliminationNodeFactState::Skipped
        )
    });
    let players = definition
        .seeds
        .iter()
        .copied()
        .map(|player| {
            let result = terminal_results.get(&player).copied();
            EliminationPlayerProjection {
                player,
                in_contention: result.is_none(),
                placement: result.map(|(placement, _)| placement),
                exit_stage: result.map(|(_, exit_stage)| exit_stage),
            }
        })
        .collect();
    Ok(EliminationProjection {
        nodes: definition
            .nodes
            .iter()
            .copied()
            .zip(facts.iter().map(|fact| fact.state))
            .zip(possible_entrants)
            .map(
                |((descriptor, state), possible_entrants)| EliminationNodeProjection {
                    descriptor,
                    state,
                    possible_entrants,
                },
            )
            .collect(),
        complete,
        players,
        reset_required: reset_required(definition, facts),
    })
}

fn nominal_placement(
    definition: &EliminationDefinition,
    descriptor: &EliminationNodeDescriptor,
) -> u32 {
    let placement_stage = definition
        .nodes
        .iter()
        .find(|node| {
            node.sources
                .contains(&EliminationSource::LoserOf(descriptor.id))
        })
        .map_or(descriptor.stage, |node| node.stage);
    let bracket_size = seeding::bracket_size_for(definition.seeds.len());
    let placement = match placement_stage {
        EliminationStage::SingleRound { round_index } => (bracket_size
            >> round_index
                .checked_add(1)
                .expect("a topology round index is representable"))
        .checked_add(1)
        .expect("a single-elimination placement is representable"),
        EliminationStage::SingleFinal | EliminationStage::GrandFinal | EliminationStage::Reset => 2,
        EliminationStage::Bronze => 4,
        EliminationStage::LosersMinor { round_index } => (bracket_size
            >> round_index
                .checked_add(2)
                .expect("a topology round index is representable"))
        .checked_mul(3)
        .and_then(|place| place.checked_add(1))
        .expect("a lower-minor placement is representable"),
        EliminationStage::LosersMajor { round_index } => (bracket_size
            >> round_index
                .checked_add(1)
                .expect("a topology round index is representable"))
        .checked_add(1)
        .expect("a lower-major placement is representable"),
        EliminationStage::WinnersRound { .. } => definition.seeds.len(),
    };
    u32::try_from(placement)
        .expect("a constructed elimination field has fixed-width placements")
        .min(
            u32::try_from(definition.seeds.len())
                .expect("a constructed elimination field has fixed-width placements"),
        )
}

fn possible_entrants(
    definition: &EliminationDefinition,
    facts: &[EliminationNodeFact],
) -> Vec<[Vec<PlayerId>; 2]> {
    let mut projected: Vec<[Vec<PlayerId>; 2]> = Vec::with_capacity(definition.nodes.len());
    for descriptor in &definition.nodes {
        let withdrawn = withdrawn_before_node(definition, facts, descriptor.id);
        let sides = descriptor.sources.map(|source| {
            let planned_loser_sides = match source {
                EliminationSource::LoserOf(node)
                    if facts[node.value()].state == EliminationNodeFactState::Planned =>
                {
                    Some(&projected[node.value()])
                }
                _ => None,
            };
            let mut players = match source {
                EliminationSource::InitialSeed { seed_index } => definition
                    .seeds
                    .get(seed_index)
                    .copied()
                    .into_iter()
                    .collect(),
                EliminationSource::Vacant => Vec::new(),
                EliminationSource::WinnerOf(node) | EliminationSource::LoserOf(node) => {
                    match facts[node.value()].state {
                        EliminationNodeFactState::Resolved { .. } => {
                            let (winner, loser) = resolution_players(facts[node.value()].state);
                            match source {
                                EliminationSource::WinnerOf(_) => winner,
                                EliminationSource::LoserOf(_) => loser,
                                EliminationSource::InitialSeed { .. }
                                | EliminationSource::Vacant => unreachable!(),
                            }
                            .into_iter()
                            .collect()
                        }
                        EliminationNodeFactState::Active { entrants } => entrants.to_vec(),
                        EliminationNodeFactState::Planned => {
                            projected[node.value()].iter().flatten().copied().collect()
                        }
                        EliminationNodeFactState::Skipped => Vec::new(),
                    }
                }
            };
            players.retain(|player| !withdrawn.contains(player));
            players.sort_by_key(|player| {
                definition
                    .seeds
                    .iter()
                    .position(|seed| seed == player)
                    .expect("validated elimination entrant belongs to the seed field")
            });
            players.dedup();
            let loser_is_impossible = planned_loser_sides.is_some_and(|source_sides| {
                players.len() < 2
                    || source_sides
                        .iter()
                        .any(|side| side.iter().all(|player| withdrawn.contains(player)))
            });
            if loser_is_impossible {
                players.clear();
            }
            players
        });
        projected.push(sides);
    }
    projected
}

fn validate_facts(
    definition: &EliminationDefinition,
    facts: &[EliminationNodeFact],
) -> Result<(), EliminationError> {
    if facts.len() != definition.nodes.len()
        || facts
            .iter()
            .enumerate()
            .any(|(index, fact)| fact.node.value() != index)
    {
        return Err(EliminationError::IncoherentFacts);
    }
    let seeds: HashSet<_> = definition.seeds.iter().copied().collect();
    if seeds.len() != definition.seeds.len() {
        return Err(EliminationError::IncoherentFacts);
    }
    for (descriptor, fact) in definition.nodes.iter().zip(facts) {
        let sources_ready = descriptor
            .sources
            .iter()
            .all(|source| source_ready(facts, *source));
        let source_entrants = descriptor
            .sources
            .map(|source| source_player(definition, facts, source));
        let causal_withdrawn = withdrawn_before_node(definition, facts, descriptor.id);
        let eligible = source_entrants
            .map(|entrant| entrant.filter(|player| !causal_withdrawn.contains(player)));
        let players_valid = |entrants: [Option<PlayerId>; 2]| {
            entrants
                .into_iter()
                .flatten()
                .all(|player| seeds.contains(&player))
        };
        if !players_valid(source_entrants) {
            return Err(EliminationError::IncoherentFacts);
        }
        let valid = match fact.state {
            EliminationNodeFactState::Planned => true,
            EliminationNodeFactState::Active { entrants } => {
                sources_ready
                    && eligible == entrants.map(Some)
                    && entrants[0] != entrants[1]
                    && !(definition.conditional[descriptor.id.value()]
                        && descriptor.stage == EliminationStage::Reset
                        && !reset_required(definition, facts))
            }
            EliminationNodeFactState::Resolved {
                entrants,
                resolution,
            } => {
                if !sources_ready || !players_valid(entrants) {
                    false
                } else {
                    match resolution {
                        EliminationNodeResolution::MatchDecided { winner, loser } => {
                            entrants == source_entrants
                                && eligible == entrants
                                && winner != loser
                                && entrants.contains(&Some(winner))
                                && entrants.contains(&Some(loser))
                        }
                        EliminationNodeResolution::WithdrawalWalkover { winner, withdrawn } => {
                            let mut withdrawn_at_node = causal_withdrawn.clone();
                            withdrawn_at_node.insert(withdrawn);
                            let eligible = source_entrants.map(|entrant| {
                                entrant.filter(|player| !withdrawn_at_node.contains(player))
                            });
                            entrants == source_entrants
                                && winner != withdrawn
                                && !causal_withdrawn.contains(&withdrawn)
                                && entrants.contains(&Some(winner))
                                && entrants.contains(&Some(withdrawn))
                                && eligible.into_iter().flatten().eq([winner])
                        }
                        EliminationNodeResolution::WithdrawalVacancy { withdrawn } => {
                            let mut withdrawn_at_node = causal_withdrawn.clone();
                            withdrawn_at_node.insert(withdrawn);
                            let eligible = source_entrants.map(|entrant| {
                                entrant.filter(|player| !withdrawn_at_node.contains(player))
                            });
                            entrants == source_entrants
                                && !causal_withdrawn.contains(&withdrawn)
                                && entrants.contains(&Some(withdrawn))
                                && eligible.into_iter().flatten().next().is_none()
                        }
                        EliminationNodeResolution::MutualWithdrawalVacancy { withdrawn } => {
                            let mut withdrawn_at_node = causal_withdrawn.clone();
                            withdrawn_at_node.extend(withdrawn);
                            let eligible = source_entrants.map(|entrant| {
                                entrant.filter(|player| !withdrawn_at_node.contains(player))
                            });
                            entrants == source_entrants
                                && withdrawn[0] != withdrawn[1]
                                && withdrawn
                                    .into_iter()
                                    .all(|player| !causal_withdrawn.contains(&player))
                                && withdrawn
                                    .into_iter()
                                    .all(|player| entrants.contains(&Some(player)))
                                && eligible.into_iter().flatten().next().is_none()
                        }
                        EliminationNodeResolution::AutomaticAdvance { player } => {
                            entrants == eligible && eligible.into_iter().flatten().eq([player])
                        }
                        EliminationNodeResolution::Vacant => {
                            entrants == eligible && eligible.into_iter().flatten().next().is_none()
                        }
                    }
                }
            }
            EliminationNodeFactState::Skipped => {
                definition.conditional[descriptor.id.value()]
                    && descriptor.stage == EliminationStage::Reset
                    && sources_ready
                    && !reset_required(definition, facts)
            }
        };
        if !valid {
            return Err(EliminationError::IncoherentFacts);
        }
    }
    Ok(())
}

fn validate_wave_progression(
    definition: &EliminationDefinition,
    facts: &[EliminationNodeFact],
) -> Result<(), EliminationError> {
    let frontier = definition
        .nodes
        .iter()
        .zip(facts)
        .filter_map(|(node, fact)| {
            (!matches!(
                fact.state,
                EliminationNodeFactState::Resolved { .. } | EliminationNodeFactState::Skipped
            ))
            .then_some(node.scheduled_wave_index)
        })
        .min();
    let materialized_beyond_frontier = definition.nodes.iter().zip(facts).any(|(node, fact)| {
        frontier.is_some_and(|wave| node.scheduled_wave_index > wave)
            && !matches!(fact.state, EliminationNodeFactState::Planned)
    });
    if materialized_beyond_frontier {
        Err(EliminationError::IncoherentFacts)
    } else {
        Ok(())
    }
}

fn validate_withdrawn(
    definition: &EliminationDefinition,
    players: &[PlayerId],
) -> Result<(), EliminationError> {
    let withdrawn: HashSet<_> = players.iter().copied().collect();
    if withdrawn.len() != players.len()
        || !withdrawn.is_subset(&definition.seeds.iter().copied().collect())
    {
        Err(EliminationError::UnknownParticipant)
    } else {
        Ok(())
    }
}

fn source_ready(facts: &[EliminationNodeFact], source: EliminationSource) -> bool {
    match source {
        EliminationSource::InitialSeed { .. } | EliminationSource::Vacant => true,
        EliminationSource::WinnerOf(node) | EliminationSource::LoserOf(node) => matches!(
            facts.get(node.value()).map(|fact| fact.state),
            Some(EliminationNodeFactState::Resolved { .. } | EliminationNodeFactState::Skipped)
        ),
    }
}

fn withdrawn_before_node(
    definition: &EliminationDefinition,
    facts: &[EliminationNodeFact],
    node: EliminationNodeId,
) -> HashSet<PlayerId> {
    let mut ancestors = HashSet::new();
    let mut pending = definition.nodes[node.value()]
        .sources
        .into_iter()
        .filter_map(source_node)
        .collect::<Vec<_>>();
    while let Some(ancestor) = pending.pop() {
        if ancestors.insert(ancestor) {
            pending.extend(
                definition.nodes[ancestor.value()]
                    .sources
                    .into_iter()
                    .filter_map(source_node),
            );
        }
    }
    let mut withdrawn = HashSet::new();
    for ancestor in ancestors {
        remember_withdrawals(facts[ancestor.value()].state, &mut withdrawn);
    }
    withdrawn
}

fn source_node(source: EliminationSource) -> Option<EliminationNodeId> {
    match source {
        EliminationSource::WinnerOf(node) | EliminationSource::LoserOf(node) => Some(node),
        EliminationSource::InitialSeed { .. } | EliminationSource::Vacant => None,
    }
}

fn source_player(
    definition: &EliminationDefinition,
    facts: &[EliminationNodeFact],
    source: EliminationSource,
) -> Option<PlayerId> {
    match source {
        EliminationSource::InitialSeed { seed_index } => definition.seeds.get(seed_index).copied(),
        EliminationSource::WinnerOf(node) => resolution_players(facts[node.value()].state).0,
        EliminationSource::LoserOf(node) => resolution_players(facts[node.value()].state).1,
        EliminationSource::Vacant => None,
    }
}

fn resolution_players(state: EliminationNodeFactState) -> (Option<PlayerId>, Option<PlayerId>) {
    match state {
        EliminationNodeFactState::Resolved { resolution, .. } => match resolution {
            EliminationNodeResolution::MatchDecided { winner, loser } => {
                (Some(winner), Some(loser))
            }
            EliminationNodeResolution::WithdrawalWalkover { winner, withdrawn } => {
                (Some(winner), Some(withdrawn))
            }
            EliminationNodeResolution::WithdrawalVacancy { .. } => (None, None),
            EliminationNodeResolution::MutualWithdrawalVacancy { .. } => (None, None),
            EliminationNodeResolution::AutomaticAdvance { player } => (Some(player), None),
            EliminationNodeResolution::Vacant => (None, None),
        },
        _ => (None, None),
    }
}

fn withdrawn_players(facts: &[EliminationNodeFact]) -> HashSet<PlayerId> {
    facts
        .iter()
        .flat_map(|fact| match fact.state {
            EliminationNodeFactState::Resolved {
                resolution: EliminationNodeResolution::WithdrawalWalkover { withdrawn, .. },
                ..
            } => vec![withdrawn],
            EliminationNodeFactState::Resolved {
                resolution: EliminationNodeResolution::WithdrawalVacancy { withdrawn },
                ..
            } => vec![withdrawn],
            EliminationNodeFactState::Resolved {
                resolution: EliminationNodeResolution::MutualWithdrawalVacancy { withdrawn },
                ..
            } => withdrawn.to_vec(),
            _ => Vec::new(),
        })
        .collect()
}

fn reset_required(definition: &EliminationDefinition, facts: &[EliminationNodeFact]) -> bool {
    let Some(grand_final) = definition
        .nodes
        .iter()
        .find(|node| node.stage == EliminationStage::GrandFinal)
    else {
        return false;
    };
    let EliminationNodeFactState::Resolved {
        entrants,
        resolution,
    } = facts[grand_final.id.value()].state
    else {
        return false;
    };
    let winner = match resolution {
        EliminationNodeResolution::MatchDecided { winner, .. }
        | EliminationNodeResolution::WithdrawalWalkover { winner, .. }
        | EliminationNodeResolution::AutomaticAdvance { player: winner } => Some(winner),
        EliminationNodeResolution::WithdrawalVacancy { .. }
        | EliminationNodeResolution::MutualWithdrawalVacancy { .. }
        | EliminationNodeResolution::Vacant => None,
    };
    winner.is_some() && winner == entrants[1]
}

fn championship(state: EliminationNodeFactState) -> (Option<PlayerId>, Option<PlayerId>) {
    resolution_players(state)
}

/// Why an elimination bracket could not be created.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum BracketSetupError {
    /// A meaningful knockout bracket needs at least two participants.
    #[error("an elimination bracket needs at least two participants")]
    NotEnoughPlayers,
    /// Every seed position must refer to a distinct participant.
    #[error("participant {player:?} occurs more than once in the seed order")]
    DuplicateSeed {
        /// Participant repeated in the constructor order.
        player: PlayerId,
    },
}

pub(crate) fn validate_seeds(seeds: &[PlayerId]) -> Result<(), BracketSetupError> {
    if seeds.len() < 2 {
        return Err(BracketSetupError::NotEnoughPlayers);
    }
    let mut unique = HashSet::with_capacity(seeds.len());
    for &player in seeds {
        if !unique.insert(player) {
            return Err(BracketSetupError::DuplicateSeed { player });
        }
    }
    Ok(())
}
