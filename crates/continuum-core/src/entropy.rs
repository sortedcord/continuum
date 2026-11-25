//! Randomness abstraction, so key generation and identifier rotation can be
//! made deterministic in tests.

use sha2::{Digest, Sha256};

pub trait Entropy {
    fn fill(&mut self, buf: &mut [u8]);

    fn array<const N: usize>(&mut self) -> [u8; N]
    where
        Self: Sized,
    {
        let mut out = [0; N];
        self.fill(&mut out);
        out
    }
}

/// The operating system's CSPRNG. Use this for anything real.
#[derive(Debug, Clone, Copy, Default)]
pub struct OsEntropy;

impl Entropy for OsEntropy {
    fn fill(&mut self, buf: &mut [u8]) {
        // Without an OS RNG we cannot generate keys at all; there is no
        // meaningful way to continue.
        getrandom::fill(buf).expect("operating system RNG unavailable");
    }
}

/// Reproducible byte stream for tests and simulations.
///
/// **Not secure.** Output is `SHA-256(seed || counter)`, so anyone who knows
/// the seed can predict every byte.
#[derive(Debug, Clone)]
pub struct DeterministicEntropy {
    seed: u64,
    counter: u64,
    block: [u8; 32],
    used: usize,
}

impl DeterministicEntropy {
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            counter: 0,
            block: [0; 32],
            used: 32,
        }
    }

    fn refill(&mut self) {
        let mut hasher = Sha256::new();
        hasher.update(self.seed.to_be_bytes());
        hasher.update(self.counter.to_be_bytes());
        self.block.copy_from_slice(&hasher.finalize());
        self.counter += 1;
        self.used = 0;
    }
}

impl Entropy for DeterministicEntropy {
    fn fill(&mut self, buf: &mut [u8]) {
        for byte in buf {
            if self.used == self.block.len() {
                self.refill();
            }
            *byte = self.block[self.used];
            self.used += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_entropy_is_reproducible() {
        let a: [u8; 80] = DeterministicEntropy::new(7).array();
        let b: [u8; 80] = DeterministicEntropy::new(7).array();
        let c: [u8; 80] = DeterministicEntropy::new(8).array();
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn os_entropy_produces_varying_output() {
        let a: [u8; 32] = OsEntropy.array();
        let b: [u8; 32] = OsEntropy.array();
        assert_ne!(a, b);
    }
}
