//! `galleon-channel-core` — shared peer-policy and approval helpers for the
//! RF-B1 channel feature-crates (Signal first).
//!
//! The split pulls one channel at a time out of the monolithic
//! `clawcrew-channels` crate. Two couplings every channel needs — the allowlist
//! policy and the text-reply approval prompt — are provided here so a feature
//! crate does not have to reimplement them or reach back into
//! `clawcrew-channels` internals. Neither helper drags `clawcrew-runtime`:
//! `clawcrew-config` depends only on api/log/infra/macros, and the approval
//! helper here is the i18n-free half.
//!
//! ## [`allowlist`] — seam #1, closed
//!
//! The grant / deny / `!name` / wildcard peer-policy is security-critical and
//! has a single source of truth in `clawcrew_config::schema`
//! (`peer_policy_admits` and friends). `clawcrew-channels::allowlist` is a thin
//! wrapper over it. This module **re-exports that same policy** rather than
//! reimplementing the precedence rules — reuse over a second copy that could
//! drift. A channel crate gets the full policy (not just exact-match) by
//! calling [`allowlist::is_user_allowed`] / [`allowlist::is_identity_allowed`].
//!
//! ## [`approval`] — seam #2, i18n-free half
//!
//! [`approval::parse_approval_reply`], [`approval::new_approval_token`] and
//! [`approval::build_yesno_prompt`] are the parts of the upstream
//! `clawcrew-channels::util` approval helper that depend only on
//! `clawcrew-api`. The upstream prompt localizes its heading/labels through
//! `clawcrew_runtime::i18n`; porting that would drag the runtime, so the prompt
//! here is plain English of the IDENTICAL wire shape (`<token> yes|no|always`),
//! and the reply parser is shared verbatim. Localization stays the documented
//! ceiling (`APPROVAL_I18N_CEILING`) — the one part of seam #2 that cannot
//! close without the runtime.

/// Peer-policy allowlist — re-exported from the `clawcrew_config::schema` SSOT.
///
/// `clawcrew-channels::allowlist` wraps the same functions; sharing them here
/// keeps the grant/deny/precedence rules defined once. See the crate-level docs
/// for why this is a re-export and not a reimplementation.
pub mod allowlist {
    /// Case-sensitivity selector for the comparison. The channel decides which
    /// applies; the matcher does not infer it.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Match {
        /// Exact `==` match (E.164 numbers, UUIDs).
        Sensitive,
        /// `eq_ignore_ascii_case` (IRC nicks, Matrix MXIDs).
        CaseInsensitive,
    }

    /// `!`-prefix that marks a resolved peer entry as a deny rule.
    pub use clawcrew_config::schema::PEER_DENY_PREFIX as DENY_PREFIX;
    use clawcrew_config::schema::{peer_policy_admits, peer_policy_denies};

    fn matcher_for(mode: Match) -> impl Fn(&str, &str) -> bool {
        move |entry: &str, user: &str| match mode {
            Match::Sensitive => entry == user,
            Match::CaseInsensitive => entry.eq_ignore_ascii_case(user),
        }
    }

    /// Whether an account is authorized across every identifier it is known by,
    /// against a single snapshot of the resolved peer list — the full
    /// grant/deny/`!name`/wildcard policy, not just exact match.
    #[must_use]
    pub fn is_identity_allowed_by(
        allowed: &[String],
        identities: &[&str],
        match_fn: impl Fn(&str, &str) -> bool,
    ) -> bool {
        peer_policy_admits(allowed, identities, match_fn)
    }

    /// [`is_identity_allowed_by`] with the shared case-sensitivity selector.
    #[must_use]
    pub fn is_identity_allowed(allowed: &[String], identities: &[&str], mode: Match) -> bool {
        is_identity_allowed_by(allowed, identities, matcher_for(mode))
    }

    /// Single-identifier convenience over [`is_identity_allowed`].
    #[must_use]
    pub fn is_user_allowed(allowed: &[String], user: &str, mode: Match) -> bool {
        is_identity_allowed_by(allowed, &[user], matcher_for(mode))
    }

    /// Single-identifier form with a caller-provided matcher (phone/email
    /// normalization etc.).
    #[must_use]
    pub fn is_user_allowed_by(
        allowed: &[String],
        user: &str,
        match_fn: impl Fn(&str, &str) -> bool,
    ) -> bool {
        is_identity_allowed_by(allowed, &[user], match_fn)
    }

    /// Whether any identifier of one account is explicitly denied, independent
    /// of any grant.
    #[must_use]
    pub fn is_identity_denied_by(
        allowed: &[String],
        identities: &[&str],
        match_fn: impl Fn(&str, &str) -> bool,
    ) -> bool {
        peer_policy_denies(allowed, identities, match_fn)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        // One runnable check per non-trivial behavior: that the re-exported
        // policy really is the deny/wildcard SSOT and not exact-match-only.
        #[test]
        fn reexports_full_deny_and_wildcard_policy() {
            // Wildcard grants anyone...
            assert!(is_user_allowed(&["*".to_string()], "anyone", Match::Sensitive));
            // ...but a `!name` deny outranks it (the policy the signal crate's
            // exact-match ceiling could not express).
            let denied = vec!["*".to_string(), "!alice".to_string()];
            assert!(!is_user_allowed(&denied, "alice", Match::Sensitive));
            assert!(is_user_allowed(&denied, "bob", Match::Sensitive));
            // Empty list denies.
            assert!(!is_user_allowed(&[], "alice", Match::Sensitive));
            // Identity-level deny on one alias beats a wildcard reached via
            // another alias.
            let eq = |e: &str, u: &str| e == u;
            let list = vec!["*".to_string(), "!alice.example".to_string()];
            assert!(!is_identity_allowed_by(
                &list,
                &["alice.example", "did:plc:alice"],
                eq
            ));
        }
    }
}

