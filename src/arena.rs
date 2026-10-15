//! Pure Arena pairing, scoring, and standings from host-owned facts.
//!
//! # Lichess-inspired boundary
//!
//! The fixed 2/1/0 points/performance ranking, fire, berserk,
//! quick/repeated-draw scoring, rematch cooldown, colour strike, and rank/rating
//! pairing quality are based on lila commit
//! `60e225b83429835e08f7175539013f37f5eb5e4e`
//! (`Player.scala`, `arena/ArenaSheet.scala`, `arena/AntmaPairing.scala`, and
//! `TournamentApi.scala`). This is a supported subset, not a complete or
//! continuously synchronized Lichess implementation.
//! The default configuration supplies stable scoring thresholds; callers may
//! replace those thresholds without changing the policy shape.
//!
//! [`project`] returns the shared [`crate::standings::StandingsTable`] shape
//! plus participant-id-ordered metrics; [`player_metrics`] omits ranking. [`pair`]
//! derives a pairing wave and its ranks from current waiting facts. All three
//! consume previously awarded games and require player ids to form the exact
//! dense prefix `0..players.len()`. [`award_game`] scores one new result from
//! semantic history without a roster or standings and accepts sparse ids.
//!
//! Hosts distinguish active entrants from the currently waiting pool. Unpaused,
//! non-withdrawn entrants count as active while online or still playing; offline
//! idle entrants do not. Only when exactly two entrants are active may recent
//! opponents meet again, and both must have waited the configured rematch
//! interval. Fresh opponents need no minimum wait. Inactive entrants remain in
//! the facts and historical standings.
//! The bounded rematch wait is this crate's policy, not a copy of Lichess's
//! separate waiting-pool batching rule.
//!
//! The default [`ArenaPairingPolicy::Grouped`] policy deterministically orders
//! waiters by longest wait, fixed Arena rank, and dense player-id order. It
//! solves private bounded groups exactly for game count and then pairing
//! quality before an exact leftover-repair pass. The bound is not a participant
//! cap. Fixed group pairs are not reconsidered, so no complete-pool global
//! optimum is claimed. Caller-controlled rank/rating cost overflow remains a
//! pairing error. [`ArenaPairingPolicy::Grouped`] never
//! silently falls back to [`ArenaPairingPolicy::Greedy`].
//!
//! Other deliberate deviations are no team-battle, bot-specific draw, or
//! variant-specific long-game rules.

mod calculation;
mod config;
mod pairing;
mod player;
mod scoring;
mod standings;
mod weights;

use crate::standings::StandingsTable;
use crate::{Pairing, PlayedGameOutcome, PlayerId, Score};
pub use config::{
    ArenaBerserkPolicy, ArenaConfig, ArenaPairingPolicy, ArenaQuickDrawPolicy,
    ArenaRepeatedDrawPolicy, ArenaScoringConfig, ArenaSetupError, DEFAULT_BERSERK_BONUS,
    DEFAULT_BERSERK_MINIMUM_BONUS_PLIES, DEFAULT_MAX_COLOR_STRIKE,
    DEFAULT_QUICK_DRAW_MINIMUM_SCORING_PLIES, DEFAULT_REMATCH_COOLDOWN, DEFAULT_REMATCH_WAIT_SECS,
    DEFAULT_REPEATED_DRAW_MINIMUM_UNSUPPRESSED_PLIES, MAX_REMATCH_WAIT_SECS, MIN_REMATCH_WAIT_SECS,
    ScoringThresholds,
};
pub use standings::{ArenaPerformance, ArenaStandingMetrics};

/// One participant available to pure Arena calculations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArenaPlayerFact {
    /// Dense Arena-local identity.
    pub player: PlayerId,
    /// Current Arena-relative rating supplied by the host.
    pub arena_rating: Option<i32>,
    /// Whether this entrant counts toward the active pairing field. Hosts keep
    /// unpaused, non-withdrawn entrants active while online or still playing,
    /// and exclude offline idle entrants. Inactive entrants retain their facts
    /// and history. This is distinct from being available to pair right now.
    pub active: bool,
    /// Milliseconds this player has been waiting, or `None` when the host does
    /// not currently offer them for pairing. `Some` requires `active` to be true.
    pub waiting_millis: Option<u64>,
}

