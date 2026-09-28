//! Logging setup built on `tracing`.

use std::sync::OnceLock;
use tracing_subscriber::EnvFilter;

/// Environment variable that controls the log filter, e.g. `KILN_LOG=debug` or
/// `KILN_LOG=info,kiln_render=trace`.
pub const LOG_ENV_VAR: &str = "KILN_LOG";

const DEFAULT_FILTER: &str = "info";

static INIT: OnceLock<bool> = OnceLock::new();

/// Install the global `tracing` subscriber.
///
/// Safe to call any number of times from any thread: only the first call installs the
/// subscriber. Returns `true` if Kiln's subscriber is active, or `false` if another
/// subscriber was already installed by the host application.
pub fn init_logging() -> bool {
    *INIT.get_or_init(|| {
        let filter = EnvFilter::try_from_env(LOG_ENV_VAR)
            .unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));
        tracing_subscriber::fmt().with_env_filter(filter).with_target(true).try_init().is_ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TC-CORE-05: repeated and concurrent initialization never panics and is consistent.
    #[test]
    fn tc_core_05_init_is_idempotent() {
        let first = init_logging();
        let results: Vec<bool> = std::thread::scope(|s| {
            let hs: Vec<_> = (0..8).map(|_| s.spawn(init_logging)).collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert!(results.iter().all(|&r| r == first));
        assert_eq!(init_logging(), first);
        tracing::info!("logging initialized once");
    }
}
