use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug)]
pub struct RateLimit {
    pub burst: u32,
    pub per_minute: u32,
}

impl Default for RateLimit {
    fn default() -> RateLimit {
        RateLimit {
            burst: 60,
            per_minute: 120,
        }
    }
}

struct Bucket {
    tokens: f64,
    filled_at: Instant,
}

pub struct Limiter {
    limit: RateLimit,
    max_buckets: usize,
    buckets: Mutex<HashMap<String, Bucket>>,
}

const IDLE_BUCKET: Duration = Duration::from_secs(600);
pub const MAX_BUCKETS: usize = 100_000;

pub fn address_key(address: IpAddr) -> String {
    match address {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => v4.to_string(),
            None => {
                let [a, b, c, d, ..] = v6.segments();
                format!("{a:x}:{b:x}:{c:x}:{d:x}::/64")
            }
        },
    }
}

impl Limiter {
    pub fn new(limit: RateLimit, max_buckets: usize) -> Limiter {
        Limiter {
            limit,
            max_buckets: max_buckets.max(1),
            buckets: Mutex::new(HashMap::new()),
        }
    }

    fn refill(&self, bucket: &mut Bucket, now: Instant) {
        let burst = f64::from(self.limit.burst);
        let per_second = f64::from(self.limit.per_minute) / 60.0;
        let elapsed = now.duration_since(bucket.filled_at).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * per_second).min(burst);
        bucket.filled_at = now;
    }

    pub fn exhausted(&self, key: &str) -> bool {
        let now = Instant::now();
        let mut buckets = self
            .buckets
            .lock()
            .expect("the limiter lock is never poisoned");
        match buckets.get_mut(key) {
            Some(bucket) => {
                self.refill(bucket, now);
                bucket.tokens < 1.0
            }
            None => false,
        }
    }

    pub fn allow(&self, key: &str) -> bool {
        let now = Instant::now();
        let mut buckets = self
            .buckets
            .lock()
            .expect("the limiter lock is never poisoned");
        if !buckets.contains_key(key) && buckets.len() >= self.max_buckets {
            buckets.retain(|_, bucket| now.duration_since(bucket.filled_at) < IDLE_BUCKET);
            if buckets.len() >= self.max_buckets
                && let Some(oldest) = buckets
                    .iter()
                    .min_by_key(|(_, bucket)| bucket.filled_at)
                    .map(|(key, _)| key.clone())
            {
                buckets.remove(&oldest);
            }
        }
        let bucket = buckets.entry(key.to_string()).or_insert(Bucket {
            tokens: f64::from(self.limit.burst),
            filled_at: now,
        });
        self.refill(bucket, now);
        if bucket.tokens < 1.0 {
            return false;
        }
        bucket.tokens -= 1.0;
        true
    }

    pub fn forget_idle(&self) {
        let now = Instant::now();
        self.buckets
            .lock()
            .expect("the limiter lock is never poisoned")
            .retain(|_, bucket| now.duration_since(bucket.filled_at) < IDLE_BUCKET);
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.buckets
            .lock()
            .expect("the limiter lock is never poisoned")
            .len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv6_addresses_share_a_bucket_per_64() {
        let one: IpAddr = "2001:db8:1:2:aaaa::1".parse().unwrap();
        let two: IpAddr = "2001:db8:1:2:ffff::2".parse().unwrap();
        let other: IpAddr = "2001:db8:1:3::1".parse().unwrap();
        assert_eq!(address_key(one), address_key(two));
        assert_ne!(address_key(one), address_key(other));
        assert_eq!(
            address_key("::ffff:192.0.2.7".parse().unwrap()),
            "192.0.2.7"
        );
    }

    #[test]
    fn the_limiter_never_holds_more_buckets_than_its_cap() {
        let limiter = Limiter::new(RateLimit::default(), 3);
        for key in ["a", "b", "c", "d", "e"] {
            assert!(limiter.allow(key));
        }
        assert_eq!(limiter.len(), 3);
    }

    #[test]
    fn checking_a_bucket_does_not_spend_it() {
        let limiter = Limiter::new(
            RateLimit {
                burst: 1,
                per_minute: 1,
            },
            10,
        );
        assert!(!limiter.exhausted("a"));
        assert!(limiter.allow("a"));
        assert!(limiter.exhausted("a"));
        assert!(!limiter.allow("a"));
    }
}
