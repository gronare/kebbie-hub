use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use kebbie_hub_protocol::Id;
use tokio::sync::watch;

const FIRST_PRUNE: usize = 1024;

type Channel = watch::Sender<u64>;

struct Channels {
    by_key: HashMap<String, Weak<Channel>>,
    prune_at: usize,
}

pub struct Wakeups {
    channels: Mutex<Channels>,
}

pub struct Subscription {
    _channel: Arc<Channel>,
    receiver: watch::Receiver<u64>,
}

impl Subscription {
    pub fn mark_seen(&mut self) {
        self.receiver.borrow_and_update();
    }

    pub async fn changed(&mut self) {
        let _ = self.receiver.changed().await;
    }

    #[cfg(test)]
    pub fn woken(&self) -> bool {
        self.receiver.has_changed().unwrap_or(false)
    }
}

pub fn machine_key(machine: &Id) -> String {
    format!("machine:{machine}")
}

pub fn device_key(device: &Id) -> String {
    format!("device:{device}")
}

pub fn account_key(account: &str) -> String {
    format!("account:{account}")
}

pub fn pairing_key(pairing: &str) -> String {
    format!("pairing:{pairing}")
}

impl Wakeups {
    pub fn new() -> Wakeups {
        Wakeups {
            channels: Mutex::new(Channels {
                by_key: HashMap::new(),
                prune_at: FIRST_PRUNE,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Channels> {
        self.channels
            .lock()
            .expect("the wakeups lock is never poisoned")
    }

    pub fn subscribe(&self, key: &str) -> Subscription {
        let mut channels = self.lock();
        if let Some(channel) = channels.by_key.get(key).and_then(Weak::upgrade) {
            return Subscription {
                receiver: channel.subscribe(),
                _channel: channel,
            };
        }
        if channels.by_key.len() >= channels.prune_at {
            channels
                .by_key
                .retain(|_, channel| channel.strong_count() > 0);
            channels.prune_at = (channels.by_key.len() * 2).max(FIRST_PRUNE);
        }
        let channel = Arc::new(watch::Sender::new(0));
        channels
            .by_key
            .insert(key.to_string(), Arc::downgrade(&channel));
        Subscription {
            receiver: channel.subscribe(),
            _channel: channel,
        }
    }

    pub fn wake(&self, key: &str) {
        let channel = self.lock().by_key.get(key).and_then(Weak::upgrade);
        if let Some(channel) = channel {
            channel.send_modify(|count| *count = count.wrapping_add(1));
        }
    }

    pub fn forget_idle(&self) {
        self.lock()
            .by_key
            .retain(|_, channel| channel.strong_count() > 0);
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.lock().by_key.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn waking_one_key_leaves_every_other_waiter_asleep() {
        let wakeups = Wakeups::new();
        let mut first = wakeups.subscribe("machine:a");
        let mut also_first = wakeups.subscribe("machine:a");
        let other = wakeups.subscribe("machine:b");
        first.mark_seen();
        wakeups.wake("machine:a");
        first.changed().await;
        also_first.changed().await;
        assert!(!other.woken());
    }

    #[test]
    fn a_wake_before_the_wait_is_not_lost() {
        let wakeups = Wakeups::new();
        let mut waiter = wakeups.subscribe("pairing:p");
        waiter.mark_seen();
        wakeups.wake("pairing:p");
        assert!(waiter.woken());
    }

    #[test]
    fn keys_nobody_waits_on_are_forgotten() {
        let wakeups = Wakeups::new();
        let kept = wakeups.subscribe("device:kept");
        drop(wakeups.subscribe("device:gone"));
        wakeups.wake("device:nobody");
        wakeups.forget_idle();
        assert_eq!(wakeups.len(), 1);
        drop(kept);
    }
}
