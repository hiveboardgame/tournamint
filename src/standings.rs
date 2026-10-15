//! Format-neutral standings output and shared configurable-plan machinery.
//!
//! [`StandingsTable`], [`StandingGroup`], and [`StandingRow`] represent
//! competition-ranked participant groups without prescribing how a tournament
//! format produces them. Depending on the owning format and projection, a
//! group may be a resolved place, an exact tie, or participants that are
//! currently unresolved or indistinguishable. The owning format defines those
//! grouping semantics and deterministic presentation order.
//!
//! Swiss and round-robin additionally use the configurable [`StandingsPlan`]
//! engine. Their format modules prepare their own scoring contexts and
//! criteria, while this module owns the ordered-plan and sequential
//! subgroup-partitioning semantics they share.

use core::cmp::Ordering;
use std::collections::HashMap;

use crate::{PlayerId, Score};

/// Treatment of administratively awarded games in Direct Encounter.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
pub enum DirectEncounterForfeitPolicy {
    /// Exclude adjudicated games from the direct-encounter table.
    #[default]
    Exclude,
    /// Include their configured awarded score.
    IncludeAsScored,
}

/// How multiple encounters between the same two participants contribute to
/// a Direct Encounter table.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
pub enum RepeatedEncounterPolicy {
    /// Use the exact average score across the encounters, matching the pinned
    /// FIDE tie-break default.
    #[default]
    Average,
    /// Add every encounter score.
    Sum,
}

/// Format-typed options for Direct Encounter criteria.
///
/// `B` is the owning format's score-basis type. For example, Swiss uses
/// [`crate::swiss::ScoreBasis`]. Formats whose Direct Encounter has no score
/// basis expose their own native option type instead.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(deny_unknown_fields)
)]
pub struct DirectEncounterOptions<B> {
    /// Format-native score projection used in the subgroup table.
    pub score_basis: B,
    /// Treatment of adjudicated games.
    pub forfeits: DirectEncounterForfeitPolicy,
    /// Treatment of repeated encounters.
    pub repeated_encounters: RepeatedEncounterPolicy,
}

/// Ascending or descending one-based Tournament Pairing Number order.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
pub enum TournamentPairingNumberOrder {
    /// Lower pairing numbers rank first.
    Ascending,
    /// Higher pairing numbers rank first.
    Descending,
}

/// Format-typed options for Progressive Score criteria.
///
/// `B` is the owning format's score-basis type. For example, Swiss uses
/// [`crate::swiss::ScoreBasis`]. PS-C1 omits only the cumulative score after
/// scheduled round one; round-one points remain in every later cumulative
/// score. Formats with one fixed points projection expose their own native
/// option type instead.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(deny_unknown_fields)
)]
pub struct ProgressiveScoreOptions<B> {
    /// Format-native score projection.
    pub score_basis: B,
    /// Whether to apply Progressive Score Cut-1 (PS-C1).
    pub cut_first_round: bool,
}

