//! Immutable round-robin schedules and pure result projections.
//!
//! The complete schedule is fixed at construction, and each projection is
//! calculated from the supplied result facts. Single passes use Berger pairings and colours.
//! Repeated passes reverse colours, with the final two rounds exchanged in every nonfinal pass
//! to improve colour sequences at pass boundaries. For two passes this follows the Berger
//! recommendation; applying it to further passes is this crate's deterministic extension.

use core::cmp::Ordering;
use core::num::NonZeroU32;
use std::collections::{HashMap, HashSet};

use crate::standings::{
    CriterionValue, DirectEncounterForfeitPolicy, ExactRatio, RepeatedEncounterPolicy,
    SonnebornBergerContribution, StandingsError, StandingsPlan, StandingsTable,
    TournamentPairingNumberOrder, apply_criteria, partition_ascending, partition_descending,
    sum_sonneborn_berger_contributions,
};
use crate::{
    AdjudicatedSideResult, Color, GameOutcome, GameSideOutcome, MatchDisposition, MatchPointSystem,
    MatchScore, PlayerId, PointSystem, Score,
};

mod matches;
pub use matches::{RoundRobinCompletedMatchProjection, RoundRobinMatchProjection};
use matches::{ScoredEncounter, project_matches};

/// Score used for primary standings and score-based round-robin tiebreaks.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
pub enum RoundRobinPrimaryScore {
    /// Points earned from individual games.
    #[default]
    GamePoints,
    /// Points earned from the combined result of all games against an opponent.
    MatchPoints,
}

/// Stable identity of one scheduled round-robin game.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(transparent)
)]
pub struct RoundRobinGameId(usize);

impl RoundRobinGameId {
    /// Recreates a stored schedule identifier.
    pub const fn new(value: usize) -> Self {
        Self(value)
    }

    /// Zero-based identifier assigned in schedule order.
    pub const fn value(self) -> usize {
        self.0
    }
}

/// Immutable configuration for a round-robin definition.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(deny_unknown_fields)
)]
pub struct RoundRobinConfig {
    /// Number of complete passes over the field.
    pub repeats: u32,
    /// Orientation of the Berger table: White uses its listed colours, and Black reverses them.
    /// With an odd field the highest table number is a resting slot, so the first participant
    /// rests in the first round of the canonical pass rather than receiving a colour.
    pub initial_color: Color,
    /// Semantic game scoring.
    pub point_system: PointSystem,
    /// Score used for primary standings and score-based tiebreaks.
    pub primary_score: RoundRobinPrimaryScore,
    /// Aggregate awards; available only with a positive even number of repeats.
    pub match_point_system: MatchPointSystem,
    /// Default standings projection.
    pub standings: RoundRobinStandingsPlan,
}

impl RoundRobinConfig {
    /// Whether all games against each opponent form a color-balanced match.
    pub const fn has_matches(&self) -> bool {
        self.repeats > 0 && self.repeats.is_multiple_of(2)
    }

    fn primary_point_system(&self) -> PointSystem {
        match self.primary_score {
            RoundRobinPrimaryScore::GamePoints => self.point_system,
            RoundRobinPrimaryScore::MatchPoints => PointSystem {
                win: self.match_point_system.win,
                draw: self.match_point_system.draw,
                loss: self.match_point_system.loss,
                forfeit_loss: self.match_point_system.forfeit_loss,
            },
        }
    }
}

/// One game in the immutable schedule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(deny_unknown_fields)
)]
pub struct ScheduledRoundRobinGame {
    /// Stable game identity.
    pub id: RoundRobinGameId,
    /// Zero-based position in the complete repeated schedule.
    pub round_index: usize,
    /// Zero-based pass index.
    pub pass_index: usize,
    /// Zero-based board index among games in this round.
    pub board_index: usize,
    /// Scheduled White.
    pub white: PlayerId,
    /// Scheduled Black.
    pub black: PlayerId,
    /// The same matchup's game in the preceding pass, if one exists.
    pub previous_repeat: Option<RoundRobinGameId>,
    /// The same matchup's game in the following pass, if one exists.
    pub next_repeat: Option<RoundRobinGameId>,
}

/// One scheduled round within one pass.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoundRobinRound {
    /// Zero-based position in the complete repeated schedule.
    pub round_index: usize,
    /// Zero-based pass index.
    pub pass_index: usize,
    /// Scheduled games.
    pub games: Vec<ScheduledRoundRobinGame>,
    /// Participant resting in an odd field. A rest has no opponent, colour,
    /// game, or score.
    pub resting: Option<PlayerId>,
}

/// Complete immutable schedule.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoundRobinSchedule {
    /// Every round of every pass, in play order.
    pub rounds: Vec<RoundRobinRound>,
}

/// Options for round-robin Progressive Score.
///
/// Uses the configured primary score. Match points enter the running score
/// in the last scheduled round of each complete match. PS-C1 omits the cumulative score after global
/// scheduled round one while retaining round-one points in later totals. The
/// default is base Progressive without that cut.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(deny_unknown_fields)
)]
pub struct RoundRobinProgressiveOptions {
    /// Whether to apply Progressive Score Cut-1 (PS-C1).
    pub cut_first_round: bool,
}

/// Options for round-robin Direct Encounter.
///
/// Direct Encounter uses the configured primary score. Predetermined-pairing forfeits are included as scored
/// by default, and repeated encounters are averaged per opponent by default.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(deny_unknown_fields)
)]
pub struct RoundRobinDirectEncounterOptions {
    /// Treatment of administratively awarded games.
    pub forfeits: DirectEncounterForfeitPolicy,
    /// Treatment of repeated encounters with the same opponent.
    pub repeated_encounters: RepeatedEncounterPolicy,
}

impl Default for RoundRobinDirectEncounterOptions {
    fn default() -> Self {
        Self {
            forfeits: DirectEncounterForfeitPolicy::IncludeAsScored,
            repeated_encounters: RepeatedEncounterPolicy::Average,
        }
    }
}

/// Options for round-robin Sonneborn-Berger.
///
/// Uses the configured primary score and has no unplayed-round policy. Low cuts discard the
/// requested number of lowest normalized completed contributions. The default
/// applies no low cuts.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(deny_unknown_fields)
)]
pub struct RoundRobinSonnebornBergerOptions {
    /// Number of lowest normalized contributions to discard.
    pub cut_lowest: u32,
}

/// Options for the round-robin-only Koya criterion.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(deny_unknown_fields)
)]
pub struct KoyaOptions {
    /// Numerator of the opponent-score threshold relative to their maximum
    /// available score.
    pub threshold_numerator: u32,
    /// Non-zero denominator of the threshold.
    pub threshold_denominator: NonZeroU32,
    /// Whether administratively awarded games contribute.
    pub include_adjudicated: bool,
}

impl Default for KoyaOptions {
    fn default() -> Self {
        Self {
            threshold_numerator: 1,
            threshold_denominator: NonZeroU32::new(2).expect("two is non-zero"),
            include_adjudicated: true,
        }
    }
}

