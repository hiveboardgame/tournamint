//! Typed, ephemeral input shared by the Swiss pairing engines.

use crate::domain::{
    ForbiddenPair, ForbiddenPairEntry, PlayerColorData, PlayerId, PointSystem, Score,
};
use crate::swiss::{MatchPointSystem, PlayerRoundRecord, SwissMatchRecord};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SwissPrimaryScoreProjection {
    GamePoints,
    MatchPoints(MatchPointSystem),
}

impl SwissPrimaryScoreProjection {
    pub(crate) const fn select(self, game_points: Score, match_points: Score) -> Score {
        match self {
            Self::GamePoints => game_points,
            Self::MatchPoints(_) => match_points,
        }
    }
}

/// One participant's borrowed, typed input to the Swiss pairing engines.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SwissPairingPlayerEntry<'a> {
    pub(crate) id: PlayerId,
    pub(crate) is_valid: bool,
    pub(crate) rank_index: usize,
    pub(crate) score_without_acceleration: Score,
    pub(crate) played_games: usize,
    pub(crate) played_matches: usize,
    pub(crate) color_data: PlayerColorData,
    pub(crate) records: &'a [PlayerRoundRecord],
    pub(crate) accelerations: &'a [Score],
}

#[cfg(test)]
pub(crate) struct SwissPairingPlayerMut<'a> {
    pub(crate) id: &'a mut PlayerId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SwissPairingPlayer {
    id: PlayerId,
    score_without_acceleration: Score,
    rank_index: usize,
    played_games: usize,
    played_matches: usize,
    is_valid: bool,
    color_data: PlayerColorData,
    records: Vec<PlayerRoundRecord>,
    accelerations: Vec<Score>,
}

impl SwissPairingPlayer {
    fn from_entry(player: SwissPairingPlayerEntry<'_>) -> Self {
        Self {
            id: player.id,
            score_without_acceleration: player.score_without_acceleration,
            rank_index: player.rank_index,
            played_games: player.played_games,
            played_matches: player.played_matches,
            is_valid: player.is_valid,
            color_data: player.color_data,
            records: player.records.to_vec(),
            accelerations: player.accelerations.to_vec(),
        }
    }

    fn entry(&self) -> SwissPairingPlayerEntry<'_> {
        SwissPairingPlayerEntry {
            id: self.id,
            is_valid: self.is_valid,
            rank_index: self.rank_index,
            score_without_acceleration: self.score_without_acceleration,
            played_games: self.played_games,
            played_matches: self.played_matches,
            color_data: self.color_data,
            records: &self.records,
            accelerations: &self.accelerations,
        }
    }
}

/// Ephemeral, typed input prepared for one Swiss pairing run.
///
/// Historical semantics stay in [`PlayerRoundRecord`]. This context owns only
/// the compact computed data needed by the pairing algorithms; it has no
/// alternate match representation or durable state semantics.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct SwissPairingSnapshot {
    players: Vec<SwissPairingPlayer>,
    forbidden_pairs: Vec<ForbiddenPair>,
    primary_score: SwissPrimaryScoreProjection,
    pairing_allocated_bye_primary: Score,
}

impl SwissPairingSnapshot {
    pub(crate) fn new() -> Self {
        Self::with_scoring(SwissPrimaryScoreProjection::GamePoints, Score::new(0))
    }

    pub(crate) fn with_scoring(
        primary_score: SwissPrimaryScoreProjection,
        pairing_allocated_bye_primary: Score,
    ) -> Self {
        Self {
            players: Vec::new(),
            forbidden_pairs: Vec::new(),
            primary_score,
            pairing_allocated_bye_primary,
        }
    }

    pub(crate) fn push_player(&mut self, player: SwissPairingPlayerEntry<'_>) {
        self.players.push(SwissPairingPlayer::from_entry(player));
    }

    #[cfg(test)]
    pub(crate) fn player_mut(&mut self, player_index: usize) -> Option<SwissPairingPlayerMut<'_>> {
        let player = self.players.get_mut(player_index)?;
        Some(SwissPairingPlayerMut { id: &mut player.id })
    }

    pub(crate) fn push_forbidden_pair(&mut self, entry: ForbiddenPairEntry<'_>) {
        self.forbidden_pairs.push(ForbiddenPair::from_entry(entry));
    }

    #[cfg(test)]
    pub(crate) fn player(&self, player_index: usize) -> Option<SwissPairingPlayerEntry<'_>> {
        self.players
            .get(player_index)
            .map(SwissPairingPlayer::entry)
    }

    pub(crate) fn players(
        &self,
    ) -> impl ExactSizeIterator<Item = SwissPairingPlayerEntry<'_>> + '_ {
        self.players.iter().map(SwissPairingPlayer::entry)
    }

    pub(crate) fn forbidden_pairs(&self) -> impl Iterator<Item = ForbiddenPairEntry<'_>> + '_ {
        self.forbidden_pairs.iter().map(ForbiddenPair::entry)
    }

    pub(crate) fn primary_points(
        &self,
        record: PlayerRoundRecord,
        game_point_system: PointSystem,
    ) -> Option<Score> {
        match record {
            PlayerRoundRecord::PairingAllocatedBye => Some(self.pairing_allocated_bye_primary),
            PlayerRoundRecord::RequestedBye | PlayerRoundRecord::Withdrawn => Some(Score::new(0)),
            PlayerRoundRecord::Match { state, .. } => match self.primary_score {
                SwissPrimaryScoreProjection::GamePoints => state
                    .games()
                    .iter()
                    .try_fold(0u32, |total, game| {
                        total.checked_add(game_point_system.points_for(game.outcome).value())
                    })
                    .map(Score::new),
                SwissPrimaryScoreProjection::MatchPoints(match_points) => {
                    let SwissMatchRecord::Complete {
                        disposition,
                        aggregate,
                        ..
                    } = state
                    else {
                        return None;
                    };
                    Some(match_points.projected_points(disposition, aggregate))
                }
            },
        }
    }
}

impl Default for SwissPairingSnapshot {
    fn default() -> Self {
        Self::new()
    }
}
