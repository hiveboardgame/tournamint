use std::cmp::Ordering;

use smallvec::SmallVec;

#[cfg(target_pointer_width = "64")]
type UintMax = usize;
#[cfg(target_pointer_width = "32")]
type UintMax = u64;

type Limbs = SmallVec<[UintMax; 2]>;

// The weighted matching implementation below follows Edmonds' blossom
// primal-dual algorithm as presented by Galil and as implemented by NetworkX's
// BSD-licensed max_weight_matching routine. It is adapted to this project as a
// stateful solver over Rust-owned edge-weight storage and integer weights only.

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct BigWeight {
    limbs: Limbs,
}

impl Clone for BigWeight {
    fn clone(&self) -> Self {
        Self {
            limbs: self.limbs.clone(),
        }
    }

    fn clone_from(&mut self, source: &Self) {
        self.limbs.clone_from(&source.limbs);
    }
}

impl BigWeight {
    pub(crate) fn zero() -> Self {
        Self {
            limbs: Limbs::new(),
        }
    }

    pub(crate) fn from_usize(value: usize) -> Self {
        if value == 0 {
            Self::zero()
        } else {
            let mut limbs = Limbs::new();
            limbs.push(value as UintMax);
            Self { limbs }
        }
    }

    #[cfg(test)]
    pub(super) fn from_limbs(limbs: &[UintMax]) -> Self {
        let mut result = Self {
            limbs: Limbs::from_slice(limbs),
        };
        result.normalize();
        result
    }

    fn normalize(&mut self) {
        while self.limbs.last().copied() == Some(0) {
            self.limbs.pop();
        }
    }

    pub(crate) fn is_zero(&self) -> bool {
        self.limbs.is_empty()
    }

    pub(crate) fn set_zero(&mut self) {
        self.limbs.clear();
    }

    pub(crate) fn limb_len(&self) -> usize {
        self.limbs.len()
    }

    pub(super) fn is_even(&self) -> bool {
        self.limbs.first().is_none_or(|value| value & 1 == 0)
    }

    pub(crate) fn add_assign(&mut self, rhs: &Self) {
        let len = self.limbs.len().max(rhs.limbs.len());
        self.limbs.resize(len, 0);
        let mut carry = false;
        for index in 0..len {
            let lhs = self.limbs[index];
            let rhs_value = rhs.limbs.get(index).copied().unwrap_or(0);
            let (with_rhs, carry_rhs) = lhs.overflowing_add(rhs_value);
            let (with_carry, carry_carry) = with_rhs.overflowing_add(UintMax::from(carry));
            self.limbs[index] = with_carry;
            carry = carry_rhs || carry_carry;
        }
        if carry {
            self.limbs.push(1);
        }
    }

    pub(crate) fn sub_assign(&mut self, rhs: &Self) {
        debug_assert!((*self).cmp(rhs) != Ordering::Less);
        let mut borrow = false;
        for index in 0..self.limbs.len() {
            let rhs_value = rhs.limbs.get(index).copied().unwrap_or(0);
            let (after_rhs, borrow_rhs) = self.limbs[index].overflowing_sub(rhs_value);
            let (after_borrow, borrow_borrow) = after_rhs.overflowing_sub(UintMax::from(borrow));
            self.limbs[index] = after_borrow;
            borrow = borrow_rhs || borrow_borrow;
        }
        debug_assert!(!borrow);
        self.normalize();
    }

