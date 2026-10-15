use crate::swiss::{PairingError, PairingFailure, PairingLimit, RoundPairings};

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(super) enum BursteinStatus {
    #[error("malformed pairing snapshot")]
    MalformedTournament,
    #[error("adjusted score limit exceeded")]
    LimitAdjustedScore,
    #[error("Buchholz total limit exceeded")]
    LimitBuchholzTotal,
    #[error("Sonneborn-Berger product limit exceeded")]
    LimitSonnebornBergerProduct,
    #[error("Sonneborn-Berger total limit exceeded")]
    LimitSonnebornBergerTotal,
    #[error("no eligible bye player")]
    NoBye,
    #[error("no valid Burstein matching")]
    NoMatching,
    #[error("accelerated score limit exceeded")]
    LimitAcceleratedScore,
}

pub(super) type BursteinComputeResult = RoundPairings;

#[derive(Debug, thiserror::Error)]
#[error("{status}")]
pub(super) struct BursteinComputeFailure {
    pub(super) status: BursteinStatus,
}

pub(super) fn compute_failure(status: BursteinStatus) -> BursteinComputeFailure {
    BursteinComputeFailure { status }
}

pub(super) fn compute_matching_error_from_status(status: BursteinStatus) -> PairingFailure {
    match status {
        BursteinStatus::LimitAdjustedScore => {
            PairingError::LimitExceeded(PairingLimit::AdjustedScore).into()
        }
        BursteinStatus::LimitBuchholzTotal => {
            PairingError::LimitExceeded(PairingLimit::BuchholzTotal).into()
        }
        BursteinStatus::LimitSonnebornBergerProduct => {
            PairingError::LimitExceeded(PairingLimit::SonnebornBergerProduct).into()
        }
        BursteinStatus::LimitSonnebornBergerTotal => {
            PairingError::LimitExceeded(PairingLimit::SonnebornBergerTotal).into()
        }
        BursteinStatus::NoBye => PairingError::NoBye.into(),
        BursteinStatus::NoMatching => PairingError::NoValidPairing.into(),
        BursteinStatus::LimitAcceleratedScore => {
            PairingError::LimitExceeded(PairingLimit::AcceleratedScore).into()
        }
        BursteinStatus::MalformedTournament => {
            panic!("Burstein solver received malformed private tournament state")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_recoverable_private_status_maps_to_its_exact_boundary_error() {
        for (status, expected) in [
            (
                BursteinStatus::LimitAdjustedScore,
                PairingError::LimitExceeded(PairingLimit::AdjustedScore).into(),
            ),
            (
                BursteinStatus::LimitBuchholzTotal,
                PairingError::LimitExceeded(PairingLimit::BuchholzTotal).into(),
            ),
            (
                BursteinStatus::LimitSonnebornBergerProduct,
                PairingError::LimitExceeded(PairingLimit::SonnebornBergerProduct).into(),
            ),
            (
                BursteinStatus::LimitSonnebornBergerTotal,
                PairingError::LimitExceeded(PairingLimit::SonnebornBergerTotal).into(),
            ),
            (BursteinStatus::NoBye, PairingError::NoBye.into()),
            (
                BursteinStatus::NoMatching,
                PairingError::NoValidPairing.into(),
            ),
            (
                BursteinStatus::LimitAcceleratedScore,
                PairingError::LimitExceeded(PairingLimit::AcceleratedScore).into(),
            ),
        ] {
            assert_eq!(compute_matching_error_from_status(status), expected);
        }
    }

    #[test]
    #[should_panic(expected = "Burstein solver received malformed private tournament state")]
    fn malformed_tournament_panics_at_the_private_boundary() {
        let _ = compute_matching_error_from_status(BursteinStatus::MalformedTournament);
    }
}
