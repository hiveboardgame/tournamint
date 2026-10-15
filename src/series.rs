//! Pure progression for a two-entrant series.
//!
//! The host stores the plan and game facts. [`project`] derives the current
//! score, while [`next_action`] returns at most one atomic persistence action.

use std::num::NonZeroU16;

use crate::{GameOutcome, MatchScore, PlayerId};

/// Which entrant receives White for a game in the repeating colour cycle.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum EntrantSide {
    /// The first element of the entrants array.
    First,
    /// The second element of the entrants array.
    Second,
}

/// How many tied sets may be played in a phase.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum SetLimit {
    /// Play at most this many sets before moving to the next phase.
    AtMost(NonZeroU16),
    /// Repeat this final phase until a set has a winner.
    UntilDecisive,
}

/// Whether games made unnecessary by an insurmountable lead are played.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum ClinchPolicy {
    /// Finish every game in the set.
    PlayAll,
    /// Stop once the trailing entrant cannot tie the set.
    EarlyClinch,
}

/// One phase of a series plan.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct SeriesPhase {
    /// Games created together for each set.
    pub games_per_set: NonZeroU16,
    /// White assignment template inverted on every odd repeated set in this phase.
    pub color_order: Vec<EntrantSide>,
    /// Set repetition rule for this phase.
    pub set_limit: SetLimit,
    /// In-set clinching rule.
    pub clinch: ClinchPolicy,
}

/// Complete validated-by-use series plan.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct SeriesPlan {
    /// Finite phases followed by one until-decisive phase.
    pub phases: Vec<SeriesPhase>,
}

impl SeriesPlan {
    /// A balanced plan made of two-game sets repeated until decisive.
    pub fn balanced() -> Self {
        Self {
            phases: vec![SeriesPhase {
                games_per_set: NonZeroU16::new(2).expect("two is nonzero"),
                color_order: vec![EntrantSide::First, EntrantSide::Second],
                set_limit: SetLimit::UntilDecisive,
                clinch: ClinchPolicy::PlayAll,
            }],
        }
    }

    /// Checks the structural invariants required by projection.
    pub fn validate(&self) -> Result<(), SeriesError> {
        if self.phases.is_empty() {
            return Err(SeriesError::EmptyPhases);
        }
        let last = self.phases.len() - 1;
        let mut game_count = 0usize;
        for (index, phase) in self.phases.iter().enumerate() {
            if phase.color_order.len() != usize::from(phase.games_per_set.get()) {
                return Err(SeriesError::InvalidColorOrderLength { phase: index });
            }
            let first = phase
                .color_order
                .iter()
                .filter(|side| **side == EntrantSide::First)
                .count();
            let second = phase.color_order.len() - first;
            if first.abs_diff(second) > 1 {
                return Err(SeriesError::UnbalancedColorOrder { phase: index });
            }
            match phase.set_limit {
                SetLimit::AtMost(_) if index == last => {
                    return Err(SeriesError::FinalPhaseMustBeUntilDecisive);
                }
                SetLimit::UntilDecisive if index != last => {
                    return Err(SeriesError::NonFinalPhaseMustBeFinite { phase: index });
                }
                SetLimit::AtMost(_) | SetLimit::UntilDecisive => {}
            }
            let sets = match phase.set_limit {
                SetLimit::AtMost(limit) => usize::from(limit.get()),
                SetLimit::UntilDecisive => 1,
            };
            let phase_games = usize::from(phase.games_per_set.get())
                .checked_mul(sets)
                .ok_or(SeriesError::ValueOutOfRange)?;
            game_count = game_count
                .checked_add(phase_games)
                .ok_or(SeriesError::ValueOutOfRange)?;
            // A tie in the representative final set must still permit runtime
            // construction of the first game in the following set.
            game_id(game_count)?;
        }
        Ok(())
    }
}

/// Deterministic zero-based ordinal of a game within its series.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct SeriesGameId(u32);

impl SeriesGameId {
    /// Recreates a persisted series-game ordinal.
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Returns the zero-based ordinal.
    pub const fn value(self) -> u32 {
        self.0
    }
}

/// Current persistence state of one series game.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SeriesGameState {
    /// Created but not released to physical play.
    Planned,
    /// Released and awaiting a terminal physical-game fact.
    Released,
    /// Terminal effective game outcome.
    Completed(GameOutcome),
    /// Not played because an earlier result clinched the set.
    Skipped,
    /// Not played because the host ended the series by withdrawal.
    Cancelled,
}

/// One current game fact owned by the host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SeriesGameFact {
    /// Deterministic game ordinal.
    pub id: SeriesGameId,
    /// Entrant assigned White.
    pub white: PlayerId,
    /// Current state.
    pub state: SeriesGameState,
}

/// Specification for a game created as part of an atomic set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SeriesGameSpec {
    /// Deterministic game ordinal.
    pub id: SeriesGameId,
    /// Zero-based phase containing the set.
    pub phase_index: usize,
    /// Zero-based set index within the phase.
    pub set_index: u32,
    /// Zero-based game index within the set.
    pub game_index: u16,
    /// Scheduled White.
    pub white: PlayerId,
    /// Scheduled Black.
    pub black: PlayerId,
}

