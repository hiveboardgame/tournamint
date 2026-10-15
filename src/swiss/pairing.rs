use crate::{Pairing, PlayerId};
use std::collections::HashSet;

/// Which immutable Swiss bye award applies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
pub enum ByeKind {
    /// The selector or authoritative round decision assigned the participant
    /// the system-specific award fixed in the tournament's Swiss
    /// configuration.
    PairingAllocated,
    /// The player requested this round off; this always awards zero points.
    ///
    /// Unlike a PAB this does not count as participation or colour history.
    /// Swiss opponent tie-breaks treat it under the configured unplayed-round
    /// policy; the FIDE March 2026 default uses Article 16 dummy semantics.
    Requested,
}

/// A player sitting out one Swiss round, and why.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(deny_unknown_fields)
)]
pub struct ByeAssignment {
    /// The player sitting the round out.
    pub player: PlayerId,
    /// Which immutable Swiss bye award applies.
    pub kind: ByeKind,
}

/// One Swiss round's ready pairings: players who received a bye and the rest,
/// paired up and awaiting results.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct RoundPairings {
    /// Pairings awaiting a result.
    pub games: Vec<Pairing>,
    /// Players sitting this round out. Byes are scored immediately at pairing
    /// time; no game result is submitted for them.
    pub byes: Vec<ByeAssignment>,
}

impl RoundPairings {
    /// Structural coverage only: accepted history must remain replayable after
    /// result corrections change present-day rematch or colour constraints.
    pub(super) fn assigns_exactly(&self, eligible: &HashSet<PlayerId>) -> bool {
        let mut assigned = HashSet::with_capacity(eligible.len());
        for pairing in &self.games {
            if pairing.white() == pairing.black()
                || !eligible.contains(&pairing.white())
                || !eligible.contains(&pairing.black())
                || !assigned.insert(pairing.white())
                || !assigned.insert(pairing.black())
            {
                return false;
            }
        }
        for bye_ in &self.byes {
            if !eligible.contains(&bye_.player) || !assigned.insert(bye_.player) {
                return false;
            }
        }
        assigned == *eligible
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for RoundPairings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Repr {
            games: Vec<Pairing>,
            byes: Vec<ByeAssignment>,
        }

        let repr = Repr::deserialize(deserializer)?;
        let mut participants = HashSet::with_capacity(repr.games.len() * 2 + repr.byes.len());
        for game in &repr.games {
            if game.white() == game.black()
                || !participants.insert(game.white())
                || !participants.insert(game.black())
            {
                return Err(serde::de::Error::custom(
                    "a round participant must occur exactly once",
                ));
            }
        }
        for bye in &repr.byes {
            if !participants.insert(bye.player) {
                return Err(serde::de::Error::custom(
                    "a round participant must occur exactly once",
                ));
            }
        }
        Ok(Self {
            games: repr.games,
            byes: repr.byes,
        })
    }
}
