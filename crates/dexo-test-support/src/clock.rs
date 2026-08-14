use std::{
    sync::Mutex,
    time::{Duration, SystemTime},
};

pub trait Clock: Send + Sync {
    fn now(&self) -> SystemTime;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

pub struct FakeClock(Mutex<SystemTime>);

impl FakeClock {
    pub fn new(now: SystemTime) -> Self {
        Self(Mutex::new(now))
    }

    pub fn advance(&self, by: Duration) {
        // ponytail: Mutex poison panics; switch to a cell/atomic clock if tests share one clock across threads that panic
        let mut now = self.0.lock().unwrap();
        *now += by;
    }
}

impl Clock for FakeClock {
    fn now(&self) -> SystemTime {
        *self.0.lock().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::{Clock, FakeClock};
    use std::time::{Duration, SystemTime};
    #[test]
    fn fake_clock_advances_deterministically() {
        let start = SystemTime::UNIX_EPOCH;
        let clock = FakeClock::new(start);
        clock.advance(Duration::from_secs(5));
        assert_eq!(clock.now(), start + Duration::from_secs(5));
    }
}
