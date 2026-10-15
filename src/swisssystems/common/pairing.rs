use crate::domain::Color;

pub(crate) struct PairingPlayers {
    pub(crate) white: usize,
    pub(crate) black: usize,
}

pub(crate) fn pairing_from_player_color(
    player0: usize,
    player1: usize,
    player0_color: Color,
) -> PairingPlayers {
    match player0_color {
        Color::White => PairingPlayers {
            white: player0,
            black: player1,
        },
        Color::Black => PairingPlayers {
            white: player1,
            black: player0,
        },
    }
}
