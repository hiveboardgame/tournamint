//! Pure Swiss-system pairing and standings from accepted round facts.
//!
//! [`SwissConfig`] selects the documented supported subset of FIDE Dutch,
//! Burstein, or Double-Swiss rules, together with immutable scoring,
//! acceleration, PAB awards, and standings criteria. Pairing uses exact
//! maximum-weight matching where the selected system requires it. See the
//! feature guide and source map for the explicit conformance boundary.
//!
//! Standings may be projected before the configured round count is complete.
//! Article 16 values in such projections are project-provided provisional
//! "as if the tournament ended now" values over materialized history; later
//! records may change an earlier requested/withdrawal-tail record from
//! category 16.2.5 to 16.2.3. Its VUR status does not change.
//! Completed-tournament values are normative for the documented supported
//! rules.
//!
//! The other formats live in [`arena`](crate::arena),
//! [`elimination`](crate::elimination) and
//! [`round_robin`](crate::round_robin). Types shared by all four — player
//! ids, colours, scores, point systems — live at the crate root.

mod calculation;
mod config;
mod history;
mod pairing;
mod pairing_error;
mod player_data;
mod round;
mod round_status;
mod snapshot;
mod standings;

pub(crate) use config::SwissPairingSystem;
pub use config::{
    AccelerationPolicy, BuchholzOptions, BursteinConfig, DoubleSwissByePoints,
    DoubleSwissByePointsError, DoubleSwissConfig, DoubleSwissPrimaryScore, DutchConfig,
    FlatAcceleration, MatchPointSystem, ScoreBasis, SingleGameSwissByePoints, SwissConfig,
    SwissCriterion, SwissStandingsPlan, SwissSystemConfig, UnplayedRoundPolicy,
};
pub use history::MatchDisposition;
pub(crate) use history::{LogicalGame, LogicalGames, PlayerRoundRecord, SwissMatchRecord};
pub(crate) use history::{project_double_swiss_match, project_single_game_match};
pub use pairing::{ByeAssignment, ByeKind, RoundPairings};
pub(crate) use pairing_error::PairingFailure;
pub use pairing_error::{PairingError, PairingInternalError, PairingLimit};
pub use round::PairNextRoundError;
pub(crate) use round_status::RoundStatus;
pub(crate) use snapshot::{
    SwissPairingPlayerEntry, SwissPairingSnapshot, SwissPrimaryScoreProjection,
};
pub use standings::{SwissPlayerSummary, SwissProjection};
use std::collections::HashSet;

use crate::{GameOutcome, MatchScore, Pairing, PlayerId, Score};

pub(crate) use self::calculation::SwissCalculation;

/// Position of one logical game within a Swiss encounter.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
pub enum SwissLeg {
    /// The only game in a Dutch or Burstein encounter.
    Single,
    /// First game of a Double-Swiss encounter, in the accepted pairing's colours.
    First,
    /// Second game of a Double-Swiss encounter, with reversed colours.
    Second,
}

/// Stable native identity of one logical Swiss game.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(deny_unknown_fields)
)]
pub struct SwissGameId {
    /// Zero-based accepted round index.
    pub round_index: u32,
    /// Zero-based pairing index within the accepted round.
    pub pairing_index: u32,
    /// Logical game position within the encounter.
    pub leg: SwissLeg,
}

/// State of the accepted Swiss history, independent of an attempted pairing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SwissHistoryStatus {
    /// The latest accepted round still has unresolved logical games.
    AwaitingResults,
    /// The accepted prefix is complete and another round may be selected.
    ReadyToPair,
    /// The configured number of accepted rounds is complete.
    ConfiguredRoundsComplete,
}

/// Outcome of a successful next-round selection operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SwissNextRound {
    /// Exact pairings and byes selected for acceptance by the host.
    Pairings(RoundPairings),
    /// The eligible field cannot produce another legal round.
    ///
    /// This can occur before `expected_rounds`, even without requested byes or
    /// restrictions: the remaining non-rematch opponents or colour constraints
    /// need not permit a complete pairing. The configured count is a limit,
    /// not a guarantee that every round can be paired.
    PairingExhausted,
}

