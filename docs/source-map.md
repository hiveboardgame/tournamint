# Algorithm sources and regression map

The Swiss systems implement documented subsets of the FIDE rules effective in
February 2026; tie-break behavior follows the March 2026 regulations. Arena
pairing and scoring are informed by lila commit
`60e225b83429835e08f7175539013f37f5eb5e4e`. These are pinned behavioral
references, not certification claims.

| Area | Implementation | Primary regression evidence |
| --- | --- | --- |
| Series actions and phase transitions | `src/series.rs` | `series::tests` |
| Round Robin schedule and standings | `src/round_robin.rs` | `round_robin::tests`, `tests/pure_formats.rs` |
| Swiss fact projection and selection | `src/swiss.rs`, `src/swiss/` | `tests/pure_formats.rs` |
| Dutch | `src/swisssystems/dutch/` | differential solver tests |
| Burstein | `src/swisssystems/burstein/` | exhaustive pairing-order tests |
| Double Swiss | `src/swisssystems/double_swiss/` | independent oracle/differential tests |
| Elimination topology and patches | `src/elimination.rs`, `src/elimination/topology.rs` | topology tests, `tests/pure_formats.rs` |
| Arena pairing and scoring | `src/arena/` | scoring/pairing tests, `tests/pure_formats.rs` |

Reference chapters:

- FIDE C.04.1 Basic Rules for Swiss Systems
- FIDE C.04.2 General Handling Rules
- FIDE C.04.3 Dutch System
- FIDE C.04.4.2 Burstein System
- FIDE C.04.5 Double-Swiss Pairing System
- FIDE C.04.7 Accelerated Systems
- FIDE Play-Off and Tie-Break Regulations
