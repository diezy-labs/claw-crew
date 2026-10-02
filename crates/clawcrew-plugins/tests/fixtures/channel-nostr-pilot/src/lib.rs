//! Nostr pilot channel plugin.
//!
//! Proves the WIT `channel-plugin` contract end to end without porting full
//! nostr logic. Channels get no `wasi:http` outbound, so this is INBOUND-FIRST:
//! `configure` resolves scoped config + secret at point of use, `poll-message`
//! drains the host inbound queue, and `parse-webhook` authenticates an inbound
//! request against the scoped relay secret before decoding its JSON payload.

#[cfg(target_family = "wasm")]
mod component {
    wit_bindgen::generate!({
        path: "../../../../../wit/v0",
        world: "channel-plugin",
        features: ["plugins-wit-v0"],
    });

    use exports::clawcrew::plugin::channel::{
        ApprovalRequest, ApprovalResponse, ChannelCapabilities, Guest as Channel, InboundMessage,
        SendMessage, WebhookRejection, WebhookRequest, WebhookResponse,
    };
    use exports::clawcrew::plugin::plugin_info::Guest as PluginInfo;
    use clawcrew::plugin::config::get as config_get;
    use clawcrew::plugin::secrets::get as secret_get;

    struct NostrPilot;

    impl PluginInfo for NostrPilot {
        fn plugin_name() -> String {
            "channel-nostr-pilot".to_string()
        }

        fn plugin_version() -> String {
            "0.0.0".to_string()
        }
    }

    impl Channel for NostrPilot {
        fn name() -> String {
            "channel-nostr-pilot".to_string()
        }

        fn configure() -> Result<(), String> {
            // Resolve the public config and the scoped secret in one frame; both
            // imports share one canonical revision. Values are not retained for
            // later operations — each export re-resolves at point of use.
            let config = config_get().map_err(|_| "public config unavailable".to_string())?;
            let public: serde_json::Value =
                serde_json::from_str(&config).map_err(|_| "public config not an object".to_string())?;
            if public
                .get("relay_url")
                .and_then(serde_json::Value::as_str)
                .is_none_or(str::is_empty)
            {
                return Err("expected non-empty relay_url config".to_string());
            }
            if public.get("relay_secret").is_some() {
                return Err("secret property leaked into public config".to_string());
            }
            let secret = secret_get("relay_secret")
                .map_err(|_| "expected scoped relay_secret".to_string())?;
            if secret.is_empty() {
                return Err("expected non-empty relay_secret".to_string());
            }
            Ok(())
        }

        fn send(_message: SendMessage) -> Result<(), String> {
            // Channels have no outbound HTTP; a real nostr relay publish would
            // need a host egress surface this world does not grant. Pilot stub.
            Err("channel-nostr-pilot is inbound-first: outbound send is unsupported".to_string())
        }

        fn poll_message() -> Option<InboundMessage> {
            // Drain the host-run inbound queue; the host stamps channel/alias.
            let message = clawcrew::plugin::inbound::inbound_poll()?;
            Some(InboundMessage {
                id: message.id,
                sender: message.sender,
                reply_target: message.reply_target,
                content: message.content,
                channel: message.channel,
                channel_alias: message.channel_alias,
                timestamp: message.timestamp,
                thread_ts: message.thread_ts,
                interruption_scope_id: message.interruption_scope_id,
                attachments: Vec::new(),
                subject: message.subject,
            })
        }

        fn get_channel_capabilities() -> ChannelCapabilities {
            ChannelCapabilities::HEALTH_CHECK | ChannelCapabilities::WEBHOOK_INGRESS
        }

        fn health_check() -> bool {
            true
        }

        fn self_handle() -> Option<String> {
            None
        }

        fn self_addressed_mention() -> Option<String> {
            None
        }

        fn drop_self_message(_msg: InboundMessage) -> bool {
            false
        }