/// A configurable round-robin standings criterion.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
pub enum RoundRobinCriterion {
    /// Configured primary score.
    PrimaryScore,
    /// Points earned from individual games.
    GamePoints,
    /// Points earned from completed matches; requires even repeats.
    MatchPoints,
    /// Completed match wins, including full-match forfeit wins; requires even repeats.
    MatchesWon,
    /// One-based Tournament Pairing Number assigned by constructor order.
    ///
    /// The configured direction affects ranking only; the reported metric is
    /// always the same positive pairing number.
    TournamentPairingNumber(TournamentPairingNumberOrder),
    /// Progressive Score across every immutable scheduled round, optionally
    /// omitting only the cumulative score after global round one (PS-C1).
    Progressive(RoundRobinProgressiveOptions),
    /// Recursive subgroup-local Direct Encounter.
    ///
    /// For a partial result set, a project-defined extension computes exact
    /// lower and upper bounds for every participant. It splits only bound
    /// components whose order cannot be changed by any unresolved result and
    /// preserves every incomplete overlap as a tie for this criterion. Direct
    /// Encounter is recursively reapplied only to completed exact sub-ties.
    /// Adjudicated results are completed encounters; only an absent scheduled
    /// result creates uncertainty. FIDE Article 6.3 defines its analogous
    /// incomplete-table rule for Swiss tournaments, not round-robin.
    DirectEncounter(RoundRobinDirectEncounterOptions),
    /// Sonneborn-Berger using configured primary score, with optional iterative
    /// low cuts.
    SonnebornBerger(RoundRobinSonnebornBergerOptions),
    /// Round-robin-only Koya.
    Koya(KoyaOptions),
    /// Number of supplied results for which the participant's configured
    /// award equals the configured win award (WIN).
    NumberOfWins,
    /// Played wins plus adjudicated forfeit wins (WON).
    GamesWon,
    /// Played wins plus adjudicated forfeit wins while scheduled as Black
    /// (BWG).
    GamesWonWithBlack,
}

/// Ordered round-robin standings plan.
pub type RoundRobinStandingsPlan = StandingsPlan<RoundRobinCriterion>;

impl Default for RoundRobinStandingsPlan {
    fn default() -> Self {
        Self::new(vec![RoundRobinCriterion::PrimaryScore])
    }
}

/// Why an immutable round-robin definition could not be built.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RoundRobinError {
    /// Fewer than two participants were entered.
    #[error("a round robin needs at least two unique participants")]
    TooFewPlayers,
    /// The initial participant order contains a duplicate.
    #[error("participant {0:?} occurs more than once")]
    DuplicatePlayer(PlayerId),
    /// No complete pass was requested.
    #[error("a round robin needs at least one pass")]
    ZeroRepeats,
    /// Match scoring was requested without a positive even number of games per opponent.
    #[error("round-robin match points and match wins require an even number of games per opponent")]
    MatchScoringRequiresEvenRepeats,
    /// A configured schedule dimension overflowed.
    #[error("round-robin schedule dimensions overflowed")]
    DimensionOverflow,
}

/// One participant's complete Round Robin result summary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RoundRobinPlayerSummary {
    /// Participant owning this summary.
    pub player: PlayerId,
    /// Configured primary score.
    pub primary_score: Score,
    /// Points from every supplied game result.
    pub game_points: Score,
    /// Points from completed matches, or zero when matches are unavailable.
    pub match_points: Score,
    /// Completed regular matches, including split forfeits; absent for odd repeats.
    pub matches_played: Option<u32>,
    /// Completed match wins, including full-match forfeit wins.
    pub match_wins: u32,
    /// Completed regular match draws.
    pub match_draws: u32,
    /// Completed match losses, including full-match and double forfeits.
    pub match_losses: u32,
    /// Supplied results for games that actually began.
    pub games_played: u32,
    /// Semantic wins, including adjudicated wins.
    pub wins: u32,
    /// Semantic draws, including adjudicated draws.
    pub draws: u32,
    /// Semantic losses, including forfeits and double forfeits.
    pub losses: u32,
    /// Scheduled rests across all immutable passes.
    pub rests: u32,
}

/// One complete projection driven by the schedule's native game identities.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoundRobinProjection {
    /// Configured standings columns and positionally aligned row values.
    pub standings: StandingsTable<RoundRobinCriterion, CriterionValue>,
    /// Participant summaries in immutable constructor order.
    pub players: Vec<RoundRobinPlayerSummary>,
    /// Complete immutable rounds with any supplied result awards attached.
    pub rounds: Vec<RoundRobinRoundProjection>,
    /// One aggregate per opponent pair for even repeats; empty for odd repeats.
    pub matches: Vec<RoundRobinMatchProjection>,
}

impl RoundRobinProjection {
    /// Scheduled games without an outcome fact, in schedule order.
    pub fn unresolved_games(&self) -> impl Iterator<Item = RoundRobinGameId> + '_ {
        self.rounds
            .iter()
            .flat_map(|round| &round.games)
            .filter_map(|game| game.completed.is_none().then_some(game.scheduled_game.id))
    }

    /// Whether every scheduled logical game has an outcome fact.
    pub fn is_complete(&self) -> bool {
        self.unresolved_games().next().is_none()
    }
}

/// One projected round within one immutable pass.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoundRobinRoundProjection {
    /// Zero-based position in the complete repeated schedule.
    pub round_index: usize,
    /// Zero-based pass index.
    pub pass_index: usize,
    /// Participant resting in an odd field.
    pub resting: Option<PlayerId>,
    /// Scheduled logical games in board order.
    pub games: Vec<RoundRobinGameProjection>,
}

/// One scheduled logical game and its optional completed award.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RoundRobinGameProjection {
    /// Immutable schedule entry.
    pub scheduled_game: ScheduledRoundRobinGame,
    /// Effective result and awarded points, when supplied.
    pub completed: Option<RoundRobinCompletedGameProjection>,
}

/// Engine-owned result awards for a completed round-robin game.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RoundRobinCompletedGameProjection {
    /// Effective semantic outcome.
    pub outcome: GameOutcome,
    /// Configured awards in White, Black order.
    pub awarded_points: [Score; 2],
}

/// One host-owned effective result fact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RoundRobinGameFact {
    /// Native schedule identity.
    pub id: RoundRobinGameId,
    /// Current effective outcome.
    pub outcome: GameOutcome,
}

