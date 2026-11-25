//! When to make this machine visible as a Quick Share receiver.

use std::time::Duration;

use continuum_core::clock::Timestamp;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    /// Never advertise.
    Off,
    /// Advertise only for `window` after an Android sender's BLE trigger was
    /// last seen. This is the default: the device name is not broadcast on
    /// the LAN all the time.
    WhenSenderNearby { window: Duration },
    /// Advertise all the time, like NearDrop. Works without Bluetooth.
    Always,
}

impl Default for Visibility {
    fn default() -> Self {
        Self::WhenSenderNearby {
            window: Duration::from_secs(60),
        }
    }
}

/// Decides whether the Quick Share mDNS record should be published.
#[derive(Debug, Clone, Default)]
pub struct ReceiverVisibility {
    mode: Visibility,
    last_trigger: Option<Timestamp>,
}

impl ReceiverVisibility {
    pub fn new(mode: Visibility) -> Self {
        Self {
            mode,
            last_trigger: None,
        }
    }

    pub fn set_mode(&mut self, mode: Visibility) {
        self.mode = mode;
    }

    /// Call whenever a BLE scan reports a Quick Share trigger.
    pub fn on_trigger(&mut self, at: Timestamp) {
        self.last_trigger = Some(self.last_trigger.map_or(at, |last| last.max(at)));
    }

    pub fn should_advertise(&self, now: Timestamp) -> bool {
        match self.mode {
            Visibility::Off => false,
            Visibility::Always => true,
            Visibility::WhenSenderNearby { window } => self
                .last_trigger
                .is_some_and(|seen| now.saturating_since(seen) < window),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: u64) -> Timestamp {
        Timestamp::from_millis(secs * 1000)
    }

    #[test]
    fn advertises_only_inside_trigger_window() {
        let mut visibility = ReceiverVisibility::default();
        assert!(!visibility.should_advertise(at(0)));
        visibility.on_trigger(at(10));
        assert!(visibility.should_advertise(at(69)));
        assert!(!visibility.should_advertise(at(70)));
        visibility.on_trigger(at(65));
        assert!(visibility.should_advertise(at(100)));
    }

    #[test]
    fn off_and_always_ignore_triggers() {
        let mut off = ReceiverVisibility::new(Visibility::Off);
        off.on_trigger(at(0));
        assert!(!off.should_advertise(at(0)));
        assert!(ReceiverVisibility::new(Visibility::Always).should_advertise(at(0)));
    }
}