/// Text-reply approval helper (i18n-free half of the upstream
/// `clawcrew-channels::util` prompt). See crate docs.
pub mod approval {
    use clawcrew_api::channel::ChannelApprovalResponse;

    pub const APPROVAL_REPLY_YES: &str = "yes";
    pub const APPROVAL_REPLY_YES_SHORT: &str = "y";
    pub const APPROVAL_REPLY_APPROVE: &str = "approve";
    pub const APPROVAL_REPLY_NO: &str = "no";
    pub const APPROVAL_REPLY_NO_SHORT: &str = "n";
    pub const APPROVAL_REPLY_DENY: &str = "deny";
    pub const APPROVAL_REPLY_ALWAYS: &str = "always";

    /// Parse a `<token> <action>` approval reply. Depends only on
    /// `clawcrew-api`; the wire shape is identical to upstream so prompts built
    /// by [`build_yesno_prompt`] round-trip through it.
    #[must_use]
    pub fn parse_approval_reply(text: &str) -> Option<(String, ChannelApprovalResponse)> {
        let lower = text.trim().to_lowercase();
        let mut parts = lower.splitn(2, ' ');
        let token = parts.next()?.to_string();
        if token.len() != 6 || !token.chars().all(|c| c.is_ascii_alphanumeric()) {
            return None;
        }
        let action_word = parts.next()?.split_whitespace().next()?;
        let response = match action_word {
            APPROVAL_REPLY_YES | APPROVAL_REPLY_YES_SHORT | APPROVAL_REPLY_APPROVE => {
                ChannelApprovalResponse::Approve
            }
            APPROVAL_REPLY_NO | APPROVAL_REPLY_NO_SHORT | APPROVAL_REPLY_DENY => {
                ChannelApprovalResponse::Deny
            }
            APPROVAL_REPLY_ALWAYS => ChannelApprovalResponse::AlwaysApprove,
            _ => return None,
        };
        Some((token, response))
    }

    /// 6-char lowercase-alphanumeric approval token.
    #[must_use]
    pub fn new_approval_token() -> String {
        use rand::RngExt;
        const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
        let mut rng = rand::rng();
        (0..6)
            .map(|_| CHARSET[rng.random_range(0..CHARSET.len())] as char)
            .collect()
    }

    /// Plain-English yes/no/always approval prompt of the exact wire shape
    /// [`parse_approval_reply`] expects.
    ///
    /// APPROVAL_I18N_CEILING: the upstream prompt localizes heading/labels via
    /// `clawcrew_runtime::i18n`. Porting the Fluent catalogue would drag the
    /// runtime, so this stays plain English. `token`/`tool_name`/`args` are the
    /// protocol-exact values echoed verbatim — never localized — so the ceiling
    /// cannot desync the prompt from the parser.
    #[must_use]
    pub fn build_yesno_prompt(
        token: &str,
        tool_name: &str,
        arguments_summary: &str,
        position: Option<(u32, u32)>,
    ) -> String {
        let position_line = match position {
            Some((_, total)) if total <= 1 => String::new(),
            Some((index, total)) => format!("Tool call {index} of {total}\n"),
            None => String::new(),
        };
        format!(
            "APPROVAL REQUIRED [{token}]\n{position_line}Tool: {tool_name}\nArgs: {arguments_summary}\n\nReply `{token} {APPROVAL_REPLY_YES}`, `{token} {APPROVAL_REPLY_NO}`, or `{token} {APPROVAL_REPLY_ALWAYS}`."
        )
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn prompt_roundtrips_through_the_parser() {
            let token = new_approval_token();
            assert_eq!(token.len(), 6);
            let prompt = build_yesno_prompt(&token, "shell", "ls -la", Some((2, 3)));
            assert!(prompt.contains(&format!("[{token}]")));
            assert!(prompt.contains("Tool call 2 of 3"));
            let (parsed, resp) =
                parse_approval_reply(&format!("{token} yes")).expect("yes parses");
            assert_eq!(parsed, token);
            assert!(matches!(resp, ChannelApprovalResponse::Approve));
            assert!(matches!(
                parse_approval_reply(&format!("{token} always")).unwrap().1,
                ChannelApprovalResponse::AlwaysApprove
            ));
            // A single-call batch shows no position line.
            assert!(!build_yesno_prompt(&token, "shell", "x", Some((1, 1))).contains("Tool call"));
            assert!(parse_approval_reply("notoken yes").is_none());
        }
    }
}
