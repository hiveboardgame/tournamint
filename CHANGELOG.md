# Changelog

## Unreleased

- Require exactly two active Arena entrants and a configured wait by both
  players before allowing a recent-opponent rematch. Add explicit `active`
  participant facts and required persisted `rematch_wait_secs` rules (8–50
  seconds, default 15). Older saved Arena configurations must be regenerated
  before deployment; the new required field has no implicit compatibility default.
- Replace round-robin pairing and colour generation with deterministic Berger
  schedules. Repeated passes invert colours, and every nonfinal pass swaps its
  final two rounds to improve cycle boundaries. This changes generated game
  identities and assignments; regenerate predeployment schedules and exports.
- Improve double-elimination crossover connections to delay avoidable rematches.
  Regenerate predeployment brackets; existing node numbers alone do not identify
  the old source connections.
- Preserve earned automatic advances when players withdraw. Hosts with durable
  withdrawals must use the existing withdrawal-aware `next_release` API with
  either supported cadence, rather than repair newly awarded byes after `next_wave`.
- Add serde support for native Arena and round-robin configurations, a public
  Arena configuration validator, and frozen persistence-contract fixtures.
- Validate generated Swiss round coverage before committing it, and detect an
  exhausted score-group loop with a distinct internal error.
- Order newly generated Dutch boards by pairing scores and initial ranking.
- Document legitimate early Swiss exhaustion and exact dense player identities.

## 0.1.0

- Pure Series progression with atomic write actions.
- Pure Round Robin scheduling and result projection.
- Pure Dutch, Burstein, and Double-Swiss pairing and standings.
- Pure single/bronze and double/reset elimination patches and projections.
- Pure Arena pairing and scoring from participant and terminal-game facts.
