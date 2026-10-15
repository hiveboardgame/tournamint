/// Tournament points in the exact integer unit used by the relevant format.
///
/// Deliberately an integer: formats with configurable
/// [`PointSystem`](crate::PointSystem) values express half-points by scaling
/// the whole system up, while formats with intrinsic scoring define their
/// integer unit directly. That keeps every score comparison exact.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(transparent)
)]
pub struct Score(u32);

impl Score {
    /// A score of `value` points.
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// The number of points.
    pub const fn value(self) -> u32 {
        self.0
    }
}

/// A player's place in the pairing order: score first, initial ranking as the
/// tie-break. Ordering *is* the pairing order - `Less` means "pairs earlier",
/// i.e. a higher score, or an equal score with a better initial rank.
///
/// Note the asymmetry: higher score is better, but *lower* rank index is
/// better, which is why this is a type with a derived `Ord` rather than a
/// hand-written comparison over four loose `usize`s.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct ScoreRank {
    score: core::cmp::Reverse<usize>,
    rank_index: usize,
}

impl ScoreRank {
    pub(crate) const fn new(score: usize, rank_index: usize) -> Self {
        Self {
            score: core::cmp::Reverse(score),
            rank_index,
        }
    }

    /// True when `self` pairs strictly later than `other`.
    pub(crate) fn pairs_after(self, other: Self) -> bool {
        self > other
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cmp::Ordering;

    #[test]
    fn score_rank_sorts_higher_score_then_lower_rank_first() {
        let order = |score0, rank0, score1, rank1| {
            ScoreRank::new(score0, rank0).cmp(&ScoreRank::new(score1, rank1))
        };

        assert_eq!(order(20, 5, 10, 0), Ordering::Less);
        assert_eq!(order(10, 0, 20, 5), Ordering::Greater);
        assert_eq!(order(10, 2, 10, 4), Ordering::Less);
        assert_eq!(order(10, 4, 10, 2), Ordering::Greater);
        assert_eq!(order(10, 2, 10, 2), Ordering::Equal);
    }

    #[test]
    fn pairs_after_is_strict() {
        let strong = ScoreRank::new(20, 5);
        let weak = ScoreRank::new(10, 0);

        assert!(weak.pairs_after(strong));
        assert!(!strong.pairs_after(weak));
        assert!(!strong.pairs_after(strong));
    }
}
