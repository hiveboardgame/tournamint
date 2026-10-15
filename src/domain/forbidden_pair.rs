use std::ops::Range;

use crate::domain::PlayerId;

/// A group of players who must not be paired with each other during a
/// zero-based half-open round range, registered before that range begins.
///
/// The same borrowed shape serves both directions: it is what callers push
/// into a Swiss pairing snapshot and what reading one back
/// hands out.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ForbiddenPairEntry<'a> {
    pub(crate) round_range: Range<usize>,
    pub(crate) players: &'a [PlayerId],
}

/// The owned form the ephemeral Swiss pairing snapshot stores.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ForbiddenPair {
    round_range: Range<usize>,
    players: Vec<PlayerId>,
}

impl ForbiddenPair {
    pub(crate) fn from_entry(entry: ForbiddenPairEntry<'_>) -> Self {
        Self {
            round_range: entry.round_range,
            players: entry.players.to_vec(),
        }
    }

    pub(crate) fn entry(&self) -> ForbiddenPairEntry<'_> {
        ForbiddenPairEntry {
            round_range: self.round_range.clone(),
            players: &self.players,
        }
    }
}
