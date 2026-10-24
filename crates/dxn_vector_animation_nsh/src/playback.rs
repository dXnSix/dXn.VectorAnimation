//! Playback: which part of the timeline plays, how fast, which way and how
//! often; where it is after some time (the sampled fallback's clock); and
//! the composite animation of the time value (the native graph's).

use nsh_core::{
    animation::Easing,
    ui::{AnimatedValue, Animation, Timing},
};

use crate::source::Repeat;

/// Which way playback runs (`direction`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, nsh_core::ui::PropEnum)]
pub enum Direction {
    /// From the start to the end.
    #[default]
    Normal,
    /// From the end to the start.
    Reverse,
    /// To the end, then back, and on.
    Alternate,
}

/// What plays.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Playback {
    /// Where the played part starts (seconds).
    pub start: f64,
    /// Where it ends.
    pub end: f64,
    /// Speed (1 as made).
    pub rate: f64,
    /// Which way.
    pub direction: Direction,
    /// How many runs.
    pub repeat: Repeat,
}

impl Playback {
    /// The time a run starts from.
    pub fn first(&self) -> f64 {
        match self.direction {
            Direction::Reverse => self.end,
            _ => self.start,
        }
    }

    /// The time the last run ends at.
    pub fn last(&self) -> f64 {
        let runs = self.runs();
        match (self.direction, runs) {
            (Direction::Reverse, _) => self.start,
            (Direction::Alternate, Some(n)) if n % 2 == 0 => self.start,
            _ => self.end,
        }
    }

    /// Runs in all (`None`: forever).
    fn runs(&self) -> Option<u32> {
        match self.repeat {
            Repeat::Once => Some(1),
            Repeat::Times(n) => Some(n.max(1)),
            Repeat::Forever => None,
        }
    }

    fn span(&self) -> f64 {
        (self.end - self.start).abs()
    }

    fn seconds(&self, distance: f64) -> f64 {
        distance.abs() / self.rate.abs().max(1e-6)
    }

    /// Where playback started at `from` is after `elapsed` seconds, and
    /// whether it has ended.
    pub fn position(&self, from: f64, elapsed: f64) -> (f64, bool) {
        let span = self.span();
        if span <= 0.0 {
            return (self.start, true);
        }
        let elapsed = elapsed.max(0.0) * self.rate.abs().max(1e-6);
        // The first run goes from `from` to its end; then whole runs.
        let (first_target, forward) = match self.direction {
            Direction::Reverse => (self.start, false),
            _ => (self.end, true),
        };
        let first_len = (first_target - from).abs();
        if elapsed < first_len {
            let t = if forward {
                from + elapsed
            } else {
                from - elapsed
            };
            return (t, false);
        }
        let rest = elapsed - first_len;
        let runs_done = (rest / span).floor();
        let within = rest - runs_done * span;
        let run = runs_done as u64 + 1; // Runs finished, the first included.
        if let Some(n) = self.runs()
            && run >= u64::from(n)
        {
            return (self.last(), true);
        }
        let t = match self.direction {
            Direction::Normal => self.start + within,
            Direction::Reverse => self.end - within,
            // Run 1 went forward; even runs come back.
            Direction::Alternate => {
                if run % 2 == 1 {
                    self.end - within
                } else {
                    self.start + within
                }
            }
        };
        (t, false)
    }

    /// The composite animation of `time` playing from `from`.
    pub fn animation(&self, time: &AnimatedValue, from: f64) -> Animation {
        let leg = |to: f64, length: f64| {
            Animation::timing(
                time,
                to as f32,
                Timing::new((self.seconds(length) * 1000.0) as f32, Easing::Linear),
            )
        };
        let jump = |to: f64| Animation::timing(time, to as f32, Timing::new(0.0, Easing::Linear));
        let span = self.span();
        let (first, body) = match self.direction {
            Direction::Normal => (
                leg(self.end, self.end - from),
                Animation::sequence([jump(self.start), leg(self.end, span)]),
            ),
            Direction::Reverse => (
                leg(self.start, from - self.start),
                Animation::sequence([jump(self.end), leg(self.start, span)]),
            ),
            Direction::Alternate => (
                leg(self.end, self.end - from),
                Animation::sequence([leg(self.start, span), leg(self.end, span)]),
            ),
        };
        match self.runs() {
            Some(1) => first,
            Some(n) => {
                let more = n - 1;
                let body = match self.direction {
                    // Each body is two runs there and back.
                    Direction::Alternate => {
                        let pairs = more / 2;
                        let tail = more % 2 == 1;
                        let mut steps = Vec::new();
                        if pairs > 0 {
                            steps.push(body.looped(Some(pairs)).reset_before_iteration(false));
                        }
                        if tail {
                            steps.push(leg(self.start, span));
                        }
                        Animation::sequence(steps)
                    }
                    _ => body.looped(Some(more)).reset_before_iteration(false),
                };
                Animation::sequence([first, body])
            }
            None => Animation::sequence([first, body.looped(None).reset_before_iteration(false)]),
        }
    }
}
