use std::time::Duration;

use tokio::time::{self, Interval, MissedTickBehavior};

/// Wakes the event loop for animation while a component is busy.
/// The first tick is immediate. The default interval is 100 ms.
pub struct AnimationTimer {
    period: Duration,
    timer: Option<Interval>,
}

impl Default for AnimationTimer {
    fn default() -> Self {
        Self::new(Duration::from_millis(100))
    }
}

impl AnimationTimer {
    /// Sets the time between animation ticks, with a minimum of 1 ms.
    pub fn new(period: Duration) -> Self {
        Self {
            period: period.max(Duration::from_millis(1)),
            timer: None,
        }
    }

    pub(super) async fn tick(&mut self) {
        let timer = self.timer.get_or_insert_with(|| {
            let mut timer = time::interval(self.period);
            timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
            timer
        });
        timer.tick().await;
    }
}
