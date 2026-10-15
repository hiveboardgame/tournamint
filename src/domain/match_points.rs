use crate::{MatchScore, Score};

/// One participant's disposition for a completed aggregate match.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Deserialize, serde::Serialize),
    serde(rename_all = "snake_case")
)]
pub enum MatchDisposition {
    /// An ordinary encounter. Partial and split forfeits are regular.
    Regular,
    /// The opponent forfeited every logical game in the match.
    ForfeitWin,
    /// This participant forfeited every logical game in the match.
    ForfeitLoss,
    /// Both participants forfeited every logical game in the match.
    DoubleForfeit,
}

impl MatchDisposition {
    pub(crate) const fn from_forfeits(forfeited_all: [bool; 2]) -> [Self; 2] {
        match forfeited_all {
            [true, true] => [Self::DoubleForfeit; 2],
            [true, false] => [Self::ForfeitLoss, Self::ForfeitWin],
            [false, true] => [Self::ForfeitWin, Self::ForfeitLoss],
            [false, false] => [Self::Regular; 2],
        }
    }
}

/// Match-level points used by aggregate match formats.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(deny_unknown_fields)
)]
pub struct MatchPointSystem {
    /// Points for a regular aggregate win or full-match forfeit win.
    pub win: Score,
    /// Points for a regular aggregate draw.
    pub draw: Score,
    /// Points for a regular aggregate loss.
    pub loss: Score,
    /// Points for a full-match forfeit loss.
    pub forfeit_loss: Score,
}

impl MatchPointSystem {
    /// Ordinary 2 / 1 / 0 match scoring.
    pub const STANDARD: Self = Self {
        win: Score::new(2),
        draw: Score::new(1),
        loss: Score::new(0),
        forfeit_loss: Score::new(0),
    };

    pub(crate) const fn regular_points(self, aggregate: MatchScore) -> Score {
        match aggregate {
            MatchScore::Win => self.win,
            MatchScore::Draw => self.draw,
            MatchScore::Loss => self.loss,
        }
    }

    pub(crate) const fn projected_points(
        self,
        disposition: MatchDisposition,
        aggregate: MatchScore,
    ) -> Score {
        match disposition {
            MatchDisposition::Regular => self.regular_points(aggregate),
            MatchDisposition::ForfeitWin => self.win,
            MatchDisposition::ForfeitLoss => self.forfeit_loss,
            MatchDisposition::DoubleForfeit => Score::new(0),
        }
    }
}
