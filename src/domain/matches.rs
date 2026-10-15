use crate::domain::MatchScore;

/// The result of a game that actually began.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
pub enum PlayedGameOutcome {
    /// White won.
    WhiteWin,
    /// The game was drawn.
    Draw,
    /// Black won.
    BlackWin,
}

/// One side's administrative result in a game that did not begin.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
pub enum AdjudicatedSideResult {
    /// Win awarded because the opponent forfeited.
    ForfeitWin,
    /// Draw awarded administratively.
    Draw,
    /// Loss awarded by forfeit.
    ForfeitLoss,
    /// This side receives zero as part of a mutual double forfeit.
    DoubleForfeit,
}

/// Why an adjudicated game outcome could not be constructed.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("the two adjudicated side results are not a supported combination")]
pub struct InvalidAdjudicatedGameOutcome;

/// A validated two-sided administrative result for a game that did not
/// begin.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdjudicatedGameOutcome {
    white: AdjudicatedSideResult,
    black: AdjudicatedSideResult,
}

#[cfg(feature = "serde")]
impl serde::Serialize for AdjudicatedGameOutcome {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct as _;
        let mut state = serializer.serialize_struct("AdjudicatedGameOutcome", 2)?;
        state.serialize_field("white", &self.white)?;
        state.serialize_field("black", &self.black)?;
        state.end()
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for AdjudicatedGameOutcome {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Repr {
            white: AdjudicatedSideResult,
            black: AdjudicatedSideResult,
        }
        let repr = Repr::deserialize(deserializer)?;
        Self::new(repr.white, repr.black).map_err(serde::de::Error::custom)
    }
}

impl AdjudicatedGameOutcome {
    /// Validates and constructs an adjudicated result.
    pub fn new(
        white: AdjudicatedSideResult,
        black: AdjudicatedSideResult,
    ) -> Result<Self, InvalidAdjudicatedGameOutcome> {
        let valid = matches!(
            (white, black),
            (
                AdjudicatedSideResult::ForfeitWin,
                AdjudicatedSideResult::ForfeitLoss
            ) | (
                AdjudicatedSideResult::ForfeitLoss,
                AdjudicatedSideResult::ForfeitWin
            ) | (AdjudicatedSideResult::Draw, AdjudicatedSideResult::Draw)
                | (
                    AdjudicatedSideResult::Draw,
                    AdjudicatedSideResult::ForfeitLoss
                )
                | (
                    AdjudicatedSideResult::ForfeitLoss,
                    AdjudicatedSideResult::Draw
                )
                | (
                    AdjudicatedSideResult::DoubleForfeit,
                    AdjudicatedSideResult::DoubleForfeit
                )
        );
        valid
            .then_some(Self { white, black })
            .ok_or(InvalidAdjudicatedGameOutcome)
    }

    /// White's awarded result.
    pub const fn white(self) -> AdjudicatedSideResult {
        self.white
    }

    /// Black's awarded result.
    pub const fn black(self) -> AdjudicatedSideResult {
        self.black
    }
}

/// A game's semantic result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GameOutcome {
    /// The game began and ended with a played win, draw, or loss. Timeout,
    /// resignation, and rules losses after play begins remain played results.
    Played(PlayedGameOutcome),
    /// The game did not begin and points were awarded administratively.
    Adjudicated(AdjudicatedGameOutcome),
}

/// One participant's view of a semantic game result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
pub enum GameSideOutcome {
    /// The game began; this is the participant's result.
    Played(MatchScore),
    /// The game did not begin; this is the participant's award.
    Adjudicated(AdjudicatedSideResult),
}

impl GameSideOutcome {
    /// Whether the game actually began.
    pub const fn counts_as_played(self) -> bool {
        matches!(self, Self::Played(_))
    }

    /// Win/draw/loss classification used by aggregate match formats.
    pub const fn score(self) -> MatchScore {
        match self {
            Self::Played(score) => score,
            Self::Adjudicated(AdjudicatedSideResult::ForfeitWin) => MatchScore::Win,
            Self::Adjudicated(AdjudicatedSideResult::Draw) => MatchScore::Draw,
            Self::Adjudicated(
                AdjudicatedSideResult::ForfeitLoss | AdjudicatedSideResult::DoubleForfeit,
            ) => MatchScore::Loss,
        }
    }
}

impl GameOutcome {
    /// White wins a game that began.
    pub const fn white_win() -> Self {
        Self::Played(PlayedGameOutcome::WhiteWin)
    }

    /// A game that began is drawn.
    pub const fn draw() -> Self {
        Self::Played(PlayedGameOutcome::Draw)
    }

    /// Black wins a game that began.
    pub const fn black_win() -> Self {
        Self::Played(PlayedGameOutcome::BlackWin)
    }