    pub(super) fn assign_slack(&mut self, lhs: &Self, rhs: &Self, edge_weight: &Self) {
        if lhs.limbs.len() <= 1 && rhs.limbs.len() <= 1 && edge_weight.limbs.len() <= 1 {
            let lhs_value = lhs.limbs.first().copied().unwrap_or(0);
            let rhs_value = rhs.limbs.first().copied().unwrap_or(0);
            let edge_value = edge_weight.limbs.first().copied().unwrap_or(0);
            let (sum, carry) = lhs_value.overflowing_add(rhs_value);
            let (low, borrow) = sum.overflowing_sub(edge_value);
            debug_assert!(carry || !borrow);
            let high = UintMax::from(carry) - UintMax::from(borrow);

            self.limbs.clear();
            if low != 0 {
                self.limbs.push(low);
            }
            if high != 0 {
                if low == 0 {
                    self.limbs.push(0);
                }
                self.limbs.push(high);
            }
            return;
        }

        let len = lhs
            .limbs
            .len()
            .max(rhs.limbs.len())
            .max(edge_weight.limbs.len());
        self.limbs.resize(len, 0);
        let mut add_carry = false;
        let mut borrow = false;
        for index in 0..len {
            let lhs_value = lhs.limbs.get(index).copied().unwrap_or(0);
            let rhs_value = rhs.limbs.get(index).copied().unwrap_or(0);
            let (with_rhs, carry_rhs) = lhs_value.overflowing_add(rhs_value);
            let (sum, carry_add) = with_rhs.overflowing_add(UintMax::from(add_carry));
            add_carry = carry_rhs || carry_add;

            let edge_value = edge_weight.limbs.get(index).copied().unwrap_or(0);
            let (after_rhs, borrow_rhs) = sum.overflowing_sub(edge_value);
            let (after_borrow, borrow_borrow) = after_rhs.overflowing_sub(UintMax::from(borrow));
            self.limbs[index] = after_borrow;
            borrow = borrow_rhs || borrow_borrow;
        }

        if add_carry && borrow {
            borrow = false;
        } else if add_carry {
            self.limbs.push(1);
        }
        debug_assert!(!borrow);
        self.normalize();
    }

    pub(super) fn slack_is_zero(lhs: &Self, rhs: &Self, edge_weight: &Self) -> bool {
        if lhs.limbs.len() <= 1 && rhs.limbs.len() <= 1 && edge_weight.limbs.len() <= 1 {
            let lhs_value = lhs.limbs.first().copied().unwrap_or(0);
            let rhs_value = rhs.limbs.first().copied().unwrap_or(0);
            let edge_value = edge_weight.limbs.first().copied().unwrap_or(0);
            let (sum, carry) = lhs_value.overflowing_add(rhs_value);
            return !carry && sum == edge_value;
        }

        let len = lhs
            .limbs
            .len()
            .max(rhs.limbs.len())
            .max(edge_weight.limbs.len());
        let mut carry = false;
        for index in 0..len {
            let lhs_value = lhs.limbs.get(index).copied().unwrap_or(0);
            let rhs_value = rhs.limbs.get(index).copied().unwrap_or(0);
            let (with_rhs, carry_rhs) = lhs_value.overflowing_add(rhs_value);
            let (sum, carry_add) = with_rhs.overflowing_add(UintMax::from(carry));
            if sum != edge_weight.limbs.get(index).copied().unwrap_or(0) {
                return false;
            }
            carry = carry_rhs || carry_add;
        }
        !carry
    }

    pub(super) fn slack_less_than(
        lhs: &Self,
        rhs: &Self,
        edge_weight: &Self,
        threshold: &Self,
    ) -> bool {
        if lhs.limbs.len() <= 1
            && rhs.limbs.len() <= 1
            && edge_weight.limbs.len() <= 1
            && threshold.limbs.len() <= 1
        {
            let lhs_value = lhs.limbs.first().copied().unwrap_or(0);
            let rhs_value = rhs.limbs.first().copied().unwrap_or(0);
            let edge_value = edge_weight.limbs.first().copied().unwrap_or(0);
            let threshold_value = threshold.limbs.first().copied().unwrap_or(0);
            let (sum, carry) = lhs_value.overflowing_add(rhs_value);
            let (low, borrow) = sum.overflowing_sub(edge_value);
            debug_assert!(carry || !borrow);
            let high = UintMax::from(carry) - UintMax::from(borrow);
            return high == 0 && low < threshold_value;
        }

        let len = lhs
            .limbs
            .len()
            .max(rhs.limbs.len())
            .max(edge_weight.limbs.len())
            .max(threshold.limbs.len());
        let mut add_carry = false;
        let mut borrow = false;
        let mut ordering = Ordering::Equal;
        for index in 0..len {
            let lhs_value = lhs.limbs.get(index).copied().unwrap_or(0);
            let rhs_value = rhs.limbs.get(index).copied().unwrap_or(0);
            let (with_rhs, carry_rhs) = lhs_value.overflowing_add(rhs_value);
            let (sum, carry_add) = with_rhs.overflowing_add(UintMax::from(add_carry));
            add_carry = carry_rhs || carry_add;

            let edge_value = edge_weight.limbs.get(index).copied().unwrap_or(0);
            let (after_edge, borrow_edge) = sum.overflowing_sub(edge_value);
            let (result, borrow_borrow) = after_edge.overflowing_sub(UintMax::from(borrow));
            borrow = borrow_edge || borrow_borrow;

            let threshold_value = threshold.limbs.get(index).copied().unwrap_or(0);
            if result != threshold_value {
                ordering = result.cmp(&threshold_value);
            }
        }

        if add_carry && borrow {
            borrow = false;
        } else if add_carry {
            ordering = Ordering::Greater;
        }
        debug_assert!(!borrow);
        ordering == Ordering::Less
    }

