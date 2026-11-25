//! Capability negotiation.
//!
//! Each side lists the features it speaks as `(name, min_version..=version)`.
//! A feature is usable on a session if both sides list it and their version
//! ranges overlap; the highest common version wins. Unknown features are
//! ignored, so new features never break older peers.

use std::collections::BTreeMap;

use continuum_proto::v1::Capability;

/// Negotiated features: name to agreed version.
pub type Negotiated = BTreeMap<String, u32>;

/// Shorthand for a capability spanning `min..=max`.
pub fn capability(name: &str, min: u32, max: u32) -> Capability {
    Capability {
        name: name.to_owned(),
        version: max,
        min_version: min,
    }
}

fn range(capability: &Capability) -> (u32, u32) {
    let max = capability.version;
    let min = match capability.min_version {
        0 => max,
        min => min.min(max),
    };
    (min, max)
}

/// The features both sides can use, at the highest shared version.
pub fn negotiate(ours: &[Capability], theirs: &[Capability]) -> Negotiated {
    let mut agreed = Negotiated::new();
    for mine in ours {
        let (my_min, my_max) = range(mine);
        for other in theirs.iter().filter(|c| c.name == mine.name) {
            let (their_min, their_max) = range(other);
            let (low, high) = (my_min.max(their_min), my_max.min(their_max));
            if high > 0 && low <= high {
                let best = agreed.entry(mine.name.clone()).or_insert(0);
                *best = (*best).max(high);
            }
        }
    }
    agreed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_highest_shared_version() {
        let ours = [capability("clipboard", 1, 3), capability("page", 1, 1)];
        let theirs = [capability("clipboard", 2, 2), capability("page", 1, 1)];
        let agreed = negotiate(&ours, &theirs);
        assert_eq!(agreed.get("clipboard"), Some(&2));
        assert_eq!(agreed.get("page"), Some(&1));
    }

    #[test]
    fn disjoint_ranges_are_dropped() {
        let agreed = negotiate(&[capability("camera", 1, 1)], &[capability("camera", 2, 3)]);
        assert!(agreed.is_empty());
    }

    #[test]
    fn unknown_features_are_ignored() {
        let agreed = negotiate(&[capability("page", 1, 1)], &[capability("sidecar", 1, 1)]);
        assert!(agreed.is_empty());
    }

    #[test]
    fn missing_min_version_means_exact() {
        let exact = Capability {
            name: "ping".into(),
            version: 2,
            min_version: 0,
        };
        assert!(negotiate(&[capability("ping", 1, 1)], std::slice::from_ref(&exact)).is_empty());
        assert_eq!(
            negotiate(&[capability("ping", 1, 2)], &[exact]).get("ping"),
            Some(&2)
        );
    }

    #[test]
    fn negotiation_is_symmetric() {
        let a = [capability("x", 1, 4), capability("y", 2, 2)];
        let b = [capability("x", 3, 5), capability("y", 1, 3)];
        assert_eq!(negotiate(&a, &b), negotiate(&b, &a));
    }
}