/// Nonempty opening set produced from a validated series plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InitialSeriesSet {
    /// First game in the opening set.
    pub first: SeriesGameSpec,
    /// Remaining games in the opening set, in game order.
    pub remaining: Vec<SeriesGameSpec>,
    /// First game to release atomically.
    pub release: SeriesGameId,
}

/// One atomic write requested from the host.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SeriesAction {
    /// Release an already-created game.
    Release {
        /// Created game to expose to play.
        game: SeriesGameId,
    },
    /// Create a whole set and release its first game in the same write.
    CreateSet {
        /// Complete set, in game order.
        games: Vec<SeriesGameSpec>,
        /// First game to release atomically.
        release: SeriesGameId,
    },
    /// Skip a clinched suffix and decide the series atomically.
    SkipAndDecide {
        /// Created games which are no longer needed.
        games: Vec<SeriesGameId>,
        /// Series winner.
        winner: PlayerId,
    },
    /// Decide the series without any accompanying game writes.
    Decide {
        /// Series winner.
        winner: PlayerId,
    },
}

/// Doubled half-point score for the current set.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SeriesScore {
    /// First entrant's half-points.
    pub first: u64,
    /// Second entrant's half-points.
    pub second: u64,
}

/// Pure view of the current series facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SeriesProjection {
    /// Phase containing the current or terminal set.
    pub phase_index: usize,
    /// Set index within the phase.
    pub set_index: u32,
    /// Current set score in half-points.
    pub score: SeriesScore,
    /// Winner when the facts already decide the series.
    pub winner: Option<PlayerId>,
    /// Released game still awaiting a result, if any.
    pub in_progress: Option<SeriesGameId>,
    /// Every fully materialized set, including tied predecessors.
    pub sets: Vec<SeriesSetProjection>,
}

/// One fully materialized set in series order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SeriesSetProjection {
    /// Zero-based phase containing this set.
    pub phase_index: usize,
    /// Zero-based set index within the phase.
    pub set_index: u32,
    /// Current exact doubled half-point score.
    pub score: SeriesScore,
    /// Materialized games in set order.
    pub games: Vec<SeriesGameProjection>,
}

/// One materialized series game and its exact score contribution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SeriesGameProjection {
    /// Deterministic series-game identity.
    pub id: SeriesGameId,
    /// Zero-based phase containing this game.
    pub phase_index: usize,
    /// Zero-based set index within the phase.
    pub set_index: u32,
    /// Zero-based game index within the set.
    pub game_index: u16,
    /// Participants in White, Black order.
    pub participants: [PlayerId; 2],
    /// Current host-owned lifecycle fact.
    pub state: SeriesGameState,
    /// Exact contribution to the first and second series entrants.
    pub score_contribution: SeriesScore,
}

/// Invalid plan or incoherent game facts.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SeriesError {
    /// A phase's set template must contain one assignment per game.
    #[error("series phase {phase} color order length does not match its games per set")]
    InvalidColorOrderLength {
        /// Zero-based invalid phase.
        phase: usize,
    },
    /// Set assignments must be equal, or differ by one for an odd-sized set.
    #[error("series phase {phase} color order gives one entrant too many White assignments")]
    UnbalancedColorOrder {
        /// Zero-based invalid phase.
        phase: usize,
    },
    /// At least one phase is required.
    #[error("the series phase list is empty")]
    EmptyPhases,
    /// Only the final phase may repeat without a limit.
    #[error("series phase {phase} is non-final but repeats until decisive")]
    NonFinalPhaseMustBeFinite {
        /// Zero-based invalid phase.
        phase: usize,
    },
    /// The last phase must guarantee a decision.
    #[error("the final series phase must repeat until decisive")]
    FinalPhaseMustBeUntilDecisive,
    /// The entrants must be distinct.
    #[error("a series needs two distinct entrants")]
    DuplicateEntrants,
    /// Facts are not a prefix of sets generated by the plan.
    #[error("the series game facts are structurally incoherent")]
    IncoherentFacts,
    /// A deterministic ordinal could not be represented.
    #[error("the series game ordinal exceeds its numeric representation")]
    ValueOutOfRange,
}

#[derive(Clone, Debug)]
struct Evaluation {
    projection: SeriesProjection,
    action: Option<SeriesAction>,
}

/// Projects the current series without requesting a write.
pub fn project(
    plan: &SeriesPlan,
    entrants: [PlayerId; 2],
    games: &[SeriesGameFact],
) -> Result<SeriesProjection, SeriesError> {
    Ok(evaluate(plan, entrants, games, false)?.projection)
}