    /// White's view of the result.
    pub const fn white(self) -> GameSideOutcome {
        match self {
            Self::Played(PlayedGameOutcome::WhiteWin) => GameSideOutcome::Played(MatchScore::Win),
            Self::Played(PlayedGameOutcome::Draw) => GameSideOutcome::Played(MatchScore::Draw),
            Self::Played(PlayedGameOutcome::BlackWin) => GameSideOutcome::Played(MatchScore::Loss),
            Self::Adjudicated(result) => GameSideOutcome::Adjudicated(result.white()),
        }
    }

    /// Black's view of the result.
    pub const fn black(self) -> GameSideOutcome {
        match self {
            Self::Played(PlayedGameOutcome::WhiteWin) => GameSideOutcome::Played(MatchScore::Loss),
            Self::Played(PlayedGameOutcome::Draw) => GameSideOutcome::Played(MatchScore::Draw),
            Self::Played(PlayedGameOutcome::BlackWin) => GameSideOutcome::Played(MatchScore::Win),
            Self::Adjudicated(result) => GameSideOutcome::Adjudicated(result.black()),
        }
    }

    /// Whether the game actually began.
    pub const fn counts_as_played(self) -> bool {
        matches!(self, Self::Played(_))
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for GameOutcome {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(match self {
            Self::Played(PlayedGameOutcome::WhiteWin) => "played_white_win",
            Self::Played(PlayedGameOutcome::Draw) => "played_draw",
            Self::Played(PlayedGameOutcome::BlackWin) => "played_black_win",
            Self::Adjudicated(result) => match (result.white(), result.black()) {
                (AdjudicatedSideResult::ForfeitWin, AdjudicatedSideResult::ForfeitLoss) => {
                    "adjudicated_white_forfeit_win"
                }
                (AdjudicatedSideResult::ForfeitLoss, AdjudicatedSideResult::ForfeitWin) => {
                    "adjudicated_black_forfeit_win"
                }
                (AdjudicatedSideResult::Draw, AdjudicatedSideResult::Draw) => "adjudicated_draw",
                (AdjudicatedSideResult::Draw, AdjudicatedSideResult::ForfeitLoss) => {
                    "adjudicated_white_draw_black_forfeit_loss"
                }
                (AdjudicatedSideResult::ForfeitLoss, AdjudicatedSideResult::Draw) => {
                    "adjudicated_white_forfeit_loss_black_draw"
                }
                (AdjudicatedSideResult::DoubleForfeit, AdjudicatedSideResult::DoubleForfeit) => {
                    "adjudicated_double_forfeit"
                }
                _ => unreachable!("validated adjudicated outcome"),
            },
        })
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for GameOutcome {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error as _;
        let token = <String as serde::Deserialize>::deserialize(deserializer)?;
        let adjudicated = |white, black| {
            AdjudicatedGameOutcome::new(white, black)
                .map(Self::Adjudicated)
                .map_err(D::Error::custom)
        };
        match token.as_str() {
            "played_white_win" => Ok(Self::white_win()),
            "played_draw" => Ok(Self::draw()),
            "played_black_win" => Ok(Self::black_win()),
            "adjudicated_white_forfeit_win" => adjudicated(
                AdjudicatedSideResult::ForfeitWin,
                AdjudicatedSideResult::ForfeitLoss,
            ),
            "adjudicated_black_forfeit_win" => adjudicated(
                AdjudicatedSideResult::ForfeitLoss,
                AdjudicatedSideResult::ForfeitWin,
            ),
            "adjudicated_draw" => {
                adjudicated(AdjudicatedSideResult::Draw, AdjudicatedSideResult::Draw)
            }
            "adjudicated_white_draw_black_forfeit_loss" => adjudicated(
                AdjudicatedSideResult::Draw,
                AdjudicatedSideResult::ForfeitLoss,
            ),
            "adjudicated_white_forfeit_loss_black_draw" => adjudicated(
                AdjudicatedSideResult::ForfeitLoss,
                AdjudicatedSideResult::Draw,
            ),
            "adjudicated_double_forfeit" => adjudicated(
                AdjudicatedSideResult::DoubleForfeit,
                AdjudicatedSideResult::DoubleForfeit,
            ),
            _ => Err(D::Error::unknown_variant(
                &token,
                &[
                    "played_white_win",
                    "played_draw",
                    "played_black_win",
                    "adjudicated_white_forfeit_win",
                    "adjudicated_black_forfeit_win",
                    "adjudicated_draw",
                    "adjudicated_white_draw_black_forfeit_loss",
                    "adjudicated_white_forfeit_loss_black_draw",
                    "adjudicated_double_forfeit",
                ],
            )),
        }
    }
}
