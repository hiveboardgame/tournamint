use crate::domain::ForbiddenPairEntry;
use crate::swiss::{MatchDisposition, PlayerRoundRecord, SwissMatchRecord};

const WORD_BITS: usize = usize::BITS as usize;

/// Which pairs may not meet, as a dense player x player bitmap - covering both
/// arbiter-entered forbidden groups and pairs that have already played.
pub(crate) struct ForbiddenPairMatrix {
    player_count: usize,
    cells: Vec<usize>,
}

impl ForbiddenPairMatrix {
    pub(crate) fn new(player_count: usize) -> Self {
        let cell_count = player_count
            .checked_mul(player_count)
            .expect("Swiss player matrix dimensions fit addressable memory");
        let word_count =
            cell_count / WORD_BITS + usize::from(!cell_count.is_multiple_of(WORD_BITS));
        Self {
            player_count,
            cells: vec![0; word_count],
        }
    }

    fn cell_index(&self, source: usize, target: usize) -> Option<usize> {
        if source >= self.player_count || target >= self.player_count {
            return None;
        }
        source.checked_mul(self.player_count)?.checked_add(target)
    }

    pub(crate) fn insert(&mut self, source: usize, target: usize) -> bool {
        let Some(index) = self.cell_index(source, target) else {
            return false;
        };
        let Some(word) = self.cells.get_mut(index / WORD_BITS) else {
            return false;
        };
        *word |= 1usize << (index % WORD_BITS);
        true
    }

    pub(crate) fn contains(&self, source: usize, target: usize) -> bool {
        self.cell_index(source, target)
            .and_then(|index| {
                self.cells
                    .get(index / WORD_BITS)
                    .map(|word| word & (1usize << (index % WORD_BITS)) != 0)
            })
            .unwrap_or(false)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum BuildForbiddenPairMatrixError {
    #[error("malformed pairing snapshot")]
    MalformedTournament,
}

#[derive(Clone, Copy)]
pub(crate) struct ForbiddenPairPlayer<'a> {
    pub(crate) id: usize,
    pub(crate) is_valid: bool,
    pub(crate) records: &'a [PlayerRoundRecord],
}

pub(crate) fn build_forbidden_pair_matrix<'a>(
    entries: impl IntoIterator<Item = ForbiddenPairEntry<'a>>,
    played_rounds: usize,
    players_len: usize,
    players: impl IntoIterator<Item = ForbiddenPairPlayer<'a>>,
) -> Result<ForbiddenPairMatrix, BuildForbiddenPairMatrixError> {
    let mut forbidden_pairs = ForbiddenPairMatrix::new(players_len);
    for entry in entries {
        if !entry.round_range.contains(&played_rounds) {
            continue;
        }
        for &source in entry.players {
            let source = source.index();
            if source >= players_len {
                return Err(BuildForbiddenPairMatrixError::MalformedTournament);
            }
            for &target in entry.players {
                let target = target.index();
                if target >= players_len {
                    return Err(BuildForbiddenPairMatrixError::MalformedTournament);
                }
                if !forbidden_pairs.insert(source, target) {
                    return Err(BuildForbiddenPairMatrixError::MalformedTournament);
                }
            }
        }
    }

    for player in players {
        if !player.is_valid {
            continue;
        }
        if player.id >= players_len {
            return Err(BuildForbiddenPairMatrixError::MalformedTournament);
        }
        for record in player.records.iter().copied() {
            let PlayerRoundRecord::Match {
                opponent, state, ..
            } = record
            else {
                continue;
            };
            // C.04.5 treats a two-game match containing one or two single-game
            // forfeits as a regular match.  It therefore blocks a repeat even
            // when neither board was physically played (for example opposite
            // single forfeits).  A complete-match forfeit has a non-regular
            // disposition and remains eligible for a later pairing.
            let games = state.games();
            if games.iter().any(|game| game.outcome.counts_as_played())
                || matches!(
                    state,
                    SwissMatchRecord::Complete {
                        disposition: MatchDisposition::Regular,
                        ..
                    }
                ) && games.len() == 2
            {
                let opponent = opponent.index();
                if opponent >= players_len {
                    return Err(BuildForbiddenPairMatrixError::MalformedTournament);
                }
                if !forbidden_pairs.insert(player.id, opponent) {
                    return Err(BuildForbiddenPairMatrixError::MalformedTournament);
                }
            }
        }
    }

    Ok(forbidden_pairs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forbidden_pair_matrix_uses_player_bounds() {
        let mut matrix = ForbiddenPairMatrix::new(3);

        assert!(matrix.insert(1, 2));
        assert!(matrix.contains(1, 2));
        assert!(!matrix.contains(2, 1));
        assert!(!matrix.insert(3, 0));
        assert!(!matrix.insert(0, 3));
        assert!(!matrix.contains(3, 0));
        assert!(!matrix.contains(0, 3));
    }

    #[test]
    fn out_of_range_player_id_is_a_malformed_tournament() {
        let entry = ForbiddenPairEntry {
            players: &[crate::PlayerId::new(1)],
            round_range: 0..1,
        };
        let result = build_forbidden_pair_matrix([entry], 0, 1, std::iter::empty());

        assert!(matches!(
            result,
            Err(BuildForbiddenPairMatrixError::MalformedTournament)
        ));
    }
}