/// Projects the history of a series ended externally by withdrawal.
///
/// Earlier sets must have tied normally. The final set must contain completed
/// games followed by cancellations, with no released or planned games left.
/// The host owns the withdrawal winner; game scores never decide this projection.
pub fn project_withdrawal(
    plan: &SeriesPlan,
    entrants: [PlayerId; 2],
    games: &[SeriesGameFact],
) -> Result<SeriesProjection, SeriesError> {
    Ok(evaluate(plan, entrants, games, true)?.projection)
}

/// Returns at most one atomic persistence action for the current facts.
///
/// `None` means a released game is in progress or the supplied facts already
/// represent an externally blocked/terminal state.
pub fn next_action(
    plan: &SeriesPlan,
    entrants: [PlayerId; 2],
    games: &[SeriesGameFact],
) -> Result<Option<SeriesAction>, SeriesError> {
    Ok(evaluate(plan, entrants, games, false)?.action)
}

/// Builds the nonempty opening set for a series.
pub fn initial_set(
    plan: &SeriesPlan,
    entrants: [PlayerId; 2],
) -> Result<InitialSeriesSet, SeriesError> {
    plan.validate()?;
    if entrants[0] == entrants[1] {
        return Err(SeriesError::DuplicateEntrants);
    }
    initial_set_after_validation(plan, entrants)
}

fn initial_set_after_validation(
    plan: &SeriesPlan,
    entrants: [PlayerId; 2],
) -> Result<InitialSeriesSet, SeriesError> {
    let mut games = set_specs(plan, entrants, 0, 0, 0)?.into_iter();
    let first = games.next().ok_or(SeriesError::IncoherentFacts)?;
    Ok(InitialSeriesSet {
        first,
        remaining: games.collect(),
        release: first.id,
    })
}

fn evaluate(
    plan: &SeriesPlan,
    entrants: [PlayerId; 2],
    games: &[SeriesGameFact],
    withdrawn: bool,
) -> Result<Evaluation, SeriesError> {
    plan.validate()?;
    if entrants[0] == entrants[1] {
        return Err(SeriesError::DuplicateEntrants);
    }
    validate_fact_prefix(plan, entrants, games)?;
    if games.is_empty() {
        if withdrawn {
            return Err(SeriesError::IncoherentFacts);
        }
        let initial = initial_set_after_validation(plan, entrants)?;
        let specs = std::iter::once(initial.first)
            .chain(initial.remaining)
            .collect();
        return Ok(Evaluation {
            projection: SeriesProjection {
                phase_index: 0,
                set_index: 0,
                score: SeriesScore::default(),
                winner: None,
                in_progress: None,
                sets: Vec::new(),
            },
            action: Some(SeriesAction::CreateSet {
                games: specs,
                release: initial.release,
            }),
        });
    }

    let mut phase_index = 0;
    let mut set_index = 0u32;
    let mut offset = 0usize;
    let mut projected_sets = Vec::new();
    loop {
        let phase = plan
            .phases
            .get(phase_index)
            .ok_or(SeriesError::IncoherentFacts)?;
        let set_len = usize::from(phase.games_per_set.get());
        let end = offset
            .checked_add(set_len)
            .ok_or(SeriesError::ValueOutOfRange)?;
        if games.len() < end {
            return Err(SeriesError::IncoherentFacts);
        }
        let set = evaluate_set(
            phase.clinch,
            entrants,
            &games[offset..end],
            withdrawn && end == games.len(),
        )?;
        projected_sets.push(project_set(
            entrants,
            phase_index,
            set_index,
            &games[offset..end],
            set.score,
        )?);
        if end < games.len() {
            if !matches!(set.state, SetState::Tied) {
                return Err(SeriesError::IncoherentFacts);
            }
            (phase_index, set_index) = following_set(plan, phase_index, set_index)?;
            offset = end;
            continue;
        }

        let (winner, in_progress, action) = match set.state {
            SetState::Withdrawn => (None, None, None),
            SetState::Released(game) => (None, Some(game), None),
            SetState::Planned(game) => (None, None, Some(SeriesAction::Release { game })),
            SetState::Decisive(winner, ref skipped) if skipped.is_empty() => {
                (Some(winner), None, Some(SeriesAction::Decide { winner }))
            }
            SetState::Decisive(winner, skipped) => (
                Some(winner),
                None,
                Some(SeriesAction::SkipAndDecide {
                    games: skipped,
                    winner,
                }),
            ),
            SetState::SettledWinner(winner) => (Some(winner), None, None),
            SetState::Tied => {
                let (next_phase, next_set) = following_set(plan, phase_index, set_index)?;
                let specs = set_specs(plan, entrants, next_phase, next_set, end)?;
                let release = specs[0].id;
                (
                    None,
                    None,
                    Some(SeriesAction::CreateSet {
                        games: specs,
                        release,
                    }),
                )
            }
        };
        return Ok(Evaluation {
            projection: SeriesProjection {
                phase_index,
                set_index,
                score: set.score,
                winner,
                in_progress,
                sets: projected_sets,
            },
            action,
        });
    }
}

