/// A player, as the engine refers to them.
///
/// Ids are host-assigned 0-based ordinals. Formats which return player-indexed
/// slices require the ids in their input field to form the dense range
/// `0..players.len()`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(transparent)
)]
pub struct PlayerId(usize);

impl PlayerId {
    /// Constructs a host-facing player ordinal.
    pub const fn new(value: usize) -> Self {
        Self(value)
    }

    /// The 0-based index this id stands for, for use against the per-player
    /// slices the engine returns.
    pub const fn index(self) -> usize {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct PlayerCount(usize);

impl PlayerCount {
    pub(crate) const fn new(value: usize) -> Self {
        Self(value)
    }

    pub(crate) const fn value(self) -> usize {
        self.0
    }
}
