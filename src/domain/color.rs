/// A concrete side assigned to a player in a scheduled board game.
///
/// Byes, rests, and absent colour preferences are represented structurally or
/// with [`Option<Color>`], never as a third colour.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
pub enum Color {
    /// The side that moves first.
    White,
    /// The side that moves second.
    Black,
}

impl Color {
    /// Returns the other concrete board side.
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::White => Self::Black,
            Self::Black => Self::White,
        }
    }
}

/// A player's colour history condensed into what the pairing rules actually
/// ask about: how lopsided their colours are so far, which colour they have
/// just had twice running, and which colour they are therefore owed.
///
/// Computed once per player by `swiss::player_data`, then carried unchanged
/// through every Swiss system, which is why it travels as one value rather
/// than four loose fields.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PlayerColorData {
    /// `|games as white - games as black|`.
    pub(crate) color_imbalance: usize,
    /// The colour this player should receive, or `None` if either is fine.
    pub(crate) color_preference: Option<Color>,
    /// The same real colour held in the last two colour-bearing rounds, or
    /// `None` otherwise.
    pub(crate) repeated_color: Option<Color>,
    pub(crate) strong_color_preference: bool,
}

impl PlayerColorData {
    /// A player with no played colour history: balanced, owed nothing.
    #[cfg(test)]
    pub(crate) const fn no_played_color_history() -> Self {
        Self {
            color_imbalance: 0,
            color_preference: None,
            repeated_color: None,
            strong_color_preference: false,
        }
    }

    /// FIDE C.04.1: an imbalance of more than one game is *absolute* - the
    /// player must receive the colour that reduces it.
    pub(crate) fn has_absolute_imbalance(self) -> bool {
        self.color_imbalance > 1
    }

    /// An absolute colour preference: either an absolute imbalance, or the
    /// same colour in each of the last two colour-bearing rounds.
    pub(crate) fn has_absolute_preference(self) -> bool {
        self.has_absolute_imbalance() || self.repeated_color.is_some()
    }
}
