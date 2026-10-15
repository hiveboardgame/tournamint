use core::cmp::Ordering;

/// One player's win/draw/loss classification before the tournament format
/// maps it to points, either intrinsically or through a
/// [`PointSystem`](crate::PointSystem).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
pub enum MatchScore {
    /// This player lost.
    Loss,
    /// Neither player won.
    Draw,
    /// This player won.
    Win,
}

impl MatchScore {
    pub(crate) const fn from_ordering(ordering: Ordering) -> Self {
        match ordering {
            Ordering::Less => Self::Loss,
            Ordering::Equal => Self::Draw,
            Ordering::Greater => Self::Win,
        }
    }

    pub(crate) fn invert(self) -> Self {
        match self {
            Self::Loss => Self::Win,
            Self::Draw => Self::Draw,
            Self::Win => Self::Loss,
        }
    }
}