    pub(crate) fn add_assign_usize(&mut self, rhs: usize) {
        let mut carry = rhs as UintMax;
        let mut index = 0;
        while carry != 0 {
            if index == self.limbs.len() {
                self.limbs.push(carry);
                return;
            }
            let (sum, overflow) = self.limbs[index].overflowing_add(carry);
            self.limbs[index] = sum;
            carry = UintMax::from(overflow);
            index += 1;
        }
    }

    pub(crate) fn sub_assign_usize(&mut self, rhs: usize) {
        let mut subtrahend = rhs as UintMax;
        let mut index = 0;
        while subtrahend != 0 {
            debug_assert!(index < self.limbs.len());
            let (difference, borrow) = self.limbs[index].overflowing_sub(subtrahend);
            self.limbs[index] = difference;
            subtrahend = UintMax::from(borrow);
            index += 1;
        }
        self.normalize();
    }

    // Dutch edge weights intentionally emulate fixed-width legacy arithmetic
    // when rule priorities are packed into a bounded dynamic weight.
    pub(crate) fn wrapping_add_assign(&mut self, rhs: &Self, width_limbs: usize) {
        self.limbs.resize(width_limbs, 0);
        let mut carry = false;
        for index in 0..width_limbs {
            let rhs_value = rhs.limbs.get(index).copied().unwrap_or(0);
            let (with_rhs, carry_rhs) = self.limbs[index].overflowing_add(rhs_value);
            let (with_carry, carry_carry) = with_rhs.overflowing_add(UintMax::from(carry));
            self.limbs[index] = with_carry;
            carry = carry_rhs || carry_carry;
        }
        self.normalize();
    }

    pub(crate) fn wrapping_sub_assign_usize(&mut self, rhs: usize, width_limbs: usize) {
        self.limbs.resize(width_limbs, 0);
        let mut subtrahend = rhs as UintMax;
        for index in 0..width_limbs {
            let (difference, borrow) = self.limbs[index].overflowing_sub(subtrahend);
            self.limbs[index] = difference;
            if !borrow {
                break;
            }
            subtrahend = 1;
        }
        self.normalize();
    }

    pub(crate) fn or_assign_usize(&mut self, rhs: usize) {
        if rhs == 0 {
            return;
        }
        if self.limbs.is_empty() {
            self.limbs.push(rhs as UintMax);
        } else {
            self.limbs[0] |= rhs as UintMax;
        }
    }

    pub(crate) fn shl_assign(&mut self, shift: usize) {
        if self.is_zero() || shift == 0 {
            return;
        }

        let limb_bits = UintMax::BITS as usize;
        let limb_shift = shift / limb_bits;
        let bit_shift = shift % limb_bits;
        let original_len = self.limbs.len();
        let shifted_len = original_len + limb_shift;
        if limb_shift != 0 {
            self.limbs.resize(shifted_len, 0);
            self.limbs.copy_within(0..original_len, limb_shift);
            self.limbs[..limb_shift].fill(0);
        }
        if bit_shift == 0 {
            return;
        }

        let mut carry = 0;
        for index in limb_shift..shifted_len {
            let value = self.limbs[index];
            self.limbs[index] = (value << bit_shift) | carry;
            carry = value >> (limb_bits - bit_shift);
        }
        if carry != 0 {
            self.limbs.push(carry);
        }
    }

