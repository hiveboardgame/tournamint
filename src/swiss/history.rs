use crate::{
    AdjudicatedSideResult, Color, GameSideOutcome, MatchScore, PlayerId, PointSystem, Score,
};

pub use crate::MatchDisposition;

/// Canonical derived semantics for one side of a completed Swiss match.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MatchSideProjection {
    pub(crate) disposition: MatchDisposition,
    pub(crate) aggregate: MatchScore,
    pub(crate) game_points: Score,
}

/// Canonical derived semantics for both sides of a completed Swiss match.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SwissMatchProjection {
    pub(crate) player: MatchSideProjection,
    pub(crate) opponent: MatchSideProjection,
}

fn single_game_disposition(outcome: GameSideOutcome) -> MatchDisposition {
    match outcome {
        GameSideOutcome::Adjudicated(AdjudicatedSideResult::ForfeitWin) => {
            MatchDisposition::ForfeitWin
        }
        GameSideOutcome::Adjudicated(AdjudicatedSideResult::ForfeitLoss) => {
            MatchDisposition::ForfeitLoss
        }
        GameSideOutcome::Adjudicated(AdjudicatedSideResult::DoubleForfeit) => {
            MatchDisposition::DoubleForfeit
        }
        GameSideOutcome::Played(_) | GameSideOutcome::Adjudicated(AdjudicatedSideResult::Draw) => {
            MatchDisposition::Regular
        }
    }
}

fn single_game_aggregate(player: GameSideOutcome, opponent: GameSideOutcome) -> Option<MatchScore> {
    match (player, opponent) {
        (GameSideOutcome::Played(player), GameSideOutcome::Played(opponent))
            if player.invert() == opponent =>
        {
            Some(player)
        }
        (
            GameSideOutcome::Adjudicated(
                AdjudicatedSideResult::ForfeitWin | AdjudicatedSideResult::Draw,
            ),
            GameSideOutcome::Adjudicated(AdjudicatedSideResult::ForfeitLoss),
        ) => Some(MatchScore::Win),
        (
            GameSideOutcome::Adjudicated(AdjudicatedSideResult::ForfeitLoss),
            GameSideOutcome::Adjudicated(
                AdjudicatedSideResult::ForfeitWin | AdjudicatedSideResult::Draw,
            ),
        ) => Some(MatchScore::Loss),
        (
            GameSideOutcome::Adjudicated(AdjudicatedSideResult::Draw),
            GameSideOutcome::Adjudicated(AdjudicatedSideResult::Draw),
        )
        | (
            GameSideOutcome::Adjudicated(AdjudicatedSideResult::DoubleForfeit),
            GameSideOutcome::Adjudicated(AdjudicatedSideResult::DoubleForfeit),
        ) => Some(MatchScore::Draw),
        _ => None,
    }
}

/// Projects a validated, reciprocal single-game Swiss result.
///
/// `None` means the two side outcomes are not a supported reciprocal pair.
pub(crate) fn project_single_game_match(
    player: GameSideOutcome,
    opponent: GameSideOutcome,
    point_system: PointSystem,
) -> Option<SwissMatchProjection> {
    let player_aggregate = single_game_aggregate(player, opponent)?;

    Some(SwissMatchProjection {
        player: MatchSideProjection {
            disposition: single_game_disposition(player),
            aggregate: player_aggregate,
            game_points: point_system.points_for(player),
        },
        opponent: MatchSideProjection {
            disposition: single_game_disposition(opponent),
            aggregate: player_aggregate.invert(),
            game_points: point_system.points_for(opponent),
        },
    })
}

fn forfeited(game: GameSideOutcome) -> bool {
    matches!(
        game,
        GameSideOutcome::Adjudicated(
            AdjudicatedSideResult::ForfeitLoss | AdjudicatedSideResult::DoubleForfeit
        )
    )
}

/// Projects a completed Double-Swiss match, rejecting any logical game whose
/// two side outcomes are not reciprocal.
///
/// FIDE C.04.5 defines a full match forfeit only when a player forfeits both
/// games. Split forfeits and every other combination are regular.
pub(crate) fn project_double_swiss_match(
    player_games: [GameSideOutcome; 2],
    opponent_games: [GameSideOutcome; 2],
    point_system: PointSystem,
) -> Option<SwissMatchProjection> {
    let first = project_single_game_match(player_games[0], opponent_games[0], point_system)?;
    let second = project_single_game_match(player_games[1], opponent_games[1], point_system)?;
    let player_forfeited_both = player_games.into_iter().all(forfeited);
    let opponent_forfeited_both = opponent_games.into_iter().all(forfeited);
    let [player_disposition, opponent_disposition] =
        MatchDisposition::from_forfeits([player_forfeited_both, opponent_forfeited_both]);
    let player_points = Score::new(
        first
            .player
            .game_points
            .value()
            .checked_add(second.player.game_points.value())?,
    );
    let opponent_points = Score::new(
        first
            .opponent
            .game_points
            .value()
            .checked_add(second.opponent.game_points.value())?,
    );
    let player_aggregate = MatchScore::from_ordering(player_points.cmp(&opponent_points));

    Some(SwissMatchProjection {
        player: MatchSideProjection {
            disposition: player_disposition,
            aggregate: player_aggregate,
            game_points: player_points,
        },
        opponent: MatchSideProjection {
            disposition: opponent_disposition,
            aggregate: player_aggregate.invert(),
            game_points: opponent_points,
        },
    })
}

