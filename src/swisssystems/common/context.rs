use crate::domain::{Color, PointSystem};
use crate::swiss::SwissPairingSnapshot;

/// The conditions a pairing run happens under, independent of which Swiss
/// system is doing the pairing: the snapshot to pair, how far the tournament
/// has got, and how results are scored and coloured.
///
/// Every system's input embeds this and adds only its own sizing knobs.
#[derive(Clone, Copy)]
pub(crate) struct PairingContext<'a> {
    pub(crate) tournament: &'a SwissPairingSnapshot,
    pub(crate) played_rounds: usize,
    pub(crate) point_system: PointSystem,
    pub(crate) initial_color: Color,
}
