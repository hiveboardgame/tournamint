mod colors;
#[cfg(test)]
mod differential;
mod forbidden;
mod input;
mod rules;
mod scores;
mod solver;
mod weights;

use crate::swiss::{PairingFailure, RoundPairings};

use crate::swisssystems::bracketing::compute_matching_error_from_status;
pub(crate) use input::DutchInput;

pub(crate) fn compute_matching(input: DutchInput<'_>) -> Result<RoundPairings, PairingFailure> {
    solver::compute_matching_impl(&input)
        .map_err(|failure| compute_matching_error_from_status(failure.status))
}
