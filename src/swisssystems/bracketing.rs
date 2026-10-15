//! Dutch bracket-and-exchange pairing machinery, plus the low-level matching
//! utilities also used by Double-Swiss.
//!
//! Dutch builds score groups, matches them with the blossom solver, and falls
//! back to player exchanges when a group cannot be paired. Double-Swiss has a
//! separate C4-C8 search and reuses only the projections, status mapping, and
//! matching-edge helpers in this module.

mod brackets;
mod player;
mod solver;
mod status;
mod weights;

pub(crate) use brackets::{
    BracketContext, ByeContext, ScoreGroupShifts, build_score_group_shifts,
    compute_solver_matching, solve_score_groups,
};
pub(crate) use player::{BracketPlayer, BracketSlot, build_bracket_players};
pub(crate) use solver::{
    InitialSolverInput, build_initial_solver, build_pairing_result, validate_initial_matching,
};
pub(crate) use status::{
    BracketStatus, ComputeFailure, compute_failure, compute_matching_error_from_status,
};
pub(crate) use weights::{BaseWeightRequest, BracketSystem, FloatDirection, set_edge_weight};