/// One participant's semantic result in a scheduled logical game.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogicalGame {
    /// This participant's scheduled colour.
    pub color: Color,
    /// This participant's played or adjudicated result.
    pub outcome: GameSideOutcome,
}

/// The initialized logical games in one Swiss encounter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogicalGames {
    games: [Option<LogicalGame>; 2],
}

impl LogicalGames {
    pub(crate) const fn empty() -> Self {
        Self {
            games: [None, None],
        }
    }

    pub(crate) const fn single(game: LogicalGame) -> Self {
        Self {
            games: [Some(game), None],
        }
    }

    pub(crate) const fn double(first: LogicalGame, second: LogicalGame) -> Self {
        Self {
            games: [Some(first), Some(second)],
        }
    }

    /// Number of initialized logical games.
    pub fn len(self) -> usize {
        self.games.iter().flatten().count()
    }

    /// Iterates only initialized games.
    pub fn iter(self) -> impl Iterator<Item = LogicalGame> {
        self.games.into_iter().flatten()
    }
}

/// Completion state of one participant's scheduled Swiss match.
///
/// Pending matches expose only the logical games received so far. A match
/// disposition and aggregate are created together only when every logical
/// game required by the selected Swiss system is present.
///
/// A completed Dutch or Burstein record contains one logical game; a completed
/// Double-Swiss record contains two. Values are derived from accepted facts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SwissMatchRecord {
    /// The scheduled match still needs one or more logical game results.
    Pending {
        /// Logical games received so far.
        games: LogicalGames,
    },
    /// Every required logical game has been received and classified.
    Complete {
        /// The complete system-required logical-game set.
        games: LogicalGames,
        /// Derived match disposition.
        disposition: MatchDisposition,
        /// Derived aggregate win, draw, or loss.
        aggregate: MatchScore,
    },
}

impl SwissMatchRecord {
    /// Logical games currently stored for this match.
    pub const fn games(self) -> LogicalGames {
        match self {
            Self::Pending { games } | Self::Complete { games, .. } => games,
        }
    }

    /// Coupled disposition and aggregate for a completed match.
    pub const fn completion(self) -> Option<(MatchDisposition, MatchScore)> {
        match self {
            Self::Pending { .. } => None,
            Self::Complete {
                disposition,
                aggregate,
                ..
            } => Some((disposition, aggregate)),
        }
    }

    pub(crate) fn complete_single(
        games: LogicalGames,
        disposition: MatchDisposition,
        aggregate: MatchScore,
    ) -> Option<Self> {
        Self::complete_with_count(games, disposition, aggregate, 1)
    }

    pub(crate) fn complete_double(
        games: LogicalGames,
        disposition: MatchDisposition,
        aggregate: MatchScore,
    ) -> Option<Self> {
        Self::complete_with_count(games, disposition, aggregate, 2)
    }

    fn complete_with_count(
        games: LogicalGames,
        disposition: MatchDisposition,
        aggregate: MatchScore,
        required_game_count: usize,
    ) -> Option<Self> {
        (games.len() == required_game_count).then_some(Self::Complete {
            games,
            disposition,
            aggregate,
        })
    }
}

/// One registered participant's projection for a Swiss round.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayerRoundRecord {
    /// A scheduled encounter.
    Match {
        /// Opponent.
        opponent: PlayerId,
        /// Colour scheduled for the first logical game.
        scheduled_first_color: Color,
        /// Pending or completed match semantics.
        state: SwissMatchRecord,
    },
    /// System-selected unpaired participant with a configured award.
    PairingAllocatedBye,
    /// Caller-requested unpaired participant; always scores zero.
    RequestedBye,
    /// Participant withdrawn before this round.
    Withdrawn,
}

impl PlayerRoundRecord {
    #[allow(dead_code)]
    pub(crate) fn games(self) -> LogicalGames {
        match self {
            Self::Match { state, .. } => state.games(),
            _ => LogicalGames::empty(),
        }
    }
}
