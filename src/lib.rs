//! Pure tournament scheduling, pairing, progression, scoring, and standings.
//!
//! Every public operation takes immutable facts and returns a projection, a
//! proposed pairing, a node patch, or one optional atomic Series action. The
//! host owns persistence, lifecycle, physical games, clocks, and transactions.
//!
//! # A pure Swiss round
//!
//! ```
//! use tournamint::swiss::{
//!     AccelerationPolicy, DutchConfig, SingleGameSwissByePoints, SwissConfig,
//!     SwissFacts, SwissNextRound, SwissNextRoundInput, SwissStandingsPlan, SwissSystemConfig,
//! };
//! use tournamint::{Color, PlayerId, PointSystem};
//!
//! let players: Vec<_> = (0..4).map(PlayerId::new).collect();
//! let points = PointSystem::STANDARD;
//! let facts = SwissFacts {
//!     config: SwissConfig {
//!         system: SwissSystemConfig::Dutch(DutchConfig {
//!             bye_points: SingleGameSwissByePoints::game_win(points),
//!         }),
//!         expected_rounds: 3,
//!         initial_color: Color::White,
//!         game_point_system: points,
//!         acceleration: AccelerationPolicy::None,
//!         standings: SwissStandingsPlan::default(),
//!     },
//!     initial_ranking: players.clone(),
//!     rounds: vec![],
//! };
//! let next = tournamint::swiss::pair_next_round(
//!     &facts,
//!     &SwissNextRoundInput { eligible: players, ..Default::default() },
//! )?;
//! let SwissNextRound::Pairings(round) = next else {
//!     panic!("a fresh four-player field is pairable");
//! };
//! assert_eq!(round.games.len(), 2);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod acceleration;
mod domain;
mod limits;
mod matching;
mod swisssystems;

pub mod arena;
pub mod elimination;
pub mod round_robin;
pub mod series;
pub mod standings;
pub mod swiss;

// Common domain vocabulary reused by the formats where applicable.
pub use domain::{
    AdjudicatedGameOutcome, AdjudicatedSideResult, Color, GameOutcome, GameSideOutcome,
    InvalidAdjudicatedGameOutcome, MatchDisposition, MatchPointSystem, MatchScore, Pairing,
    PlayedGameOutcome, PlayerId, PointSystem, Score,
};