/// Format-typed options for Sonneborn-Berger criteria.
///
/// `B` is the format-native score basis. `U` is the format-native policy for
/// unresolved or unplayed rounds. Formats without either choice expose their
/// own native option type instead.
/// Every low cut is selected after contributions are normalized: lowest
/// opponent score first, then lowest product. The Swiss FIDE policy additionally
/// applies its iterative Article 16.5 VUR choice.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(deny_unknown_fields)
)]
pub struct SonnebornBergerOptions<B, U> {
    /// Format-native score projection.
    pub score_basis: B,
    /// Number of lowest normalized contributions to discard.
    pub cut_lowest: u32,
    /// Format-native treatment of unresolved or unplayed rounds.
    pub unplayed: U,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SonnebornBergerContribution {
    pub(crate) opponent_score: u64,
    pub(crate) product: u64,
    pub(crate) vur: bool,
    pub(crate) order: usize,
}

/// Applies ordinary SB low cuts, with the optional Article 16.5 VUR choice,
/// and sums only the surviving products.
pub(crate) fn sum_sonneborn_berger_contributions(
    mut contributions: Vec<SonnebornBergerContribution>,
    cut_lowest: usize,
    fide_vur_exception: bool,
) -> Result<u64, StandingsError> {
    for _ in 0..cut_lowest.min(contributions.len()) {
        let ordinary_index = contributions
            .iter()
            .enumerate()
            .min_by_key(|(_, contribution)| {
                (
                    contribution.opponent_score,
                    contribution.product,
                    contribution.order,
                )
            })
            .map(|(index, _)| index)
            .expect("a bounded cut has a contribution");
        let cut_index = if fide_vur_exception {
            contributions
                .iter()
                .enumerate()
                .filter(|(_, contribution)| contribution.vur)
                .min_by_key(|(_, contribution)| {
                    (
                        contribution.product,
                        contribution.opponent_score,
                        contribution.order,
                    )
                })
                .filter(|(vur_index, vur)| {
                    *vur_index == ordinary_index
                        || vur.product >= contributions[ordinary_index].product
                })
                .map_or(ordinary_index, |(index, _)| index)
        } else {
            ordinary_index
        };
        contributions.swap_remove(cut_index);
    }

    contributions.iter().try_fold(0u64, |total, contribution| {
        total
            .checked_add(contribution.product)
            .ok_or(StandingsError::ArithmeticOverflow)
    })
}

/// An ordered list of criteria used to split tied groups.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(deny_unknown_fields)
)]
pub struct StandingsPlan<C> {
    /// Criteria in application order. Repeated criteria are retained as
    /// separate occurrences.
    pub criteria: Vec<C>,
}

impl<C> StandingsPlan<C> {
    /// Creates a plan from its ordered criteria.
    pub fn new(criteria: Vec<C>) -> Self {
        Self { criteria }
    }

    /// Creates a plan with no criteria. Every included participant remains
    /// in one tied group.
    pub fn empty() -> Self {
        Self {
            criteria: Vec::new(),
        }
    }
}

/// A format-neutral projection of competition-ranked participant groups.
///
/// Depending on the owning format and current projection, a group may be a
/// resolved place, an exact tie, or participants that remain unresolved or
/// indistinguishable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StandingsTable<C, V> {
    /// Value columns shared by every row, in positional order.
    pub columns: Vec<C>,
    /// Best group first.
    pub groups: Vec<StandingGroup<V>>,
}

/// One competition-ranked participant group.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StandingGroup<V> {
    /// One-based competition rank. A two-player group at rank one makes the
    /// next group's rank three.
    pub rank: u32,
    /// Zero-based criterion that first separated this group from the
    /// preceding group. The leading group has no preceding boundary.
    pub separated_by: Option<u32>,
    /// Participants in deterministic presentation order. Their order inside
    /// this vector neither splits the group nor distinguishes its members.
    pub players: Vec<StandingRow<V>>,
}

/// A participant displayed in a standings group.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StandingRow<V> {
    /// Stable library participant identifier.
    pub player: PlayerId,
    /// Values positionally aligned with [`StandingsTable::columns`].
    pub values: Vec<V>,
}

/// A public, non-lossy value suitable for explaining a standings projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
pub enum CriterionValue {
    /// Exact configured score units.
    Score(Score),
    /// Exact non-negative criterion-native integer, including a count, sum,
    /// or product as appropriate to the criterion.
    Integer(u64),
    /// The criterion is subgroup-dependent or otherwise has no meaningful
    /// standalone value.
    NotApplicable,
}

/// Why a standings projection could not be produced.
///
/// This public boundary contains only caller-correctable plan failures and
/// checked arithmetic overflow. Violations of crate-private standings
/// invariants are not representable as lossy public input errors.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum StandingsError {
    /// The plan contains a criterion unavailable for this format or scoring
    /// mode.
    #[error("criterion {criterion_index} is not valid for this tournament: {reason}")]
    InvalidCriterion {
        /// Zero-based occurrence in the submitted plan.
        criterion_index: usize,
        /// Format-specific explanation.
        reason: &'static str,
    },
    /// Caller-controlled values overflowed an exact calculation.
    #[error("standings arithmetic overflowed")]
    ArithmeticOverflow,
}

