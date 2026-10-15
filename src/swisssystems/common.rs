//! Machinery shared by every Swiss system: the player projection they all
//! pair from, the FIDE colour rules, forbidden-pair lookup and bye eligibility.

mod bye;
mod colors;
mod context;
mod forbidden;
mod pairing;
mod projection;

pub(crate) use bye::{
    eligible_for_double_swiss_pairing_allocated_bye, eligible_for_single_game_pairing_allocated_bye,
};

pub(crate) use colors::{
    choose_player_neutral_color, color_preferences_are_compatible, first_color_difference,
    initial_color_for_rank, opposite_initial_color_for_rank, previous_played_color,
};

pub(crate) use forbidden::{
    BuildForbiddenPairMatrixError, ForbiddenPairMatrix, ForbiddenPairPlayer,
    build_forbidden_pair_matrix,
};

pub(crate) use context::PairingContext;
pub(crate) use pairing::pairing_from_player_color;

pub(crate) use projection::{
    BuildSwissPlayerProjectionsError, SwissPlayerProjection, build_swiss_player_projections,
    checked_accelerated_score, player_acceleration_at_round,
};
