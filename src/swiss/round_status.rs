/// Whether a round still has games outstanding, reported when a result is
/// recorded.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RoundStatus {
    /// At least one game of this round has no result yet.
    InProgress,
    /// Every game has a result; the next round can be paired.
    Complete,
}
