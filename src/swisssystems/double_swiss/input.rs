use crate::swisssystems::common::PairingContext;

pub(crate) struct DoubleSwissInput<'a> {
    pub(crate) pairing: PairingContext<'a>,
    pub(crate) expected_rounds: usize,
    pub(crate) points_type_max: usize,
}
