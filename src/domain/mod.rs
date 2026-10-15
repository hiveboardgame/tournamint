//! Shared tournament vocabulary: participants, semantic outcomes, pairings,
//! colours, scores, and point systems.
//! [`Color`] always names a concrete board side; formats represent byes,
//! rests, and absent preferences separately.
//!
//! Restructured from the internal representation in BBP Pairings (Apache-2.0,
//! Copyright 2016-2026 Jeremy Bierema), and modified - notably a typed
//! vocabulary (`Score`, `PlayerId`) in place of loose integers.

mod color;
mod forbidden_pair;
mod match_points;
mod match_score;
mod matches;
mod pairing;
mod player;
mod point_system;
mod score;

pub use color::Color;
pub(crate) use color::PlayerColorData;

pub use match_points::{MatchDisposition, MatchPointSystem};
pub use match_score::MatchScore;

pub(crate) use forbidden_pair::{ForbiddenPair, ForbiddenPairEntry};

pub(crate) use player::PlayerCount;
pub use player::PlayerId;

pub use matches::{
    AdjudicatedGameOutcome, AdjudicatedSideResult, GameOutcome, GameSideOutcome,
    InvalidAdjudicatedGameOutcome, PlayedGameOutcome,
};

pub use pairing::Pairing;

pub use point_system::PointSystem;

pub use score::Score;
pub(crate) use score::ScoreRank;
