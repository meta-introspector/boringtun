use std::time::Duration;

#[derive(Clone, Copy, Debug)]
pub struct Instant(Duration);

impl Instant {
    pub fn now() -> Self {
        Self(Duration::ZERO)
    }
    pub fn duration_since(&self, earlier: Instant) -> Duration {
        self.0.saturating_sub(earlier.0)
    }
}
