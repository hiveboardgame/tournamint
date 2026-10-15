use std::collections::HashMap;

use crate::matching::{BigWeight, MatchingError, MatchingSolver};

use super::player::BracketSlot;
use super::status::BracketStatus;

/// Everything the Dutch bracket machinery knows about one score group when it
/// asks the weight implementation to price that group's candidate edges.
///
/// The last two fields serve the "single downfloater is the bye assignee"
/// priority band.
pub(crate) struct BaseWeightRequest<'a> {
    pub(crate) bracket: &'a [BracketSlot],
    pub(crate) score_group_begin: usize,
    pub(crate) next_score_group_begin: usize,
    pub(crate) bye_assignee_score: usize,
    pub(crate) score_group_size_bits: usize,
    pub(crate) score_groups_shift: usize,
    pub(crate) score_group_shifts: &'a HashMap<usize, usize>,
    pub(crate) is_single_downfloater_the_bye_assignee: bool,
    pub(crate) unplayed_game_ranks: &'a HashMap<usize, usize>,
}

/// Which way a player floated relative to their score group in a past round.
#[derive(Clone, Copy)]
pub(crate) enum FloatDirection {
    Down,
    Up,
    None,
}

/// What the bracket machinery cannot decide for itself: how Dutch prices
/// candidate edges.
///
/// Keeping the weight layout behind this interface separates rule priorities
/// from the bracket-and-exchange algorithm.
pub(crate) trait BracketSystem {
    fn base_edge_weights(
        &self,
        request: BaseWeightRequest<'_>,
    ) -> Result<Vec<Vec<BigWeight>>, BracketStatus>;
}

pub(crate) fn set_edge_weight(
    solver: &mut MatchingSolver,
    vertex0: usize,
    vertex1: usize,
    edge_weight: BigWeight,
) -> Result<(), BracketStatus> {
    match solver.set_big_edge_weight(vertex0, vertex1, edge_weight) {
        Ok(()) => Ok(()),
        Err(MatchingError::InvalidEdge) => Err(BracketStatus::MalformedTournament),
    }
}
