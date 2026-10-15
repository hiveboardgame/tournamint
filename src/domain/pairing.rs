use crate::domain::PlayerId;

/// A pairing decision assigning White and Black.
///
/// Construction is intentionally infallible. Each pure format operation
/// validates pairings in the context of its complete input facts.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(deny_unknown_fields)
)]
pub struct Pairing {
    white: PlayerId,
    black: PlayerId,
}

impl Pairing {
    /// A pairing of `white` against `black`.
    ///
    /// Callers use identifiers from one format-native participant set.
    pub const fn new(white: PlayerId, black: PlayerId) -> Self {
        Self { white, black }
    }

    /// The player with white.
    pub fn white(self) -> PlayerId {
        self.white
    }

    /// The player with black.
    pub fn black(self) -> PlayerId {
        self.black
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_constructor_uses_player_ids() {
        let pairing = Pairing::new(PlayerId::new(2), PlayerId::new(5));

        assert_eq!(pairing.white(), PlayerId::new(2));
        assert_eq!(pairing.black(), PlayerId::new(5));
    }
}
