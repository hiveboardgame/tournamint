use crate::{Color, MatchScore, PlayerId};

/// Ephemeral projection accumulator derived solely from Arena facts.
#[derive(Clone, Debug)]
pub(super) struct ArenaPlayerProjection {
    pub(super) waiting_millis: Option<u64>,
    pub(super) arena_rating: Option<i32>,
    pub(super) points: u32,
    pub(super) opponents: Vec<PlayerId>,
    pub(super) whites: u32,
    pub(super) blacks: u32,
    pub(super) color_strike: i64,
    pub(super) games_scored: u32,
    pub(super) wins: u32,
    pub(super) draws: u32,
    pub(super) losses: u32,
    pub(super) streak: u32,
    pub(super) best_streak: u32,
    pub(super) berserks: u32,
    pub(super) performance_sum: i64,
    pub(super) performance_games: u32,
    pub(super) games_played: u32,
    pub(super) no_starts: u32,
}

impl ArenaPlayerProjection {
    pub(super) fn new(waiting_millis: Option<u64>, arena_rating: Option<i32>) -> Self {
        Self {
            waiting_millis,
            arena_rating,
            points: 0,
            opponents: Vec::new(),
            whites: 0,
            blacks: 0,
            color_strike: 0,
            games_scored: 0,
            wins: 0,
            draws: 0,
            losses: 0,
            streak: 0,
            best_streak: 0,
            berserks: 0,
            performance_sum: 0,
            performance_games: 0,
            games_played: 0,
            no_starts: 0,
        }
    }

    pub(super) const fn waiting_millis(&self) -> Option<u64> {
        self.waiting_millis
    }

    pub(super) fn color_balance(&self) -> i64 {
        i64::from(self.whites) - i64::from(self.blacks)
    }

    pub(super) fn could_play(&self, other: &Self, max_strike: u32) -> bool {
        let max_strike = i64::from(max_strike);
        (self.color_strike > -max_strike || other.color_strike > -max_strike)
            && (self.color_strike < max_strike || other.color_strike < max_strike)
    }

    pub(super) fn color_history_key(&self) -> (i64, i64) {
        (self.color_strike, self.color_balance())
    }

    pub(super) fn played_recently(&self, opponent: PlayerId, cooldown: u32) -> bool {
        self.opponents
            .iter()
            .rev()
            .take(cooldown as usize)
            .any(|recent| *recent == opponent)
    }

    pub(super) fn record_pairing(&mut self, opponent: PlayerId, color: Color) -> Result<(), ()> {
        self.opponents.push(opponent);
        match color {
            Color::White => {
                self.whites = self.whites.checked_add(1).ok_or(())?;
                self.color_strike = self.color_strike.max(0) + 1;
            }
            Color::Black => {
                self.blacks = self.blacks.checked_add(1).ok_or(())?;
                self.color_strike = self.color_strike.min(0) - 1;
            }
        }
        Ok(())
    }

    pub(super) fn record_scored(
        &mut self,
        score: MatchScore,
        points: u32,
        berserked: bool,
        performance: Option<i64>,
    ) -> Result<(), ()> {
        self.points = self.points.checked_add(points).ok_or(())?;
        self.games_scored = self.games_scored.checked_add(1).ok_or(())?;
        match score {
            MatchScore::Win => self.wins = self.wins.checked_add(1).ok_or(())?,
            MatchScore::Draw => self.draws = self.draws.checked_add(1).ok_or(())?,
            MatchScore::Loss => self.losses = self.losses.checked_add(1).ok_or(())?,
        }
        if berserked {
            self.berserks = self.berserks.checked_add(1).ok_or(())?;
        }
        self.streak = if score == MatchScore::Win {
            self.streak.checked_add(1).ok_or(())?
        } else {
            0
        };
        self.best_streak = self.best_streak.max(self.streak);
        if let Some(contribution) = performance {
            self.performance_sum = self.performance_sum.checked_add(contribution).ok_or(())?;
            self.performance_games = self.performance_games.checked_add(1).ok_or(())?;
        }
        Ok(())
    }
}
