use std::collections::HashMap;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// Sliding window used by gateway rate limiting.
pub const RATE_LIMIT_WINDOW_SECS: u64 = 60;

/// Fallback max distinct client keys tracked in gateway rate limiter.
pub const RATE_LIMIT_MAX_KEYS_DEFAULT: usize = 10_000;

/// Fallback max distinct idempotency keys retained in gateway memory.
pub const IDEMPOTENCY_MAX_KEYS_DEFAULT: usize = 10_000;

/// How often the rate limiter sweeps stale IP entries from its map.
const RATE_LIMITER_SWEEP_INTERVAL_SECS: u64 = 300;

/// Sliding window rate limiter
#[derive(Debug)]
pub struct SlidingWindowRateLimiter {
    limit_per_window: u32,
    window: Duration,
    max_keys: usize,
    requests: Mutex<(HashMap<String, Vec<Instant>>, Instant)>,
}

impl SlidingWindowRateLimiter {
    pub fn new(limit_per_window: u32, window: Duration, max_keys: usize) -> Self {
        Self {
            limit_per_window,
            window,
            max_keys: max_keys.max(1),
            requests: Mutex::new((HashMap::new(), Instant::now())),
        }
    }

    fn prune_stale(requests: &mut HashMap<String, Vec<Instant>>, cutoff: Instant) {
        requests.retain(|_, timestamps| {
            timestamps.retain(|t| *t > cutoff);
            !timestamps.is_empty()
        });
    }

    pub fn allow(&self, key: &str) -> bool {
        if self.limit_per_window == 0 {
            return true;
        }

        let now = Instant::now();
        let cutoff = now.checked_sub(self.window).unwrap_or_else(Instant::now);

        let mut guard = self.requests.lock();
        let (requests, last_sweep) = &mut *guard;

        if last_sweep.elapsed() >= Duration::from_secs(RATE_LIMITER_SWEEP_INTERVAL_SECS) {
            Self::prune_stale(requests, cutoff);
            *last_sweep = now;
        }

        if !requests.contains_key(key) && requests.len() >= self.max_keys {
            Self::prune_stale(requests, cutoff);
            *last_sweep = now;

            if requests.len() >= self.max_keys {
                let evict_key = requests
                    .iter()
                    .min_by_key(|(_, timestamps)| timestamps.last().copied().unwrap_or(cutoff))
                    .map(|(k, _)| k.clone());
                if let Some(evict_key) = evict_key {
                    requests.remove(&evict_key);
                }
            }
        }

        let entry = requests.entry(key.to_owned()).or_default();
        entry.retain(|instant| *instant > cutoff);

        if entry.len() >= self.limit_per_window as usize {
            return false;
        }

        entry.push(now);
        true
    }
}

/// Rate limiter with separate pair and webhook limiters
#[derive(Debug)]
pub struct GatewayRateLimiter {
    pair: SlidingWindowRateLimiter,
    webhook: SlidingWindowRateLimiter,
}

impl GatewayRateLimiter {
    pub fn new(pair_per_minute: u32, webhook_per_minute: u32, max_keys: usize) -> Self {
        let window = Duration::from_secs(RATE_LIMIT_WINDOW_SECS);
        Self {
            pair: SlidingWindowRateLimiter::new(pair_per_minute, window, max_keys),
            webhook: SlidingWindowRateLimiter::new(webhook_per_minute, window, max_keys),
        }
    }

    pub fn allow_pair(&self, key: &str) -> bool {
        self.pair.allow(key)
    }

    pub fn allow_webhook(&self, key: &str) -> bool {
        self.webhook.allow(key)
    }
}

/// Idempotency store to prevent duplicate request processing
#[derive(Debug)]
pub struct IdempotencyStore {
    ttl: Duration,
    max_keys: usize,
    entries: Mutex<IdempotencyEntries>,
    #[cfg(feature = "plugins-wasm")]
    next_generation: std::sync::atomic::AtomicU64,
}

#[derive(Debug, Default)]
struct IdempotencyEntries {
    committed: HashMap<String, Instant>,
    #[cfg(feature = "plugins-wasm")]
    pending: HashMap<String, PendingIdempotencyReservation>,
}

#[cfg(feature = "plugins-wasm")]
#[derive(Debug)]
struct PendingIdempotencyReservation {
    generation: u64,
    status: tokio::sync::watch::Sender<clawcrew_api::webhook::WebhookReservationStatus>,
}

impl IdempotencyStore {
    pub fn new(ttl: Duration, max_keys: usize) -> Self {
        Self {
            ttl,
            max_keys: max_keys.max(1),
            entries: Mutex::new(IdempotencyEntries::default()),
            #[cfg(feature = "plugins-wasm")]
            next_generation: std::sync::atomic::AtomicU64::new(1),
        }
    }

    fn record_if_new(&self, key: &str) -> bool {
        let now = Instant::now();
        let mut entries = self.entries.lock();

        entries
            .committed
            .retain(|_, seen_at| now.duration_since(*seen_at) < self.ttl);

        let pending_contains = {
            #[cfg(feature = "plugins-wasm")]
            {
                entries.pending.contains_key(key)
            }
            #[cfg(not(feature = "plugins-wasm"))]
            {
                false
            }
        };
        if entries.committed.contains_key(key) || pending_contains {
            return false;
        }

        if entries.committed.len() >= self.max_keys {
            let evict_key = entries
                .committed
                .iter()
                .min_by_key(|(_, seen_at)| *seen_at)
                .map(|(k, _)| k.clone());
            if let Some(evict_key) = evict_key {
                entries.committed.remove(&evict_key);
            } else {
                return false;
            }
        }

        entries.committed.insert(key.to_owned(), now);
        true
    }
}
