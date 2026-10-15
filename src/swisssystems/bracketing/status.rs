use crate::swiss::{PairingError, PairingFailure, PairingInternalError, PairingLimit};

/// How a bracket-based pairing run failed.
///
/// These messages are internal; semantic failures are mapped at the
/// automatic-pairing boundary, while malformed private input panics.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum BracketStatus {
    #[error("malformed pairing snapshot")]
    MalformedTournament,
    #[error("no valid pairing")]
    NoValidPairing,
    #[error("accelerated score limit exceeded")]
    LimitAcceleratedScore,
    #[error("unmatched players remain with no score group left to process")]
    EmptyScoreGroup,
}

#[derive(Debug, thiserror::Error)]
#[error("{status}")]
pub(crate) struct ComputeFailure {
    pub(crate) status: BracketStatus,
}

pub(crate) fn compute_failure(status: BracketStatus) -> ComputeFailure {
    ComputeFailure { status }
}

pub(crate) fn compute_matching_error_from_status(status: BracketStatus) -> PairingFailure {
    match status {
        BracketStatus::NoValidPairing => PairingError::NoValidPairing.into(),
        BracketStatus::LimitAcceleratedScore => {
            PairingError::LimitExceeded(PairingLimit::AcceleratedScore).into()
        }
        BracketStatus::EmptyScoreGroup => PairingInternalError::EmptyScoreGroup.into(),
        BracketStatus::MalformedTournament => {
            panic!("Swiss bracket solver received malformed private tournament state")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_score_group_is_an_internal_failure_not_exhaustion() {
        assert_eq!(
            compute_matching_error_from_status(BracketStatus::EmptyScoreGroup),
            PairingFailure::Internal(PairingInternalError::EmptyScoreGroup),
        );
    }

    #[test]
    #[should_panic(expected = "Swiss bracket solver received malformed private tournament state")]
    fn malformed_tournament_panics_at_the_private_boundary() {
        let _ = compute_matching_error_from_status(BracketStatus::MalformedTournament);
    }
}
