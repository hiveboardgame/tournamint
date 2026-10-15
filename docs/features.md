# Pure API and fact boundary

The host supplies stable `PlayerId` ordinals and immutable facts. Tournamint
does not retain state between calls.

## Series

A `SeriesPlan` has a balanced arbitrary colour cycle and phases. Finite phases
use `AtMost`; the final phase uses `UntilDecisive`. Sets may contain multiple
games and use `PlayAll` or `EarlyClinch`.

`project` reports phase, set, half-point score, released game, and winner.
`next_action` requests at most one atomic host write: `Release`, `CreateSet`,
`SkipAndDecide`, or `Decide`. There is no wait action; `None` is the blocked or
no-write result. Game specs include their phase index for host clock selection.

## Round Robin

`schedule` returns the deterministic circle schedule, native identities,
repeat links, and odd-field rests. `project` consumes sparse game facts and
returns standings, player totals, aggregate matches, and unresolved identities.

All games against one opponent form one match when `repeats` is positive and
even. `primary_score` selects game or match points; `PrimaryScore`, `GamePoints`,
and `MatchPoints` are distinct standings criteria. `MatchesWon` counts aggregate
wins, while the existing game-win criteria keep counting individual games.
Match points and match wins are rejected for odd repeat counts.

Match awards are available only once every game has a result. Clearing a result
removes the aggregate award until the match is complete again. Full-match
forfeits and double forfeits use the shared match-point system; partial and split
forfeits use aggregate game points. An odd-field rest awards nothing.

Direct Encounter, Sonneborn–Berger, Koya, and Progressive use the selected primary
score. With match points they count one encounter per opponent, use opponents'
match totals, and apply forfeit exclusions to full-match forfeits. Progressive
credits a completed match in its last scheduled game round, independently of
fact arrival order.

## Swiss

`SwissFacts` contains configuration, stable initial ranking, and accepted
rounds with exact pairings, byes, and current effective outcomes.
`pair_next_round` adds current eligibility, requested byes, and supported
restrictions. `project` accepts a partially completed current round.
Each encounter has exactly one outcome position for Dutch/Burstein or two
for Double Swiss. Positions follow the accepted pairing's colours, then
reversed colours; `None` preserves an unresolved leg. For example,
`[None, Some(GameOutcome::white_win())]` records only a Double-Swiss return
leg, awarding its win to the accepted pairing's Black player.
Corrections replace an outcome at its existing position and rerun projection.

Dutch, Burstein, and Double Swiss retain their scoring, tiebreak, exact
matching, and independent solver/oracle coverage.

## Elimination

`topology` creates a single/bronze or double/reset graph. Its player slice fills
standard seed positions: rating order gives ordinary seeding, while a player
permutation gives custom placement. Bye positions and connections stay fixed;
hosts can keep rating seeds separately for labels and standings presentation.
`initial_facts` supplies planned node facts. `next_wave` implements the complete-wave policy.
`next_release` accepts an `EliminationReleaseCadence` and the host's complete
durable withdrawal set. Its dependency-ready policy materializes every node
whose own sources are terminal, even while unrelated contests remain active,
and follows automatic, vacant, and conditional-reset resolutions to one
deterministic fixed point. A release batch can therefore span nominal waves.
The durable set applies to newly materialized nodes; when a withdrawal first
becomes durable, the host applies `withdraw_players` to currently materialized
facts before asking for the next release.
Facts produced under wave cadence are valid input under dependency-ready
cadence. Facts with a mixed wave frontier are not valid input under wave
cadence and are rejected as incoherent.
`record_winner` and `withdraw` return replacement patches for the host to
persist. Materialized entrants remain facts, so later withdrawals do not
rewrite resolved contests. Hosts using `next_wave` apply durable
withdrawals with `withdraw_players`, which repairs every causally exposed
automatic or vacant leaf without changing resolved played contests.
Projections include nodes, completion, podium, and placement tiers. Typed
stages let the host select stage-specific Series plans.

## Arena

Player facts contain identity, current Arena-relative rating, and optional
waiting time. Semantic terminal facts contain pairing, each participant's
per-game rating snapshot, played/no-start result, plies, and berserk flags in
settlement order. Historical performance uses those terminal snapshots, while
pairing and the pre-performance standings fallback use the current Arena
rating. `award_game` scores one new terminal from both participants' prior
semantic results. The host retains that award alongside the game in an
`ArenaAwardedGameFact`; `pair`, `project`, and `player_metrics` require these
awarded facts and never rescore them. `pair` returns current waiting-pool
pairings and their one-based competition ranks; `project` derives standings
and metrics, while `player_metrics` omits ranking. Pending and cancelled
games are not scoring facts.
Presence, intent, cutoff, and physical games belong to the host.
`ArenaConfig::default()` supplies the canonical scoring and pairing policy;
`with_scoring_thresholds` retains that policy while replacing its berserk and
draw ply boundaries. `ArenaConfig::plain()` remains available for hosts that
explicitly want intrinsic 2/1/0 scoring without adjustments.