fn project_set(
    entrants: [PlayerId; 2],
    phase_index: usize,
    set_index: u32,
    games: &[SeriesGameFact],
    score: SeriesScore,
) -> Result<SeriesSetProjection, SeriesError> {
    let games = games
        .iter()
        .enumerate()
        .map(|(game_index, game)| {
            let black = if game.white == entrants[0] {
                entrants[1]
            } else {
                entrants[0]
            };
            let mut score_contribution = SeriesScore::default();
            if let SeriesGameState::Completed(outcome) = game.state {
                add_outcome(&mut score_contribution, entrants, game.white, outcome)?;
            }
            Ok(SeriesGameProjection {
                id: game.id,
                phase_index,
                set_index,
                game_index: u16::try_from(game_index).map_err(|_| SeriesError::ValueOutOfRange)?,
                participants: [game.white, black],
                state: game.state,
                score_contribution,
            })
        })
        .collect::<Result<Vec<_>, SeriesError>>()?;
    Ok(SeriesSetProjection {
        phase_index,
        set_index,
        score,
        games,
    })
}

fn validate_fact_prefix(
    plan: &SeriesPlan,
    entrants: [PlayerId; 2],
    facts: &[SeriesGameFact],
) -> Result<(), SeriesError> {
    let mut phase_index = 0;
    let mut set_index = 0u32;
    let mut offset = 0usize;
    while offset < facts.len() {
        let phase = plan
            .phases
            .get(phase_index)
            .ok_or(SeriesError::IncoherentFacts)?;
        let set_len = usize::from(phase.games_per_set.get());
        let end = offset
            .checked_add(set_len)
            .ok_or(SeriesError::ValueOutOfRange)?;
        if facts.len() < end {
            return Err(SeriesError::IncoherentFacts);
        }
        for (game_index, fact) in facts[offset..end].iter().enumerate() {
            let id = game_id(offset + game_index)?;
            let side = phase
                .color_order
                .get(game_index)
                .copied()
                .ok_or(SeriesError::IncoherentFacts)?;
            let side = side_for_set(side, set_index);
            if fact.id != id || fact.white != white_for(entrants, side) {
                return Err(SeriesError::IncoherentFacts);
            }
        }
        offset = end;
        if offset < facts.len() {
            (phase_index, set_index) = following_set(plan, phase_index, set_index)?;
        }
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct SetEvaluation {
    score: SeriesScore,
    state: SetState,
}

#[derive(Clone, Debug)]
enum SetState {
    Released(SeriesGameId),
    Planned(SeriesGameId),
    Tied,
    Decisive(PlayerId, Vec<SeriesGameId>),
    SettledWinner(PlayerId),
    Withdrawn,
}

fn evaluate_set(
    clinch: ClinchPolicy,
    entrants: [PlayerId; 2],
    games: &[SeriesGameFact],
    withdrawn: bool,
) -> Result<SetEvaluation, SeriesError> {
    let mut score = SeriesScore::default();
    for (index, game) in games.iter().enumerate() {
        match game.state {
            SeriesGameState::Completed(outcome) => {
                add_outcome(&mut score, entrants, game.white, outcome)?;
                if clinch == ClinchPolicy::EarlyClinch {
                    let remaining = games.len() - index - 1;
                    if let Some(winner) = clinched_winner(score, entrants, remaining) {
                        let suffix = &games[index + 1..];
                        if withdrawn
                            && suffix
                                .iter()
                                .all(|game| game.state == SeriesGameState::Cancelled)
                        {
                            return Ok(SetEvaluation {
                                score,
                                state: SetState::Withdrawn,
                            });
                        }
                        if withdrawn {
                            return Err(SeriesError::IncoherentFacts);
                        }
                        if suffix
                            .iter()
                            .all(|game| game.state == SeriesGameState::Planned)
                        {
                            return Ok(SetEvaluation {
                                score,
                                state: SetState::Decisive(
                                    winner,
                                    suffix.iter().map(|game| game.id).collect(),
                                ),
                            });
                        }
                        if !suffix.is_empty()
                            && suffix
                                .iter()
                                .all(|game| game.state == SeriesGameState::Skipped)
                        {
                            return Ok(SetEvaluation {
                                score,
                                state: SetState::SettledWinner(winner),
                            });
                        }
                        if suffix.is_empty() {
                            return Ok(SetEvaluation {
                                score,
                                state: SetState::Decisive(winner, Vec::new()),
                            });
                        }
                        return Err(SeriesError::IncoherentFacts);
                    }
                }
            }
            SeriesGameState::Released => {
                if withdrawn {
                    return Err(SeriesError::IncoherentFacts);
                }
                if games[index + 1..]
                    .iter()
                    .any(|game| game.state != SeriesGameState::Planned)
                {
                    return Err(SeriesError::IncoherentFacts);
                }
                return Ok(SetEvaluation {
                    score,
                    state: SetState::Released(game.id),
                });
            }
            SeriesGameState::Planned => {
                if withdrawn {
                    return Err(SeriesError::IncoherentFacts);
                }
                if games[index + 1..]
                    .iter()
                    .any(|game| game.state != SeriesGameState::Planned)
                {
                    return Err(SeriesError::IncoherentFacts);
                }
                return Ok(SetEvaluation {
                    score,
                    state: SetState::Planned(game.id),
                });
            }
            SeriesGameState::Cancelled if withdrawn => {
                if games[index + 1..]
                    .iter()
                    .any(|game| game.state != SeriesGameState::Cancelled)
                {
                    return Err(SeriesError::IncoherentFacts);
                }
                return Ok(SetEvaluation {
                    score,
                    state: SetState::Withdrawn,
                });
            }
            SeriesGameState::Skipped | SeriesGameState::Cancelled => {
                return Err(SeriesError::IncoherentFacts);
            }
        }
    }
    if withdrawn {
        return Ok(SetEvaluation {
            score,
            state: SetState::Withdrawn,
        });
    }
    let state = match score.first.cmp(&score.second) {
        std::cmp::Ordering::Greater => SetState::Decisive(entrants[0], Vec::new()),
        std::cmp::Ordering::Less => SetState::Decisive(entrants[1], Vec::new()),
        std::cmp::Ordering::Equal => SetState::Tied,
    };
    Ok(SetEvaluation { score, state })
}

fn set_specs(
    plan: &SeriesPlan,
    entrants: [PlayerId; 2],
    phase_index: usize,
    set_index: u32,
    series_start: usize,
) -> Result<Vec<SeriesGameSpec>, SeriesError> {
    let phase = plan
        .phases
        .get(phase_index)
        .ok_or(SeriesError::IncoherentFacts)?;
    (0..phase.games_per_set.get())
        .map(|game_index| {
            let index = series_start
                .checked_add(usize::from(game_index))
                .ok_or(SeriesError::ValueOutOfRange)?;
            let id = game_id(index)?;
            let side = phase
                .color_order
                .get(usize::from(game_index))
                .copied()
                .ok_or(SeriesError::IncoherentFacts)?;
            let side = side_for_set(side, set_index);
            let white = white_for(entrants, side);
            let black = if white == entrants[0] {
                entrants[1]
            } else {
                entrants[0]
            };
            Ok(SeriesGameSpec {
                id,
                phase_index,
                set_index,
                game_index,
                white,
                black,
            })
        })
        .collect()
}

const fn side_for_set(side: EntrantSide, set_index: u32) -> EntrantSide {
    if set_index.is_multiple_of(2) {
        side
    } else {
        match side {
            EntrantSide::First => EntrantSide::Second,
            EntrantSide::Second => EntrantSide::First,
        }
    }
}

fn white_for(entrants: [PlayerId; 2], side: EntrantSide) -> PlayerId {
    match side {
        EntrantSide::First => entrants[0],
        EntrantSide::Second => entrants[1],
    }
}

fn game_id(index: usize) -> Result<SeriesGameId, SeriesError> {
    Ok(SeriesGameId::new(
        u32::try_from(index).map_err(|_| SeriesError::ValueOutOfRange)?,
    ))
}

fn following_set(
    plan: &SeriesPlan,
    phase_index: usize,
    set_index: u32,
) -> Result<(usize, u32), SeriesError> {
    let following = set_index
        .checked_add(1)
        .ok_or(SeriesError::ValueOutOfRange)?;
    match plan.phases[phase_index].set_limit {
        SetLimit::AtMost(limit) if following < u32::from(limit.get()) => {
            Ok((phase_index, following))
        }
        SetLimit::AtMost(_) => Ok((phase_index + 1, 0)),
        SetLimit::UntilDecisive => Ok((phase_index, following)),
    }
}

fn clinched_winner(
    score: SeriesScore,
    entrants: [PlayerId; 2],
    remaining: usize,
) -> Option<PlayerId> {
    let remaining = u64::try_from(remaining).ok()?.checked_mul(2)?;
    if score.first > score.second.checked_add(remaining)? {
        Some(entrants[0])
    } else if score.second > score.first.checked_add(remaining)? {
        Some(entrants[1])
    } else {
        None
    }
}

fn add_outcome(
    score: &mut SeriesScore,
    entrants: [PlayerId; 2],
    white: PlayerId,
    outcome: GameOutcome,
) -> Result<(), SeriesError> {
    let white_points = match outcome.white().score() {
        MatchScore::Win => 2,
        MatchScore::Draw => 1,
        MatchScore::Loss => 0,
    };
    let black_points = match outcome.black().score() {
        MatchScore::Win => 2,
        MatchScore::Draw => 1,
        MatchScore::Loss => 0,
    };
    let (first, second) = if white == entrants[0] {
        (white_points, black_points)
    } else if white == entrants[1] {
        (black_points, white_points)
    } else {
        return Err(SeriesError::IncoherentFacts);
    };
    score.first = score
        .first
        .checked_add(first)
        .ok_or(SeriesError::ValueOutOfRange)?;
    score.second = score
        .second
        .checked_add(second)
        .ok_or(SeriesError::ValueOutOfRange)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn players() -> [PlayerId; 2] {
        [PlayerId::new(0), PlayerId::new(1)]
    }

    fn plan(games: u16, clinch: ClinchPolicy) -> SeriesPlan {
        SeriesPlan {
            phases: vec![SeriesPhase {
                games_per_set: NonZeroU16::new(games).expect("test plans use nonzero set sizes"),
                color_order: (0..games)
                    .map(|index| {
                        if index % 2 == 0 {
                            EntrantSide::First
                        } else {
                            EntrantSide::Second
                        }
                    })
                    .collect(),
                set_limit: SetLimit::UntilDecisive,
                clinch,
            }],
        }
    }

    fn initial(plan: &SeriesPlan) -> Vec<SeriesGameFact> {
        let Some(SeriesAction::CreateSet { games, .. }) =
            next_action(plan, players(), &[]).unwrap()
        else {
            panic!("first action must create a set")
        };
        games
            .into_iter()
            .map(|game| SeriesGameFact {
                id: game.id,
                white: game.white,
                state: SeriesGameState::Planned,
            })
            .collect()
    }

    #[test]
    fn withdrawal_preserves_a_leading_score_without_deciding_or_releasing_games() {
        let plan = plan(6, ClinchPolicy::EarlyClinch);
        let mut facts = initial(&plan);
        for (index, fact) in facts.iter_mut().enumerate() {
            fact.state = if index < 4 {
                let first_wins = index < 3;
                SeriesGameState::Completed(if (fact.white == players()[0]) == first_wins {
                    GameOutcome::white_win()
                } else {
                    GameOutcome::black_win()
                })
            } else {
                SeriesGameState::Cancelled
            };
        }
        let projected = project_withdrawal(&plan, players(), &facts).unwrap();
        assert_eq!(
            projected.score,
            SeriesScore {
                first: 6,
                second: 2
            }
        );
        assert_eq!(projected.winner, None);
        assert_eq!(projected.in_progress, None);
        assert_eq!(projected.sets.len(), 1);
        assert_eq!(
            project(&plan, players(), &facts),
            Err(SeriesError::IncoherentFacts)
        );
        assert_eq!(
            next_action(&plan, players(), &facts),
            Err(SeriesError::IncoherentFacts)
        );
        facts[5].state = SeriesGameState::Completed(GameOutcome::draw());
        assert_eq!(
            project_withdrawal(&plan, players(), &facts),
            Err(SeriesError::IncoherentFacts)
        );
    }

    #[test]
    fn withdrawal_requires_tied_predecessors_and_a_terminal_last_set() {
        let plan = plan(2, ClinchPolicy::EarlyClinch);
        let mut facts = initial(&plan);
        facts
            .iter_mut()
            .for_each(|fact| fact.state = SeriesGameState::Completed(GameOutcome::draw()));
        let Some(SeriesAction::CreateSet { games, .. }) =
            next_action(&plan, players(), &facts).unwrap()
        else {
            panic!("tied opening set creates another set");
        };
        facts.extend(games.into_iter().map(|game| SeriesGameFact {
            id: game.id,
            white: game.white,
            state: SeriesGameState::Cancelled,
        }));
        let projected = project_withdrawal(&plan, players(), &facts).unwrap();
        assert_eq!(projected.sets.len(), 2);
        assert_eq!(
            projected.sets[0].score,
            SeriesScore {
                first: 2,
                second: 2
            }
        );
        assert_eq!(projected.score, SeriesScore::default());
        for state in [
            SeriesGameState::Released,
            SeriesGameState::Planned,
            SeriesGameState::Skipped,
        ] {
            facts[2].state = state;
            assert_eq!(
                project_withdrawal(&plan, players(), &facts),
                Err(SeriesError::IncoherentFacts)
            );
        }
        facts[2].state = SeriesGameState::Cancelled;
        facts[0].state = SeriesGameState::Completed(GameOutcome::white_win());
        assert_eq!(
            project_withdrawal(&plan, players(), &facts),
            Err(SeriesError::IncoherentFacts)
        );
    }

    #[test]
    fn every_real_action_and_waiting_none_are_distinct() {
        let series_plan = plan(3, ClinchPolicy::EarlyClinch);
        let entrants = players();
        let mut facts = initial(&series_plan);
        assert_eq!(
            next_action(&series_plan, entrants, &facts).unwrap(),
            Some(SeriesAction::Release { game: facts[0].id })
        );
        facts[0].state = SeriesGameState::Released;
        assert_eq!(next_action(&series_plan, entrants, &facts).unwrap(), None);
        facts[0].state = SeriesGameState::Completed(GameOutcome::white_win());
        facts[1].state = SeriesGameState::Completed(GameOutcome::black_win());
        assert_eq!(
            next_action(&series_plan, entrants, &facts).unwrap(),
            Some(SeriesAction::SkipAndDecide {
                games: vec![facts[2].id],
                winner: entrants[0],
            })
        );
        facts[2].state = SeriesGameState::Skipped;
        assert_eq!(next_action(&series_plan, entrants, &facts).unwrap(), None);

        let one_game = plan(1, ClinchPolicy::PlayAll);
        let mut complete = initial(&one_game);
        complete[0].state = SeriesGameState::Completed(GameOutcome::white_win());
        assert_eq!(
            next_action(&one_game, entrants, &complete).unwrap(),
            Some(SeriesAction::Decide {
                winner: entrants[0]
            })
        );
    }

    #[test]
    fn tied_finite_phases_advance_to_the_next_phase_color_order() {
        let plan = SeriesPlan {
            phases: vec![
                SeriesPhase {
                    games_per_set: NonZeroU16::new(2).unwrap(),
                    color_order: vec![EntrantSide::First, EntrantSide::Second],
                    set_limit: SetLimit::AtMost(NonZeroU16::new(1).unwrap()),
                    clinch: ClinchPolicy::PlayAll,
                },
                SeriesPhase {
                    games_per_set: NonZeroU16::new(1).unwrap(),
                    color_order: vec![EntrantSide::Second],
                    set_limit: SetLimit::UntilDecisive,
                    clinch: ClinchPolicy::PlayAll,
                },
            ],
        };
        let entrants = players();
        let mut facts = initial(&plan);
        for fact in &mut facts {
            fact.state = SeriesGameState::Completed(GameOutcome::draw());
        }
        let Some(SeriesAction::CreateSet { games, release }) =
            next_action(&plan, entrants, &facts).unwrap()
        else {
            panic!("tied finite phase must create the next phase")
        };
        assert_eq!(games[0].phase_index, 1);
        assert_eq!(games[0].white, entrants[1]);
        assert_eq!(release, SeriesGameId::new(2));
    }

    #[test]
    fn skipped_games_require_a_genuine_early_clinch_prefix() {
        for (games, clinch) in [(1, ClinchPolicy::EarlyClinch), (3, ClinchPolicy::PlayAll)] {
            let plan = plan(games, clinch);
            let mut facts = initial(&plan);
            for fact in &mut facts {
                fact.state = SeriesGameState::Skipped;
            }
            assert_eq!(
                next_action(&plan, players(), &facts),
                Err(SeriesError::IncoherentFacts)
            );
        }

        let plan = plan(3, ClinchPolicy::EarlyClinch);
        let mut facts = initial(&plan);
        facts[0].state = SeriesGameState::Completed(GameOutcome::draw());
        facts[1].state = SeriesGameState::Skipped;
        facts[2].state = SeriesGameState::Skipped;
        assert_eq!(
            next_action(&plan, players(), &facts),
            Err(SeriesError::IncoherentFacts)
        );
    }

    #[test]
    fn at_most_two_sets_stays_in_the_same_phase_after_a_tie() {
        let plan = SeriesPlan {
            phases: vec![
                SeriesPhase {
                    games_per_set: NonZeroU16::new(2).unwrap(),
                    color_order: vec![EntrantSide::Second, EntrantSide::First],
                    set_limit: SetLimit::AtMost(NonZeroU16::new(2).unwrap()),
                    clinch: ClinchPolicy::PlayAll,
                },
                SeriesPhase {
                    games_per_set: NonZeroU16::new(1).unwrap(),
                    color_order: vec![EntrantSide::First],
                    set_limit: SetLimit::UntilDecisive,
                    clinch: ClinchPolicy::PlayAll,
                },
            ],
        };
        let mut facts = initial(&plan);
        facts
            .iter_mut()
            .for_each(|fact| fact.state = SeriesGameState::Completed(GameOutcome::draw()));
        let Some(SeriesAction::CreateSet { games, .. }) =
            next_action(&plan, players(), &facts).unwrap()
        else {
            panic!("next set")
        };
        assert!(
            games
                .iter()
                .all(|game| game.phase_index == 0 && game.set_index == 1)
        );
        assert_eq!(games[0].white, players()[0]);
        assert_eq!(games[1].white, players()[1]);
    }

    #[test]
    fn repeated_sets_invert_default_and_custom_color_orders() {
        let entrants = players();
        for (color_order, inverted_whites) in [
            (
                vec![EntrantSide::First, EntrantSide::Second],
                vec![entrants[1], entrants[0]],
            ),
            (
                vec![
                    EntrantSide::First,
                    EntrantSide::Second,
                    EntrantSide::Second,
                    EntrantSide::First,
                ],
                vec![entrants[1], entrants[0], entrants[0], entrants[1]],
            ),
        ] {
            let plan = SeriesPlan {
                phases: vec![SeriesPhase {
                    games_per_set: NonZeroU16::new(color_order.len() as u16).unwrap(),
                    color_order: color_order.clone(),
                    set_limit: SetLimit::UntilDecisive,
                    clinch: ClinchPolicy::PlayAll,
                }],
            };
            let mut facts = initial(&plan);
            facts.iter_mut().for_each(|fact| {
                fact.state = SeriesGameState::Completed(GameOutcome::draw());
            });

            let Some(SeriesAction::CreateSet { games, .. }) =
                next_action(&plan, entrants, &facts).unwrap()
            else {
                panic!("tied opening set must create the inverted second set")
            };
            assert_eq!(
                games.iter().map(|game| game.white).collect::<Vec<_>>(),
                inverted_whites,
            );
            facts.extend(games.into_iter().map(|game| SeriesGameFact {
                id: game.id,
                white: game.white,
                state: SeriesGameState::Completed(GameOutcome::draw()),
            }));
            let projection = project(&plan, entrants, &facts).unwrap();
            assert_eq!(projection.sets.len(), 2);
            assert_eq!(
                projection.sets[1]
                    .games
                    .iter()
                    .map(|game| game.participants[0])
                    .collect::<Vec<_>>(),
                inverted_whites,
            );

            let Some(SeriesAction::CreateSet { games, .. }) =
                next_action(&plan, entrants, &facts).unwrap()
            else {
                panic!("tied second set must create the original third-set order")
            };
            assert_eq!(
                games.iter().map(|game| game.white).collect::<Vec<_>>(),
                color_order
                    .iter()
                    .map(|side| white_for(entrants, *side))
                    .collect::<Vec<_>>(),
            );
        }
    }

    #[test]
    fn play_all_releases_the_suffix_even_after_an_early_score_clinch() {
        let plan = plan(3, ClinchPolicy::PlayAll);
        let mut facts = initial(&plan);
        facts[0].state = SeriesGameState::Completed(GameOutcome::white_win());
        facts[1].state = SeriesGameState::Completed(GameOutcome::black_win());
        assert_eq!(
            next_action(&plan, players(), &facts).unwrap(),
            Some(SeriesAction::Release { game: facts[2].id })
        );
    }

    #[test]
    fn maximum_set_size_has_a_representable_doubled_score() {
        let plan = plan(u16::MAX, ClinchPolicy::PlayAll);
        let mut facts = initial(&plan);
        for fact in &mut facts {
            fact.state = SeriesGameState::Completed(if fact.white == players()[0] {
                GameOutcome::white_win()
            } else {
                GameOutcome::black_win()
            });
        }
        let projection = project(&plan, players(), &facts).unwrap();
        assert_eq!(projection.score.first, u64::from(u16::MAX) * 2);
    }

    #[test]
    fn validation_rejects_runtime_id_overflow() {
        let overflow = SeriesPlan {
            phases: vec![
                SeriesPhase {
                    games_per_set: NonZeroU16::new(u16::MAX).unwrap(),
                    color_order: (0..u16::MAX)
                        .map(|index| {
                            if index % 2 == 0 {
                                EntrantSide::First
                            } else {
                                EntrantSide::Second
                            }
                        })
                        .collect(),
                    set_limit: SetLimit::AtMost(NonZeroU16::new(u16::MAX).unwrap()),
                    clinch: ClinchPolicy::PlayAll,
                },
                SeriesPhase {
                    games_per_set: NonZeroU16::new(u16::MAX).unwrap(),
                    color_order: (0..u16::MAX)
                        .map(|index| {
                            if index % 2 == 0 {
                                EntrantSide::First
                            } else {
                                EntrantSide::Second
                            }
                        })
                        .collect(),
                    set_limit: SetLimit::AtMost(NonZeroU16::new(1).unwrap()),
                    clinch: ClinchPolicy::PlayAll,
                },
                SeriesPhase {
                    games_per_set: NonZeroU16::new(1).unwrap(),
                    color_order: vec![EntrantSide::First],
                    set_limit: SetLimit::AtMost(NonZeroU16::new(1).unwrap()),
                    clinch: ClinchPolicy::PlayAll,
                },
                SeriesPhase {
                    games_per_set: NonZeroU16::new(u16::MAX).unwrap(),
                    color_order: (0..u16::MAX)
                        .map(|index| {
                            if index % 2 == 0 {
                                EntrantSide::First
                            } else {
                                EntrantSide::Second
                            }
                        })
                        .collect(),
                    set_limit: SetLimit::UntilDecisive,
                    clinch: ClinchPolicy::PlayAll,
                },
            ],
        };
        assert_eq!(overflow.validate(), Err(SeriesError::ValueOutOfRange));
    }

    #[test]
    fn validation_requires_one_balanced_inventory_per_set() {
        let mut invalid_length = plan(2, ClinchPolicy::PlayAll);
        invalid_length.phases[0].color_order.pop();
        assert_eq!(
            invalid_length.validate(),
            Err(SeriesError::InvalidColorOrderLength { phase: 0 })
        );

        let mut invalid_inventory = plan(4, ClinchPolicy::PlayAll);
        invalid_inventory.phases[0].color_order = vec![
            EntrantSide::First,
            EntrantSide::First,
            EntrantSide::First,
            EntrantSide::Second,
        ];
        assert_eq!(
            invalid_inventory.validate(),
            Err(SeriesError::UnbalancedColorOrder { phase: 0 })
        );

        let odd = plan(3, ClinchPolicy::PlayAll);
        assert_eq!(odd.validate(), Ok(()));
    }
}