/// Why round-robin facts could not be projected.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RoundRobinProjectionError {
    /// The schedule definition is invalid.
    #[error(transparent)]
    Definition(#[from] RoundRobinError),
    /// A result names a game outside this schedule.
    #[error("round-robin result names unknown game {0:?}")]
    UnknownGame(RoundRobinGameId),
    /// At most one effective outcome may exist for a game.
    #[error("round-robin game {0:?} has more than one outcome fact")]
    DuplicateResult(RoundRobinGameId),
    /// Standings arithmetic or configuration failed.
    #[error(transparent)]
    Standings(#[from] StandingsError),
}

/// Generates the complete immutable schedule from configuration and players.
pub fn schedule(
    config: &RoundRobinConfig,
    players: &[PlayerId],
) -> Result<RoundRobinSchedule, RoundRobinError> {
    validate_config(config, players)?;
    build_schedule(config, players)
}

/// Projects standings and totals from immutable result facts.
pub fn project(
    config: &RoundRobinConfig,
    players: &[PlayerId],
    game_facts: &[RoundRobinGameFact],
) -> Result<RoundRobinProjection, RoundRobinProjectionError> {
    let definition = RoundRobinCalculation::new(config.clone(), players)?;
    let game_count = definition.scheduled_games().count();
    let mut outcomes = vec![None; game_count];
    for fact in game_facts {
        let Some(slot) = outcomes.get_mut(fact.id.value()) else {
            return Err(RoundRobinProjectionError::UnknownGame(fact.id));
        };
        if slot.replace(fact.outcome).is_some() {
            return Err(RoundRobinProjectionError::DuplicateResult(fact.id));
        }
    }
    definition.project_slots(outcomes).map_err(Into::into)
}

/// Private calculation context shared by scheduling and projection.
#[derive(Clone, Debug, Eq, PartialEq)]
struct RoundRobinCalculation {
    config: RoundRobinConfig,
    players: Vec<PlayerId>,
    schedule: RoundRobinSchedule,
}

impl RoundRobinCalculation {
    fn new(config: RoundRobinConfig, players: &[PlayerId]) -> Result<Self, RoundRobinError> {
        validate_config(&config, players)?;
        let schedule = build_schedule(&config, players)?;
        Ok(Self {
            config,
            players: players.to_vec(),
            schedule,
        })
    }

    fn scheduled_games(&self) -> impl Iterator<Item = &ScheduledRoundRobinGame> {
        self.schedule
            .rounds
            .iter()
            .flat_map(|round| round.games.iter())
    }

    fn project_slots(
        &self,
        slots: Vec<Option<GameOutcome>>,
    ) -> Result<RoundRobinProjection, StandingsError> {
        let mut slot_index = 0usize;
        let rounds = self
            .schedule
            .rounds
            .iter()
            .map(|round| {
                let games = round
                    .games
                    .iter()
                    .map(|&scheduled_game| {
                        let outcome = slots[slot_index];
                        slot_index += 1;
                        RoundRobinGameProjection {
                            scheduled_game,
                            completed: outcome.map(|outcome| RoundRobinCompletedGameProjection {
                                outcome,
                                awarded_points: [
                                    self.config.point_system.points_for(outcome.white()),
                                    self.config.point_system.points_for(outcome.black()),
                                ],
                            }),
                        }
                    })
                    .collect();
                RoundRobinRoundProjection {
                    round_index: round.round_index,
                    pass_index: round.pass_index,
                    resting: round.resting,
                    games,
                }
            })
            .collect();
        let matches = project_matches(self, &slots)?;
        let mut projected = ResultProjection::new(self, slots, &matches)?;
        let mut row_values = HashMap::with_capacity(self.players.len());
        for &player in &self.players {
            let values = self
                .config
                .standings
                .criteria
                .iter()
                .copied()
                .map(|criterion| projected.metric(self, criterion, player))
                .collect::<Result<Vec<_>, _>>()?;
            row_values.insert(player, values);
        }
        let groups = apply_criteria(
            &self.players,
            &self.config.standings.criteria,
            |_, criterion, group| projected.partition(self, criterion, group),
            |player| Ok(row_values[&player].clone()),
        )?;
        Ok(RoundRobinProjection {
            standings: StandingsTable {
                columns: self.config.standings.criteria.clone(),
                groups,
            },
            players: projected.player_summaries(self)?,
            matches,
            rounds,
        })
    }
}

fn validate_config(config: &RoundRobinConfig, players: &[PlayerId]) -> Result<(), RoundRobinError> {
    if players.len() < 2 {
        return Err(RoundRobinError::TooFewPlayers);
    }
    if config.repeats == 0 {
        return Err(RoundRobinError::ZeroRepeats);
    }
    if !config.has_matches()
        && (config.primary_score == RoundRobinPrimaryScore::MatchPoints
            || config.standings.criteria.iter().any(|criterion| {
                matches!(
                    criterion,
                    RoundRobinCriterion::MatchPoints | RoundRobinCriterion::MatchesWon
                )
            }))
    {
        return Err(RoundRobinError::MatchScoringRequiresEvenRepeats);
    }
    let mut seen = HashSet::with_capacity(players.len());
    for &player in players {
        if !seen.insert(player) {
            return Err(RoundRobinError::DuplicatePlayer(player));
        }
    }
    schedule_dimensions(players.len(), repeat_count(config.repeats)?)?;
    Ok(())
}

fn repeat_count(repeats: u32) -> Result<usize, RoundRobinError> {
    usize::try_from(repeats).map_err(|_| RoundRobinError::DimensionOverflow)
}

#[derive(Clone, Copy)]
struct ScheduleDimensions {
    effective_count: usize,
    rounds_per_pass: usize,
    total_rounds: usize,
    total_games: usize,
}

fn schedule_dimensions(
    player_count: usize,
    repeats: usize,
) -> Result<ScheduleDimensions, RoundRobinError> {
    let effective_count = player_count
        .checked_add(usize::from(!player_count.is_multiple_of(2)))
        .ok_or(RoundRobinError::DimensionOverflow)?;
    let rounds_per_pass = effective_count
        .checked_sub(1)
        .ok_or(RoundRobinError::DimensionOverflow)?;
    let total_rounds = rounds_per_pass
        .checked_mul(repeats)
        .ok_or(RoundRobinError::DimensionOverflow)?;
    let pairs = player_count
        .checked_mul(player_count - 1)
        .and_then(|value| value.checked_div(2))
        .ok_or(RoundRobinError::DimensionOverflow)?;
    let total_games = pairs
        .checked_mul(repeats)
        .ok_or(RoundRobinError::DimensionOverflow)?;
    Ok(ScheduleDimensions {
        effective_count,
        rounds_per_pass,
        total_rounds,
        total_games,
    })
}

fn berger_pairs(
    effective_count: usize,
    rounds_per_pass: usize,
) -> Result<Vec<Vec<(usize, usize)>>, RoundRobinError> {
    let rotating_count = effective_count - 1;
    let half = effective_count / 2;
    let mut rounds = Vec::with_capacity(rounds_per_pass);
    for round_index in 0..rounds_per_pass {
        let pivot = round_index
            .checked_mul(half)
            .ok_or(RoundRobinError::DimensionOverflow)?
            % rotating_count;
        let mut pairs = Vec::with_capacity(half);
        pairs.push(if round_index.is_multiple_of(2) {
            (pivot, rotating_count)
        } else {
            (rotating_count, pivot)
        });
        for offset in 1..half {
            let white = pivot
                .checked_add(offset)
                .ok_or(RoundRobinError::DimensionOverflow)?
                % rotating_count;
            let black = pivot
                .checked_add(rotating_count - offset)
                .ok_or(RoundRobinError::DimensionOverflow)?
                % rotating_count;
            pairs.push((white, black));
        }
        rounds.push(pairs);
    }
    Ok(rounds)
}

fn build_schedule(
    config: &RoundRobinConfig,
    players: &[PlayerId],
) -> Result<RoundRobinSchedule, RoundRobinError> {
    let repeats = repeat_count(config.repeats)?;
    let dimensions = schedule_dimensions(players.len(), repeats)?;
    let has_rest_slot = !players.len().is_multiple_of(2);
    let rest_slot = has_rest_slot.then_some(players.len());
    let single_pass = berger_pairs(dimensions.effective_count, dimensions.rounds_per_pass)?;

    let mut rounds: Vec<RoundRobinRound> = Vec::with_capacity(dimensions.total_rounds);
    let mut next_game_id = 0usize;
    let mut previous_games: HashMap<(usize, usize), (usize, usize)> = HashMap::new();

    for pass_index in 0..repeats {
        for pass_round_index in 0..dimensions.rounds_per_pass {
            // The final pass keeps the canonical order. Earlier passes exchange their last
            // two rounds, including at the boundaries of tournaments with more than two passes.
            let source_round_index = if pass_index + 1 < repeats
                && dimensions.rounds_per_pass >= 2
                && pass_round_index >= dimensions.rounds_per_pass - 2
            {
                if pass_round_index == dimensions.rounds_per_pass - 2 {
                    dimensions.rounds_per_pass - 1
                } else {
                    dimensions.rounds_per_pass - 2
                }
            } else {
                pass_round_index
            };
            let pairs = &single_pass[source_round_index];
            let round_index = pass_index
                .checked_mul(dimensions.rounds_per_pass)
                .and_then(|base| base.checked_add(pass_round_index))
                .ok_or(RoundRobinError::DimensionOverflow)?;
            let mut games = Vec::with_capacity(pairs.len());
            let mut resting = None;
            for &(a, b) in pairs {
                if rest_slot.is_some_and(|slot| a == slot || b == slot) {
                    let player_index = if Some(a) == rest_slot { b } else { a };
                    resting = Some(players[player_index]);
                    continue;
                }
                let first_takes_white = match config.initial_color {
                    Color::White => pass_index.is_multiple_of(2),
                    Color::Black => !pass_index.is_multiple_of(2),
                };
                let (white_index, black_index) = if first_takes_white { (a, b) } else { (b, a) };
                let id = RoundRobinGameId(next_game_id);
                let matchup = (a.min(b), a.max(b));
                let previous_repeat = previous_games.get(&matchup).map(|&(round, board)| {
                    let previous = &mut rounds[round].games[board];
                    previous.next_repeat = Some(id);
                    previous.id
                });
                previous_games.insert(matchup, (round_index, games.len()));
                let scheduled = ScheduledRoundRobinGame {
                    id,
                    round_index,
                    pass_index,
                    board_index: games.len(),
                    white: players[white_index],
                    black: players[black_index],
                    previous_repeat,
                    next_repeat: None,
                };
                games.push(scheduled);
                next_game_id = next_game_id
                    .checked_add(1)
                    .ok_or(RoundRobinError::DimensionOverflow)?;
            }
            rounds.push(RoundRobinRound {
                round_index,
                pass_index,
                games,
                resting,
            });
        }
    }

    debug_assert_eq!(next_game_id, dimensions.total_games);
    Ok(RoundRobinSchedule { rounds })
}

struct ResultProjection {
    encounters: Vec<ScoredEncounter>,
    game_points: Vec<u64>,
    match_points: Vec<u64>,
    matches_played: Vec<u32>,
    matches_won: Vec<u32>,
    matches_drawn: Vec<u32>,
    matches_lost: Vec<u32>,
    player_index: HashMap<PlayerId, usize>,
    points: Vec<u64>,
    number_of_wins: Vec<u32>,
    games_played: Vec<u32>,
    games_won: Vec<u32>,
    games_drawn: Vec<u32>,
    games_lost: Vec<u32>,
    games_won_with_black: Vec<u32>,
    rests: Vec<u32>,
    sonneborn_berger_by_options: HashMap<RoundRobinSonnebornBergerOptions, Vec<u64>>,
    koya_by_options: HashMap<KoyaOptions, Vec<u64>>,
    progressive: Option<ProgressiveScores>,
    #[cfg(test)]
    sonneborn_berger_computations: usize,
    #[cfg(test)]
    koya_computations: usize,
    #[cfg(test)]
    progressive_computations: usize,
}

struct ProgressiveScores {
    base: Vec<u64>,
    cut_first_round: Vec<u64>,
}

impl ResultProjection {
    fn new(
        definition: &RoundRobinCalculation,
        slots: Vec<Option<GameOutcome>>,
        matches: &[RoundRobinMatchProjection],
    ) -> Result<Self, StandingsError> {
        let player_index: HashMap<_, _> = definition
            .players
            .iter()
            .copied()
            .enumerate()
            .map(|(index, player)| (player, index))
            .collect();
        let mut points = vec![0u64; definition.players.len()];
        let mut number_of_wins = vec![0u32; definition.players.len()];
        let mut games_played = vec![0u32; definition.players.len()];
        let mut games_won = vec![0u32; definition.players.len()];
        let mut games_drawn = vec![0u32; definition.players.len()];
        let mut games_lost = vec![0u32; definition.players.len()];
        let mut games_won_with_black = vec![0u32; definition.players.len()];
        let mut rests = vec![0u32; definition.players.len()];

        for resting in definition
            .schedule
            .rounds
            .iter()
            .filter_map(|round| round.resting)
        {
            increment_count(&mut rests[player_index[&resting]])?;
        }

        for (game, outcome) in definition.scheduled_games().zip(slots.iter().copied()) {
            let Some(outcome) = outcome else { continue };
            let white_index = player_index[&game.white];
            let black_index = player_index[&game.black];
            let white_side = outcome.white();
            let black_side = outcome.black();
            if outcome.counts_as_played() {
                increment_count(&mut games_played[white_index])?;
                increment_count(&mut games_played[black_index])?;
            }
            let white_points = definition.config.point_system.points_for(white_side);
            let black_points = definition.config.point_system.points_for(black_side);
            points[white_index] = points[white_index]
                .checked_add(u64::from(white_points.value()))
                .ok_or(StandingsError::ArithmeticOverflow)?;
            points[black_index] = points[black_index]
                .checked_add(u64::from(black_points.value()))
                .ok_or(StandingsError::ArithmeticOverflow)?;
            if white_points == definition.config.point_system.win {
                number_of_wins[white_index] = number_of_wins[white_index]
                    .checked_add(1)
                    .ok_or(StandingsError::ArithmeticOverflow)?;
            }
            if black_points == definition.config.point_system.win {
                number_of_wins[black_index] = number_of_wins[black_index]
                    .checked_add(1)
                    .ok_or(StandingsError::ArithmeticOverflow)?;
            }
            match white_side.score() {
                MatchScore::Win => increment_count(&mut games_won[white_index])?,
                MatchScore::Draw => increment_count(&mut games_drawn[white_index])?,
                MatchScore::Loss => increment_count(&mut games_lost[white_index])?,
            }
            match black_side.score() {
                MatchScore::Win => {
                    increment_count(&mut games_won[black_index])?;
                    increment_count(&mut games_won_with_black[black_index])?;
                }
                MatchScore::Draw => increment_count(&mut games_drawn[black_index])?,
                MatchScore::Loss => increment_count(&mut games_lost[black_index])?,
            }
        }

        let mut match_points = vec![0u64; definition.players.len()];
        let mut matches_played = vec![0u32; definition.players.len()];
        let mut matches_won = vec![0u32; definition.players.len()];
        let mut matches_drawn = vec![0u32; definition.players.len()];
        let mut matches_lost = vec![0u32; definition.players.len()];
        for encounter in matches {
            let Some(completed) = encounter.completed else {
                continue;
            };
            for (side, player) in encounter.participants.into_iter().enumerate() {
                let index = player_index[&player];
                match_points[index] = match_points[index]
                    .checked_add(u64::from(completed.match_points[side].value()))
                    .ok_or(StandingsError::ArithmeticOverflow)?;
                if completed.dispositions[side] == MatchDisposition::Regular {
                    increment_count(&mut matches_played[index])?;
                }
                let result = match completed.dispositions[side] {
                    MatchDisposition::Regular => completed.aggregate[side],
                    MatchDisposition::ForfeitWin => MatchScore::Win,
                    MatchDisposition::ForfeitLoss | MatchDisposition::DoubleForfeit => {
                        MatchScore::Loss
                    }
                };
                match result {
                    MatchScore::Win => increment_count(&mut matches_won[index])?,
                    MatchScore::Draw => increment_count(&mut matches_drawn[index])?,
                    MatchScore::Loss => increment_count(&mut matches_lost[index])?,
                }
            }
        }
        let encounters = ScoredEncounter::project(definition, &slots, matches);
        let game_points = points;
        let points = match definition.config.primary_score {
            RoundRobinPrimaryScore::GamePoints => game_points.clone(),
            RoundRobinPrimaryScore::MatchPoints => match_points.clone(),
        };
        Ok(Self {
            encounters,
            game_points,
            match_points,
            matches_played,
            matches_won,
            matches_drawn,
            matches_lost,
            player_index,
            points,
            number_of_wins,
            games_played,
            games_won,
            games_drawn,
            games_lost,
            games_won_with_black,
            rests,
            sonneborn_berger_by_options: HashMap::new(),
            koya_by_options: HashMap::new(),
            progressive: None,
            #[cfg(test)]
            sonneborn_berger_computations: 0,
            #[cfg(test)]
            koya_computations: 0,
            #[cfg(test)]
            progressive_computations: 0,
        })
    }

    fn player_summaries(
        &self,
        definition: &RoundRobinCalculation,
    ) -> Result<Vec<RoundRobinPlayerSummary>, StandingsError> {
        definition
            .players
            .iter()
            .copied()
            .map(|player| {
                let index = self.player_index[&player];
                Ok(RoundRobinPlayerSummary {
                    player,
                    primary_score: score_from_total(self.points[index])?,
                    game_points: score_from_total(self.game_points[index])?,
                    match_points: score_from_total(self.match_points[index])?,
                    matches_played: definition
                        .config
                        .has_matches()
                        .then_some(self.matches_played[index]),
                    match_wins: self.matches_won[index],
                    match_draws: self.matches_drawn[index],
                    match_losses: self.matches_lost[index],
                    games_played: self.games_played[index],
                    wins: self.games_won[index],
                    draws: self.games_drawn[index],
                    losses: self.games_lost[index],
                    rests: self.rests[index],
                })
            })
            .collect()
    }

    fn partition(
        &mut self,
        definition: &RoundRobinCalculation,
        criterion: &RoundRobinCriterion,
        group: &[PlayerId],
    ) -> Result<Vec<Vec<PlayerId>>, StandingsError> {
        Ok(match criterion {
            RoundRobinCriterion::PrimaryScore => {
                partition_descending(group, |player| self.points[self.player_index[&player]])
            }
            RoundRobinCriterion::GamePoints => {
                partition_descending(group, |player| self.game_points[self.player_index[&player]])
            }
            RoundRobinCriterion::MatchPoints => partition_descending(group, |player| {
                self.match_points[self.player_index[&player]]
            }),
            RoundRobinCriterion::MatchesWon => {
                partition_descending(group, |player| self.matches_won[self.player_index[&player]])
            }
            RoundRobinCriterion::TournamentPairingNumber(order) => match order {
                TournamentPairingNumberOrder::Ascending => {
                    partition_ascending(group, |player| self.player_index[&player])
                }
                TournamentPairingNumberOrder::Descending => {
                    partition_descending(group, |player| self.player_index[&player])
                }
            },
            RoundRobinCriterion::Progressive(options) => {
                self.ensure_progressive(definition)?;
                let values = self
                    .progressive
                    .as_ref()
                    .expect("Progressive Score values were prepared")
                    .values(*options);
                let player_index = &self.player_index;
                partition_descending(group, |player| values[player_index[&player]])
            }
            RoundRobinCriterion::DirectEncounter(options) => {
                return self.direct_encounter(definition, group, *options);
            }
            RoundRobinCriterion::SonnebornBerger(options) => {
                self.ensure_sonneborn_berger(definition, *options)?;
                let values = self
                    .sonneborn_berger_by_options
                    .get(options)
                    .expect("Sonneborn-Berger values were prepared");
                let player_index = &self.player_index;
                partition_descending(group, |player| values[player_index[&player]])
            }
            RoundRobinCriterion::Koya(options) => {
                self.ensure_koya(definition, *options)?;
                let values = self
                    .koya_by_options
                    .get(options)
                    .expect("Koya values were prepared");
                let player_index = &self.player_index;
                partition_descending(group, |player| values[player_index[&player]])
            }
            RoundRobinCriterion::NumberOfWins => partition_descending(group, |player| {
                self.number_of_wins[self.player_index[&player]]
            }),
            RoundRobinCriterion::GamesWon => {
                partition_descending(group, |player| self.games_won[self.player_index[&player]])
            }
            RoundRobinCriterion::GamesWonWithBlack => partition_descending(group, |player| {
                self.games_won_with_black[self.player_index[&player]]
            }),
        })
    }

    fn direct_encounter(
        &self,
        definition: &RoundRobinCalculation,
        group: &[PlayerId],
        options: RoundRobinDirectEncounterOptions,
    ) -> Result<Vec<Vec<PlayerId>>, StandingsError> {
        let group_set: HashSet<_> = group.iter().copied().collect();
        let mut encounters = DirectEncounterTable::new();

        for encounter in &self.encounters {
            let [first, second] = encounter.participants;
            if !group_set.contains(&first) || !group_set.contains(&second) {
                continue;
            }
            let Some(completed) = encounter.completed else {
                for pair in [(first, second), (second, first)] {
                    let aggregate = encounters.entry(pair).or_default();
                    aggregate.unresolved = aggregate
                        .unresolved
                        .checked_add(1)
                        .ok_or(StandingsError::ArithmeticOverflow)?;
                }
                continue;
            };
            if options.forfeits == DirectEncounterForfeitPolicy::Exclude && completed.adjudicated {
                continue;
            }
            for (side, pair) in [(first, second), (second, first)].into_iter().enumerate() {
                let aggregate = encounters.entry(pair).or_default();
                aggregate.points = aggregate
                    .points
                    .checked_add(u64::from(completed.points[side].value()))
                    .ok_or(StandingsError::ArithmeticOverflow)?;
                aggregate.included = aggregate
                    .included
                    .checked_add(1)
                    .ok_or(StandingsError::ArithmeticOverflow)?;
            }
        }

        Self::partition_direct_encounter(
            group,
            options,
            definition.config.primary_point_system(),
            &encounters,
        )
    }

    fn partition_direct_encounter(
        group: &[PlayerId],
        options: RoundRobinDirectEncounterOptions,
        point_system: PointSystem,
        encounters: &DirectEncounterTable,
    ) -> Result<Vec<Vec<PlayerId>>, StandingsError> {
        // Repeated-encounter averages are calculated per opponent before those
        // contributions are added. Bounds therefore preserve the same
        // denominator semantics when only some repeats are unresolved or an
        // adjudicated encounter may be excluded.
        let mut player_bounds = Vec::with_capacity(group.len());
        for &player in group {
            let mut bounds = ExactBounds::ZERO;
            let mut complete = true;
            for &opponent in group {
                if opponent == player {
                    continue;
                }
                let tally = encounters
                    .get(&(player, opponent))
                    .copied()
                    .unwrap_or_default();
                complete &= tally.unresolved == 0;
                bounds =
                    bounds.checked_add(direct_encounter_bounds(tally, options, point_system)?)?;
            }
            player_bounds.push((player, bounds, complete));
        }

        // Closed score intervals form ordered components. Sorting by the upper
        // endpoint ensures that once the current component's lowest possible
        // value is strictly above the next interval's upper endpoint, every
        // later interval is also below that component. Touching or transitively
        // overlapping intervals remain together.
        let mut comparison_error = None;
        player_bounds.sort_by(|(_, left, _), (_, right, _)| {
            match right.upper.checked_cmp(left.upper) {
                Ok(ordering) => ordering,
                Err(error) => {
                    comparison_error = Some(error);
                    Ordering::Equal
                }
            }
        });
        if let Some(error) = comparison_error {
            return Err(error);
        }

        let mut partitions: Vec<(Vec<PlayerId>, bool)> = Vec::new();
        let mut component_lower: Option<ExactRatio> = None;
        for (player, bounds, complete) in player_bounds {
            let starts_group = match component_lower {
                None => true,
                Some(lower) => lower.checked_cmp(bounds.upper)? == Ordering::Greater,
            };
            if starts_group {
                partitions.push((Vec::new(), complete));
                component_lower = Some(bounds.lower);
            } else {
                component_lower = Some(
                    component_lower
                        .expect("an existing component has a lower bound")
                        .checked_min(bounds.lower)?,
                );
                partitions
                    .last_mut()
                    .expect("the first interval starts a component")
                    .1 &= complete;
            }
            partitions
                .last_mut()
                .expect("the first interval starts a component")
                .0
                .push(player);
        }

        if partitions.len() == 1 {
            return Ok(vec![group.to_vec()]);
        }
        let presentation_order: HashMap<_, _> = group
            .iter()
            .enumerate()
            .map(|(index, player)| (*player, index))
            .collect();
        let mut recursive = Vec::new();
        for (mut partition, complete) in partitions {
            partition.sort_by_key(|player| presentation_order[player]);
            if partition.len() > 1 && complete {
                recursive.extend(Self::partition_direct_encounter(
                    &partition,
                    options,
                    point_system,
                    encounters,
                )?);
            } else {
                // An incomplete component exists because its closed score
                // intervals overlap. Reapplying Direct Encounter to that
                // component would discard the uncertainty that made it one
                // component and could fabricate an order from provisional
                // totals. Later configured criteria may still split it.
                recursive.push(partition);
            }
        }
        Ok(recursive)
    }

    fn ensure_sonneborn_berger(
        &mut self,
        definition: &RoundRobinCalculation,
        options: RoundRobinSonnebornBergerOptions,
    ) -> Result<(), StandingsError> {
        if self.sonneborn_berger_by_options.contains_key(&options) {
            return Ok(());
        }
        let values = self.compute_sonneborn_berger(definition, options);
        let values = values?;
        self.sonneborn_berger_by_options.insert(options, values);
        #[cfg(test)]
        {
            self.sonneborn_berger_computations += 1;
        }
        Ok(())
    }

    fn ensure_progressive(
        &mut self,
        definition: &RoundRobinCalculation,
    ) -> Result<(), StandingsError> {
        if self.progressive.is_some() {
            return Ok(());
        }

        let player_count = definition.players.len();
        let mut running = vec![0u64; player_count];
        let mut base = vec![0u64; player_count];
        let mut cut_first_round = vec![0u64; player_count];
        let mut encounters = self.encounters.iter().peekable();
        for round in &definition.schedule.rounds {
            while encounters
                .peek()
                .is_some_and(|encounter| encounter.round_index == round.round_index)
            {
                let encounter = encounters.next().expect("peeked encounter exists");
                let Some(completed) = encounter.completed else {
                    continue;
                };
                for (side, player) in encounter.participants.into_iter().enumerate() {
                    let index = self.player_index[&player];
                    running[index] = running[index]
                        .checked_add(u64::from(completed.points[side].value()))
                        .ok_or(StandingsError::ArithmeticOverflow)?;
                }
            }

            for index in 0..player_count {
                base[index] = base[index]
                    .checked_add(running[index])
                    .ok_or(StandingsError::ArithmeticOverflow)?;
                if round.round_index != 0 {
                    cut_first_round[index] = cut_first_round[index]
                        .checked_add(running[index])
                        .ok_or(StandingsError::ArithmeticOverflow)?;
                }
            }
        }

        self.progressive = Some(ProgressiveScores {
            base,
            cut_first_round,
        });
        #[cfg(test)]
        {
            self.progressive_computations += 1;
        }
        Ok(())
    }

    fn compute_sonneborn_berger(
        &self,
        definition: &RoundRobinCalculation,
        options: RoundRobinSonnebornBergerOptions,
    ) -> Result<Vec<u64>, StandingsError> {
        let mut contributions = vec![Vec::new(); definition.players.len()];
        for (order, encounter) in self.encounters.iter().enumerate() {
            let Some(completed) = encounter.completed else {
                continue;
            };
            let white_index = self.player_index[&encounter.participants[0]];
            let black_index = self.player_index[&encounter.participants[1]];
            let white_earned = u64::from(completed.points[0].value());
            let black_earned = u64::from(completed.points[1].value());
            let black_score = self.points[black_index];
            let white_score = self.points[white_index];
            contributions[white_index].push(SonnebornBergerContribution {
                opponent_score: black_score,
                product: white_earned
                    .checked_mul(black_score)
                    .ok_or(StandingsError::ArithmeticOverflow)?,
                vur: false,
                order,
            });
            contributions[black_index].push(SonnebornBergerContribution {
                opponent_score: white_score,
                product: black_earned
                    .checked_mul(white_score)
                    .ok_or(StandingsError::ArithmeticOverflow)?,
                vur: false,
                order,
            });
        }
        contributions
            .into_iter()
            .map(|player| {
                sum_sonneborn_berger_contributions(
                    player,
                    usize::try_from(options.cut_lowest)
                        .map_err(|_| StandingsError::ArithmeticOverflow)?,
                    false,
                )
            })
            .collect()
    }

    fn ensure_koya(
        &mut self,
        definition: &RoundRobinCalculation,
        options: KoyaOptions,
    ) -> Result<(), StandingsError> {
        if self.koya_by_options.contains_key(&options) {
            return Ok(());
        }
        let values = self.compute_koya(definition, options);
        let values = values?;
        self.koya_by_options.insert(options, values);
        #[cfg(test)]
        {
            self.koya_computations += 1;
        }
        Ok(())
    }

    fn compute_koya(
        &self,
        definition: &RoundRobinCalculation,
        options: KoyaOptions,
    ) -> Result<Vec<u64>, StandingsError> {
        let points = definition.config.primary_point_system();
        let maximum_score = u64::from(
            points
                .win
                .max(points.draw)
                .max(points.loss)
                .max(points.forfeit_loss)
                .value(),
        );
        let opponent_count = u64::try_from(definition.players.len() - 1)
            .map_err(|_| StandingsError::ArithmeticOverflow)?;
        let encounters_per_opponent = match definition.config.primary_score {
            RoundRobinPrimaryScore::GamePoints => definition.config.repeats,
            RoundRobinPrimaryScore::MatchPoints => 1,
        };
        let maximum_total = maximum_score
            .checked_mul(opponent_count)
            .and_then(|value| value.checked_mul(u64::from(encounters_per_opponent)))
            .ok_or(StandingsError::ArithmeticOverflow)?;
        let threshold_left =
            |score: u64| score.checked_mul(u64::from(options.threshold_denominator.get()));
        let threshold_right = maximum_total
            .checked_mul(u64::from(options.threshold_numerator))
            .ok_or(StandingsError::ArithmeticOverflow)?;
        let mut values = vec![0u64; definition.players.len()];
        for encounter in &self.encounters {
            let Some(completed) = encounter.completed else {
                continue;
            };
            if !options.include_adjudicated && completed.adjudicated {
                continue;
            }
            for side in 0..2 {
                let index = self.player_index[&encounter.participants[side]];
                let opponent_index = self.player_index[&encounter.participants[1 - side]];
                if threshold_left(self.points[opponent_index])
                    .ok_or(StandingsError::ArithmeticOverflow)?
                    >= threshold_right
                {
                    values[index] = values[index]
                        .checked_add(u64::from(completed.points[side].value()))
                        .ok_or(StandingsError::ArithmeticOverflow)?;
                }
            }
        }
        Ok(values)
    }

    fn metric(
        &mut self,
        definition: &RoundRobinCalculation,
        criterion: RoundRobinCriterion,
        player: PlayerId,
    ) -> Result<CriterionValue, StandingsError> {
        let index = self.player_index[&player];
        Ok(match criterion {
            RoundRobinCriterion::PrimaryScore => {
                CriterionValue::Score(score_from_total(self.points[index])?)
            }
            RoundRobinCriterion::GamePoints => {
                CriterionValue::Score(score_from_total(self.game_points[index])?)
            }
            RoundRobinCriterion::MatchPoints => {
                CriterionValue::Score(score_from_total(self.match_points[index])?)
            }
            RoundRobinCriterion::MatchesWon => {
                CriterionValue::Integer(u64::from(self.matches_won[index]))
            }
            RoundRobinCriterion::TournamentPairingNumber(_) => CriterionValue::Integer(
                u64::try_from(index)
                    .map_err(|_| StandingsError::ArithmeticOverflow)?
                    .checked_add(1)
                    .ok_or(StandingsError::ArithmeticOverflow)?,
            ),
            RoundRobinCriterion::Progressive(options) => {
                self.ensure_progressive(definition)?;
                CriterionValue::Integer(
                    self.progressive
                        .as_ref()
                        .expect("Progressive Score values were prepared")
                        .values(options)[index],
                )
            }
            RoundRobinCriterion::DirectEncounter(_) => CriterionValue::NotApplicable,
            RoundRobinCriterion::SonnebornBerger(options) => {
                self.ensure_sonneborn_berger(definition, options)?;
                CriterionValue::Integer(self.sonneborn_berger_by_options[&options][index])
            }
            RoundRobinCriterion::Koya(options) => {
                self.ensure_koya(definition, options)?;
                CriterionValue::Score(score_from_total(self.koya_by_options[&options][index])?)
            }
            RoundRobinCriterion::NumberOfWins => {
                CriterionValue::Integer(u64::from(self.number_of_wins[index]))
            }
            RoundRobinCriterion::GamesWon => {
                CriterionValue::Integer(u64::from(self.games_won[index]))
            }
            RoundRobinCriterion::GamesWonWithBlack => {
                CriterionValue::Integer(u64::from(self.games_won_with_black[index]))
            }
        })
    }
}

impl ProgressiveScores {
    fn values(&self, options: RoundRobinProgressiveOptions) -> &[u64] {
        if options.cut_first_round {
            &self.cut_first_round
        } else {
            &self.base
        }
    }
}

#[derive(Clone, Copy, Default)]
struct DirectEncounterTally {
    points: u64,
    included: u64,
    unresolved: u64,
}

type DirectEncounterTable = HashMap<(PlayerId, PlayerId), DirectEncounterTally>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ExactBounds {
    lower: ExactRatio,
    upper: ExactRatio,
}

impl ExactBounds {
    const ZERO: Self = Self {
        lower: ExactRatio::ZERO,
        upper: ExactRatio::ZERO,
    };

    fn checked_add(self, other: Self) -> Result<Self, StandingsError> {
        Ok(Self {
            lower: self.lower.checked_add(other.lower)?,
            upper: self.upper.checked_add(other.upper)?,
        })
    }
}

fn direct_encounter_bounds(
    tally: DirectEncounterTally,
    options: RoundRobinDirectEncounterOptions,
    point_system: PointSystem,
) -> Result<ExactBounds, StandingsError> {
    let played_min = u64::from(
        [point_system.win, point_system.draw, point_system.loss]
            .into_iter()
            .map(Score::value)
            .min()
            .expect("played outcomes are non-empty"),
    );
    let played_max = u64::from(
        [point_system.win, point_system.draw, point_system.loss]
            .into_iter()
            .map(Score::value)
            .max()
            .expect("played outcomes are non-empty"),
    );
    let included_max = if options.forfeits == DirectEncounterForfeitPolicy::IncludeAsScored {
        played_max.max(u64::from(point_system.forfeit_loss.value()))
    } else {
        played_max
    };

    match options.repeated_encounters {
        RepeatedEncounterPolicy::Sum => {
            let upper = tally
                .unresolved
                .checked_mul(included_max)
                .and_then(|additional| tally.points.checked_add(additional))
                .ok_or(StandingsError::ArithmeticOverflow)?;
            Ok(ExactBounds {
                // An unresolved game can become a double forfeit when included,
                // or an excluded adjudication under the Exclude policy.
                lower: ExactRatio::integer(tally.points),
                upper: ExactRatio::integer(upper),
            })
        }
        RepeatedEncounterPolicy::Average if tally.unresolved == 0 => {
            let exact = if tally.included == 0 {
                ExactRatio::ZERO
            } else {
                ExactRatio::new(tally.points, tally.included)
            };
            Ok(ExactBounds {
                lower: exact,
                upper: exact,
            })
        }
        RepeatedEncounterPolicy::Average
            if options.forfeits == DirectEncounterForfeitPolicy::IncludeAsScored =>
        {
            let denominator = tally
                .included
                .checked_add(tally.unresolved)
                .ok_or(StandingsError::ArithmeticOverflow)?;
            let upper = tally
                .unresolved
                .checked_mul(included_max)
                .and_then(|additional| tally.points.checked_add(additional))
                .ok_or(StandingsError::ArithmeticOverflow)?;
            Ok(ExactBounds {
                lower: ExactRatio::new(tally.points, denominator),
                upper: ExactRatio::new(upper, denominator),
            })
        }
        RepeatedEncounterPolicy::Average => {
            // Under Exclude, every unresolved game may either be an excluded
            // adjudication or a played result. The average is monotone in the
            // number of equal-valued additions, so its extrema occur with none
            // included or with every unresolved game included at the minimum
            // or maximum played award.
            let current = if tally.included == 0 {
                ExactRatio::ZERO
            } else {
                ExactRatio::new(tally.points, tally.included)
            };
            let denominator = tally
                .included
                .checked_add(tally.unresolved)
                .ok_or(StandingsError::ArithmeticOverflow)?;
            let with_minimum = tally
                .unresolved
                .checked_mul(played_min)
                .and_then(|additional| tally.points.checked_add(additional))
                .ok_or(StandingsError::ArithmeticOverflow)?;
            let with_maximum = tally
                .unresolved
                .checked_mul(played_max)
                .and_then(|additional| tally.points.checked_add(additional))
                .ok_or(StandingsError::ArithmeticOverflow)?;
            Ok(ExactBounds {
                lower: current.checked_min(ExactRatio::new(with_minimum, denominator))?,
                upper: current.checked_max(ExactRatio::new(with_maximum, denominator))?,
            })
        }
    }
}

fn score_from_total(total: u64) -> Result<Score, StandingsError> {
    u32::try_from(total)
        .map(Score::new)
        .map_err(|_| StandingsError::ArithmeticOverflow)
}

fn increment_count(count: &mut u32) -> Result<(), StandingsError> {
    *count = count
        .checked_add(1)
        .ok_or(StandingsError::ArithmeticOverflow)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_game_links_lookup_and_projection_share_one_schedule_identity() {
        let players = [PlayerId::new(0), PlayerId::new(1)];
        let config = RoundRobinConfig {
            primary_score: Default::default(),
            match_point_system: MatchPointSystem::STANDARD,
            repeats: 2,
            initial_color: Color::White,
            point_system: PointSystem::STANDARD,
            standings: RoundRobinStandingsPlan::new(vec![RoundRobinCriterion::PrimaryScore]),
        };
        let schedule = schedule(&config, &players).unwrap();
        let first = schedule.rounds[0].games[0];
        let second = schedule.rounds[1].games[0];

        assert_eq!(first.next_repeat, Some(second.id));
        assert_eq!(first.previous_repeat, None);
        assert_eq!(second.previous_repeat, Some(first.id));
        assert_eq!(second.next_repeat, None);
        let projection = project(
            &config,
            &players,
            &[RoundRobinGameFact {
                id: first.id,
                outcome: GameOutcome::white_win(),
            }],
        )
        .unwrap();
        assert_eq!(
            projection.unresolved_games().collect::<Vec<_>>(),
            vec![second.id]
        );
        assert_eq!(
            projection.rounds[first.round_index].games[first.board_index].completed,
            Some(RoundRobinCompletedGameProjection {
                outcome: GameOutcome::white_win(),
                awarded_points: [Score::new(2), Score::new(0)],
            })
        );
        assert_eq!(
            projection.standings.columns,
            vec![RoundRobinCriterion::PrimaryScore]
        );
        let white = projection
            .players
            .iter()
            .find(|summary| summary.player == first.white)
            .unwrap();
        assert_eq!(white.games_played, 1);
        assert_eq!(white.wins, 1);
        assert_eq!(white.draws, 0);
        assert_eq!(white.losses, 0);
    }

    #[test]
    fn recursive_direct_encounter_reuses_root_tallies() {
        let [a, b, c] = [PlayerId::new(0), PlayerId::new(1), PlayerId::new(2)];
        let tally = |points| DirectEncounterTally {
            points,
            included: 2,
            unresolved: 0,
        };
        let encounters = DirectEncounterTable::from([
            ((a, b), tally(4)),
            ((b, a), tally(0)),
            ((a, c), tally(2)),
            ((c, a), tally(2)),
            ((b, c), tally(3)),
            ((c, b), tally(1)),
        ]);

        for repeated_encounters in [
            RepeatedEncounterPolicy::Average,
            RepeatedEncounterPolicy::Sum,
        ] {
            let options = RoundRobinDirectEncounterOptions {
                forfeits: DirectEncounterForfeitPolicy::IncludeAsScored,
                repeated_encounters,
            };
            assert_eq!(
                ResultProjection::partition_direct_encounter(
                    &[a, b, c],
                    options,
                    PointSystem::STANDARD,
                    &encounters,
                ),
                Ok(vec![vec![a], vec![b], vec![c]])
            );
        }
    }

    #[test]
    fn incomplete_average_bounds_handle_non_monotone_scoring_and_exclusions() {
        let tally = DirectEncounterTally {
            points: 4,
            included: 2,
            unresolved: 1,
        };
        let point_system = PointSystem {
            win: Score::new(1),
            draw: Score::new(4),
            loss: Score::new(3),
            forfeit_loss: Score::new(5),
        };
        let options = |forfeits, repeated_encounters| RoundRobinDirectEncounterOptions {
            forfeits,
            repeated_encounters,
        };

        assert_eq!(
            direct_encounter_bounds(
                tally,
                options(
                    DirectEncounterForfeitPolicy::IncludeAsScored,
                    RepeatedEncounterPolicy::Average,
                ),
                point_system,
            )
            .unwrap(),
            ExactBounds {
                lower: ExactRatio::new(4, 3),
                upper: ExactRatio::integer(3),
            }
        );
        assert_eq!(
            direct_encounter_bounds(
                tally,
                options(
                    DirectEncounterForfeitPolicy::Exclude,
                    RepeatedEncounterPolicy::Average,
                ),
                point_system,
            )
            .unwrap(),
            ExactBounds {
                lower: ExactRatio::new(5, 3),
                upper: ExactRatio::new(8, 3),
            }
        );
        assert_eq!(
            direct_encounter_bounds(
                tally,
                options(
                    DirectEncounterForfeitPolicy::IncludeAsScored,
                    RepeatedEncounterPolicy::Sum,
                ),
                point_system,
            )
            .unwrap(),
            ExactBounds {
                lower: ExactRatio::integer(4),
                upper: ExactRatio::integer(9),
            }
        );
        assert_eq!(
            direct_encounter_bounds(
                tally,
                options(
                    DirectEncounterForfeitPolicy::Exclude,
                    RepeatedEncounterPolicy::Sum,
                ),
                point_system,
            )
            .unwrap(),
            ExactBounds {
                lower: ExactRatio::integer(4),
                upper: ExactRatio::integer(8),
            }
        );
    }
}