/// Reduced non-negative ratio used by exact standings calculations.
///
/// This stays crate-private because it is an implementation detail of the
/// format-native criteria, not another public metric representation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ExactRatio {
    numerator: u64,
    denominator: u64,
}

impl ExactRatio {
    pub(crate) const ZERO: Self = Self {
        numerator: 0,
        denominator: 1,
    };

    pub(crate) fn new(numerator: u64, denominator: u64) -> Self {
        assert_ne!(
            denominator, 0,
            "a standings ratio must have a non-zero denominator"
        );
        let divisor = gcd(numerator, denominator);
        Self {
            numerator: numerator / divisor,
            denominator: denominator / divisor,
        }
    }

    pub(crate) const fn integer(numerator: u64) -> Self {
        Self {
            numerator,
            denominator: 1,
        }
    }

    pub(crate) fn checked_add(self, other: Self) -> Result<Self, StandingsError> {
        let common = gcd(self.denominator, other.denominator);
        let left_scale = other.denominator / common;
        let right_scale = self.denominator / common;
        let numerator = self
            .numerator
            .checked_mul(left_scale)
            .and_then(|left| {
                other
                    .numerator
                    .checked_mul(right_scale)
                    .and_then(|right| left.checked_add(right))
            })
            .ok_or(StandingsError::ArithmeticOverflow)?;
        let denominator = self
            .denominator
            .checked_mul(left_scale)
            .ok_or(StandingsError::ArithmeticOverflow)?;
        Ok(Self::new(numerator, denominator))
    }

    pub(crate) fn checked_cmp(self, other: Self) -> Result<Ordering, StandingsError> {
        let common = gcd(self.denominator, other.denominator);
        let left = u128::from(self.numerator) * u128::from(other.denominator / common);
        let right = u128::from(other.numerator) * u128::from(self.denominator / common);
        Ok(left.cmp(&right))
    }

    pub(crate) fn checked_min(self, other: Self) -> Result<Self, StandingsError> {
        Ok(if self.checked_cmp(other)? == Ordering::Greater {
            other
        } else {
            self
        })
    }

    pub(crate) fn checked_max(self, other: Self) -> Result<Self, StandingsError> {
        Ok(if self.checked_cmp(other)? == Ordering::Less {
            other
        } else {
            self
        })
    }
}

fn gcd(mut left: u64, mut right: u64) -> u64 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left.max(1)
}

/// Applies ordered criteria to the current tied subgroups.
///
/// `players` must already be in deterministic presentation order. The
/// callback sees only the subgroup that remains tied at that point, which is
/// essential for Direct Encounter and other group-aware criteria.
pub(crate) fn apply_criteria<C, V, F, M>(
    players: &[PlayerId],
    criteria: &[C],
    mut partition: F,
    mut metrics: M,
) -> Result<Vec<StandingGroup<V>>, StandingsError>
where
    F: FnMut(usize, &C, &[PlayerId]) -> Result<Vec<Vec<PlayerId>>, StandingsError>,
    M: FnMut(PlayerId) -> Result<Vec<V>, StandingsError>,
{
    let presentation_order: HashMap<PlayerId, usize> = players
        .iter()
        .copied()
        .enumerate()
        .map(|(index, player)| (player, index))
        .collect();
    struct PendingGroup {
        players: Vec<PlayerId>,
        separated_by: Option<u32>,
    }

    let mut groups = vec![PendingGroup {
        players: players.to_vec(),
        separated_by: None,
    }];

    for (criterion_index, criterion) in criteria.iter().enumerate() {
        let mut next = Vec::new();
        for group in groups {
            if group.players.len() < 2 {
                next.push(group);
                continue;
            }

            for (partition_index, players) in partition(criterion_index, criterion, &group.players)?
                .into_iter()
                .enumerate()
            {
                let separated_by = if partition_index == 0 {
                    group.separated_by
                } else {
                    Some(
                        u32::try_from(criterion_index)
                            .map_err(|_| StandingsError::ArithmeticOverflow)?,
                    )
                };
                next.push(PendingGroup {
                    players,
                    separated_by,
                });
            }
        }
        groups = next;
    }

    let mut rank = 1u32;
    let mut standing_groups = Vec::with_capacity(groups.len());
    for mut group in groups {
        group
            .players
            .sort_by_key(|player| presentation_order[player]);
        let group_len =
            u32::try_from(group.players.len()).map_err(|_| StandingsError::ArithmeticOverflow)?;
        standing_groups.push(StandingGroup {
            rank,
            separated_by: group.separated_by,
            players: group
                .players
                .into_iter()
                .map(|player| {
                    Ok(StandingRow {
                        player,
                        values: metrics(player)?,
                    })
                })
                .collect::<Result<Vec<_>, StandingsError>>()?,
        });
        rank = rank
            .checked_add(group_len)
            .ok_or(StandingsError::ArithmeticOverflow)?;
    }

    Ok(standing_groups)
}

