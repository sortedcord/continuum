//! "Find My Device" paging (capability `page`, version 1).

use continuum_core::capabilities::capability;
use continuum_core::proto::features::page::v1 as wire;
use continuum_core::proto::v1::Capability;
use prost::Message;

pub const CAPABILITY: &str = "page";

pub fn capability_v1() -> Capability {
    capability(CAPABILITY, 1, 1)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageAction {
    /// Ring for this many seconds, or until stopped.
    Start { duration_s: u32 },
    /// The pager cancelled.
    Stop,
    /// The user found the device.
    Found,
}

/// Longest ring a peer may request.
pub const MAX_DURATION_S: u32 = 120;

pub fn encode(action: PageAction) -> Vec<u8> {
    let action = match action {
        PageAction::Start { duration_s } => wire::page::Action::Start(wire::Start { duration_s }),
        PageAction::Stop => wire::page::Action::Stop(wire::Stop {}),
        PageAction::Found => wire::page::Action::Found(wire::Found {}),
    };
    wire::Page {
        action: Some(action),
    }
    .encode_to_vec()
}

/// Decodes a page message; ring durations are clamped to
/// [`MAX_DURATION_S`] (0 means the default of 30 s).
pub fn decode(body: &[u8]) -> Option<PageAction> {
    let message = wire::Page::decode(body).ok()?;
    Some(match message.action? {
        wire::page::Action::Start(start) => PageAction::Start {
            duration_s: match start.duration_s {
                0 => 30,
                d => d.min(MAX_DURATION_S),
            },
        },
        wire::page::Action::Stop(_) => PageAction::Stop,
        wire::page::Action::Found(_) => PageAction::Found,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        for action in [
            PageAction::Start { duration_s: 20 },
            PageAction::Stop,
            PageAction::Found,
        ] {
            assert_eq!(decode(&encode(action)), Some(action));
        }
    }

    #[test]
    fn clamps_duration() {
        let long = encode(PageAction::Start { duration_s: 9999 });
        assert_eq!(
            decode(&long),
            Some(PageAction::Start {
                duration_s: MAX_DURATION_S
            })
        );
        let zero = encode(PageAction::Start { duration_s: 0 });
        assert_eq!(decode(&zero), Some(PageAction::Start { duration_s: 30 }));
        assert_eq!(decode(&[0xff]), None);
    }
}
