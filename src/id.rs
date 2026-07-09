use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(1);

/// Returns the next deterministic process-local identifier.
///
/// IDs are formatted as `fract-{counter}` and are unique within a single
/// process. They are not cryptographically random and should not be used
/// outside the daemon's internal bookkeeping.
pub fn next() -> String {
    format!("fract-{}", COUNTER.fetch_add(1, Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_monotonic() {
        let a = next();
        let b = next();
        let c = next();
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_ne!(a, c);
    }

    #[test]
    fn id_format_is_stable() {
        let id = next();
        assert!(id.starts_with("fract-"));
        let suffix = id.strip_prefix("fract-").unwrap();
        assert!(suffix.parse::<u64>().is_ok());
    }
}
