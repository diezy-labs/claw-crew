//! `galleon-channel-signal` — RF-B1 proof-of-pattern.
//!
//! This crate demonstrates splitting ONE Tier B channel (`signal`) out of the
//! monolithic `clawcrew-channels` crate into an independent feature-crate. It is
//! a **skeleton**, not a migration: it implements the minimal [`Channel`]
//! surface so `cargo check` proves the pattern compiles against the real trait.
//! The behavioral port of `clawcrew-channels::signal` is deferred — every body
//! that would carry ported logic is a clearly-marked `todo!("RF-B1: ...")`.
//!
//! Why `signal`: upstream `channel-signal = []` has zero optional deps (verified
//! in `crates/clawcrew-channels/Cargo.toml`), so this crate depends only on the
//! `Channel` trait surface in `clawcrew-api`.

use clawcrew_api::attribution::{Attributable, ChannelKind, Role};
use clawcrew_api::channel::{Channel, ChannelMessage, SendMessage};

/// Signal channel handle (skeleton).
///
/// Fields the real port will carry (signal-cli endpoint, account number,
/// attachment dir, …) are intentionally omitted until RF-B1 moves the body
/// over; the config-bound alias is the one piece attribution needs now.
pub struct SignalChannel {
    alias: String,
}

impl SignalChannel {
    /// Construct a skeleton handle bound to a config alias.
    pub fn new(alias: impl Into<String>) -> Self {
        Self {
            alias: alias.into(),
        }
    }
}

impl Attributable for SignalChannel {
    fn role(&self) -> Role {
        Role::Channel(ChannelKind::Signal)
    }

    fn alias(&self) -> &str {
        &self.alias
    }
}

#[async_trait::async_trait]
impl Channel for SignalChannel {
    fn name(&self) -> &str {
        "signal"
    }

    async fn send(&self, _message: &SendMessage) -> anyhow::Result<()> {
        todo!("RF-B1: port send() from clawcrew-channels::signal")
    }

    async fn listen(
        &self,
        _tx: tokio::sync::mpsc::Sender<ChannelMessage>,
    ) -> anyhow::Result<()> {
        todo!("RF-B1: port listen() from clawcrew-channels::signal")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Smallest runnable check: the skeleton satisfies the real `Channel`
    // trait object and reports its identity without touching a `todo!` body.
    #[test]
    fn skeleton_is_a_channel() {
        let ch = SignalChannel::new("primary");
        let dyn_ch: &dyn Channel = &ch;
        assert_eq!(dyn_ch.name(), "signal");
        assert_eq!(ch.alias(), "primary");
        assert!(matches!(ch.role(), Role::Channel(ChannelKind::Signal)));
    }
}