    // Keep only the configured limb width after shifts; overflow beyond that
    // width is intentionally discarded for legacy-compatible priority packing.
    pub(crate) fn wrapping_shl_assign(&mut self, shift: usize, width_limbs: usize) {
        self.limbs.resize(width_limbs, 0);
        if width_limbs == 0 || shift == 0 {
            self.normalize();
            return;
        }

        let limb_bits = UintMax::BITS as usize;
        let limb_shift = shift / limb_bits;
        let bit_shift = shift % limb_bits;
        if limb_shift >= width_limbs {
            self.limbs.clear();
            return;
        }

        for index in (0..width_limbs).rev() {
            let mut value = 0;
            if index >= limb_shift {
                value = self.limbs[index - limb_shift] << bit_shift;
                if bit_shift != 0 && index > limb_shift {
                    value |= self.limbs[index - limb_shift - 1] >> (limb_bits - bit_shift);
                }
            }
            self.limbs[index] = value;
        }
        self.normalize();
    }

    pub(crate) fn shr_assign(&mut self, shift: usize) {
        if self.is_zero() || shift == 0 {
            return;
        }

        let limb_bits = UintMax::BITS as usize;
        let limb_shift = shift / limb_bits;
        let bit_shift = shift % limb_bits;
        if limb_shift >= self.limbs.len() {
            self.limbs.clear();
            return;
        }
        self.limbs.drain(0..limb_shift);
        if bit_shift != 0 {
            let mut carry = 0;
            for index in (0..self.limbs.len()).rev() {
                let value = self.limbs[index];
                self.limbs[index] = (value >> bit_shift) | carry;
                carry = value << (limb_bits - bit_shift);
            }
        }
        self.normalize();
    }
}

impl Ord for BigWeight {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.limbs.len() != other.limbs.len() {
            return self.limbs.len().cmp(&other.limbs.len());
        }
        for index in (0..self.limbs.len()).rev() {
            if self.limbs[index] != other.limbs[index] {
                return self.limbs[index].cmp(&other.limbs[index]);
            }
        }
        Ordering::Equal
    }
}

impl PartialOrd for BigWeight {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::{BigWeight, UintMax};

    #[test]
    fn assign_slack_preserves_final_carry() {
        let lhs = BigWeight::from_limbs(&[0, UintMax::MAX]);
        let rhs = BigWeight::from_limbs(&[0, 1]);
        let edge_weight = BigWeight::zero();
        let mut slack = BigWeight::zero();

        slack.assign_slack(&lhs, &rhs, &edge_weight);

        assert_eq!(slack, BigWeight::from_limbs(&[0, 0, 1]));
    }

    #[test]
    fn assign_slack_cancels_final_carry_and_borrow() {
        let lhs = BigWeight::from_limbs(&[0, UintMax::MAX]);
        let rhs = BigWeight::from_limbs(&[0, 1]);
        let edge_weight = BigWeight::from_limbs(&[0, 1]);
        let mut slack = BigWeight::zero();

        slack.assign_slack(&lhs, &rhs, &edge_weight);

        assert_eq!(slack, BigWeight::from_limbs(&[0, UintMax::MAX]));
    }

    #[test]
    fn slack_is_zero_accepts_final_carry() {
        let lhs = BigWeight::from_limbs(&[UintMax::MAX, UintMax::MAX]);
        let rhs = BigWeight::from_limbs(&[1]);
        let edge_weight = BigWeight::from_limbs(&[0, 0, 1]);

        assert!(BigWeight::slack_is_zero(&lhs, &rhs, &edge_weight));
    }

