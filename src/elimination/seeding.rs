#[cfg(test)]
use crate::domain::PlayerId;

/// The smallest power of two at least `player_count` - an elimination
/// bracket always has this many leaf slots, with the shortfall filled by
/// byes.
pub(crate) fn bracket_size_for(player_count: usize) -> usize {
    if player_count == 0 {
        return 0;
    }
    // `PlayerId` is non-zero-sized, and every safely constructible slice is
    // bounded by `isize::MAX` bytes. Its length therefore remains below the
    // `usize` half-range at which `checked_next_power_of_two` can fail.
    player_count
        .checked_next_power_of_two()
        .expect("a constructible player slice cannot exhaust the bracket size")
}

/// Standard recursive-doubling bracket seeding: for `bracket_size` 8 this is
/// `[1, 8, 4, 5, 2, 7, 3, 6]`. Adjacent pairs are round-1 matches - seed `i`
/// always meets seed `bracket_size + 1 - i`, which keeps top seeds apart for
/// as long as the bracket allows (seed 1 and 2 can only meet in the final,
/// seeds 1-4 can't meet before the semifinal, and so on).
pub(crate) fn standard_seed_order(bracket_size: usize) -> Vec<usize> {
    if bracket_size == 0 {
        return Vec::new();
    }

    let mut seeds = vec![1usize];
    let mut current_size = 1usize;
    while current_size < bracket_size {
        let next_size = current_size
            .checked_mul(2)
            .expect("a constructible bracket cannot exhaust its seed topology");
        let mut next = Vec::with_capacity(next_size);
        for &seed in &seeds {
            next.push(seed);
            next.push(
                next_size
                    .checked_add(1)
                    .and_then(|limit| limit.checked_sub(seed))
                    .expect("a bracket seed always fits its topology"),
            );
        }
        seeds = next;
        current_size = next_size;
    }
    seeds
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg(test)]
pub(crate) enum RoundOneSlot {
    Player(PlayerId),
    Bye,
}

/// Maps `initial_rank_order` (best-to-worst)
/// onto the standard seed order, in bracket position order - adjacent pairs
/// are round-1 matches. Seed numbers beyond `initial_rank_order.len()` don't
/// correspond to a real player and become byes. Byes always land against a
/// real opponent, never against each other: whenever byes exist at all, the
/// real player count exceeds half the bracket size, and round-1 pairs sum to
/// `bracket_size + 1`, so two seed numbers that are both above
/// `initial_rank_order.len()` can never be paired together.
#[cfg(test)]
pub(crate) fn round_one_slots(initial_rank_order: &[PlayerId]) -> Vec<RoundOneSlot> {
    let player_count = initial_rank_order.len();
    let bracket_size = bracket_size_for(player_count);
    standard_seed_order(bracket_size)
        .into_iter()
        .map(|seed| {
            if seed <= player_count {
                RoundOneSlot::Player(initial_rank_order[seed - 1])
            } else {
                RoundOneSlot::Bye
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bracket_size_rounds_up_to_the_next_power_of_two() {
        assert_eq!(bracket_size_for(0), 0);
        assert_eq!(bracket_size_for(1), 1);
        assert_eq!(bracket_size_for(2), 2);
        assert_eq!(bracket_size_for(3), 4);
        assert_eq!(bracket_size_for(4), 4);
        assert_eq!(bracket_size_for(5), 8);
        assert_eq!(bracket_size_for(8), 8);
        assert_eq!(bracket_size_for(9), 16);
    }

    #[test]
    fn largest_constructible_player_slice_length_has_a_representable_bracket_size() {
        let max_player_slice_len = (isize::MAX as usize) / std::mem::size_of::<PlayerId>();
        assert!(bracket_size_for(max_player_slice_len).is_power_of_two());
    }

    #[test]
    fn standard_seed_order_matches_the_well_known_bracket_shapes() {
        assert_eq!(standard_seed_order(1), vec![1]);
        assert_eq!(standard_seed_order(2), vec![1, 2]);
        assert_eq!(standard_seed_order(4), vec![1, 4, 2, 3]);
        assert_eq!(standard_seed_order(8), vec![1, 8, 4, 5, 2, 7, 3, 6]);
        assert_eq!(
            standard_seed_order(16),
            vec![1, 16, 8, 9, 4, 13, 5, 12, 2, 15, 7, 10, 3, 14, 6, 11]
        );
    }

    #[test]
    fn round_one_pairs_always_sum_to_bracket_size_plus_one() {
        for bracket_size in [2, 4, 8, 16, 32] {
            let seeds = standard_seed_order(bracket_size);
            for pair in seeds.chunks(2) {
                assert_eq!(pair[0] + pair[1], bracket_size + 1);
            }
        }
    }

    #[test]
    fn byes_never_land_against_each_other() {
        for player_count in 1..=20usize {
            let players: Vec<PlayerId> = (0..player_count).map(PlayerId::new).collect();
            let slots = round_one_slots(&players);
            for pair in slots.chunks(2) {
                assert!(
                    !matches!(pair, [RoundOneSlot::Bye, RoundOneSlot::Bye]),
                    "player_count={player_count} produced a bye-vs-bye pairing"
                );
            }
        }
    }

    #[test]
    fn round_one_slots_place_top_seeds_first_and_byes_only_when_needed() {
        let players: Vec<PlayerId> = (0..5).map(PlayerId::new).collect();
        let slots = round_one_slots(&players);
        // bracket_size_for(5) == 8, standard_seed_order(8) == [1,8,4,5,2,7,3,6].
        // Seeds 6,7,8 don't exist (player_count == 5), so those positions are byes.
        assert_eq!(
            slots,
            vec![
                RoundOneSlot::Player(PlayerId::new(0)), // seed 1
                RoundOneSlot::Bye,                      // seed 8
                RoundOneSlot::Player(PlayerId::new(3)), // seed 4
                RoundOneSlot::Player(PlayerId::new(4)), // seed 5
                RoundOneSlot::Player(PlayerId::new(1)), // seed 2
                RoundOneSlot::Bye,                      // seed 7
                RoundOneSlot::Player(PlayerId::new(2)), // seed 3
                RoundOneSlot::Bye,                      // seed 6
            ]
        );
    }
}