        fn start_typing(_recipient: String) -> Result<(), String> {
            Ok(())
        }

        fn stop_typing(_recipient: String) -> Result<(), String> {
            Ok(())
        }

        fn supports_draft_updates() -> bool {
            false
        }

        fn send_draft(_message: SendMessage) -> Result<Option<String>, String> {
            Ok(None)
        }

        fn update_draft(
            _recipient: String,
            _message_id: String,
            _text: String,
        ) -> Result<(), String> {
            Ok(())
        }

        fn update_draft_progress(
            _recipient: String,
            _message_id: String,
            _text: String,
        ) -> Result<(), String> {
            Ok(())
        }

        fn finalize_draft(
            _recipient: String,
            _message_id: String,
            _final_text: String,
        ) -> Result<(), String> {
            Ok(())
        }

        fn cancel_draft(_recipient: String, _message_id: String) -> Result<(), String> {
            Ok(())
        }

        fn supports_multi_message_streaming() -> bool {
            false
        }

        fn multi_message_delay_ms() -> u64 {
            800
        }

        fn add_reaction(
            _channel: String,
            _message_id: String,
            _emoji: String,
        ) -> Result<(), String> {
            Ok(())
        }

        fn remove_reaction(
            _channel: String,
            _message_id: String,
            _emoji: String,
        ) -> Result<(), String> {
            Ok(())
        }

        fn pin_message(_channel: String, _message_id: String) -> Result<(), String> {
            Ok(())
        }

        fn unpin_message(_channel: String, _message_id: String) -> Result<(), String> {
            Ok(())
        }

        fn redact_message(
            _channel: String,
            _message_id: String,
            _reason: Option<String>,
        ) -> Result<(), String> {
            Ok(())
        }

        fn request_approval(
            _recipient: String,
            _request: ApprovalRequest,
        ) -> Result<Option<ApprovalResponse>, String> {
            Ok(None)
        }

        fn request_choice(
            _question: String,
            _choices: Vec<String>,
            _timeout_secs: u64,
        ) -> Result<Option<String>, String> {
            Ok(None)
        }

        fn supports_free_form_ask() -> bool {
            true
        }

        fn webhook_path() -> Option<String> {
            Some("nostr-pilot".to_string())
        }

        fn parse_webhook(request: WebhookRequest) -> Result<WebhookResponse, WebhookRejection> {
            let WebhookRequest {
                method: _,
                query: _,
                headers,
                body,
            } = request;

            // Authenticate against the scoped relay secret at point of use.
            let secret = secret_get("relay_secret").map_err(|_| {
                WebhookRejection::Unauthorized("scoped relay secret unavailable".to_string())
            })?;
            let supplied = headers
                .iter()
                .find(|(name, _)| name == "x-nostr-secret")
                .map(|(_, value)| value.as_str());
            if supplied != Some(secret.as_str()) {
                return Err(WebhookRejection::Unauthorized(
                    "relay secret mismatch".to_string(),
                ));
            }

            // Decode a minimal inbound event payload.
            let payload: serde_json::Value = serde_json::from_slice(&body).map_err(|error| {
                WebhookRejection::BadRequest(format!("payload parse detail: {error}"))
            })?;
            let field = |name: &str| {
                payload
                    .get(name)
                    .and_then(serde_json::Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string)
                    .ok_or_else(|| {
                        WebhookRejection::BadRequest(format!("payload parse detail: missing {name}"))
                    })
            };
            Ok(WebhookResponse::Messages(vec![InboundMessage {
                id: field("id")?,
                sender: field("sender")?,
                reply_target: field("reply_target")?,
                content: field("content")?,
                // Host replaces these with the admitted logical endpoint.
                channel: "nostr-pilot".to_string(),
                channel_alias: None,
                timestamp: 0,
                thread_ts: None,
                interruption_scope_id: None,
                attachments: Vec::new(),
                subject: None,
            }]))
        }
    }

    export!(NostrPilot);
}