/// Terminal scoring disposition of one Arena game.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArenaTerminalResult {
    /// A game began and reached a semantic board result.
    Played(PlayedGameOutcome),
    /// The game did not start because this participant was absent.
    NoStart {
        /// Participant who failed to start.
        absent: PlayerId,
    },
}

/// One semantic terminal game in settlement order, without its scoring award.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArenaTerminalGameFact {
    /// Exact participants and colours.
    pub pairing: Pairing,
    /// White's rating snapshot for this terminal game, used for Black's performance.
    pub white_rating: Option<i32>,
    /// Black's rating snapshot for this terminal game, used for White's performance.
    pub black_rating: Option<i32>,
    /// Played or no-start terminal result.
    pub result: ArenaTerminalResult,
    /// Played plies; ignored for no-start facts.
    pub plies: u32,
    /// Whether White declared berserk.
    pub white_berserked: bool,
    /// Whether Black declared berserk.
    pub black_berserked: bool,
}

/// A terminal game with its retained scoring decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArenaAwardedGameFact {
    /// Semantic result and participant snapshots.
    pub game: ArenaTerminalGameFact,
    /// Award computed when this game was settled; projections never rescore it.
    pub award: ArenaGameAward,
}

/// Complete Arena projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArenaProjection {
    /// Ranked exact tie groups.
    pub standings: StandingsTable<(), ()>,
    /// Metrics in dense player-id order.
    pub players: Vec<ArenaStandingMetrics>,
}

/// Engine-owned awards for one terminal Arena fact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArenaGameAward {
    /// Exact awarded points in White, Black order.
    pub awarded_points: [Score; 2],
    /// Whether fire doubled each side's base result points for this game.
    pub doubled: [bool; 2],
}

/// A pairing wave and the ranks used to select it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArenaPairing {
    /// Selected games for the current waiting pool.
    pub pairings: Vec<Pairing>,
    /// One-based competition ranks in dense player-id order.
    pub ranks: Vec<u32>,
}

/// Invalid Arena configuration or facts.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ArenaError {
    /// Immutable Arena configuration is invalid.
    #[error("invalid Arena configuration")]
    InvalidConfig,
    /// Player ids, active/waiting eligibility, pairings, or terminal results are incoherent.
    #[error("Arena facts are incoherent")]
    IncoherentFacts,
    /// Caller-controlled pairing or score arithmetic overflowed.
    #[error("Arena calculation overflowed")]
    ArithmeticOverflow,
}

/// Selects pairings and returns the ranks used for the current waiting pool.
pub fn pair(
    config: &ArenaConfig,
    players: &[ArenaPlayerFact],
    terminal_games: &[ArenaAwardedGameFact],
) -> Result<ArenaPairing, ArenaError> {
    calculation::pair_facts(config, players, terminal_games)
}

/// Projects scores and standings from previously awarded games.
pub fn project(
    config: &ArenaConfig,
    players: &[ArenaPlayerFact],
    terminal_games: &[ArenaAwardedGameFact],
) -> Result<ArenaProjection, ArenaError> {
    calculation::project_facts(config, players, terminal_games)
}

/// Aggregates player statistics from awarded games without computing rankings.
pub fn player_metrics(
    config: &ArenaConfig,
    players: &[ArenaPlayerFact],
    terminal_games: &[ArenaAwardedGameFact],
) -> Result<Vec<ArenaStandingMetrics>, ArenaError> {
    calculation::player_metrics(config, players, terminal_games)
}

/// Awards one newly completed game from its participants' prior results.
///
/// `prior_games` must contain the complete earlier history of both participants
/// in settlement order; unrelated games may be included. Player ids may be
/// sparse and no roster is required. Ratings do not affect the new award:
/// streaks follow semantic results, including no-start wins/losses. The new
/// `game` must not be included in `prior_games`.
pub fn award_game(
    config: &ArenaConfig,
    prior_games: &[ArenaTerminalGameFact],
    game: &ArenaTerminalGameFact,
) -> Result<ArenaGameAward, ArenaError> {
    calculation::award_game_from_history(config, prior_games, game)
}
