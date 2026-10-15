//! Pairing engines for the documented supported subset of FIDE Dutch,
//! Burstein, and Double-Swiss.
//!
//! Ported to Rust from BBP Pairings (Apache-2.0, Copyright 2016-2026 Jeremy Bierema),
//! and modified. The Dutch and Burstein implementations retain adapted
//! score-group, float, colour, and matching machinery. Double-Swiss is a
//! separate implementation of the pinned February 2026 supported subset.

pub(crate) mod bracketing;
pub(crate) mod burstein;
pub(crate) mod common;
pub(crate) mod double_swiss;
pub(crate) mod dutch;
