# tournamint

*Pairings in mint condition.*

`tournamint` is a deterministic, pure tournament-algorithm library. Immutable
configuration and host-owned facts go in; projections, proposed pairings, node
patches, or one optional Series action come out. The crate performs no I/O,
reads no wall clock, owns no database, and controls no tournament lifecycle.

| Format | Public operations |
| --- | --- |
| Series | `project(plan, entrants, games)`, `project_withdrawal(...)`, `next_action(...)` |
| Round Robin | `schedule(config, players)`, `project(config, players, results)` |
| Swiss | `pair_next_round(facts, input)`, `project(facts)` |
| Elimination | `topology`, `next_wave`, `next_release`, `record_winner`, `withdraw`, `withdraw_players`, `project` |
| Arena | `award_game(config, prior_games, game)`, `pair(config, players, awarded_games)`, `project(...)`, `player_metrics(...)` |

`PlayerId` is a directly constructible host-facing ordinal. Native schedule,
round, node, and Series-game identities are deterministic within their
definition; none is a database identifier.

`ArenaConfig::default()` selects the supported fire, berserk, draw-suppression,
and grouped-pairing policy. Hosts can replace its ply boundaries with
`ArenaConfig::with_scoring_thresholds(ScoringThresholds)`.
The saved `rematch_wait_secs` must be between 8 and 50 seconds (default 15).
Recent opponents may rematch only when exactly two entrants are active and both
have waited that long. First meetings and opponents outside the history window
can pair immediately. Hosts supply `ArenaPlayerFact.active` separately from
waiting time: playing and offline entrants remain active unless paused or
withdrawn. Inactive entrants retain their history and standings.

## Fact ownership

The host persists configuration, participants, accepted Swiss rounds, exact
pairings and outcomes, elimination node facts, and physical game state. It
also owns joining, presence, pause/resume, withdrawal timestamps, deadlines,
clocks, and transactions. Corrections replace an effective outcome fact and
rerun the same pure calculation.

For elimination, `next_wave` uses complete-wave scheduling without a withdrawal
input. Hosts with withdrawals use `next_release` with either `Wave` or
`DependencyReady` cadence and the complete durable withdrawn set. Dependency-ready
batches may span nominal waves as automatic and vacant nodes unlock descendants.
Apply `withdraw_players` to current active contests before calling `next_release`.
An already awarded automatic advance remains earned when its recipient withdraws.

Series progression is split from mutation. `next_action` returns at most one
of `Release`, `CreateSet`, `SkipAndDecide`, or `Decide`. `None` means there is
nothing to persist, normally because a released physical game is in progress.
`CreateSet` creates a complete set and releases its first game atomically.

```rust
use tournamint::round_robin::{schedule, RoundRobinConfig, RoundRobinStandingsPlan};
use tournamint::{Color, MatchPointSystem, PlayerId, PointSystem};

let players: Vec<_> = (0..4).map(PlayerId::new).collect();
let config = RoundRobinConfig {
    primary_score: Default::default(),
    match_point_system: MatchPointSystem::STANDARD,
    repeats: 1,
    initial_color: Color::White,
    point_system: PointSystem::STANDARD,
    standings: RoundRobinStandingsPlan::default(),
};
assert_eq!(schedule(&config, &players)?.rounds.len(), 3);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Round robin supports game points or match points as its primary score. With an
even number of games per opponent, all repeats form one match. Match points are
awarded only after every game has a result; the default is 2 for a match win,
1 for a draw, and 0 for a loss. The alternate score can break primary-score ties.
Direct Encounter, Sonneborn–Berger, Koya, and Progressive use the selected primary
score. Full-match and split forfeits follow the same semantics as Double Swiss;
a match consisting entirely of double forfeits awards neither player points.

See [the feature guide](docs/features.md) for the complete fact boundary.

## Persisted formats

With the `serde` feature, serialized field names, enum tags and scalar forms are
storage contracts. They include the native round-robin and Arena configurations;
Swiss system, scoring, acceleration and standings options; accepted round pairings
and bye kinds; elimination topology, stages, sources, node facts and resolutions;
Series rules; and native player/game/node identifiers. Fixed JSON fixtures protect
these shapes, in addition to validation and round-trip tests.

Hosts must save the chosen Arena configuration and use it throughout the event,
including settlement and finalization. Reconstructing it from a later version's
defaults can change a running event's rules. Hosts can validate it before saving
with `ArenaConfig::validate`.

Adding a required field, renaming a variant, or changing a serialized representation
requires an explicit compatibility decision and a changelog entry. New optional
settings need deliberate backward-compatible defaults where older saved data must
remain readable; deriving `Default` for a whole configuration is not that policy.
Changes to regenerated RR schedules or elimination source connections also require
an upgrade decision even when their JSON types are unchanged. Hosts should verify
regenerated RR participants and colours against their saved game identities.

Rust 1.88. Apache-2.0 for the crate and BSD-3-Clause for the blossom solver.