/// One projected accepted Swiss round.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwissRoundProjection {
    /// Zero-based accepted round index.
    pub round_index: u32,
    /// Pairing-oriented encounters in accepted order.
    pub encounters: Vec<SwissEncounterProjection>,
    /// Accepted byes and their configured awards.
    pub byes: Vec<SwissByeProjection>,
    /// Players already withdrawn when this round was accepted.
    pub withdrawn: Vec<PlayerId>,
}

/// One accepted Swiss encounter and its logical games.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwissEncounterProjection {
    /// Zero-based pairing index within its accepted round.
    pub pairing_index: u32,
    /// Accepted first-game orientation.
    pub pairing: Pairing,
    /// Every logical game required by the configured system.
    pub games: Vec<SwissGameProjection>,
    /// Aggregate game and match awards once every logical game is complete.
    pub completion: Option<SwissMatchCompletion>,
}

/// One logical Swiss game with its optional completed award.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SwissGameProjection {
    /// Stable native logical-game identity.
    pub id: SwissGameId,
    /// Actual colour orientation for this logical game.
    pub pairing: Pairing,
    /// Effective result and game-point award, when supplied.
    pub completed: Option<SwissCompletedGameProjection>,
}

/// Engine-owned awards for one completed Swiss logical game.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SwissCompletedGameProjection {
    /// Effective semantic outcome.
    pub outcome: GameOutcome,
    /// Configured game-point awards in this game's White, Black order.
    pub awarded_points: [Score; 2],
}

/// Completed encounter semantics aligned with the accepted pairing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SwissMatchCompletion {
    /// Match disposition for the accepted pairing's White, Black players.
    pub dispositions: [MatchDisposition; 2],
    /// Aggregate result for the accepted pairing's White, Black players.
    pub aggregate: [MatchScore; 2],
    /// Total game-point awards for the accepted pairing's White, Black players.
    pub game_points: [Score; 2],
    /// Match-point awards for the accepted pairing's White, Black players.
    pub match_points: [Score; 2],
}

/// One accepted Swiss bye with its configured awards.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SwissByeProjection {
    /// Participant sitting out the round.
    pub player: PlayerId,
    /// Accepted bye kind.
    pub kind: ByeKind,
    /// Configured game-point award.
    pub game_points: Score,
    /// Configured match-point award.
    pub match_points: Score,
}

/// Effective outcomes for one accepted pairing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwissEncounterFact {
    /// Pairing orientation generated for the round.
    pub pairing: Pairing,
    /// Exactly one position for Dutch/Burstein, or two for Double Swiss.
    /// Positions preserve logical leg identity: the first uses the accepted
    /// pairing's colours and the second reverses them. `None` means that leg
    /// is unresolved, including when the second leg finishes before the first.
    pub outcomes: Vec<Option<GameOutcome>>,
}

/// One accepted round, including exact pairings, byes, and current outcomes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwissRoundFact {
    /// Players eligible when this round was accepted. Omitted ranked players
    /// had withdrawn before this round and remain excluded thereafter.
    pub eligible: Vec<PlayerId>,
    /// Exact accepted selector output.
    pub pairings: RoundPairings,
    /// Effective encounter outcomes; corrections replace an element here.
    pub encounters: Vec<SwissEncounterFact>,
}

/// Complete immutable Swiss facts owned by the host.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwissFacts {
    /// Tournament configuration.
    pub config: SwissConfig,
    /// Stable best-first ranking. Must contain every `PlayerId` from zero to
    /// `initial_ranking.len() - 1` exactly once, in the desired ranking order.
    /// Player identity and ranking position need not be the same.
    pub initial_ranking: Vec<PlayerId>,
    /// Accepted rounds in zero-based order.
    pub rounds: Vec<SwissRoundFact>,
}