/// Partitions a subgroup by a descending scalar value.
pub(crate) fn partition_descending<K: Ord, F: Fn(PlayerId) -> K>(
    players: &[PlayerId],
    value: F,
) -> Vec<Vec<PlayerId>> {
    let mut keyed: Vec<_> = players
        .iter()
        .copied()
        .map(|player| (value(player), player))
        .collect();
    keyed.sort_by(|(left, _), (right, _)| right.cmp(left));

    let mut result: Vec<Vec<PlayerId>> = Vec::new();
    let mut previous: Option<&K> = None;
    for (key, player) in &keyed {
        if previous != Some(key) {
            result.push(Vec::new());
            previous = Some(key);
        }
        result
            .last_mut()
            .expect("a key creates a group")
            .push(*player);
    }
    result
}

/// Partitions a subgroup by an ascending scalar value.
pub(crate) fn partition_ascending<K: Ord, F: Fn(PlayerId) -> K>(
    players: &[PlayerId],
    value: F,
) -> Vec<Vec<PlayerId>> {
    let mut keyed: Vec<_> = players
        .iter()
        .copied()
        .map(|player| (value(player), player))
        .collect();
    keyed.sort_by(|(left, _), (right, _)| left.cmp(right));

    let mut result: Vec<Vec<PlayerId>> = Vec::new();
    let mut previous: Option<&K> = None;
    for (key, player) in &keyed {
        if previous != Some(key) {
            result.push(Vec::new());
            previous = Some(key);
        }
        result
            .last_mut()
            .expect("a key creates a group")
            .push(*player);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum TestCriterion {
        Score,
        HeadToHead,
    }

    fn ids(count: usize) -> Vec<PlayerId> {
        (0..count).map(PlayerId::new).collect()
    }

    fn sb(
        opponent_score: u64,
        product: u64,
        vur: bool,
        order: usize,
    ) -> SonnebornBergerContribution {
        SonnebornBergerContribution {
            opponent_score,
            product,
            vur,
            order,
        }
    }

    #[test]
    fn sonneborn_berger_ordinary_cuts_use_opponent_score_then_product() {
        assert_eq!(
            sum_sonneborn_berger_contributions(
                vec![sb(1, 5, false, 0), sb(2, 0, false, 1)],
                1,
                false,
            ),
            Ok(0),
            "the lowest opponent score is cut before the lowest product"
        );
        assert_eq!(
            sum_sonneborn_berger_contributions(
                vec![sb(1, 5, false, 0), sb(1, 2, false, 1), sb(2, 7, false, 2),],
                1,
                false,
            ),
            Ok(12),
            "equal opponent scores are broken by the lowest product"
        );
    }

    #[test]
    fn sonneborn_berger_fide_cuts_compare_and_prefer_vur_contributions_iteratively() {
        assert_eq!(
            sum_sonneborn_berger_contributions(
                vec![sb(1, 10, false, 0), sb(2, 5, true, 1)],
                1,
                true,
            ),
            Ok(5),
            "a lower VUR product leaves the ordinary candidate selected"
        );
        assert_eq!(
            sum_sonneborn_berger_contributions(
                vec![sb(1, 2, false, 0), sb(2, 5, true, 1)],
                1,
                true,
            ),
            Ok(2),
            "a higher VUR product is selected by Article 16.5"
        );
        assert_eq!(
            sum_sonneborn_berger_contributions(
                vec![sb(1, 2, false, 0), sb(2, 5, false, 1), sb(3, 2, true, 2),],
                2,
                true,
            ),
            Ok(5),
            "an equal distinct VUR is cut first and candidates are recomputed"
        );
    }

    #[test]
    fn sonneborn_berger_zero_and_excess_cuts_saturate() {
        let contributions = vec![sb(1, 2, false, 0), sb(2, 5, true, 1)];
        assert_eq!(
            sum_sonneborn_berger_contributions(contributions.clone(), 0, true),
            Ok(7)
        );
        assert_eq!(
            sum_sonneborn_berger_contributions(contributions, usize::MAX, true),
            Ok(0)
        );
    }

    #[test]
    fn ascending_partition_preserves_presentation_order_inside_ties() {
        let players = [
            PlayerId::new(2),
            PlayerId::new(0),
            PlayerId::new(3),
            PlayerId::new(1),
        ];
        assert_eq!(
            partition_ascending(&players, |player| player.index() % 2),
            vec![
                vec![PlayerId::new(2), PlayerId::new(0)],
                vec![PlayerId::new(3), PlayerId::new(1)],
            ]
        );
    }

    #[test]
    fn an_empty_plan_preserves_one_tied_group() {
        let groups = apply_criteria::<(), (), _, _>(
            &ids(3),
            &[],
            |_, _, _| unreachable!(),
            |_| Ok(Vec::new()),
        )
        .unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].rank, 1);
        assert_eq!(
            groups[0]
                .players
                .iter()
                .map(|row| row.player)
                .collect::<Vec<_>>(),
            ids(3)
        );
    }

    #[test]
    fn later_criteria_only_see_their_current_subgroup() {
        let players = ids(4);
        let mut observed = Vec::new();
        let standings = apply_criteria(
            &players,
            &[TestCriterion::Score, TestCriterion::HeadToHead],
            |_, criterion, group| {
                observed.push(group.to_vec());
                Ok(match criterion {
                    TestCriterion::Score => {
                        partition_descending(group, |player| usize::from(player.index() < 2))
                    }
                    TestCriterion::HeadToHead => {
                        group.iter().copied().map(|player| vec![player]).collect()
                    }
                })
            },
            |_| Ok(Vec::<()>::new()),
        )
        .unwrap();

        assert_eq!(observed, vec![ids(4), ids(2), ids(4)[2..].to_vec()]);
        assert_eq!(
            standings.iter().map(|group| group.rank).collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
        assert_eq!(
            standings
                .iter()
                .map(|group| group.separated_by)
                .collect::<Vec<_>>(),
            vec![None, Some(1), Some(0), Some(1)]
        );
    }

    #[test]
    fn deterministic_presentation_does_not_split_a_tie() {
        let players = ids(3);
        let standings = apply_criteria(
            &players,
            &[()],
            |_, _, group| Ok(vec![group.iter().rev().copied().collect()]),
            |_| Ok(Vec::<()>::new()),
        )
        .unwrap();

        assert_eq!(standings.len(), 1);
        assert_eq!(standings[0].players[0].player, PlayerId::new(0));
    }

    #[test]
    #[should_panic(expected = "a standings ratio must have a non-zero denominator")]
    fn zero_denominator_ratio_panics_as_an_internal_invariant() {
        let _ = ExactRatio::new(1, 0);
    }

    #[test]
    fn competition_ranks_skip_over_tied_positions() {
        let standings = apply_criteria(
            &ids(4),
            &[()],
            |_, _, group| Ok(vec![group[..2].to_vec(), group[2..].to_vec()]),
            |_| Ok(Vec::<()>::new()),
        )
        .unwrap();
        assert_eq!(standings[0].rank, 1);
        assert_eq!(standings[1].rank, 3);
    }
}