    #[test]
    fn slack_is_zero_rejects_near_miss() {
        let lhs = BigWeight::from_limbs(&[UintMax::MAX, UintMax::MAX]);
        let rhs = BigWeight::from_limbs(&[1]);
        let edge_weight = BigWeight::from_limbs(&[0, 1, 1]);

        assert!(!BigWeight::slack_is_zero(&lhs, &rhs, &edge_weight));
    }

    #[test]
    fn slack_is_zero_matches_one_limb_sum() {
        let lhs = BigWeight::from_limbs(&[5]);
        let rhs = BigWeight::from_limbs(&[7]);
        let edge_weight = BigWeight::from_limbs(&[12]);

        assert!(BigWeight::slack_is_zero(&lhs, &rhs, &edge_weight));
    }

    #[test]
    fn slack_is_zero_rejects_one_limb_carry() {
        let lhs = BigWeight::from_limbs(&[UintMax::MAX]);
        let rhs = BigWeight::from_limbs(&[1]);
        let edge_weight = BigWeight::zero();

        assert!(!BigWeight::slack_is_zero(&lhs, &rhs, &edge_weight));
    }

    #[test]
    fn slack_less_than_matches_materialized_slack() {
        let cases = [
            (
                BigWeight::from_limbs(&[5]),
                BigWeight::from_limbs(&[7]),
                BigWeight::from_limbs(&[3]),
                vec![
                    BigWeight::from_limbs(&[9]),
                    BigWeight::from_limbs(&[10]),
                    BigWeight::zero(),
                ],
            ),
            (
                BigWeight::from_limbs(&[0, UintMax::MAX]),
                BigWeight::from_limbs(&[0, 1]),
                BigWeight::from_limbs(&[0, 1]),
                vec![
                    BigWeight::from_limbs(&[0, UintMax::MAX]),
                    BigWeight::from_limbs(&[1, UintMax::MAX]),
                ],
            ),
            (
                BigWeight::from_limbs(&[UintMax::MAX, UintMax::MAX]),
                BigWeight::from_limbs(&[1]),
                BigWeight::from_limbs(&[0, 0, 1]),
                vec![BigWeight::zero(), BigWeight::from_limbs(&[1])],
            ),
            (
                BigWeight::from_limbs(&[UintMax::MAX, UintMax::MAX]),
                BigWeight::from_limbs(&[1]),
                BigWeight::zero(),
                vec![
                    BigWeight::from_limbs(&[0, 0, 1]),
                    BigWeight::from_limbs(&[0, 0, 2]),
                ],
            ),
        ];

        for (lhs, rhs, edge_weight, thresholds) in cases {
            let mut slack = BigWeight::zero();
            slack.assign_slack(&lhs, &rhs, &edge_weight);
            for threshold in thresholds {
                assert_eq!(
                    BigWeight::slack_less_than(&lhs, &rhs, &edge_weight, &threshold),
                    slack < threshold
                );
            }
        }
    }

    #[test]
    fn slack_less_than_rejects_one_limb_final_carry() {
        let lhs = BigWeight::from_limbs(&[UintMax::MAX]);
        let rhs = BigWeight::from_limbs(&[1]);
        let edge_weight = BigWeight::zero();
        let threshold = BigWeight::from_limbs(&[UintMax::MAX]);

        assert!(!BigWeight::slack_less_than(
            &lhs,
            &rhs,
            &edge_weight,
            &threshold
        ));
    }

    #[test]
    fn shl_assign_preserves_bit_carry() {
        let mut value = BigWeight::from_limbs(&[UintMax::MAX]);

        value.shl_assign(1);

        assert_eq!(value, BigWeight::from_limbs(&[UintMax::MAX - 1, 1]));
    }

    #[test]
    fn shl_assign_inserts_whole_limb_shift() {
        let mut value = BigWeight::from_limbs(&[7]);

        value.shl_assign(UintMax::BITS as usize);

        assert_eq!(value, BigWeight::from_limbs(&[0, 7]));
    }

    #[test]
    fn or_assign_usize_preserves_existing_high_limb() {
        let mut value = BigWeight::from_limbs(&[0, 3]);

        value.or_assign_usize(5);

        assert_eq!(value, BigWeight::from_limbs(&[5, 3]));
    }
}
