use crate::domain::Score;
use crate::swisssystems::common::PairingContext;

pub(crate) struct BursteinInput<'a> {
    pub(crate) pairing: PairingContext<'a>,
    pub(crate) max_points: Score,
    pub(crate) adjusted_score_type_max: usize,
    pub(crate) metric_type_max: usize,
}
