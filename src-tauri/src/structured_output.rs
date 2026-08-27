//! One-time capability bindings for Structured Mode output.
//!
//! The overlay is allowed to edit the Markdown, but it must never choose or
//! re-resolve the destination window. Each emitted result receives a random
//! capability bound to the capture generation and immutable HWND/PID snapshot
//! that produced it. A successful paste consumes the capability.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use uuid::Uuid;

use crate::focus::WindowTarget;

const DEFAULT_BINDING_TTL: Duration = Duration::from_secs(30 * 60);
const INVALID_BINDING: &str = "Structured output binding is invalid or expired";

#[derive(Debug, Clone, Copy)]
struct Binding {
    id: Uuid,
    generation: u64,
    target: WindowTarget,
    issued_at: Instant,
}

/// A binding removed atomically from the registry before a paste begins.
/// It can be restored after a focus-handoff failure, but never over a newer
/// result and never after its TTL.
#[derive(Debug, Clone, Copy)]
pub struct StructuredOutputClaim(Binding);

impl StructuredOutputClaim {
    pub fn target(self) -> WindowTarget {
        self.0.target
    }
}

pub struct StructuredOutputRegistry {
    pending: Mutex<Option<Binding>>,
    ttl: Duration,
}

impl Default for StructuredOutputRegistry {
    fn default() -> Self {
        Self {
            pending: Mutex::new(None),
            ttl: DEFAULT_BINDING_TTL,
        }
    }
}

impl StructuredOutputRegistry {
    /// Issue a new capability, invalidating any older result binding.
    pub fn issue(&self, generation: u64, target: WindowTarget) -> String {
        self.issue_at(generation, target, Instant::now())
    }

    /// Atomically validate and reserve a capability for one paste attempt.
    pub fn claim(
        &self,
        binding_id: &str,
        generation: u64,
    ) -> Result<StructuredOutputClaim, String> {
        self.claim_at(binding_id, generation, Instant::now())
    }

    /// Restore a reserved capability when no output primitive ran (for
    /// example, Windows refused to focus the captured target). A newer result
    /// always wins, and expired capabilities are never resurrected.
    pub fn restore(&self, claim: StructuredOutputClaim) {
        self.restore_at(claim, Instant::now());
    }

    /// Whether a non-expired result is still available to the preview panel.
    /// This is intentionally only a UI/lifecycle signal: paste authorization
    /// still requires atomically claiming the exact random token + generation.
    pub fn has_pending(&self) -> bool {
        self.has_pending_at(Instant::now())
    }

    /// Explicitly expire a panel that the user dismissed. A stale panel cannot
    /// discard a newer binding because both token and generation must match.
    pub fn discard(&self, binding_id: &str, generation: u64) -> bool {
        let Ok(id) = Uuid::parse_str(binding_id) else {
            return false;
        };
        let mut pending = self.pending.lock().unwrap_or_else(|p| p.into_inner());
        let matches = pending
            .as_ref()
            .is_some_and(|binding| binding.id == id && binding.generation == generation);
        if matches {
            *pending = None;
        }
        matches
    }

    /// Invalidate an older panel when a new external structured result cannot
    /// be given a safe destination binding.
    pub fn invalidate(&self) {
        *self.pending.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }

    fn issue_at(&self, generation: u64, target: WindowTarget, now: Instant) -> String {
        debug_assert_ne!(generation, 0, "capture generation zero is reserved");
        let binding = Binding {
            id: Uuid::new_v4(),
            generation,
            target,
            issued_at: now,
        };
        *self.pending.lock().unwrap_or_else(|p| p.into_inner()) = Some(binding);
        binding.id.to_string()
    }

    fn claim_at(
        &self,
        binding_id: &str,
        generation: u64,
        now: Instant,
    ) -> Result<StructuredOutputClaim, String> {
        let id = Uuid::parse_str(binding_id).map_err(|_| INVALID_BINDING.to_string())?;
        let mut pending = self.pending.lock().unwrap_or_else(|p| p.into_inner());
        if pending
            .as_ref()
            .is_some_and(|binding| now.saturating_duration_since(binding.issued_at) > self.ttl)
        {
            *pending = None;
        }
        let matches = pending
            .as_ref()
            .is_some_and(|binding| binding.id == id && binding.generation == generation);
        if !matches {
            return Err(INVALID_BINDING.to_string());
        }
        Ok(StructuredOutputClaim(
            pending.take().expect("matching binding must be present"),
        ))
    }

    fn has_pending_at(&self, now: Instant) -> bool {
        let mut pending = self.pending.lock().unwrap_or_else(|p| p.into_inner());
        if pending
            .as_ref()
            .is_some_and(|binding| now.saturating_duration_since(binding.issued_at) > self.ttl)
        {
            *pending = None;
        }
        pending.is_some()
    }

    fn restore_at(&self, claim: StructuredOutputClaim, now: Instant) {
        let binding = claim.0;
        if now.saturating_duration_since(binding.issued_at) > self.ttl {
            return;
        }
        let mut pending = self.pending.lock().unwrap_or_else(|p| p.into_inner());
        if pending.is_none() {
            *pending = Some(binding);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(hwnd: isize, pid: u32) -> WindowTarget {
        WindowTarget {
            hwnd,
            pid: Some(pid),
        }
    }

    #[test]
    fn exact_token_and_generation_are_required_and_success_consumes() {
        let registry = StructuredOutputRegistry::default();
        let token = registry.issue(7, target(101, 202));

        assert!(registry.claim(&Uuid::new_v4().to_string(), 7).is_err());
        assert!(registry.claim(&token, 8).is_err());

        let claim = registry.claim(&token, 7).unwrap();
        assert_eq!(claim.target(), target(101, 202));
        assert!(registry.claim(&token, 7).is_err());
    }

    #[test]
    fn failed_attempt_can_restore_but_never_overwrites_a_newer_result() {
        let registry = StructuredOutputRegistry::default();
        let first = registry.issue(3, target(11, 12));
        let claim = registry.claim(&first, 3).unwrap();
        registry.restore(claim);
        assert_eq!(registry.claim(&first, 3).unwrap().target(), target(11, 12));

        let old = registry.issue(4, target(21, 22));
        let old_claim = registry.claim(&old, 4).unwrap();
        let current = registry.issue(5, target(31, 32));
        registry.restore(old_claim);
        assert!(registry.claim(&old, 4).is_err());
        assert_eq!(
            registry.claim(&current, 5).unwrap().target(),
            target(31, 32)
        );
    }

    #[test]
    fn expired_and_explicitly_discarded_bindings_cannot_be_claimed() {
        let registry = StructuredOutputRegistry {
            pending: Mutex::new(None),
            ttl: Duration::from_secs(10),
        };
        let now = Instant::now();
        let expired = registry.issue_at(9, target(41, 42), now);
        assert!(registry.has_pending_at(now + Duration::from_secs(10)));
        assert!(!registry.has_pending_at(now + Duration::from_secs(11)));
        assert!(registry
            .claim_at(&expired, 9, now + Duration::from_secs(11))
            .is_err());

        let discarded = registry.issue(10, target(51, 52));
        assert!(!registry.discard(&discarded, 9));
        assert!(registry.discard(&discarded, 10));
        assert!(registry.claim(&discarded, 10).is_err());
    }
}