/// A restriction supported for selection of the next round.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwissPairingRestriction {
    /// Every pair within this group is forbidden for the next round.
    pub players: Vec<PlayerId>,
}

/// Host-owned participation input used only for selecting the next round.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SwissNextRoundInput {
    /// Players eligible for pairing. Every ranked player omitted here is
    /// treated as withdrawn for this calculation.
    pub eligible: Vec<PlayerId>,
    /// Eligible players requesting a zero-point bye.
    pub requested_byes: Vec<PlayerId>,
    /// Pairing restrictions effective for this next round.
    pub restrictions: Vec<SwissPairingRestriction>,
}

/// Invalid Swiss facts or a failed pure calculation.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SwissError {
    /// Configuration or initial ranking is invalid.
    #[error("invalid Swiss setup")]
    InvalidSetup,
    /// Accepted rounds and outcomes are structurally incoherent.
    #[error("accepted Swiss round facts are incoherent")]
    IncoherentFacts,
    /// Eligibility, requested byes, or restrictions name invalid players.
    #[error("Swiss next-round input is invalid")]
    InvalidInput,
    /// The selected pairing system could not produce the next round.
    #[error("Swiss next-round pairing failed: {0}")]
    Pairing(#[from] PairNextRoundError),
    /// Standings projection failed.
    #[error("Swiss standings projection failed: {0}")]
    Projection(#[from] crate::standings::StandingsError),
    /// A native round or pairing index exceeds its persisted representation.
    #[error("Swiss native game identity exceeds its numeric representation")]
    ValueOutOfRange,
}

/// Selects, but does not accept or persist, the next round.
pub fn pair_next_round(
    facts: &SwissFacts,
    input: &SwissNextRoundInput,
) -> Result<SwissNextRound, SwissError> {
    let mut tournament = rebuild(facts, true)?;
    let ranked: HashSet<_> = facts.initial_ranking.iter().copied().collect();
    let eligible: HashSet<_> = input.eligible.iter().copied().collect();
    if eligible.len() != input.eligible.len() || !eligible.is_subset(&ranked) {
        return Err(SwissError::InvalidInput);
    }
    let permanently_excluded: HashSet<_> = facts
        .rounds
        .iter()
        .flat_map(|round| {
            let accepted: HashSet<_> = round.eligible.iter().copied().collect();
            facts
                .initial_ranking
                .iter()
                .copied()
                .filter(move |player| !accepted.contains(player))
        })
        .collect();
    if !eligible.is_disjoint(&permanently_excluded) {
        return Err(SwissError::InvalidInput);
    }
    for &player in &facts.initial_ranking {
        if !eligible.contains(&player) && !permanently_excluded.contains(&player) {
            tournament
                .exclude_player(player)
                .map_err(|_| SwissError::InvalidInput)?;
        }
    }
    let requested: HashSet<_> = input.requested_byes.iter().copied().collect();
    if requested.len() != input.requested_byes.len() || !requested.is_subset(&eligible) {
        return Err(SwissError::InvalidInput);
    }
    for &player in &input.requested_byes {
        tournament
            .request_bye_for_calculation(player)
            .map_err(|_| SwissError::InvalidInput)?;
    }
    let round_index = facts.rounds.len();
    for restriction in &input.restrictions {
        tournament
            .add_calculation_restriction(&restriction.players, round_index..round_index + 1)
            .map_err(|_| SwissError::InvalidInput)?;
    }
    let natural_field = input.requested_byes.is_empty() && input.restrictions.is_empty();
    match tournament.pair_next_round() {
        Ok(pairings) => Ok(SwissNextRound::Pairings(pairings)),
        Err(PairNextRoundError::NoPairablePlayers) => Ok(SwissNextRound::PairingExhausted),
        Err(PairNextRoundError::Pairing(PairingError::NoValidPairing | PairingError::NoBye))
            if natural_field =>
        {
            Ok(SwissNextRound::PairingExhausted)
        }
        Err(error) => Err(error.into()),
    }
}

/// Projects canonical accepted rounds, standings, and participant summaries.
pub fn project(facts: &SwissFacts) -> Result<SwissProjection, SwissError> {
    let tournament = rebuild(facts, false)?;
    let history_status = if tournament.is_round_in_progress() {
        SwissHistoryStatus::AwaitingResults
    } else if facts.rounds.len() == facts.config.expected_rounds_usize() {
        SwissHistoryStatus::ConfiguredRoundsComplete
    } else {
        SwissHistoryStatus::ReadyToPair
    };
    let rounds = project_rounds(facts, &tournament)?;
    tournament
        .projection(rounds, history_status)
        .map_err(Into::into)
}

fn project_rounds(
    facts: &SwissFacts,
    tournament: &SwissCalculation,
) -> Result<Vec<SwissRoundProjection>, SwissError> {
    let is_double = matches!(facts.config.system, SwissSystemConfig::DoubleSwiss(_));
    facts
        .rounds
        .iter()
        .enumerate()
        .map(|(round_index, round)| {
            let native_round_index =
                u32::try_from(round_index).map_err(|_| SwissError::ValueOutOfRange)?;
            let eligible: HashSet<_> = round.eligible.iter().copied().collect();
            let withdrawn = facts
                .initial_ranking
                .iter()
                .copied()
                .filter(|player| !eligible.contains(player))
                .collect();
            let byes = round
                .pairings
                .byes
                .iter()
                .map(|bye_| {
                    let (game_points, match_points) = tournament.bye_points(bye_.kind);
                    SwissByeProjection {
                        player: bye_.player,
                        kind: bye_.kind,
                        game_points,
                        match_points,
                    }
                })
                .collect();
            let encounters = round
                .encounters
                .iter()
                .enumerate()
                .map(|(pairing_index, encounter)| {
                    let native_pairing_index =
                        u32::try_from(pairing_index).map_err(|_| SwissError::ValueOutOfRange)?;
                    project_encounter(
                        &facts.config,
                        native_round_index,
                        native_pairing_index,
                        encounter,
                        is_double,
                    )
                })
                .collect::<Result<Vec<_>, SwissError>>()?;
            Ok(SwissRoundProjection {
                round_index: native_round_index,
                encounters,
                byes,
                withdrawn,
            })
        })
        .collect()
}

fn project_encounter(
    config: &SwissConfig,
    round_index: u32,
    pairing_index: u32,
    encounter: &SwissEncounterFact,
    is_double: bool,
) -> Result<SwissEncounterProjection, SwissError> {
    let legs: &[SwissLeg] = if is_double {
        &[SwissLeg::First, SwissLeg::Second]
    } else {
        &[SwissLeg::Single]
    };
    let games = legs
        .iter()
        .copied()
        .enumerate()
        .map(|(game_index, leg)| {
            let pairing = if leg == SwissLeg::Second {
                Pairing::new(encounter.pairing.black(), encounter.pairing.white())
            } else {
                encounter.pairing
            };
            let completed =
                encounter.outcomes[game_index].map(|outcome| SwissCompletedGameProjection {
                    outcome,
                    awarded_points: [
                        config.game_point_system.points_for(outcome.white()),
                        config.game_point_system.points_for(outcome.black()),
                    ],
                });
            SwissGameProjection {
                id: SwissGameId {
                    round_index,
                    pairing_index,
                    leg,
                },
                pairing,
                completed,
            }
        })
        .collect();
    let completion = project_match_completion(config, &encounter.outcomes);
    Ok(SwissEncounterProjection {
        pairing_index,
        pairing: encounter.pairing,
        games,
        completion,
    })
}

fn project_match_completion(
    config: &SwissConfig,
    outcomes: &[Option<GameOutcome>],
) -> Option<SwissMatchCompletion> {
    let projection = match config.system {
        SwissSystemConfig::Dutch(_) | SwissSystemConfig::Burstein(_) => {
            let outcome = outcomes[0]?;
            project_single_game_match(outcome.white(), outcome.black(), config.game_point_system)
                .expect("validated Swiss facts contain reciprocal outcomes")
        }
        SwissSystemConfig::DoubleSwiss(_) => {
            let first = outcomes[0]?;
            let second = outcomes[1]?;
            project_double_swiss_match(
                [first.white(), second.black()],
                [first.black(), second.white()],
                config.game_point_system,
            )
            .expect("validated Double-Swiss facts have exact game-point totals")
        }
    };
    let game_points = [
        projection.player.game_points,
        projection.opponent.game_points,
    ];
    let match_points = match config.system {
        SwissSystemConfig::Dutch(_) | SwissSystemConfig::Burstein(_) => game_points,
        SwissSystemConfig::DoubleSwiss(double) => [
            double
                .match_point_system
                .projected_points(projection.player.disposition, projection.player.aggregate),
            double.match_point_system.projected_points(
                projection.opponent.disposition,
                projection.opponent.aggregate,
            ),
        ],
    };
    Some(SwissMatchCompletion {
        dispositions: [
            projection.player.disposition,
            projection.opponent.disposition,
        ],
        aggregate: [projection.player.aggregate, projection.opponent.aggregate],
        game_points,
        match_points,
    })
}

fn rebuild(facts: &SwissFacts, require_complete: bool) -> Result<SwissCalculation, SwissError> {
    let mut tournament = SwissCalculation::new(facts.config.clone(), &facts.initial_ranking)
        .map_err(|_| SwissError::InvalidSetup)?;
    let outcome_count = if matches!(facts.config.system, SwissSystemConfig::DoubleSwiss(_)) {
        2
    } else {
        1
    };
    let mut excluded = HashSet::new();
    for (round_index, round) in facts.rounds.iter().enumerate() {
        if round.encounters.len() != round.pairings.games.len() {
            return Err(SwissError::IncoherentFacts);
        }
        let ranked: HashSet<_> = facts.initial_ranking.iter().copied().collect();
        let eligible: HashSet<_> = round.eligible.iter().copied().collect();
        if eligible.len() != round.eligible.len()
            || !eligible.is_subset(&ranked)
            || !eligible.is_disjoint(&excluded)
        {
            return Err(SwissError::IncoherentFacts);
        }
        for &player in &facts.initial_ranking {
            if !eligible.contains(&player) && excluded.insert(player) {
                tournament
                    .exclude_player(player)
                    .map_err(|_| SwissError::IncoherentFacts)?;
            }
        }
        if !round.pairings.assigns_exactly(&eligible) {
            return Err(SwissError::IncoherentFacts);
        }
        tournament
            .accept_round_fact(round.pairings.clone())
            .map_err(|_| SwissError::IncoherentFacts)?;
        for (expected, encounter) in round.pairings.games.iter().zip(&round.encounters) {
            if *expected != encounter.pairing || encounter.outcomes.len() != outcome_count {
                return Err(SwissError::IncoherentFacts);
            }
            for (index, outcome) in encounter.outcomes.iter().copied().enumerate() {
                let Some(outcome) = outcome else {
                    continue;
                };
                let (white, black) = if index == 0 {
                    (encounter.pairing.white(), encounter.pairing.black())
                } else {
                    (encounter.pairing.black(), encounter.pairing.white())
                };
                tournament
                    .apply_outcome_fact(white, black, outcome)
                    .map_err(|_| SwissError::IncoherentFacts)?;
            }
        }
        if tournament.is_round_in_progress()
            && (require_complete || round_index + 1 != facts.rounds.len())
        {
            return Err(SwissError::IncoherentFacts);
        }
    }
    Ok(tournament)
}
