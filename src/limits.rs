//! Bounds on the integer widths the pairing engines are allowed to use, so a
//! large tournament fails loudly instead of overflowing.
//!
//! Ported to Rust from BBP Pairings (Apache-2.0, Copyright 2016-2026 Jeremy Bierema),
//! and modified.

pub(crate) const NATIVE_USIZE_MAX: usize = usize::MAX;

pub(crate) const fn bits_to_represent(mut value: usize) -> usize {
    let mut result = 0usize;
    while value >= 1 {
        result = result.wrapping_add(1);
        value /= 2;
    }
    result
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WrappingLimit {
    inclusive_max: Option<usize>,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct OverflowingAdd {
    pub(crate) value: usize,
    pub(crate) overflowed: bool,
}

impl WrappingLimit {
    pub(crate) fn from_type_max(type_max: usize) -> Self {
        let inclusive_max = if type_max == NATIVE_USIZE_MAX {
            None
        } else {
            Some(type_max)
        };
        Self { inclusive_max }
    }

    pub(crate) fn wrap(self, value: usize) -> usize {
        match self.inclusive_max {
            Some(max_value) => value % (max_value + 1),
            None => value,
        }
    }

    pub(crate) fn wrapping_sub(self, value: usize, subtrahend: usize) -> usize {
        match self.inclusive_max {
            Some(max_value) => {
                let modulus = max_value as u128 + 1;
                ((value as u128 % modulus + modulus - (subtrahend as u128 % modulus)) % modulus)
                    as usize
            }
            None => value.wrapping_sub(subtrahend),
        }
    }

    pub(crate) fn overflowing_add(self, value: usize, addend: usize) -> OverflowingAdd {
        match self.inclusive_max {
            Some(max_value) => {
                let value = self.wrap(value);
                let addend = self.wrap(addend);
                if value > max_value - addend {
                    OverflowingAdd {
                        value: value - (max_value - addend) - 1,
                        overflowed: true,
                    }
                } else {
                    OverflowingAdd {
                        value: value + addend,
                        overflowed: false,
                    }
                }
            }
            None => {
                let (value, overflowed) = value.overflowing_add(addend);
                OverflowingAdd { value, overflowed }
            }
        }
    }

    pub(crate) fn checked_add(self, value: usize, addend: usize) -> Option<usize> {
        match self.inclusive_max {
            Some(max_value) => {
                if value > max_value || addend > max_value || value > max_value - addend {
                    None
                } else {
                    Some(value + addend)
                }
            }
            None => value.checked_add(addend),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{OverflowingAdd, WrappingLimit, bits_to_represent};

    #[test]
    fn bits_to_represent_counts_required_binary_width() {
        assert_eq!(bits_to_represent(0), 0);
        assert_eq!(bits_to_represent(1), 1);
        assert_eq!(bits_to_represent(2), 2);
        assert_eq!(bits_to_represent(255), 8);
        assert_eq!(bits_to_represent(256), 9);
    }

    #[test]
    fn wrapping_limit_wraps_bounded_values() {
        let limit = WrappingLimit::from_type_max(2);

        assert_eq!(limit.wrap(0), 0);
        assert_eq!(limit.wrap(2), 2);
        assert_eq!(limit.wrap(3), 0);
        assert_eq!(limit.wrap(5), 2);
    }

    #[test]
    fn wrapping_limit_preserves_unbounded_values() {
        let limit = WrappingLimit::from_type_max(usize::MAX);

        assert_eq!(limit.wrap(usize::MAX), usize::MAX);
    }

    #[test]
    fn wrapping_limit_applies_bounded_arithmetic() {
        let limit = WrappingLimit::from_type_max(2);

        assert_eq!(limit.wrapping_sub(0, 1), 2);
    }

    #[test]
    fn wrapping_limit_applies_native_width_arithmetic() {
        let limit = WrappingLimit::from_type_max(usize::MAX);

        assert_eq!(limit.wrapping_sub(0, 1), usize::MAX);
    }

    #[test]
    fn wrapping_limit_reports_bounded_add_overflow() {
        let limit = WrappingLimit::from_type_max(2);

        assert_eq!(
            limit.overflowing_add(1, 1),
            OverflowingAdd {
                value: 2,
                overflowed: false
            }
        );
        assert_eq!(
            limit.overflowing_add(2, 1),
            OverflowingAdd {
                value: 0,
                overflowed: true
            }
        );
    }

    #[test]
    fn wrapping_limit_reports_native_width_add_overflow() {
        let limit = WrappingLimit::from_type_max(usize::MAX);

        assert_eq!(
            limit.overflowing_add(usize::MAX, 1),
            OverflowingAdd {
                value: 0,
                overflowed: true
            }
        );
    }

    #[test]
    fn wrapping_limit_checked_add_rejects_bounded_overflow() {
        let limit = WrappingLimit::from_type_max(2);

        assert_eq!(limit.checked_add(1, 1), Some(2));
        assert_eq!(limit.checked_add(2, 1), None);
        assert_eq!(limit.checked_add(3, 0), None);
    }

    #[test]
    fn wrapping_limit_checked_add_rejects_native_width_overflow() {
        let limit = WrappingLimit::from_type_max(usize::MAX);

        assert_eq!(limit.checked_add(usize::MAX, 1), None);
    }
}
