use crate::domain::{AdjudicatedSideResult, GameSideOutcome, MatchScore, Score};

/// Configured points for semantic game outcomes.
///
/// Scores are exact scaled integers: a 1 / ½ / 0 system is represented as
/// 2 / 1 / 0. Swiss bye awards are deliberately configured by the selected
/// Swiss system instead of leaking into this format-neutral type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(deny_unknown_fields)
)]
pub struct PointSystem {
    /// Points for a played win or an adjudicated forfeit win.
    pub win: Score,
    /// Points for a played or adjudicated draw.
    pub draw: Score,
    /// Points for a played loss.
    pub loss: Score,
    /// Points for an adjudicated forfeit loss.
    pub forfeit_loss: Score,
}

impl PointSystem {
    /// Ordinary chess scoring in half-point units.
    pub const STANDARD: Self = Self {
        win: Score::new(2),
        draw: Score::new(1),
        loss: Score::new(0),
        forfeit_loss: Score::new(0),
    };

    /// Returns the points awarded to one side of a semantic game outcome.
    /// A double forfeit is always zero, irrespective of `forfeit_loss`.
    pub const fn points_for(self, result: GameSideOutcome) -> Score {
        match result {
            GameSideOutcome::Played(MatchScore::Win)
            | GameSideOutcome::Adjudicated(AdjudicatedSideResult::ForfeitWin) => self.win,
            GameSideOutcome::Played(MatchScore::Draw)
            | GameSideOutcome::Adjudicated(AdjudicatedSideResult::Draw) => self.draw,
            GameSideOutcome::Played(MatchScore::Loss) => self.loss,
            GameSideOutcome::Adjudicated(AdjudicatedSideResult::ForfeitLoss) => self.forfeit_loss,
            GameSideOutcome::Adjudicated(AdjudicatedSideResult::DoubleForfeit) => Score::new(0),
        }
    }
}
