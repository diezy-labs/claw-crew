    /// Regression: Matrix
    /// streams paragraphs via `update_draft` and does NOT implement the
    /// `flush_draft_turn` narration contract, so it must NOT opt into the
    /// orchestrator's narration-policy + flush-barrier path. Otherwise outbound
    /// hooks fire on phantom flushes that deliver nothing while Matrix's real
    /// paragraph sends bypass the policy. `supports_multi_message_streaming` stays
    /// true (its own paragraph streaming); only the turn-flush capability is off.
    #[test]
    fn matrix_does_not_opt_into_turn_flush_narration() {
        use super::MatrixChannel;
        use std::sync::Arc;
        use clawcrew_api::channel::Channel;
        use clawcrew_config::schema::{MatrixConfig, MatrixStreamMode};

        let config = MatrixConfig {
            homeserver: "https://matrix.example".to_string(),
            access_token: Some("token".to_string()),
            stream_mode: MatrixStreamMode::MultiMessage,
            ..MatrixConfig::default()
        };
        let ch = MatrixChannel::new(
            config,
            "alias",
            Arc::new(Vec::<String>::new),
            std::env::temp_dir(),
        )
        .expect("matrix channel constructs from valid config");

        assert!(
            ch.supports_multi_message_streaming(),
            "Matrix still streams paragraphs in multi_message mode"
        );
        assert!(
            !ch.supports_turn_flush_narration(),
            "Matrix must not run the orchestrator narration-policy path on phantom flushes"
        );
    }

    mod voice_reply_delivery {
        use std::sync::Arc;

        use matrix_sdk::config::SyncSettings;
        use matrix_sdk::ruma::{owned_room_id, owned_user_id};
        use tempfile::TempDir;
        use wiremock::matchers::{body_partial_json, method, path, path_regex};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        use clawcrew_api::channel::{Channel, SendMessage};
        use clawcrew_config::schema::{
            AliasedAgentConfig, Config, MatrixConfig, OpenAITtsProviderConfig, TtsProviderConfig,
        };

        use super::super::MatrixChannel;

        /// Opus is the OpenAI family default, so `synthesize_opus` returns the
        /// provider bytes unchanged and no `ffmpeg` transcode is attempted.
        const OPUS_BYTES: &[u8] = b"OggS-fake-opus-payload";

        async fn homeserver_with_room(room: &str) -> MockServer {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path_regex(r"^/_matrix/client/versions$"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "versions": ["r0.6.0", "v1.1", "v1.2", "v1.3", "v1.4", "v1.5"],
                    "unstable_features": {}
                })))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path_regex(
                    r"^/_matrix/client/(v3|r0)/user/.*/account_data/m\.secret_storage\.default_key$",
                ))
                .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                    "errcode": "M_NOT_FOUND", "error": "not found"
                })))
                .mount(&server)
                .await;
            Mock::given(method("POST"))
                .and(path_regex(r"^/_matrix/client/(v3|r0)/keys/(upload|query)$"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "one_time_key_counts": {}, "device_keys": {}
                })))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path_regex(r"^/_matrix/client/(v3|r0)/sync$"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "next_batch": "s1",
                    "rooms": { "join": { room: {
                        "state": { "events": [] },
                        "timeline": { "limited": false, "prev_batch": "t0", "events": [] }
                    }}}
                })))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path_regex(
                    r"^/_matrix/client/(v3|r0)/rooms/.*/state/m\.room\.encryption/?$",
                ))
                .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                    "errcode": "M_NOT_FOUND", "error": "room is not encrypted"
                })))
                .mount(&server)
                .await;
            // The SDK reads the media config before any upload.
            Mock::given(method("GET"))
                .and(path_regex(r"^/_matrix/media/(v3|r0)/config$"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "m.upload.size": 10_000_000
                })))
                .mount(&server)
                .await;
            server
        }

        /// A voice group names users, so deciding whether a room should be
        /// voiced costs a member lookup. Proactive sends (cron announces) have
        /// no inbound sender, so this is the only identity available to them.
        async fn mount_room_members(server: &MockServer, room: &str, members: &[&str]) {
            let chunk: Vec<serde_json::Value> = members
                .iter()
                .enumerate()
                .map(|(i, user)| {
                    serde_json::json!({
                        "type": "m.room.member",
                        "sender": user,
                        "state_key": user,
                        "event_id": format!("$member{i}:server"),
                        "origin_server_ts": 0,
                        "room_id": room,
                        "content": { "membership": "join" }
                    })
                })
                .collect();
            Mock::given(method("GET"))
                .and(path_regex(r"^/_matrix/client/(v3|r0)/rooms/.*/members$"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(serde_json::json!({"chunk": chunk})),
                )
                .mount(server)
                .await;
        }

        async fn tts_endpoint() -> MockServer {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/v1/audio/speech"))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(OPUS_BYTES))
                .mount(&server)
                .await;
            server
        }

        fn config_with_tts(uri: String) -> Config {
            let mut config = Config::default();
            config.tts.enabled = true;
            config.providers.tts.openai.insert(
                "local".to_string(),
                OpenAITtsProviderConfig {
                    base: TtsProviderConfig {
                        api_key: Some("test-key".to_string()),
                        uri: Some(uri),
                        response_format: Some("opus".to_string()),
                        ..TtsProviderConfig::default()
                    },
                },
            );
            config.agents.insert(
                "default".to_string(),
                AliasedAgentConfig {
                    tts_provider: "openai.local".into(),
                    ..AliasedAgentConfig::default()
                },
            );
            config
        }

        async fn channel_for(
            homeserver: &MockServer,
            tts_config: Config,
            voice_peers: Vec<String>,
            state_dir: &TempDir,
        ) -> MatrixChannel {
            let matrix_config = MatrixConfig {
                homeserver: homeserver.uri(),
                access_token: Some("secret-token".to_string()),
                user_id: Some(owned_user_id!("@bot:server").to_string()),
                device_id: Some("DEVICE".to_string()),
                reply_in_thread: false,
                ack_reactions: Some(false),
                ..MatrixConfig::default()
            };
            let tts_config = Arc::new(tts_config);
            MatrixChannel::new(
                matrix_config,
                "voice",
                Arc::new(Vec::new),
                state_dir.path().to_path_buf(),
            )
            .expect("matrix channel")
            .with_voice_peer_resolver(Arc::new(move || voice_peers.clone()))
            .with_tts_manager_factory(move || {
                if !tts_config.tts.enabled {
                    return None;
                }
                Some(crate::tts::TtsManager::from_config_for_agent(
                    &tts_config,
                    Some("default"),
                ))
            })
        }

        /// A voice group lists Matrix users (`@user:server`); the reply is
        /// addressed to a room. Before this was a literal comparison of the two,
        /// so a correctly configured group never voiced anything.
        #[tokio::test]
        async fn a_room_holding_a_voice_peer_receives_both_a_voice_note_and_the_text_reply() {
            let room_id = owned_room_id!("!room:server");
            let homeserver = homeserver_with_room(room_id.as_str()).await;
            mount_room_members(&homeserver, room_id.as_str(), &["@alice:server"]).await;
            let tts = tts_endpoint().await;

            Mock::given(method("PUT"))
                .and(path_regex(
                    r"^/_matrix/client/(v3|r0)/rooms/.*/send/m\.room\.message/.*$",
                ))
                .and(body_partial_json(serde_json::json!({"msgtype": "m.text"})))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "event_id": "$text:server"
                })))
                .expect(1)
                .mount(&homeserver)
                .await;
            Mock::given(method("POST"))
                .and(path_regex(r"^.*/upload$"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "content_uri": "mxc://server/voice"
                })))
                .expect(1)
                .mount(&homeserver)
                .await;
            Mock::given(method("PUT"))
                .and(path_regex(
                    r"^/_matrix/client/(v3|r0)/rooms/.*/send/m\.room\.message/.*$",
                ))
                .and(body_partial_json(serde_json::json!({
                    "msgtype": "m.audio",
                    "org.matrix.msc3245.voice": {}
                })))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "event_id": "$voice:server"
                })))
                .expect(1)
                .mount(&homeserver)
                .await;

            let state_dir = TempDir::new().expect("temp state dir");
            let channel = channel_for(
                &homeserver,
                config_with_tts(format!("{}/v1/audio/speech", tts.uri())),
                vec!["@alice:server".to_string()],
                &state_dir,
            )
            .await;

            let client = channel.ensure_client().await.expect("matrix client");
            client
                .sync_once(SyncSettings::default())
                .await
                .expect("mock sync populates the joined room");

            channel
                .send_final(&SendMessage::new("spoken reply", room_id.as_str()))
                .await
                .expect("text reply is delivered");

            // Mock `.expect(1)` assertions verify on drop: the text message,
            // the media upload and the MSC3245 voice event must each land once.
            assert_eq!(
                tts.received_requests().await.map(|r| r.len()),
                Some(1),
                "the TTS endpoint must be asked to synthesize exactly once"
            );
        }

        /// The event wrapper was always encrypted by `send_raw`; the audio was
        /// not. Uploading the synthesized bytes in the clear hands the
        /// homeserver a playable copy of an otherwise encrypted reply, so the
        /// bytes that leave this process must not be the Opus payload.
        #[tokio::test]
        async fn an_encrypted_room_never_uploads_the_audio_in_the_clear() {
            let room_id = owned_room_id!("!room:server");
            let homeserver = homeserver_with_room(room_id.as_str()).await;
            mount_room_members(&homeserver, room_id.as_str(), &["@alice:server"]).await;
            // The shared fixture answers this route with "not encrypted"; a
            // higher priority (lower number) overrides it for this room only.
            // Everything else about the fixture is unchanged.
            Mock::given(method("GET"))
                .and(path_regex(
                    r"^/_matrix/client/(v3|r0)/rooms/.*/state/m\.room\.encryption/?$",
                ))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "algorithm": "m.megolm.v1.aes-sha2"
                })))
                .with_priority(1)
                .mount(&homeserver)
                .await;
            let tts = tts_endpoint().await;

            Mock::given(method("PUT"))
                .and(path_regex(
                    r"^/_matrix/client/(v3|r0)/rooms/.*/send/m\.room\.(message|encrypted)/.*$",
                ))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "event_id": "$sent:server"
                })))
                .mount(&homeserver)
                .await;
            Mock::given(method("POST"))
                .and(path_regex(r"^.*/upload$"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "content_uri": "mxc://server/voice"
                })))
                .mount(&homeserver)
                .await;
            // Megolm key sharing runs before the event send. Answering these
            // keeps the test on its first attempt instead of retry backoff.
            Mock::given(method("POST"))
                .and(path_regex(r"^/_matrix/client/(v3|r0)/keys/claim$"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "failures": {}, "one_time_keys": {}
                })))
                .mount(&homeserver)
                .await;
            Mock::given(method("PUT"))
                .and(path_regex(r"^/_matrix/client/(v3|r0)/sendToDevice/.*$"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
                .mount(&homeserver)
                .await;

            let state_dir = TempDir::new().expect("temp state dir");
            let channel = channel_for(
                &homeserver,
                config_with_tts(format!("{}/v1/audio/speech", tts.uri())),
                vec!["@alice:server".to_string()],
                &state_dir,
            )
            .await;

            let client = channel.ensure_client().await.expect("matrix client");
            client
                .sync_once(SyncSettings::default())
                .await
                .expect("mock sync populates the joined room");

            channel
                .send_final(&SendMessage::new("spoken reply", room_id.as_str()))
                .await
                .expect("text reply is delivered");

            let requests = homeserver.received_requests().await.unwrap_or_default();
            let uploads: Vec<Vec<u8>> = requests
                .iter()
                .filter(|r| r.url.path().ends_with("/upload"))
                .map(|r| r.body.clone())
                .collect();
            assert!(
                requests
                    .iter()
                    .all(|r| !r.url.path().contains("/send/m.room.message/")),
                "an encrypted room must carry no plaintext room message"
            );
            assert!(
                requests
                    .iter()
                    .any(|r| r.url.path().contains("/send/m.room.encrypted/")),
                "the voice note and its text reply are sent as encrypted events"
            );
            assert!(
                !uploads.is_empty(),
                "the voice note is still uploaded in an encrypted room"
            );
            for body in &uploads {
                assert_ne!(
                    body.as_slice(),
                    OPUS_BYTES,
                    "the synthesized audio must not reach the homeserver verbatim"
                );
                assert!(
                    !body.starts_with(b"OggS"),
                    "ciphertext must not carry the Ogg container magic: {:?}",
                    &body[..body.len().min(8)]
                );
            }
        }

        #[tokio::test]
        async fn suppressed_voice_delivers_text_without_synthesizing() {
            let room_id = owned_room_id!("!room:server");
            let homeserver = homeserver_with_room(room_id.as_str()).await;
            let tts = tts_endpoint().await;

            Mock::given(method("PUT"))
                .and(path_regex(
                    r"^/_matrix/client/(v3|r0)/rooms/.*/send/m\.room\.message/.*$",
                ))
                .and(body_partial_json(serde_json::json!({"msgtype": "m.text"})))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "event_id": "$text:server"
                })))
                .expect(1)
                .mount(&homeserver)
                .await;

            let state_dir = TempDir::new().expect("temp state dir");
            let channel = channel_for(
                &homeserver,
                config_with_tts(format!("{}/v1/audio/speech", tts.uri())),
                vec![room_id.to_string()],
                &state_dir,
            )
            .await;
            let client = channel.ensure_client().await.expect("matrix client");
            client
                .sync_once(SyncSettings::default())
                .await
                .expect("mock sync populates the joined room");

            channel
                .send_final(&SendMessage::new("error notice", room_id.as_str()).suppress_voice())
                .await
                .expect("text reply is delivered");

            assert_eq!(
                tts.received_requests().await.map(|r| r.len()),
                Some(0),
                "suppress_voice must not reach the synthesizer"
            );
        }

        /// Delivery-level regression: a non-member sender's reply must stay
        /// text-only even in a room that also holds a voice-group member.
        /// Before the fix, the orchestrator's no-`send_via` reply arm kept
        /// only the positive sender verdict (`force_voice`); a negative
        /// verdict collapsed to `force_voice = false` with no
        /// `suppress_voice` override, so `should_voice` fell back to
        /// `room_has_voice_peer` and voiced the reply anyway because the
        /// room's *other* occupant, `@alice:server`, was a voice peer.
        ///
        /// Drives the real production mapping
        /// (`orchestrator::voice_override_from_sender_verdict`) — not a
        /// hand-rolled stand-in — with the tri-state a non-member sender like
        /// `@bob:server` actually gets (`Some(false)`, asserted separately in
        /// `orchestrator::tests::matrix_voice_group_member_gets_a_voiced_reply_by_user_id`),
        /// so this test is tied to that mapping and fails to build without
        /// it. `matrix.rs`'s own `suppress_voice` handling predates the fix
        /// and would pass this scenario either way, which is why the room
        /// membership and the mapping call both have to be present here
        /// rather than hand-constructing an already-suppressed `SendMessage`.
        #[tokio::test]
        async fn a_non_member_senders_reply_stays_text_only_in_a_room_with_a_voice_peer() {
            let room_id = owned_room_id!("!room:server");
            let homeserver = homeserver_with_room(room_id.as_str()).await;
            mount_room_members(
                &homeserver,
                room_id.as_str(),
                &["@alice:server", "@bob:server"],
            )
            .await;
            let tts = tts_endpoint().await;

            Mock::given(method("PUT"))
                .and(path_regex(
                    r"^/_matrix/client/(v3|r0)/rooms/.*/send/m\.room\.message/.*$",
                ))
                .and(body_partial_json(serde_json::json!({"msgtype": "m.text"})))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "event_id": "$text:server"
                })))
                .expect(1)
                .mount(&homeserver)
                .await;
            // Alice being a voice peer in this room must not matter for
            // Bob's reply: nothing may ever be uploaded for it.
            Mock::given(method("POST"))
                .and(path_regex(r"^.*/upload$"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "content_uri": "mxc://server/voice"
                })))
                .expect(0)
                .mount(&homeserver)
                .await;

            let state_dir = TempDir::new().expect("temp state dir");
            let channel = channel_for(
                &homeserver,
                config_with_tts(format!("{}/v1/audio/speech", tts.uri())),
                vec!["@alice:server".to_string()],
                &state_dir,
            )
            .await;
            let client = channel.ensure_client().await.expect("matrix client");
            client
                .sync_once(SyncSettings::default())
                .await
                .expect("mock sync populates the joined room");

            let (suppress, force_voice) =
                crate::orchestrator::voice_override_from_sender_verdict(Some(false));
            assert_eq!(
                (suppress, force_voice),
                (Some(true), false),
                "sanity-check the mapping this test depends on"
            );

            let mut send_msg = SendMessage::new("reply to bob", room_id.as_str());
            if suppress.unwrap_or(false) {
                send_msg = send_msg.suppress_voice();
            } else if force_voice {
                send_msg = send_msg.force_voice();
            }

            channel
                .send_final(&send_msg)
                .await
                .expect("text reply is delivered");

            assert_eq!(
                tts.received_requests().await.map(|r| r.len()),
                Some(0),
                "a non-member sender's reply must never be synthesized, even \
                 though the room also holds voice-group member @alice:server"
            );
        }

        /// Same regression as above, for the streaming-finalization path:
        /// `finalize_draft`'s shared voice-note gate (after the
        /// per-`stream_mode` branch) must also honor the mapped
        /// `suppress_voice` rather than falling back to room membership.
        /// `MatrixStreamMode::Off` (the channel default here) makes the
        /// per-mode branch a no-op, so this needs no live draft — it
        /// exercises exactly the shared gate at issue.
        #[tokio::test]
        async fn finalize_draft_keeps_a_non_member_senders_reply_text_only() {
            let room_id = owned_room_id!("!room:server");
            let homeserver = homeserver_with_room(room_id.as_str()).await;
            mount_room_members(
                &homeserver,
                room_id.as_str(),
                &["@alice:server", "@bob:server"],
            )
            .await;
            let tts = tts_endpoint().await;

            Mock::given(method("POST"))
                .and(path_regex(r"^.*/upload$"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "content_uri": "mxc://server/voice"
                })))
                .expect(0)
                .mount(&homeserver)
                .await;

            let state_dir = TempDir::new().expect("temp state dir");
            let channel = channel_for(
                &homeserver,
                config_with_tts(format!("{}/v1/audio/speech", tts.uri())),
                vec!["@alice:server".to_string()],
                &state_dir,
            )
            .await;
            let client = channel.ensure_client().await.expect("matrix client");
            client
                .sync_once(SyncSettings::default())
                .await
                .expect("mock sync populates the joined room");

            let (suppress, _force_voice) =
                crate::orchestrator::voice_override_from_sender_verdict(Some(false));

            channel
                .finalize_draft(
                    room_id.as_str(),
                    "draft-1",
                    "reply to bob",
                    suppress.unwrap_or(false),
                )
                .await
                .expect("finalize_draft succeeds with no live draft in Off stream mode");

            assert_eq!(
                tts.received_requests().await.map(|r| r.len()),
                Some(0),
                "streaming finalization must also treat a non-member sender's \
                 negative verdict as authoritative, even though the room \
                 holds voice-group member @alice:server"
            );
        }

        #[tokio::test]
        async fn synthesis_failure_still_delivers_the_text_reply() {
            let room_id = owned_room_id!("!room:server");
            let homeserver = homeserver_with_room(room_id.as_str()).await;
            // The member list has to answer, or the voice gate fails closed
            // before synthesis is ever reached and this asserts nothing.
            mount_room_members(&homeserver, room_id.as_str(), &["@alice:server"]).await;
            // No /v1/audio/speech route mounted: synthesis fails.
            let tts = MockServer::start().await;

            Mock::given(method("PUT"))
                .and(path_regex(
                    r"^/_matrix/client/(v3|r0)/rooms/.*/send/m\.room\.message/.*$",
                ))
                .and(body_partial_json(serde_json::json!({"msgtype": "m.text"})))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "event_id": "$text:server"
                })))
                .expect(1)
                .mount(&homeserver)
                .await;
            // Nothing to upload when there is no audio.
            Mock::given(method("POST"))
                .and(path_regex(r"^.*/upload$"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "content_uri": "mxc://server/never"
                })))
                .expect(0)
                .mount(&homeserver)
                .await;

            let state_dir = TempDir::new().expect("temp state dir");
            let channel = channel_for(
                &homeserver,
                config_with_tts(format!("{}/v1/audio/speech", tts.uri())),
                vec!["@alice:server".to_string()],
                &state_dir,
            )
            .await;
            let client = channel.ensure_client().await.expect("matrix client");
            client
                .sync_once(SyncSettings::default())
                .await
                .expect("mock sync populates the joined room");

            channel
                .send_final(&SendMessage::new("spoken reply", room_id.as_str()))
                .await
                .expect("a TTS failure must not fail the text reply");

            assert!(
                !tts.received_requests()
                    .await
                    .expect("wiremock records requests")
                    .is_empty(),
                "synthesis has to be attempted for its failure to be the thing under test"
            );
        }

        /// The voice gate asks the homeserver who is in the room. When that
        /// question cannot be answered, the room is not voiced: a shared room
        /// is the wrong place to guess, and the text reply carries the answer
        /// either way.
        #[tokio::test]
        async fn an_unanswerable_member_list_never_voices_the_room() {
            let room_id = owned_room_id!("!room:server");
            let homeserver = homeserver_with_room(room_id.as_str()).await;
            Mock::given(method("GET"))
                .and(path_regex(r"^/_matrix/client/(v3|r0)/rooms/.*/members$"))
                .respond_with(ResponseTemplate::new(500))
                .mount(&homeserver)
                .await;
            // Synthesis would succeed if it were reached, so a voice note here
            // would mean the gate opened on an unverified room.
            let tts = tts_endpoint().await;

            Mock::given(method("PUT"))
                .and(path_regex(
                    r"^/_matrix/client/(v3|r0)/rooms/.*/send/m\.room\.message/.*$",
                ))
                .and(body_partial_json(serde_json::json!({"msgtype": "m.text"})))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "event_id": "$text:server"
                })))
                .expect(1)
                .mount(&homeserver)
                .await;
            Mock::given(method("POST"))
                .and(path_regex(r"^.*/upload$"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "content_uri": "mxc://server/never"
                })))
                .expect(0)
                .mount(&homeserver)
                .await;

            let state_dir = TempDir::new().expect("temp state dir");
            let channel = channel_for(
                &homeserver,
                config_with_tts(format!("{}/v1/audio/speech", tts.uri())),
                vec!["@alice:server".to_string()],
                &state_dir,
            )
            .await;
            let client = channel.ensure_client().await.expect("matrix client");
            client
                .sync_once(SyncSettings::default())
                .await
                .expect("mock sync populates the joined room");

            channel
                .send_final(&SendMessage::new("spoken reply", room_id.as_str()))
                .await
                .expect("an unreachable member list must not fail the text reply");

            assert!(
                tts.received_requests()
                    .await
                    .expect("wiremock records requests")
                    .is_empty(),
                "a room whose membership could not be read must not be synthesized for"
            );
        }
    }

    mod voice_reply_gate {
        use super::super::{
            MatrixChannel,
            allowlist::{voice_peer_matches, voice_peers_verdict},
        };
        use std::sync::Arc;
        use clawcrew_api::channel::SendMessage;
        use clawcrew_config::schema::MatrixConfig;

        const ROOM: &str = "!room:localhost";
        const PEER: &str = "@alice:localhost";

        fn matrix_config(reply_in_thread: bool) -> MatrixConfig {
            MatrixConfig {
                homeserver: "http://127.0.0.1:8008".to_string(),
                access_token: Some("test-token".to_string()),
                reply_in_thread,
                ..MatrixConfig::default()
            }
        }

        /// `tts_wired` mirrors the orchestrator having installed a resolver at
        /// all, independent of whether TTS is enabled right now.
        fn channel(voice_peers: Vec<String>, tts_wired: bool) -> MatrixChannel {
            let channel = MatrixChannel::new(
                matrix_config(false),
                "default",
                Arc::new(Vec::new),
                std::env::temp_dir(),
            )
            .expect("fixture Matrix config is valid");
            let channel = channel.with_voice_peer_resolver(Arc::new(move || voice_peers.clone()));
            if tts_wired {
                channel.with_tts_manager_factory(|| None)
            } else {
                channel
            }
        }

        // ── the half that needs no homeserver ──────────────────────────────

        #[test]
        fn force_voice_reaches_a_text_default_peer() {
            let channel = channel(Vec::new(), true);
            assert_eq!(
                channel.voice_intent(&SendMessage::new("hello", ROOM).force_voice()),
                Some(true)
            );
        }

        #[test]
        fn suppress_voice_beats_force_voice() {
            let channel = channel(Vec::new(), true);
            let message = SendMessage::new("hello", ROOM)
                .force_voice()
                .suppress_voice();
            assert_eq!(channel.voice_intent(&message), Some(false));
        }

        #[test]
        fn suppress_voice_beats_a_voice_peer() {
            let channel = channel(vec![PEER.to_string()], true);
            assert_eq!(
                channel.voice_intent(&SendMessage::new("hello", ROOM).suppress_voice()),
                Some(false)
            );
        }

        #[test]
        fn without_a_tts_resolver_nothing_is_voiced() {
            // The channel runtime never installed one, so the channel has no
            // way to synthesize regardless of configuration.
            let channel = channel(vec![PEER.to_string()], false);
            assert_eq!(
                channel.voice_intent(&SendMessage::new("hello", ROOM)),
                Some(false)
            );
            assert_eq!(
                channel.voice_intent(&SendMessage::new("hello", ROOM).force_voice()),
                Some(false)
            );
        }

        #[test]
        fn an_ordinary_reply_defers_to_the_room() {
            // Nothing about the message settles it, so the target room's
            // membership has to be consulted.
            let channel = channel(vec![PEER.to_string()], true);
            assert_eq!(channel.voice_intent(&SendMessage::new("hello", ROOM)), None);
        }

        // ── the room verdict that avoids a homeserver round-trip ───────────

        #[test]
        fn no_configured_voice_peers_voices_nobody() {
            assert_eq!(voice_peers_verdict(&[]), Some(false));
        }

        #[test]
        fn wildcard_voices_every_room_without_asking_for_members() {
            // The literal comparison this replaces never matched `"*"` against
            // a room id, so a wildcard voice group was silently inert.
            assert_eq!(voice_peers_verdict(&["*".to_string()]), Some(true));
            assert_eq!(
                voice_peers_verdict(&[PEER.to_string(), "*".to_string()]),
                Some(true)
            );
        }

        #[test]
        fn named_peers_require_the_member_list() {
            // A user ID cannot be compared with a room id, so membership is
            // the only thing that can answer this.
            assert_eq!(voice_peers_verdict(&[PEER.to_string()]), None);
        }

        #[test]
        fn membership_matching_accepts_the_same_shapes_the_runtime_does() {
            // The runtime normalizes an inbound sender with
            // `normalize_peer_username`, which strips a leading `@` and
            // lowercases. Membership matching has to agree, or a group written
            // without the `@` voices ordinary replies and silently drops
            // proactive ones.
            assert!(voice_peer_matches(PEER, PEER));
            assert!(voice_peer_matches("alice:localhost", PEER));
            assert!(voice_peer_matches("@ALICE:LOCALHOST", PEER));
            assert!(!voice_peer_matches("@bob:localhost", PEER));
            assert!(
                !voice_peer_matches(ROOM, PEER),
                "a room id is not a peer identity on either side"
            );
        }

        #[test]
        fn voice_peers_resolve_per_call_not_at_construction() {
            // Peer groups are reloadable; a cached list would go stale.
            let peers = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
            let read = Arc::clone(&peers);
            let channel = MatrixChannel::new(
                matrix_config(false),
                "default",
                Arc::new(Vec::new),
                std::env::temp_dir(),
            )
            .expect("fixture Matrix config is valid")
            .with_tts_manager_factory(|| None)
            .with_voice_peer_resolver(Arc::new(move || {
                read.lock().map(|p| p.clone()).unwrap_or_default()
            }));

            assert_eq!(voice_peers_verdict(&(channel.voice_peers)()), Some(false));
            if let Ok(mut p) = peers.lock() {
                p.push("*".to_string());
            }
            assert_eq!(voice_peers_verdict(&(channel.voice_peers)()), Some(true));
        }

        #[test]
        fn thread_anchor_follows_reply_in_thread() {
            let threaded = MatrixChannel::new(
                matrix_config(true),
                "default",
                Arc::new(Vec::new),
                std::env::temp_dir(),
            )
            .expect("fixture Matrix config is valid");
            let mut message = SendMessage::new("hello", ROOM);
            message.thread_ts = Some("$anchor:localhost".to_string());
            assert!(threaded.voice_thread_anchor(&message).is_some());

            let flat = channel(Vec::new(), true);
            assert!(
                flat.voice_thread_anchor(&message).is_none(),
                "reply_in_thread = false must keep the voice note out of a thread"
            );
        }
    }

    mod transcription_provider_resolution {
        use super::super::build_transcription_manager;
        use clawcrew_config::schema::{
            AliasedAgentConfig, Config, LocalWhisperConfig,
            LocalWhisperTranscriptionProviderConfig, TranscriptionConfig,
        };

        fn local_whisper_config(url: &str) -> LocalWhisperConfig {
            LocalWhisperConfig {
                url: url.to_string(),
                bearer_token: Some("test-token".to_string()),
                max_audio_bytes: 10 * 1024 * 1024,
                timeout_secs: 30,
            }
        }

        fn with_transcription(transcription: TranscriptionConfig) -> Config {
            Config {
                transcription,
                ..Config::default()
            }
        }

        /// A typed `[providers.transcription.local_whisper.stoa]` entry plus the
        /// owning agent's `transcription_provider`, and no legacy provider.
        fn typed_config() -> Config {
            let mut config = with_transcription(TranscriptionConfig {
                enabled: true,
                ..TranscriptionConfig::default()
            });
            config.providers.transcription.local_whisper.insert(
                "stoa".to_string(),
                LocalWhisperTranscriptionProviderConfig {
                    uri: "http://127.0.0.1:9999/v1/transcribe".to_string(),
                    ..LocalWhisperTranscriptionProviderConfig::default()
                },
            );
            config.agents.insert(
                "local".to_string(),
                AliasedAgentConfig {
                    transcription_provider: "local_whisper.stoa".into(),
                    ..AliasedAgentConfig::default()
                },
            );
            config
        }

        #[tokio::test]
        async fn registers_typed_provider_and_binds_the_agent_alias() {
            // Regression: the channel built its manager from the legacy
            // `[transcription]` section alone, so typed entries never
            // registered and the agent's provider was never read — voice
            // ingest failed with "no transcription provider registered".
            let config = typed_config();

            let manager = build_transcription_manager(&config, "local_whisper.stoa").unwrap();

            assert!(
                manager
                    .available_providers()
                    .contains(&"local_whisper.stoa"),
                "typed provider must register, got {:?}",
                manager.available_providers()
            );

            let err = manager
                .transcribe(b"not-real-audio", "voice.aiff")
                .await
                .expect_err("an unsupported format must be rejected");
            assert!(
                !err.to_string()
                    .contains("Agent has no transcription_provider configured"),
                "expected dispatch to the bound alias, got the empty-alias bail: {err}"
            );
            assert!(
                err.to_string().contains("Unsupported audio format"),
                "expected the dispatched provider to reject the format, got: {err}"
            );
        }

        #[tokio::test]
        async fn agent_alias_wins_over_the_sole_provider_heuristic() {
            // Two providers register, so the sole-provider fallback cannot
            // fire; only the agent's explicit alias can bind here.
            let mut config = typed_config();
            config.transcription.local_whisper =
                Some(local_whisper_config("http://127.0.0.1:9998/v1/transcribe"));

            let manager = build_transcription_manager(&config, "local_whisper.stoa").unwrap();
            assert!(
                manager.available_providers().len() > 1,
                "fixture must register more than one provider, got {:?}",
                manager.available_providers()
            );

            let err = manager
                .transcribe(b"not-real-audio", "voice.aiff")
                .await
                .expect_err("an unsupported format must be rejected");
            assert!(
                !err.to_string()
                    .contains("Agent has no transcription_provider configured"),
                "the explicit agent alias must be bound, got: {err}"
            );
        }

        #[tokio::test]
        async fn binds_alias_when_exactly_one_provider_is_configured() {
            let config = with_transcription(TranscriptionConfig {
                enabled: true,
                local_whisper: Some(local_whisper_config("http://127.0.0.1:9999/v1/transcribe")),
                ..TranscriptionConfig::default()
            });

            let manager = build_transcription_manager(&config, "").unwrap();
            assert_eq!(
                manager.available_providers(),
                vec!["local_whisper"],
                "fixture must register exactly one provider"
            );

            let err = manager
                .transcribe(b"not-real-audio", "voice.aiff")
                .await
                .expect_err("an unsupported format must be rejected");
            assert!(
                !err.to_string()
                    .contains("Agent has no transcription_provider configured"),
                "expected dispatch to the sole provider, got the empty-alias bail: {err}"
            );
            assert!(
                err.to_string().contains("Unsupported audio format"),
                "expected the dispatched provider to reject the format, got: {err}"
            );
        }

        #[tokio::test]
        async fn leaves_alias_unbound_when_multiple_providers_are_configured() {
            let config = with_transcription(TranscriptionConfig {
                enabled: true,
                api_key: Some("test-groq-key".to_string()),
                local_whisper: Some(local_whisper_config("http://127.0.0.1:9999/v1/transcribe")),
                ..TranscriptionConfig::default()
            });

            let manager = build_transcription_manager(&config, "").unwrap();
            assert!(
                manager.available_providers().len() > 1,
                "fixture must register more than one provider, got {:?}",
                manager.available_providers()
            );

            let err = manager
                .transcribe(b"not-real-audio", "voice.ogg")
                .await
                .expect_err("an unbound alias must fail");
            assert!(
                err.to_string()
                    .contains("Agent has no transcription_provider configured"),
                "expected the empty-alias bail, got: {err}"
            );
        }
    }

    mod media_filename_resolution {
        use super::super::build_transcription_manager;
        use super::super::inbound::transcription_safe_filename;
        use clawcrew_config::schema::TranscriptionConfig;

        fn local_whisper_config(url: &str) -> clawcrew_config::schema::LocalWhisperConfig {
            clawcrew_config::schema::LocalWhisperConfig {
                url: url.to_string(),
                bearer_token: Some("test-token".to_string()),
                max_audio_bytes: 10 * 1024 * 1024,
                timeout_secs: 30,
            }
        }

        fn config_with(transcription: TranscriptionConfig) -> clawcrew_config::schema::Config {
            clawcrew_config::schema::Config {
                transcription,
                ..clawcrew_config::schema::Config::default()
            }
        }

        #[test]
        fn unaccepted_extension_with_supported_mime_uses_mime_derived_name() {
            let resolved = transcription_safe_filename("recording.bin", Some("audio/ogg"));
            assert_eq!(resolved, "attachment.ogg");
        }

        #[test]
        fn accepted_extension_is_preserved_even_when_mime_differs() {
            let resolved = transcription_safe_filename("recording.mp3", Some("audio/ogg"));
            assert_eq!(resolved, "recording.mp3");
        }

        #[test]
        fn accepted_extension_matching_mime_is_passed_through_unchanged() {
            let resolved = transcription_safe_filename("recording.ogg", Some("audio/ogg"));
            assert_eq!(resolved, "recording.ogg");
        }

        #[test]
        fn accepted_extension_is_matched_case_insensitively() {
            let resolved = transcription_safe_filename("recording.OGG", Some("audio/ogg"));
            assert_eq!(resolved, "recording.OGG");
        }

        #[test]
        fn oga_extension_is_accepted_and_preserved() {
            let resolved = transcription_safe_filename("note.oga", Some("audio/ogg"));
            assert_eq!(resolved, "note.oga");
        }

        #[test]
        fn no_mime_with_an_accepted_extension_is_passed_through() {
            let resolved = transcription_safe_filename("recording.mp3", None);
            assert_eq!(resolved, "recording.mp3");
        }

        #[test]
        fn unaccepted_extension_with_no_mime_is_preserved_for_local_rejection() {
            let resolved = transcription_safe_filename("recording.bin", None);
            assert_eq!(resolved, "recording.bin");
        }

        #[test]
        fn no_mime_and_no_extension_is_preserved_for_local_rejection() {
            let resolved = transcription_safe_filename("Meeting recap", None);
            assert_eq!(resolved, "Meeting recap");
        }

        #[test]
        fn unsupported_mime_and_extension_are_preserved() {
            let resolved = transcription_safe_filename("recording.aac", Some("audio/aac"));
            assert_eq!(resolved, "recording.aac");
        }

        #[test]
        fn synthesizes_extension_from_mime_when_no_extension_at_all() {
            let resolved = transcription_safe_filename("Voice message", Some("audio/ogg"));
            assert_eq!(resolved, "attachment.ogg");
        }

        #[tokio::test]
        async fn caption_only_body_reaches_provider_dispatch_not_format_rejection() {
            let config = TranscriptionConfig {
                enabled: true,
                local_whisper: Some(local_whisper_config("http://127.0.0.1:9999/v1/transcribe")),
                ..TranscriptionConfig::default()
            };
            let manager = build_transcription_manager(&config_with(config), "").unwrap();

            let file_name = transcription_safe_filename("Voice message", Some("audio/ogg"));

            let err = manager
                .transcribe(b"not-real-audio", &file_name)
                .await
                .expect_err("fixture audio bytes are not real, dispatch must still fail");
            assert!(
                !err.to_string()
                    .contains("Agent has no transcription_provider configured"),
                "expected dispatch to the sole provider, got the empty-alias bail: {err}"
            );
            assert!(
                !err.to_string().contains("Unsupported audio format"),
                "expected a resolved .ogg filename to pass format validation, got: {err}"
            );
        }

        #[tokio::test]
        async fn unaccepted_extension_reaches_provider_dispatch_not_format_rejection() {
            let config = TranscriptionConfig {
                enabled: true,
                local_whisper: Some(local_whisper_config("http://127.0.0.1:9999/v1/transcribe")),
                ..TranscriptionConfig::default()
            };
            let manager = build_transcription_manager(&config_with(config), "").unwrap();

            let file_name = transcription_safe_filename("recording.bin", Some("audio/ogg"));

            let err = manager
                .transcribe(b"not-real-audio", &file_name)
                .await
                .expect_err("fixture audio bytes are not real, dispatch must still fail");
            assert!(
                !err.to_string()
                    .contains("Agent has no transcription_provider configured"),
                "expected dispatch to the sole provider, got the empty-alias bail: {err}"
            );
            assert!(
                !err.to_string().contains("Unsupported audio format"),
                "expected the unaccepted .bin extension to be replaced with the MIME-derived .ogg, got: {err}"
            );
        }

        // An unsupported format must be rejected locally by the resolver —
        // the provider endpoint must see zero requests.
        #[tokio::test]
        async fn unsupported_mime_sends_no_request() {
            use wiremock::matchers::{method, path};
            use wiremock::{Mock, MockServer, ResponseTemplate};

            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/v1/transcribe"))
                .respond_with(ResponseTemplate::new(200))
                .expect(0)
                .mount(&server)
                .await;

            let config = TranscriptionConfig {
                enabled: true,
                local_whisper: Some(local_whisper_config(&format!(
                    "{}/v1/transcribe",
                    server.uri()
                ))),
                ..TranscriptionConfig::default()
            };
            let manager = build_transcription_manager(&config_with(config), "").unwrap();

            let file_name = transcription_safe_filename("recording.aac", Some("audio/aac"));
            assert_eq!(file_name, "recording.aac");

            let err = manager
                .transcribe(b"not-real-audio", &file_name)
                .await
                .expect_err("unsupported format must be rejected before dispatch");
            assert!(
                err.to_string().contains("Unsupported audio format"),
                "expected local format rejection, got: {err}"
            );
            server.verify().await;
        }

        #[tokio::test]
        async fn missing_mime_and_extension_sends_no_request() {
            use wiremock::matchers::{method, path};
            use wiremock::{Mock, MockServer, ResponseTemplate};

            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/v1/transcribe"))
                .respond_with(ResponseTemplate::new(200))
                .expect(0)
                .mount(&server)
                .await;

            let config = TranscriptionConfig {
                enabled: true,
                local_whisper: Some(local_whisper_config(&format!(
                    "{}/v1/transcribe",
                    server.uri()
                ))),
                ..TranscriptionConfig::default()
            };
            let manager = build_transcription_manager(&config_with(config), "").unwrap();

            let file_name = transcription_safe_filename("Voice message", None);
            assert_eq!(file_name, "Voice message");

            let err = manager
                .transcribe(b"not-real-audio", &file_name)
                .await
                .expect_err("missing format must be rejected before dispatch");
            assert!(
                err.to_string().contains("Unsupported audio format"),
                "expected local format rejection, got: {err}"
            );
            server.verify().await;
        }

        // attach_media only inserts non-empty transcripts.
        #[tokio::test]
        async fn caption_only_body_transcript_is_produced_for_insertion() {
            use wiremock::matchers::{method, path};
            use wiremock::{Mock, MockServer, ResponseTemplate};

            let server = MockServer::start().await;

            Mock::given(method("POST"))
                .and(path("/v1/transcribe"))
                .respond_with(ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({"text": "this is the transcribed voice message"}),
                ))
                .mount(&server)
                .await;

            let config = TranscriptionConfig {
                enabled: true,
                local_whisper: Some(local_whisper_config(&format!(
                    "{}/v1/transcribe",
                    server.uri()
                ))),
                ..TranscriptionConfig::default()
            };
            let manager = build_transcription_manager(&config_with(config), "").unwrap();

            let file_name = transcription_safe_filename("Voice message", Some("audio/ogg"));

            let text = manager.transcribe(b"fake-audio", &file_name).await.unwrap();
            assert_eq!(text, "this is the transcribed voice message");
            assert!(!text.trim().is_empty());
        }

        #[test]
        fn transcription_only_name_may_diverge_from_the_real_filename() {
            let real_filename = "recording.bin";
            let transcribe_name = transcription_safe_filename(real_filename, Some("audio/ogg"));
            assert_ne!(transcribe_name, real_filename);
            assert_eq!(transcribe_name, "attachment.ogg");
        }
    }

    // Route-level hermetic coverage: a real (constructed) WAV file travels
    // event parsing → media download/save → attach_media → transcript
    // insertion, with only the homeserver and STT provider mocked at HTTP.
    mod inbound_route {
        use std::collections::{HashMap, HashSet};
        use std::sync::Arc;
        use std::sync::atomic::AtomicBool;
        use std::time::Duration;

        use matrix_sdk::ruma::events::AnySyncTimelineEvent;
        use matrix_sdk::ruma::serde::Raw;
        use matrix_sdk::ruma::{RoomId, room_id, user_id};
        use matrix_sdk::test_utils::mocks::MatrixMockServer;
        use matrix_sdk_test::JoinedRoomBuilder;
        use tokio::sync::{Mutex as TokioMutex, RwLock as TokioRwLock, mpsc, oneshot};
        use wiremock::matchers::{method, path, path_regex};
        use wiremock::{Mock, ResponseTemplate};
        use clawcrew_api::channel::ChannelApprovalResponse;
        use clawcrew_config::schema::{MatrixConfig, TranscriptionConfig};

        use super::super::inbound::{HandlerCtx, register_event_handlers};

        const TRANSCRIPT: &str = "route level transcript of the voice note";

        fn test_room() -> &'static RoomId {
            room_id!("!room:localhost")
        }

        fn timeline_raw(json: &serde_json::Value) -> Raw<AnySyncTimelineEvent> {
            Raw::new(json).expect("event json").cast_unchecked()
        }

        async fn recv_forwarded(
            rx: &mut mpsc::Receiver<clawcrew_api::channel::ChannelMessage>,
        ) -> clawcrew_api::channel::ChannelMessage {
            tokio::time::timeout(Duration::from_secs(15), rx.recv())
                .await
                .expect("inbound message must be forwarded before timeout")
                .expect("channel must stay open")
        }

        // 0.1 s of a 440 Hz sine at 16 kHz mono 16-bit PCM: a genuinely
        // valid, decodable WAV file, generated in-test.
        fn build_wav() -> Vec<u8> {
            let sample_rate: u32 = 16_000;
            let samples: Vec<i16> = (0..(sample_rate / 10))
                .map(|i| {
                    let t = i as f32 / sample_rate as f32;
                    ((t * 440.0 * std::f32::consts::TAU).sin() * f32::from(i16::MAX) * 0.5) as i16
                })
                .collect();
            let data_len = (samples.len() * 2) as u32;
            let mut wav = Vec::with_capacity(44 + data_len as usize);
            wav.extend_from_slice(b"RIFF");
            wav.extend_from_slice(&(36 + data_len).to_le_bytes());
            wav.extend_from_slice(b"WAVEfmt ");
            wav.extend_from_slice(&16u32.to_le_bytes());
            wav.extend_from_slice(&1u16.to_le_bytes());
            wav.extend_from_slice(&1u16.to_le_bytes());
            wav.extend_from_slice(&sample_rate.to_le_bytes());
            wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
            wav.extend_from_slice(&2u16.to_le_bytes());
            wav.extend_from_slice(&16u16.to_le_bytes());
            wav.extend_from_slice(b"data");
            wav.extend_from_slice(&data_len.to_le_bytes());
            for s in &samples {
                wav.extend_from_slice(&s.to_le_bytes());
            }
            wav
        }

        /// The handler context every route test drives, with the
        /// transcription resolver left to the caller so a test can supply the
        /// one a *configured* channel installed instead of the legacy
        /// single-endpoint resolver.
        fn handler_ctx_with_resolver(
            transcription: Option<super::super::TranscriptionResolver>,
            workspace: &std::path::Path,
            tx: mpsc::Sender<clawcrew_api::channel::ChannelMessage>,
        ) -> HandlerCtx {
            HandlerCtx {
                config: Arc::new(MatrixConfig::default()),
                alias: "test".to_string(),
                peer_resolver: Arc::new(|| vec!["*".to_string()]),
                transcription,
                workspace_dir: Some(Arc::new(workspace.to_path_buf())),
                tx,
                pending_approvals: Arc::new(TokioMutex::new(HashMap::new())),
                threads_seen: Arc::new(TokioRwLock::new(HashSet::new())),
                bot_user_id: user_id!("@bot:localhost").to_owned(),
                bot_display_name: Arc::new(TokioRwLock::new(None)),
                initial_sync_done: Arc::new(AtomicBool::new(true)),
                undecryptable_seen: Arc::new(TokioMutex::new(HashSet::new())),
            }
        }

        /// Legacy-resolver context: the single `[transcription]` endpoint at
        /// `stt_url`, which is what most route tests want.
        fn handler_ctx(
            stt_url: &str,
            workspace: &std::path::Path,
            tx: mpsc::Sender<clawcrew_api::channel::ChannelMessage>,
        ) -> HandlerCtx {
            handler_ctx_with_resolver(
                Some(super::super::legacy_transcription_resolver(
                    TranscriptionConfig {
                        enabled: true,
                        local_whisper: Some(clawcrew_config::schema::LocalWhisperConfig {
                            url: stt_url.to_string(),
                            bearer_token: Some("test-token".to_string()),
                            max_audio_bytes: 10 * 1024 * 1024,
                            timeout_secs: 30,
                        }),
                        ..TranscriptionConfig::default()
                    },
                )),
                workspace,
                tx,
            )
        }

        fn voice_event_json(event_id: &str) -> serde_json::Value {
            serde_json::json!({
                "type": "m.room.message",
                "event_id": event_id,
                "sender": "@alice:localhost",
                "origin_server_ts": 1_000_000u64,
                "content": {
                    "msgtype": "m.audio",
                    "body": "Voice message",
                    "filename": "voice.wav",
                    "url": "mxc://localhost/audioblob",
                    "org.matrix.msc3245.voice": {},
                    "info": { "mimetype": "audio/wav" }
                }
            })
        }

        async fn stt_server() -> wiremock::MockServer {
            let stt = wiremock::MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/v1/transcribe"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(serde_json::json!({ "text": TRANSCRIPT })),
                )
                .expect(1)
                .mount(&stt)
                .await;
            stt
        }

        async fn mount_media_download(matrix: &MatrixMockServer, wav: &[u8]) {
            matrix
                .mock_media_download()
                .respond_with(ResponseTemplate::new(200).set_body_raw(wav.to_vec(), "audio/wav"))
                .mount()
                .await;
            matrix
                .mock_authed_media_download()
                .ok_bytes(wav.to_vec())
                .mount()
                .await;
        }

        fn assert_stt_received_the_wav(reqs: &[wiremock::Request], wav: &[u8]) {
            assert_eq!(reqs.len(), 1, "exactly one transcription request");
            let body = &reqs[0].body;
            assert!(
                body.windows(wav.len()).any(|w| w == wav),
                "request must carry the constructed WAV bytes verbatim"
            );
            let text = String::from_utf8_lossy(body);
            assert!(
                text.contains("filename=\"voice.wav\""),
                "part filename: {text}"
            );
            assert!(text.contains("audio/wav"), "part content-type: {text}");
            assert_eq!(&wav[..4], b"RIFF");
            assert_eq!(&wav[8..12], b"WAVE");
        }

        #[tokio::test]
        async fn direct_voice_note_inserts_transcript_and_keeps_real_filename() {
            let wav = build_wav();

            let matrix = MatrixMockServer::new().await;
            let client = matrix.client_builder().build().await;
            matrix.sync_joined_room(&client, test_room()).await;
            mount_media_download(&matrix, &wav).await;

            let stt = stt_server().await;
            let workspace = tempfile::tempdir().unwrap();
            let (tx, mut rx) = mpsc::channel(4);
            let ctx = handler_ctx(
                &format!("{}/v1/transcribe", stt.uri()),
                workspace.path(),
                tx,
            );

            // The production handlers, registered exactly as `run_sync_loop`
            // does; the event arrives through the real sync-dispatch pipeline.
            let _guards = register_event_handlers(&client, &ctx);
            let json = voice_event_json("$audio1:localhost");
            matrix
                .sync_room(
                    &client,
                    JoinedRoomBuilder::new(test_room()).add_timeline_event(timeline_raw(&json)),
                )
                .await;

            let msg = recv_forwarded(&mut rx).await;
            assert!(
                msg.content
                    .contains(&format!("[voice transcript]: {TRANSCRIPT}")),
                "transcript must be inserted into the inbound content: {}",
                msg.content
            );
            assert!(
                msg.content.contains("voice.wav"),
                "marker must keep the real filename: {}",
                msg.content
            );

            let media_dir = workspace.path().join("matrix_files");
            let saved: Vec<_> = std::fs::read_dir(&media_dir)
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            assert_eq!(saved.len(), 1, "exactly one saved media file");
            let saved_path = saved[0].path();
            assert!(
                saved_path
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .ends_with("voice.wav"),
                "saved file keeps the real-name-derived filename: {saved_path:?}"
            );
            assert_eq!(std::fs::read(&saved_path).unwrap(), wav);

            assert_stt_received_the_wav(&stt.received_requests().await.unwrap(), &wav);
        }

        /// A voice note arriving on a *really configured* Matrix channel must
        /// reach the STT server the owning agent's `transcription_provider`
        /// names — not merely "some" registered provider.
        ///
        /// The channel is built by the production factory
        /// [`crate::orchestrator::build_configured_matrix_channel`], and only
        /// the resolver it installed is handed to the route, so nothing here
        /// short-circuits provider selection.
        ///
        /// Two providers are registered on purpose:
        /// [`super::super::build_transcription_manager`] binds a *sole*
        /// registered provider when the agent states no preference, so a
        /// one-provider fixture passes even with routing completely broken.
        /// The decoy is what makes the assertion mean something.
        #[tokio::test]
        async fn configured_route_transcribes_via_the_agents_provider() {
            use clawcrew_config::schema::LocalWhisperTranscriptionProviderConfig;

            let wav = build_wav();

            let matrix = MatrixMockServer::new().await;
            let client = matrix.client_builder().build().await;
            matrix.sync_joined_room(&client, test_room()).await;
            mount_media_download(&matrix, &wav).await;

            let wanted = wiremock::MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/v1/transcribe"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(serde_json::json!({ "text": TRANSCRIPT })),
                )
                .expect(1)
                .mount(&wanted)
                .await;

            let decoy = wiremock::MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/v1/transcribe"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(serde_json::json!({ "text": "decoy transcript" })),
                )
                .expect(0)
                .mount(&decoy)
                .await;

            // Provider aliases share no name with the channel alias or the
            // agent alias, so nothing can route correctly by coincidence.
            let mut config = clawcrew_config::schema::Config::default();
            config.transcription.enabled = true;
            config.channels.matrix.insert(
                "home".to_string(),
                MatrixConfig {
                    enabled: true,
                    homeserver: "https://matrix.invalid".to_string(),
                    access_token: Some("test-token".to_string()),
                    ..MatrixConfig::default()
                },
            );
            config.agents.insert(
                "listener".to_string(),
                clawcrew_config::schema::AliasedAgentConfig {
                    enabled: true,
                    channels: vec!["matrix.home".into()],
                    transcription_provider: "local_whisper.wanted".into(),
                    ..Default::default()
                },
            );
            config.providers.transcription.local_whisper.insert(
                "wanted".to_string(),
                LocalWhisperTranscriptionProviderConfig {
                    uri: format!("{}/v1/transcribe", wanted.uri()),
                    ..Default::default()
                },
            );
            config.providers.transcription.local_whisper.insert(
                "decoy".to_string(),
                LocalWhisperTranscriptionProviderConfig {
                    uri: format!("{}/v1/transcribe", decoy.uri()),
                    ..Default::default()
                },
            );

            let config_arc = Arc::new(parking_lot::RwLock::new(config));
            let channel = {
                let config = config_arc.read();
                crate::orchestrator::build_configured_matrix_channel(
                    &config_arc,
                    &config,
                    "home",
                    config
                        .channels
                        .matrix
                        .get("home")
                        .expect("configured Matrix alias"),
                )
                .expect("configured Matrix channel builds")
            };

            let workspace = tempfile::tempdir().unwrap();
            let (tx, mut rx) = mpsc::channel(4);
            let ctx =
                handler_ctx_with_resolver(channel.transcription.clone(), workspace.path(), tx);
            assert!(
                ctx.transcription.is_some(),
                "the configured channel must install a transcription resolver"
            );

            let _guards = register_event_handlers(&client, &ctx);
            let json = voice_event_json("$configured1:localhost");
            matrix
                .sync_room(
                    &client,
                    JoinedRoomBuilder::new(test_room()).add_timeline_event(timeline_raw(&json)),
                )
                .await;

            let msg = recv_forwarded(&mut rx).await;
            assert!(
                msg.content
                    .contains(&format!("[voice transcript]: {TRANSCRIPT}")),
                "transcript from the routed provider must land in the inbound content: {}",
                msg.content
            );

            assert_stt_received_the_wav(&wanted.received_requests().await.unwrap(), &wav);
            assert!(
                decoy.received_requests().await.unwrap().is_empty(),
                "the provider the agent did not name must never be called"
            );
            wanted.verify().await;
            decoy.verify().await;
        }

        #[tokio::test]
        async fn reply_to_voice_note_inserts_parent_transcript() {
            let wav = build_wav();

            let matrix = MatrixMockServer::new().await;
            let client = matrix.client_builder().build().await;
            matrix.sync_joined_room(&client, test_room()).await;
            mount_media_download(&matrix, &wav).await;

            // Parent-event fetch: /rooms/{roomId}/event/{eventId} returns the
            // parent m.audio event verbatim.
            let parent = voice_event_json("$parentaudio:localhost");
            Mock::given(method("GET"))
                .and(path_regex(
                    r"^/_matrix/client/v3/rooms/.*/event/\$parentaudio:localhost$",
                ))
                .respond_with(ResponseTemplate::new(200).set_body_json(parent))
                .mount(matrix.server())
                .await;

            let stt = stt_server().await;
            let workspace = tempfile::tempdir().unwrap();
            let (tx, mut rx) = mpsc::channel(4);
            let ctx = handler_ctx(
                &format!("{}/v1/transcribe", stt.uri()),
                workspace.path(),
                tx,
            );

            let _guards = register_event_handlers(&client, &ctx);
            let json = serde_json::json!({
                "type": "m.room.message",
                "event_id": "$reply1:localhost",
                "sender": "@alice:localhost",
                "origin_server_ts": 1_000_001u64,
                "content": {
                    "msgtype": "m.text",
                    "body": "what does this say?",
                    "m.relates_to": {
                        "m.in_reply_to": { "event_id": "$parentaudio:localhost" }
                    }
                }
            });
            matrix
                .sync_room(
                    &client,
                    JoinedRoomBuilder::new(test_room()).add_timeline_event(timeline_raw(&json)),
                )
                .await;

            let msg = recv_forwarded(&mut rx).await;
            assert!(
                msg.content
                    .contains(&format!("[voice transcript]: {TRANSCRIPT}")),
                "parent transcript must be inserted: {}",
                msg.content
            );
            assert!(
                msg.content.contains("voice.wav"),
                "marker must keep the parent's real filename: {}",
                msg.content
            );
            assert_stt_received_the_wav(&stt.received_requests().await.unwrap(), &wav);
        }

        /// `voice_origin` is the event's own MSC3245 flag, forwarded so the
        /// orchestrator can answer a `mirror` peer in kind.
        #[tokio::test]
        async fn a_voice_message_forwards_voice_origin() {
            let wav = build_wav();

            let matrix = MatrixMockServer::new().await;
            let client = matrix.client_builder().build().await;
            matrix.sync_joined_room(&client, test_room()).await;
            mount_media_download(&matrix, &wav).await;

            let stt = stt_server().await;
            let workspace = tempfile::tempdir().unwrap();
            let (tx, mut rx) = mpsc::channel(4);
            let ctx = handler_ctx(
                &format!("{}/v1/transcribe", stt.uri()),
                workspace.path(),
                tx,
            );

            let _guards = register_event_handlers(&client, &ctx);
            let json = voice_event_json("$audio-origin:localhost");
            matrix
                .sync_room(
                    &client,
                    JoinedRoomBuilder::new(test_room()).add_timeline_event(timeline_raw(&json)),
                )
                .await;

            let msg = recv_forwarded(&mut rx).await;
            assert!(
                msg.voice_origin,
                "an m.audio event carrying org.matrix.msc3245.voice is voice-origin"
            );
        }

        /// A text event is not voice-origin, whatever its body says.
        #[tokio::test]
        async fn a_text_message_does_not_forward_voice_origin() {
            let matrix = MatrixMockServer::new().await;
            let client = matrix.client_builder().build().await;
            matrix.sync_joined_room(&client, test_room()).await;

            let workspace = tempfile::tempdir().unwrap();
            let (tx, mut rx) = mpsc::channel(4);
            let ctx = handler_ctx_with_resolver(None, workspace.path(), tx);
            let _guards = register_event_handlers(&client, &ctx);

            let json = text_parent_event(
                "$text-origin:localhost",
                "@alice:localhost",
                "[voice transcript]: not actually a voice note",
            );
            matrix
                .sync_room(
                    &client,
                    JoinedRoomBuilder::new(test_room()).add_timeline_event(timeline_raw(&json)),
                )
                .await;

            let msg = recv_forwarded(&mut rx).await;
            assert!(
                !msg.voice_origin,
                "an m.text event is text-origin even when its body mimics a transcript"
            );
        }

        /// A text reply whose parent is a voice note borrows the parent's
        /// transcript but not its modality: only the event itself decides.
        #[tokio::test]
        async fn a_reply_to_a_voice_parent_does_not_forward_voice_origin() {
            let wav = build_wav();

            let matrix = MatrixMockServer::new().await;
            let client = matrix.client_builder().build().await;
            matrix.sync_joined_room(&client, test_room()).await;
            mount_media_download(&matrix, &wav).await;
            mount_parent_event(&matrix, voice_event_json("$parent-origin:localhost"), 1).await;

            let stt = stt_server().await;
            let workspace = tempfile::tempdir().unwrap();
            let (tx, mut rx) = mpsc::channel(4);
            let ctx = handler_ctx(
                &format!("{}/v1/transcribe", stt.uri()),
                workspace.path(),
                tx,
            );

            let _guards = register_event_handlers(&client, &ctx);
            let json = plain_reply_event(
                "$reply-origin:localhost",
                "$parent-origin:localhost",
                "what does this say?",
            );
            matrix
                .sync_room(
                    &client,
                    JoinedRoomBuilder::new(test_room()).add_timeline_event(timeline_raw(&json)),
                )
                .await;

            let msg = recv_forwarded(&mut rx).await;
            assert!(
                msg.content
                    .contains(&format!("[voice transcript]: {TRANSCRIPT}")),
                "the parent transcript is still inserted: {}",
                msg.content
            );
            assert!(
                !msg.voice_origin,
                "a text reply to a voice note is text-origin"
            );
        }

        // Encrypted-media variant: the event carries an E2EE `file` source;
        // the homeserver serves ciphertext and the client decrypts during
        // download, so the saved copy and the STT request must contain the
        // original plaintext WAV.
        #[tokio::test]
        async fn encrypted_voice_note_decrypts_and_inserts_transcript() {
            use std::io::Read as _;

            use matrix_sdk::ruma::OwnedMxcUri;
            use matrix_sdk::ruma::events::room::EncryptedFile;
            use matrix_sdk_base::crypto::AttachmentEncryptor;

            let wav = build_wav();
            let mut reader = wav.as_slice();
            let mut encryptor = AttachmentEncryptor::new(&mut reader);
            let mut ciphertext = Vec::new();
            encryptor.read_to_end(&mut ciphertext).unwrap();
            let keys = encryptor.finish();
            let file = EncryptedFile::new(
                OwnedMxcUri::from("mxc://localhost/encryptedaudioblob"),
                keys.encryption_info,
                keys.hashes,
            );

            let matrix = MatrixMockServer::new().await;
            let client = matrix.client_builder().build().await;
            matrix.sync_joined_room(&client, test_room()).await;
            mount_media_download(&matrix, &ciphertext).await;

            let stt = stt_server().await;
            let workspace = tempfile::tempdir().unwrap();
            let (tx, mut rx) = mpsc::channel(4);
            let ctx = handler_ctx(
                &format!("{}/v1/transcribe", stt.uri()),
                workspace.path(),
                tx,
            );

            let _guards = register_event_handlers(&client, &ctx);
            let json = serde_json::json!({
                "type": "m.room.message",
                "event_id": "$encaudio1:localhost",
                "sender": "@alice:localhost",
                "origin_server_ts": 1_000_002u64,
                "content": {
                    "msgtype": "m.audio",
                    "body": "Voice message",
                    "filename": "voice.wav",
                    "file": serde_json::to_value(&file).unwrap(),
                    "org.matrix.msc3245.voice": {},
                    "info": { "mimetype": "audio/wav" }
                }
            });
            matrix
                .sync_room(
                    &client,
                    JoinedRoomBuilder::new(test_room()).add_timeline_event(timeline_raw(&json)),
                )
                .await;

            let msg = recv_forwarded(&mut rx).await;
            assert!(
                msg.content
                    .contains(&format!("[voice transcript]: {TRANSCRIPT}")),
                "transcript must be inserted for encrypted media: {}",
                msg.content
            );
            assert!(
                msg.content.contains("voice.wav"),
                "marker must keep the real filename: {}",
                msg.content
            );

            let media_dir = workspace.path().join("matrix_files");
            let saved: Vec<_> = std::fs::read_dir(&media_dir)
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            assert_eq!(saved.len(), 1, "exactly one saved media file");
            assert_eq!(
                std::fs::read(saved[0].path()).unwrap(),
                wav,
                "saved copy must be the decrypted plaintext"
            );

            assert_stt_received_the_wav(&stt.received_requests().await.unwrap(), &wav);
        }

        fn mention_only_handler_ctx(
            tx: mpsc::Sender<clawcrew_api::channel::ChannelMessage>,
        ) -> HandlerCtx {
            let mut ctx = handler_ctx(
                "http://127.0.0.1:9/v1/transcribe",
                std::path::Path::new("/tmp"),
                tx,
            );
            // Clone config Arc contents with mention_only enabled.
            let mut config = (*ctx.config).clone();
            config.mention_only = true;
            ctx.config = Arc::new(config);
            ctx.transcription = None;
            ctx.workspace_dir = None;
            ctx
        }

        fn text_parent_event(event_id: &str, sender: &str, body: &str) -> serde_json::Value {
            serde_json::json!({
                "type": "m.room.message",
                "event_id": event_id,
                "sender": sender,
                "origin_server_ts": 1_000_000u64,
                "content": {
                    "msgtype": "m.text",
                    "body": body
                }
            })
        }

        fn plain_reply_event(event_id: &str, parent_id: &str, body: &str) -> serde_json::Value {
            serde_json::json!({
                "type": "m.room.message",
                "event_id": event_id,
                "sender": "@alice:localhost",
                "origin_server_ts": 1_000_001u64,
                "content": {
                    "msgtype": "m.text",
                    "body": body,
                    "m.relates_to": {
                        "m.in_reply_to": { "event_id": parent_id }
                    }
                }
            })
        }

        async fn mount_parent_event(
            matrix: &MatrixMockServer,
            parent: serde_json::Value,
            expected_gets: u64,
        ) {
            let parent_id = parent["event_id"]
                .as_str()
                .expect("parent event_id")
                .to_string();
            // Event ids in these tests are `$name:localhost` — escape `$` for the regex.
            let pattern = format!(
                r"^/_matrix/client/v3/rooms/.*/event/{}$",
                parent_id.replace('$', r"\$")
            );
            Mock::given(method("GET"))
                .and(path_regex(pattern))
                .respond_with(ResponseTemplate::new(200).set_body_json(parent))
                .expect(expected_gets)
                .mount(matrix.server())
                .await;
        }

        #[tokio::test]
        async fn mention_only_forwards_unmentioned_reply_to_bot_with_one_parent_fetch() {
            let matrix = MatrixMockServer::new().await;
            let client = matrix.client_builder().build().await;
            matrix.sync_joined_room(&client, test_room()).await;

            let parent = text_parent_event("$botparent:localhost", "@bot:localhost", "bot said hi");
            mount_parent_event(&matrix, parent, 1).await;

            let (tx, mut rx) = mpsc::channel(4);
            let ctx = mention_only_handler_ctx(tx);
            let _guards = register_event_handlers(&client, &ctx);

            let json = plain_reply_event(
                "$reply-bot:localhost",
                "$botparent:localhost",
                "thanks, continuing without an @mention",
            );
            matrix
                .sync_room(
                    &client,
                    JoinedRoomBuilder::new(test_room()).add_timeline_event(timeline_raw(&json)),
                )
                .await;

            let msg = recv_forwarded(&mut rx).await;
            assert_eq!(
                msg.content, "thanks, continuing without an @mention",
                "unmentioned reply-to-bot must be forwarded under mention_only"
            );
        }

        #[tokio::test]
        async fn mention_only_drops_unmentioned_reply_to_non_bot_parent() {
            let matrix = MatrixMockServer::new().await;
            let client = matrix.client_builder().build().await;
            matrix.sync_joined_room(&client, test_room()).await;

            let parent = text_parent_event(
                "$aliceparent:localhost",
                "@carol:localhost",
                "human chatter",
            );
            mount_parent_event(&matrix, parent, 1).await;

            let (tx, mut rx) = mpsc::channel(4);
            let ctx = mention_only_handler_ctx(tx);
            let _guards = register_event_handlers(&client, &ctx);

            let json = plain_reply_event(
                "$reply-human:localhost",
                "$aliceparent:localhost",
                "replying to carol, not the bot",
            );
            matrix
                .sync_room(
                    &client,
                    JoinedRoomBuilder::new(test_room()).add_timeline_event(timeline_raw(&json)),
                )
                .await;

            let timed_out = tokio::time::timeout(Duration::from_millis(500), rx.recv())
                .await
                .is_err();
            assert!(
                timed_out,
                "unmentioned reply to a non-bot parent must be dropped"
            );
        }

        #[tokio::test]
        async fn mention_only_reply_to_bot_voice_reuses_single_parent_fetch() {
            let wav = build_wav();

            let matrix = MatrixMockServer::new().await;
            let client = matrix.client_builder().build().await;
            matrix.sync_joined_room(&client, test_room()).await;
            mount_media_download(&matrix, &wav).await;

            // Parent is a bot voice note: gate + parent-media must share one GET.
            let mut parent = voice_event_json("$botvoice:localhost");
            parent["sender"] = serde_json::json!("@bot:localhost");
            mount_parent_event(&matrix, parent, 1).await;

            let stt = stt_server().await;
            let workspace = tempfile::tempdir().unwrap();
            let (tx, mut rx) = mpsc::channel(4);
            let mut ctx = handler_ctx(
                &format!("{}/v1/transcribe", stt.uri()),
                workspace.path(),
                tx,
            );
            let mut config = (*ctx.config).clone();
            config.mention_only = true;
            ctx.config = Arc::new(config);

            let _guards = register_event_handlers(&client, &ctx);
            let json = plain_reply_event(
                "$reply-voice:localhost",
                "$botvoice:localhost",
                "what did you say?",
            );
            matrix
                .sync_room(
                    &client,
                    JoinedRoomBuilder::new(test_room()).add_timeline_event(timeline_raw(&json)),
                )
                .await;

            let msg = recv_forwarded(&mut rx).await;
            assert!(
                msg.content
                    .contains(&format!("[voice transcript]: {TRANSCRIPT}")),
                "admitted reply must still attach parent media once: {}",
                msg.content
            );
            assert_stt_received_the_wav(&stt.received_requests().await.unwrap(), &wav);
        }
        #[tokio::test]
        async fn sync_ingress_suppresses_rejected_approval_replies_and_delivers_authorized_one() {
            let matrix = MatrixMockServer::new().await;
            let client = matrix.client_builder().build().await;
            matrix.sync_joined_room(&client, test_room()).await;
            let (tx, mut inbound_rx) = mpsc::channel(4);
            let ctx = HandlerCtx {
                config: Arc::new(MatrixConfig {
                    allowed_rooms: vec![test_room().to_string()],
                    ..MatrixConfig::default()
                }),
                alias: "test".to_string(),
                peer_resolver: Arc::new(|| vec!["@operator:localhost".to_string()]),
                transcription: None,
                workspace_dir: None,
                tx,
                pending_approvals: Arc::new(TokioMutex::new(HashMap::new())),
                threads_seen: Arc::new(TokioRwLock::new(HashSet::new())),
                bot_user_id: user_id!("@bot:localhost").to_owned(),
                bot_display_name: Arc::new(TokioRwLock::new(None)),
                initial_sync_done: Arc::new(AtomicBool::new(true)),
                undecryptable_seen: Arc::new(TokioMutex::new(HashSet::new())),
            };
            let (approved_tx, approved_rx) = oneshot::channel();
            let (wrong_tx, _wrong_rx) = oneshot::channel();
            let (unauthorized_tx, _unauthorized_rx) = oneshot::channel();
            {
                let mut approvals = ctx.pending_approvals.lock().await;
                approvals.insert(
                    "AUTH0001".into(),
                    crate::util::PendingApproval {
                        sender: approved_tx,
                        destination: test_room().to_string(),
                        tool_name: "tool".to_string(),
                    },
                );
                approvals.insert(
                    "WRONG001".into(),
                    crate::util::PendingApproval {
                        sender: wrong_tx,
                        destination: "!other:localhost".into(),
                        tool_name: "tool".to_string(),
                    },
                );
                approvals.insert(
                    "OTHER001".into(),
                    crate::util::PendingApproval {
                        sender: unauthorized_tx,
                        destination: test_room().to_string(),
                        tool_name: "tool".to_string(),
                    },
                );
            }

            let _guards = register_event_handlers(&client, &ctx);
            let approved = serde_json::json!({
                "type": "m.room.message",
                "event_id": "$approved:localhost",
                "sender": "@operator:localhost",
                "origin_server_ts": 1_000_000u64,
                "content": { "msgtype": "m.text", "body": "AUTH0001 approve" }
            });
            let wrong_destination = serde_json::json!({
                "type": "m.room.message",
                "event_id": "$wrong-destination:localhost",
                "sender": "@operator:localhost",
                "origin_server_ts": 1_000_001u64,
                "content": { "msgtype": "m.text", "body": "WRONG001 deny" }
            });
            let unauthorized = serde_json::json!({
                "type": "m.room.message",
                "event_id": "$unauthorized:localhost",
                "sender": "@other:localhost",
                "origin_server_ts": 1_000_002u64,
                "content": { "msgtype": "m.text", "body": "OTHER001 deny" }
            });
            matrix
                .sync_room(
                    &client,
                    JoinedRoomBuilder::new(test_room())
                        .add_timeline_event(timeline_raw(&approved))
                        .add_timeline_event(timeline_raw(&wrong_destination))
                        .add_timeline_event(timeline_raw(&unauthorized)),
                )
                .await;

            assert_eq!(
                tokio::time::timeout(Duration::from_secs(5), approved_rx)
                    .await
                    .expect("sync ingress should resolve the authorized approval")
                    .expect("sync ingress should resolve the authorized approval"),
                ChannelApprovalResponse::Approve
            );
            assert!(
                tokio::time::timeout(Duration::from_millis(200), inbound_rx.recv())
                    .await
                    .is_err(),
                "approval-shaped events must not reach agent dispatch"
            );
            let approvals = ctx.pending_approvals.lock().await;
            assert!(approvals.contains_key("WRONG001"));
            assert!(approvals.contains_key("OTHER001"));
        }
    }

    mod markers {
        use super::super::markers::{MarkerKind, parse};

        #[test]
        fn empty_text_yields_no_markers() {
            let (text, ms) = parse("");
            assert_eq!(text, "");
            assert!(ms.is_empty());
        }

        #[test]
        fn plain_text_passthrough() {
            let (text, ms) = parse("hello world");
            assert_eq!(text, "hello world");
            assert!(ms.is_empty());
        }

        #[test]
        fn single_image_marker_extracted() {
            let (text, ms) = parse("[image:https://example.com/cat.jpg]");
            assert_eq!(text, "");
            assert_eq!(ms.len(), 1);
            assert_eq!(ms[0].kind, MarkerKind::Image);
            assert_eq!(ms[0].target, "https://example.com/cat.jpg");
        }

        #[test]
        fn voice_marker_distinct_from_audio() {
            let (_, ms) = parse("[voice:/tmp/note.ogg] [audio:/tmp/song.mp3]");
            assert_eq!(ms.len(), 2);
            assert_eq!(ms[0].kind, MarkerKind::Voice);
            assert_eq!(ms[1].kind, MarkerKind::Audio);
        }

        #[test]
        fn echoed_media_placeholder_delivered_as_prose() {
            // A text-only model sees the degradation placeholder in its
            // history and may repeat it; the reply must reach the room as
            // readable text, not as a marker or a stray bracket span.
            let reply = format!(
                "I can't view that, it shows as {}.",
                clawcrew_providers::multimodal::MEDIA_PLACEHOLDER
            );
            let (text, ms) = parse(&reply);
            assert_eq!(text, reply);
            assert!(ms.is_empty());
            assert!(!text.contains('['));
        }

        #[test]
        fn multiple_markers_with_text_in_between() {
            let (text, ms) =
                parse("before [image:https://x/y.jpg] middle [file:/tmp/doc.pdf] after");
            assert_eq!(text, "before  middle  after");
            assert_eq!(ms.len(), 2);
            assert_eq!(ms[0].kind, MarkerKind::Image);
            assert_eq!(ms[1].kind, MarkerKind::File);
        }

        #[test]
        fn malformed_marker_left_in_text() {
            let (text, ms) = parse("foo [image: bar");
            assert_eq!(text, "foo [image: bar");
            assert!(ms.is_empty());
        }

        #[test]
        fn unknown_keyword_left_in_text() {
            let (text, ms) = parse("[banana:fruit]");
            assert_eq!(text, "[banana:fruit]");
            assert!(ms.is_empty());
        }

        #[test]
        fn empty_target_left_in_text() {
            let (text, ms) = parse("[image:]");
            assert_eq!(text, "[image:]");
            assert!(ms.is_empty());
        }

        #[test]
        fn marker_with_newline_inside_left_in_text() {
            let (text, ms) = parse("[image:a\nb]");
            assert!(text.contains("[image:a"));
            assert!(ms.is_empty());
        }
    }

    mod approval {
        use super::super::approval::{
            TOKEN_LEN, build_prompt_message, generate_token, generate_token_default, parse_reply,
        };
        use rand::SeedableRng;
        use rand::rngs::StdRng;
        use std::collections::HashSet;
        use clawcrew_api::channel::ChannelApprovalResponse;

        #[tokio::test]
        async fn pending_approval_requires_allowed_user_and_origin_room() {
            let pending = tokio::sync::Mutex::new(std::collections::HashMap::new());
            let (tx, rx) = tokio::sync::oneshot::channel();
            pending.lock().await.insert(
                "APPROVAL".to_string(),
                crate::util::PendingApproval {
                    sender: tx,
                    destination: "!origin:example.invalid".to_string(),
                    tool_name: "tool".to_string(),
                },
            );

            for response in [
                ChannelApprovalResponse::Approve,
                ChannelApprovalResponse::Deny,
                ChannelApprovalResponse::AlwaysApprove,
            ] {
                assert_eq!(
                    crate::util::resolve_pending_approval(
                        &pending,
                        "APPROVAL",
                        response,
                        super::super::allowlist::user_allowed(
                            &["@operator:example.invalid".to_string()],
                            "@other:example.invalid",
                        ),
                        "!origin:example.invalid",
                    )
                    .await,
                    crate::util::PendingApprovalResolution::Rejected,
                );
                assert!(pending.lock().await.contains_key("APPROVAL"));
            }

            assert_eq!(
                crate::util::resolve_pending_approval(
                    &pending,
                    "APPROVAL",
                    ChannelApprovalResponse::Approve,
                    super::super::allowlist::user_allowed(
                        &["@operator:example.invalid".to_string()],
                        "@operator:example.invalid",
                    ),
                    "!other:example.invalid",
                )
                .await,
                crate::util::PendingApprovalResolution::Rejected,
            );
            assert!(pending.lock().await.contains_key("APPROVAL"));

            assert_eq!(
                crate::util::resolve_pending_approval(
                    &pending,
                    "APPROVAL",
                    ChannelApprovalResponse::AlwaysApprove,
                    super::super::allowlist::user_allowed(
                        &["@operator:example.invalid".to_string()],
                        "@operator:example.invalid",
                    ),
                    "!origin:example.invalid",
                )
                .await,
                crate::util::PendingApprovalResolution::Resolved,
            );
            assert_eq!(rx.await.unwrap(), ChannelApprovalResponse::AlwaysApprove);

            let (approve_tx, approve_rx) = tokio::sync::oneshot::channel();
            pending.lock().await.insert(
                "APPROVE2".to_string(),
                crate::util::PendingApproval {
                    sender: approve_tx,
                    destination: "!origin:example.invalid".to_string(),
                    tool_name: "tool".to_string(),
                },
            );
            assert_eq!(
                crate::util::resolve_pending_approval(
                    &pending,
                    "APPROVE2",
                    ChannelApprovalResponse::Approve,
                    super::super::allowlist::user_allowed(
                        &["@operator:example.invalid".to_string()],
                        "@operator:example.invalid",
                    ),
                    "!origin:example.invalid",
                )
                .await,
                crate::util::PendingApprovalResolution::Resolved,
            );
            assert_eq!(approve_rx.await.unwrap(), ChannelApprovalResponse::Approve);
        }

        #[test]
        fn token_length_and_alphabet() {
            let mut rng = StdRng::seed_from_u64(42);
            let tok = generate_token(&mut rng);
            assert_eq!(tok.len(), TOKEN_LEN);
            assert!(tok.chars().all(|c| c.is_ascii_alphanumeric()));
        }

        #[test]
        fn tokens_are_diverse() {
            let mut rng = StdRng::seed_from_u64(7);
            let mut seen = HashSet::new();
            for _ in 0..1000 {
                seen.insert(generate_token(&mut rng));
            }
            assert!(
                seen.len() >= 998,
                "too many collisions: {}",
                1000 - seen.len()
            );
        }

        #[test]
        fn default_token_has_correct_length() {
            assert_eq!(generate_token_default().len(), TOKEN_LEN);
        }

        #[test]
        fn parse_approve() {
            let (tok, resp) = parse_reply("ABCDEFGH approve").expect("parses");
            assert_eq!(tok, "ABCDEFGH");
            assert_eq!(resp, ChannelApprovalResponse::Approve);
        }

        #[test]
        fn parse_deny_lowercase() {
            let (_, resp) = parse_reply("abcdefgh deny").expect("parses");
            assert_eq!(resp, ChannelApprovalResponse::Deny);
        }

        #[test]
        fn parse_always() {
            let (_, resp) = parse_reply("ABCDEFGH always").expect("parses");
            assert_eq!(resp, ChannelApprovalResponse::AlwaysApprove);
        }

        #[test]
        fn parse_yes_no_aliases() {
            assert_eq!(
                parse_reply("ABCDEFGH yes").map(|x| x.1),
                Some(ChannelApprovalResponse::Approve)
            );
            assert_eq!(
                parse_reply("ABCDEFGH no").map(|x| x.1),
                Some(ChannelApprovalResponse::Deny)
            );
        }

        #[test]
        fn rejects_wrong_token_length() {
            assert!(parse_reply("ABC approve").is_none());
            assert!(parse_reply("ABCDEFGHIJ approve").is_none());
        }

        #[test]
        fn rejects_unknown_verb() {
            assert!(parse_reply("ABCDEFGH maybe").is_none());
        }

        #[test]
        fn rejects_trailing_garbage() {
            assert!(parse_reply("ABCDEFGH approve please").is_none());
        }

        #[test]
        fn approval_prompt_message_suppresses_voice() {
            let prompt = crate::util::build_approve_deny_approval_prompt(
                &generate_token_default(),
                "shell",
                "ls -la",
                None,
            );

            let message = build_prompt_message(prompt.clone(), "!room:example.invalid");

            assert_eq!(message.recipient, "!room:example.invalid");
            assert_eq!(message.content, prompt);
            assert!(
                message.suppress_voice,
                "the approval prompt must suppress voice synthesis"
            );
            assert!(!message.force_voice);
        }

        #[test]
        fn localized_request_approval_prompt_still_parses_via_matrix_own_parser() {
            // Localization must not desync the (possibly translated) prompt
            // prose from Matrix's own approve/deny/always parser: the
            // keywords the prompt shows must remain the literal ASCII words
            // `parse_reply` expects, whatever locale is active.
            let token = generate_token_default();
            let prompt =
                crate::util::build_approve_deny_approval_prompt(&token, "shell", "ls -la", None);
            assert!(
                prompt.contains(&token),
                "prompt should echo the token verbatim; got {prompt:?}"
            );

            for (word, expected) in [
                ("approve", ChannelApprovalResponse::Approve),
                ("deny", ChannelApprovalResponse::Deny),
                ("always", ChannelApprovalResponse::AlwaysApprove),
            ] {
                let reply = format!("{token} {word}");
                assert!(
                    prompt.contains(&reply),
                    "prompt should show the exact reply {reply:?}; got {prompt:?}"
                );
                let (parsed_token, response) =
                    parse_reply(&reply).unwrap_or_else(|| panic!("{reply:?} should parse"));
                assert_eq!(parsed_token, token);
                assert_eq!(response, expected);
            }
        }
    }

    mod room_management {
        use super::super::room_management::{build_create_room_request, build_invite_user_request};
        use matrix_sdk::ruma::api::client::room::Visibility as MatrixVisibility;
        use serde_json::json;
        use clawcrew_api::channel::{RoomCreationOptions, RoomVisibility};

        #[test]
        fn create_room_request_maps_typed_options() {
            let request = build_create_room_request(&RoomCreationOptions {
                name: Some("Ops room".into()),
                topic: Some("Operations".into()),
                invites: vec!["@alice:example.org".into(), "@bob:example.org".into()],
                visibility: Some(RoomVisibility::Public),
                encryption: Some(true),
            })
            .expect("request builds");

            assert_eq!(request.name.as_deref(), Some("Ops room"));
            assert_eq!(request.topic.as_deref(), Some("Operations"));
            assert_eq!(request.visibility, MatrixVisibility::Public);
            assert_eq!(request.invite.len(), 2);
            assert_eq!(request.invite[0].as_str(), "@alice:example.org");
            assert_eq!(request.invite[1].as_str(), "@bob:example.org");
            assert_eq!(request.initial_state.len(), 1);
        }

        #[test]
        fn create_room_request_rejects_invalid_invite_user() {
            let err = build_create_room_request(&RoomCreationOptions {
                invites: vec!["not-a-mxid".into()],
                ..RoomCreationOptions::default()
            })
            .unwrap_err();

            assert!(err.to_string().contains("invalid invite user id"));
        }

        #[test]
        fn invite_user_request_parses_room_and_user_ids() {
            let request =
                build_invite_user_request("!room:example.org", "@alice:example.org").unwrap();

            assert_eq!(request.room_id.as_str(), "!room:example.org");
            assert_eq!(
                serde_json::to_value(&request.recipient).unwrap(),
                json!({"user_id": "@alice:example.org"})
            );
        }

        #[test]
        fn invite_user_request_rejects_invalid_ids() {
            let err = build_invite_user_request("not-a-room", "@alice:example.org").unwrap_err();
            assert!(err.to_string().contains("invalid room id"));

            let err = build_invite_user_request("!room:example.org", "not-a-user").unwrap_err();
            assert!(err.to_string().contains("invalid user id"));
        }
    }

    mod mention {
        use super::super::mention::{admit_group_message, is_mentioned, sender_is_user};
        use matrix_sdk::ruma::user_id;

        #[test]
        fn explicit_mention_in_user_ids_passes() {
            let bot = user_id!("@bot:example.org");
            assert!(is_mentioned(
                bot,
                None,
                Some(&["@bot:example.org".to_string()]),
                "hi",
            ));
        }

        #[test]
        fn explicit_mention_list_without_bot_rejects() {
            let bot = user_id!("@bot:example.org");
            assert!(!is_mentioned(
                bot,
                None,
                Some(&["@alice:example.org".to_string()]),
                "@bot:example.org help",
            ));
        }

        #[test]
        fn body_fallback_full_id() {
            let bot = user_id!("@bot:example.org");
            assert!(is_mentioned(bot, None, None, "@bot:example.org help"));
        }

        #[test]
        fn body_fallback_localpart_only() {
            let bot = user_id!("@bot:example.org");
            assert!(is_mentioned(bot, None, None, "hey @bot please reply"));
        }

        #[test]
        fn body_fallback_display_name() {
            let bot = user_id!("@bot:example.org");
            assert!(is_mentioned(bot, Some("ClawCrew"), None, "hi clawcrew!"));
        }

        #[test]
        fn no_mention_rejects() {
            let bot = user_id!("@bot:example.org");
            assert!(!is_mentioned(
                bot,
                Some("ClawCrew"),
                None,
                "no mention here"
            ));
        }

        #[test]
        fn admit_group_message_allows_reply_to_bot_without_mention() {
            assert!(admit_group_message(false, true));
            assert!(admit_group_message(true, false));
            assert!(admit_group_message(true, true));
            assert!(!admit_group_message(false, false));
        }

        #[test]
        fn sender_is_user_matches_bot_parent_event() {
            let bot = user_id!("@bot:example.org");
            let parent =
                r#"{"sender":"@bot:example.org","type":"m.room.message","content":{"body":"hi"}}"#;
            let other = r#"{"sender":"@alice:example.org","type":"m.room.message","content":{"body":"hi"}}"#;
            assert!(sender_is_user(parent, bot));
            assert!(!sender_is_user(other, bot));
            assert!(!sender_is_user("not-json", bot));
        }
    }

    mod allowlist {
        use super::super::allowlist::{room_allowed_static, user_allowed};

        #[test]
        fn empty_user_list_denies_all() {
            assert!(!user_allowed(&[], "@a:b"));
        }

        #[test]
        fn star_user_list_allows_all() {
            assert!(user_allowed(&["*".to_string()], "@a:b"));
        }

        #[test]
        fn user_in_list_allowed() {
            assert!(user_allowed(&["@a:b".to_string()], "@a:b"));
        }

        #[test]
        fn user_not_in_list_denied() {
            assert!(!user_allowed(&["@a:b".to_string()], "@c:d"));
        }

        #[test]
        fn user_in_list_case_insensitive() {
            // Operator-configured case shouldn't matter — Matrix MXIDs are
            // spec-lowercase but tolerated in mixed case by some servers.
            assert!(user_allowed(
                &["@Bot:Example.org".to_string()],
                "@bot:example.org"
            ));
            assert!(user_allowed(
                &["@bot:example.org".to_string()],
                "@Bot:EXAMPLE.org"
            ));
        }

        #[test]
        fn empty_room_list_allows_all() {
            assert!(room_allowed_static(&[], "!any:server"));
        }

        #[test]
        fn room_in_list_allowed() {
            assert!(room_allowed_static(
                &["!ok:server".to_string()],
                "!ok:server"
            ));
        }

        #[test]
        fn room_not_in_list_denied() {
            assert!(!room_allowed_static(
                &["!ok:server".to_string()],
                "!nope:server"
            ));
        }
    }

    mod ack_reactions {
        use std::sync::Arc;

        use tempfile::TempDir;
        use clawcrew_api::channel::Channel;
        use clawcrew_config::schema::MatrixConfig;

        use super::super::MatrixChannel;

        #[tokio::test]
        async fn matrix_remove_reaction_noops_before_parsing_when_ack_disabled() {
            let config = MatrixConfig {
                homeserver: "https://matrix.example.com".to_string(),
                access_token: Some("token".to_string()),
                ack_reactions: Some(false),
                ..MatrixConfig::default()
            };
            let state_dir = TempDir::new().expect("temp state dir");
            let channel = MatrixChannel::new(
                config,
                "matrix",
                Arc::new(Vec::<String>::new),
                state_dir.path().to_path_buf(),
            )
            .expect("matrix channel");

            channel
                .remove_reaction("bad-room", "bad-event", "✅")
                .await
                .expect("ack-disabled reaction removal should be a no-op");
        }
    }

    mod user_boundary {
        use std::sync::Arc;
        use std::time::Duration;

        use matrix_sdk::config::SyncSettings;
        use matrix_sdk::event_handler::RawEvent;
        use matrix_sdk::ruma::events::room::message::OriginalSyncRoomMessageEvent;
        use matrix_sdk::ruma::serde::Raw;
        use matrix_sdk::ruma::{owned_room_id, owned_user_id};
        use tempfile::TempDir;
        use tokio::sync::mpsc;
        use wiremock::matchers::{body_partial_json, method, path_regex};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        use clawcrew_api::channel::Channel;
        use clawcrew_config::schema::{Config, MatrixConfig, MatrixStreamMode};

        use super::super::{MatrixChannel, inbound};

        async fn assert_matrix_single_message_crosses_inbound_and_outbound_user_boundary() {
            let server = MockServer::start().await;
            let room_id = owned_room_id!("!room:server");
            let bot_user_id = owned_user_id!("@bot:server");
            let sender = "@alice:server";

            Mock::given(method("GET"))
                .and(path_regex(r"^/_matrix/client/versions$"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "versions": ["r0.6.0", "v1.1", "v1.2", "v1.3", "v1.4", "v1.5"],
                    "unstable_features": {}
                })))
                .expect(1..)
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path_regex(
                    r"^/_matrix/client/(v3|r0)/profile/.*/displayname$",
                ))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "displayname": "ClawCrew Test"
                })))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path_regex(
                    r"^/_matrix/client/(v3|r0)/user/.*/account_data/m\.secret_storage\.default_key$",
                ))
                .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                    "errcode": "M_NOT_FOUND",
                    "error": "not found"
                })))
                .mount(&server)
                .await;
            Mock::given(method("POST"))
                .and(path_regex(r"^/_matrix/client/(v3|r0)/keys/upload$"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "one_time_key_counts": {}
                })))
                .mount(&server)
                .await;
            Mock::given(method("POST"))
                .and(path_regex(r"^/_matrix/client/(v3|r0)/keys/query$"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "device_keys": {}
                })))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path_regex(r"^/_matrix/client/(v3|r0)/sync$"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "next_batch": "s1",
                    "rooms": {
                        "join": {
                            room_id.as_str(): {
                                "state": { "events": [] },
                                "timeline": {
                                    "limited": false,
                                    "prev_batch": "t0",
                                    "events": []
                                }
                            }
                        }
                    }
                })))
                .expect(1)
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path_regex(
                    r"^/_matrix/client/(v3|r0)/rooms/.*/state/m\.room\.encryption/?$",
                ))
                .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                    "errcode": "M_NOT_FOUND",
                    "error": "room is not encrypted"
                })))
                .mount(&server)
                .await;
            Mock::given(method("PUT"))
                .and(path_regex(r"^/_matrix/client/(v3|r0)/rooms/.*/typing/.*$"))
                .and(body_partial_json(serde_json::json!({ "typing": false })))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
                .mount(&server)
                .await;
            Mock::given(method("PUT"))
                .and(path_regex(
                    r"^/_matrix/client/(v3|r0)/rooms/.*/send/m\.room\.message/.*$",
                ))
                .and(body_partial_json(serde_json::json!({ "body": "..." })))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "event_id": "$draft:server"
                })))
                .expect(1)
                .mount(&server)
                .await;
            Mock::given(method("PUT"))
                .and(path_regex(
                    r"^/_matrix/client/(v3|r0)/rooms/.*/redact/.*/.*$",
                ))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "event_id": "$redaction:server"
                })))
                .expect(1)
                .mount(&server)
                .await;
            Mock::given(method("PUT"))
                .and(path_regex(
                    r"^/_matrix/client/(v3|r0)/rooms/.*/send/m\.room\.message/.*$",
                ))
                .and(body_partial_json(serde_json::json!({ "body": "ok" })))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "event_id": "$final:server"
                })))
                .expect(1)
                .mount(&server)
                .await;

            let matrix_config = MatrixConfig {
                homeserver: server.uri(),
                access_token: Some("secret-token".to_string()),
                user_id: Some(bot_user_id.to_string()),
                device_id: Some("DEVICE".to_string()),
                allowed_rooms: vec![room_id.to_string()],
                stream_mode: MatrixStreamMode::SingleMessage,
                stream_draft_delete: true,
                reply_in_thread: false,
                ack_reactions: Some(false),
                ..MatrixConfig::default()
            };
            let state_dir = TempDir::new().expect("temp state dir");
            let channel = Arc::new(
                MatrixChannel::new(
                    matrix_config.clone(),
                    "single",
                    Arc::new(move || vec![sender.to_string()]),
                    state_dir.path().to_path_buf(),
                )
                .expect("matrix channel"),
            );
            let client = channel.ensure_client().await.expect("matrix client");
            client
                .sync_once(SyncSettings::default())
                .await
                .expect("mock sync populates joined room");
            let room = client.get_room(&room_id).expect("joined room");

            let event_json = serde_json::json!({
                "type": "m.room.message",
                "sender": sender,
                "event_id": "$inbound:server",
                "origin_server_ts": 1,
                "content": {
                    "msgtype": "m.text",
                    "body": "hello"
                }
            });
            let event: OriginalSyncRoomMessageEvent =
                serde_json::from_value(event_json.clone()).expect("Matrix room-message event");
            let raw: Raw<serde_json::Value> = Raw::new(&event_json).expect("raw Matrix event");
            let (tx, mut rx) = mpsc::channel(1);
            let handler_ctx = inbound::HandlerCtx {
                config: Arc::clone(&channel.config),
                alias: channel.alias.clone(),
                peer_resolver: Arc::clone(&channel.peer_resolver),
                transcription: channel.transcription.clone(),
                workspace_dir: channel.workspace_dir.clone(),
                tx,
                pending_approvals: Arc::clone(&channel.pending_approvals),
                threads_seen: Arc::clone(&channel.threads_seen),
                bot_user_id: bot_user_id.clone(),
                bot_display_name: Arc::clone(&channel.bot_display_name),
                initial_sync_done: Arc::clone(&channel.initial_sync_done),
                undecryptable_seen: Arc::clone(&channel.undecryptable_seen),
            };
            inbound::handle_message_for_test(handler_ctx, event, room, RawEvent(raw.into_json()))
                .await
                .expect("Matrix inbound adapter accepts event");
            let message = rx.recv().await.expect("adapter forwards channel message");

            assert_eq!(message.content, "hello");
            assert_eq!(message.sender, sender);
            assert_eq!(message.reply_target, room_id.as_str());
            assert_eq!(message.channel_alias.as_deref(), Some("single"));

            let mut config = Config::default();
            config
                .channels
                .matrix
                .insert("single".to_string(), matrix_config);
            tokio::time::timeout(
                Duration::from_secs(10),
                crate::orchestrator::tests::process_message_with_dummy_provider(
                    channel as Arc<dyn Channel>,
                    message,
                    config,
                ),
            )
            .await
            .expect("Matrix inbound-to-outbound dispatch completes");
        }

        #[test]
        fn matrix_single_message_crosses_inbound_and_outbound_user_boundary() {
            crate::orchestrator::tests::run_channel_dispatch_test(|| {
                Box::pin(assert_matrix_single_message_crosses_inbound_and_outbound_user_boundary())
            });
        }
    }

    mod context {
        use super::super::context::{claim_first_visit, format_preamble, mark_seen};
        use matrix_sdk::ruma::{OwnedEventId, owned_event_id};
        use std::{collections::HashSet, sync::Arc};
        use tokio::sync::RwLock;

        fn empty() -> Arc<RwLock<HashSet<OwnedEventId>>> {
            Arc::new(RwLock::new(HashSet::new()))
        }

        #[test]
        fn preamble_includes_sender_and_body() {
            let p = format_preamble("@alice:server", "hello");
            assert_eq!(p, "[Thread root from @alice:server]: hello\n\n");
        }

        #[test]
        fn preamble_skips_body_when_empty() {
            let p = format_preamble("@alice:server", "");
            assert_eq!(p, "[Thread root from @alice:server]\n\n");
        }

        #[tokio::test]
        async fn first_visit_returns_true_then_false() {
            let set = empty();
            let id = owned_event_id!("$abc:server");
            assert!(claim_first_visit(&set, &id).await);
            assert!(!claim_first_visit(&set, &id).await);
        }

        #[tokio::test]
        async fn pre_marked_thread_returns_false() {
            let set = empty();
            let id = owned_event_id!("$abc:server");
            mark_seen(&set, id.clone()).await;
            assert!(!claim_first_visit(&set, &id).await);
        }
    }

    mod streaming {
        use super::super::MatrixChannel;
        use super::super::outbound;
        use super::super::streaming;
        use super::super::streaming::{
            MultiDraft, PartialDraft, PartialFinalizeAction, SingleDraft,
            SingleRetainedDraftAction, State, decide_partial_finalize_action, insert_multi,
            insert_partial, insert_single, mark_single_edit_delivered, multi_contains,
            normalize_matrix_progress_line, partial_contains, partial_len, partial_should_edit,
            partial_visible_text, push_single_progress, push_single_progress_line,
            single_cancel_deletes_draft, single_contains, single_edit_interval_elapsed,
            single_finalize_plan, single_render_changed, single_retained_draft_action,
            single_visible_text_with_budget, single_visible_text_with_edit_budget,
        };
        use matrix_sdk::config::SyncSettings;
        use matrix_sdk::ruma::{
            OwnedEventId,
            events::room::message::{
                MessageType, ReplacementMetadata, RoomMessageEventContent,
                RoomMessageEventContentWithoutRelation, TextMessageEventContent,
            },
            owned_event_id, owned_room_id,
        };
        use std::collections::VecDeque;
        use std::sync::Arc;
        use std::time::{Duration, Instant};
        use tempfile::TempDir;
        use wiremock::{
            Mock, MockServer, ResponseTemplate,
            matchers::{body_partial_json, method, path_regex},
        };
        use clawcrew_api::channel::{Channel, DraftProgress, DraftProgressKind, SendMessage};
        use clawcrew_config::schema::{MatrixConfig, MatrixStreamMode};
        use clawcrew_runtime::agent::loop_::{
            DRAFT_PLACEHOLDER, REASONING_FULL_PREFIX, THINKING_STATUS_PREFIX, thinking_status_text,
        };

        fn draft(text: &str, last_edit: Instant) -> PartialDraft {
            PartialDraft {
                event_id: owned_event_id!("$1:server"),
                thread_anchor: None,
                last_text: text.to_string(),
                last_edit,
            }
        }

        fn partial_draft(event_id: OwnedEventId, text: &str) -> PartialDraft {
            PartialDraft {
                event_id,
                thread_anchor: None,
                last_text: text.to_string(),
                last_edit: Instant::now(),
            }
        }

        #[test]
        fn single_message_final_budget_keeps_utf8_safe_rendered_prefix() {
            let text = format!("{}NEWEST", "**x** ".repeat(8_000));
            let budget = 48_000;
            let bounded = outbound::bounded_body(&text, Some(budget));
            let serialized = serde_json::to_vec(&RoomMessageEventContent::text_markdown(&bounded))
                .expect("plain Matrix message serializes");

            assert!(bounded.starts_with("**x** "));
            assert!(!bounded.ends_with("NEWEST"));
            assert!(serialized.len() <= budget);
            assert!(
                serialized.len() > bounded.len(),
                "formatted_body expansion must be included in the budget"
            );

            let event_id = owned_event_id!("$draft:server");
            let bounded_edit = outbound::bounded_edit_body(&text, &event_id, Some(budget));
            let edited = RoomMessageEventContentWithoutRelation::new(MessageType::Text(
                TextMessageEventContent::markdown(&bounded_edit),
            ))
            .make_replacement(ReplacementMetadata::new(event_id, None));
            assert!(bounded_edit.starts_with("**x** "));
            assert!(!bounded_edit.ends_with("NEWEST"));
            assert!(
                serde_json::to_vec(&edited)
                    .expect("replacement Matrix message serializes")
                    .len()
                    <= budget
            );

            for message_max_bytes in [0, 511] {
                let config = MatrixConfig {
                    message_max_bytes,
                    ..Default::default()
                };
                assert_eq!(config.effective_message_max_bytes(), 512);
            }
        }

        fn single_draft(event_id: OwnedEventId) -> SingleDraft {
            SingleDraft {
                event_id,
                thread_anchor: None,
                lines: VecDeque::new(),
                last_text: DRAFT_PLACEHOLDER.to_string(),
                last_edit: Instant::now(),
            }
        }

        #[test]
        fn skip_when_text_unchanged() {
            let now = Instant::now();
            let d = draft("hello", now - Duration::from_secs(60));
            assert!(!partial_should_edit(
                &d,
                "hello",
                now,
                Duration::from_millis(500)
            ));
        }

        #[test]
        fn skip_within_rate_limit() {
            let now = Instant::now();
            let d = draft("hello", now - Duration::from_millis(100));
            assert!(!partial_should_edit(
                &d,
                "world",
                now,
                Duration::from_millis(500)
            ));
        }

        #[test]
        fn allow_after_rate_limit() {
            let now = Instant::now();
            let d = draft("hello", now - Duration::from_millis(600));
            assert!(partial_should_edit(
                &d,
                "world",
                now,
                Duration::from_millis(500)
            ));
        }

        #[test]
        fn partial_visible_text_strips_attachment_markers() {
            assert_eq!(
                partial_visible_text("Report ready [DOCUMENT:report.pdf]").as_deref(),
                Some("Report ready")
            );
        }

        #[test]
        fn partial_visible_text_skips_marker_only_updates() {
            assert_eq!(partial_visible_text("[DOCUMENT:report.pdf]"), None);
        }

        #[test]
        fn single_progress_slides_oldest_entries() {
            let mut draft = single_draft(owned_event_id!("$single:server"));
            push_single_progress_line(&mut draft, "one", 2);
            push_single_progress_line(&mut draft, "two", 2);
            push_single_progress_line(&mut draft, "three", 2);

            assert_eq!(
                single_visible_text_with_budget(&draft, usize::MAX),
                "two\nthree"
            );
        }

        #[test]
        fn single_progress_limit_counts_visible_lines_not_events() {
            let mut draft = single_draft(owned_event_id!("$single:server"));
            push_single_progress_line(
                &mut draft,
                &format!("{REASONING_FULL_PREFIX}one\ntwo\nthree"),
                2,
            );

            assert_eq!(
                single_visible_text_with_budget(&draft, usize::MAX),
                format!("{REASONING_FULL_PREFIX}two\nthree")
            );

            push_single_progress_line(&mut draft, "shell: printf 'a\\nb'\nnext\r\n", 2);

            assert_eq!(
                single_visible_text_with_budget(&draft, usize::MAX),
                format!("{REASONING_FULL_PREFIX}three\nshell\\: printf \\'a\\\\nb\\'␊next")
            );
        }

        #[test]
        fn single_progress_window_keeps_the_newest_physical_reasoning_lines() {
            let mut draft = single_draft(owned_event_id!("$single:server"));
            let reasoning = (1..=20)
                .map(|line| format!("line {line}"))
                .collect::<Vec<_>>()
                .join("\n");
            push_single_progress_line(
                &mut draft,
                &format!("{REASONING_FULL_PREFIX}{reasoning}"),
                5,
            );

            assert_eq!(
                single_visible_text_with_budget(&draft, usize::MAX),
                format!("{REASONING_FULL_PREFIX}line 16\nline 17\nline 18\nline 19\nline 20")
            );
        }

        #[test]
        fn single_reasoning_progress_updates_existing_line() {
            let mut draft = single_draft(owned_event_id!("$single:server"));
            push_single_progress_line(
                &mut draft,
                &format!("{REASONING_FULL_PREFIX}Thinking (round 2) through"),
                10,
            );
            push_single_progress_line(
                &mut draft,
                &format!("{REASONING_FULL_PREFIX} carefully"),
                10,
            );
            push_single_progress_line(&mut draft, &format!("{REASONING_FULL_PREFIX} now"), 10);

            assert_eq!(
                single_visible_text_with_budget(&draft, usize::MAX),
                format!("{REASONING_FULL_PREFIX}Thinking \\(round 2\\) through carefully now")
            );
        }

        #[test]
        fn single_reasoning_progress_keeps_complete_line_until_exact_event_fitting() {
            let mut draft = single_draft(owned_event_id!("$single:server"));
            let max_bytes = format!("{REASONING_FULL_PREFIX}abcd").len();

            push_single_progress_line(&mut draft, &format!("{REASONING_FULL_PREFIX}abcd"), 10);
            push_single_progress_line(&mut draft, &format!("{REASONING_FULL_PREFIX}efgh"), 10);

            let retained = draft.lines.front().expect("reasoning line retained");
            assert_eq!(retained.text, format!("{REASONING_FULL_PREFIX}abcdefgh"));
            assert!(retained.text.len() > max_bytes);
        }

        #[test]
        fn single_reasoning_progress_does_not_merge_into_static_status() {
            let mut draft = single_draft(owned_event_id!("$single:server"));
            push_single_progress_line(
                &mut draft,
                &format!("{THINKING_STATUS_PREFIX}Thinking...\n"),
                10,
            );
            push_single_progress_line(&mut draft, &format!("{REASONING_FULL_PREFIX}The"), 10);
            push_single_progress_line(&mut draft, &format!("{REASONING_FULL_PREFIX} answer"), 10);

            assert_eq!(
                single_visible_text_with_budget(&draft, usize::MAX),
                format!("{THINKING_STATUS_PREFIX}Thinking...\n{REASONING_FULL_PREFIX}The answer")
            );
        }

        #[test]
        fn single_reasoning_preserves_kind_when_text_matches_thinking_status() {
            let mut draft = single_draft(owned_event_id!("$single:server"));
            let status = thinking_status_text(0);
            let reasoning = format!("{REASONING_FULL_PREFIX}Thinking...\n");

            // Preserve source identity even though the two raw strings match.
            push_single_progress(&mut draft, DraftProgress::status(status.clone()), 10);
            push_single_progress(&mut draft, DraftProgress::reasoning(reasoning.clone()), 10);

            assert_eq!(
                status, reasoning,
                "the regression requires an exact collision"
            );
            assert_eq!(draft.lines.len(), 2);
            assert_eq!(draft.lines[0].kind, DraftProgressKind::Status);
            assert_eq!(draft.lines[1].kind, DraftProgressKind::Reasoning);
        }

        #[test]
        fn single_reasoning_progress_starts_new_line_after_tool_progress() {
            let mut draft = single_draft(owned_event_id!("$single:server"));
            push_single_progress_line(&mut draft, &format!("{REASONING_FULL_PREFIX}The"), 10);
            push_single_progress_line(&mut draft, "\u{2705} shell: command=true (0s)\n", 10);
            push_single_progress_line(&mut draft, &format!("{REASONING_FULL_PREFIX} answer"), 10);

            assert_eq!(
                single_visible_text_with_budget(&draft, usize::MAX),
                format!(
                    "{REASONING_FULL_PREFIX}The\n✅ shell\\: command\\=true \\(0s\\)\n{REASONING_FULL_PREFIX} answer"
                )
            );
        }

        #[test]
        fn single_status_progress_remains_one_static_line() {
            let mut draft = single_draft(owned_event_id!("$single:server"));
            let status = thinking_status_text(0);
            push_single_progress_line(&mut draft, &status, 10);
            push_single_progress_line(&mut draft, &status, 10);
            push_single_progress_line(&mut draft, &status, 10);

            assert_eq!(
                single_visible_text_with_budget(&draft, usize::MAX),
                format!("{THINKING_STATUS_PREFIX}Thinking...")
            );
        }

        #[test]
        fn single_status_progress_starts_new_line_after_tool_progress() {
            let mut draft = single_draft(owned_event_id!("$single:server"));
            push_single_progress_line(&mut draft, &thinking_status_text(0), 10);
            push_single_progress_line(&mut draft, "\u{2705} shell: command=true (0s)\n", 10);
            push_single_progress_line(&mut draft, &thinking_status_text(1), 10);

            assert_eq!(
                single_visible_text_with_budget(&draft, usize::MAX),
                format!(
                    "{THINKING_STATUS_PREFIX}Thinking...\n✅ shell\\: command\\=true \\(0s\\)\n{THINKING_STATUS_PREFIX}Thinking (round 2)..."
                )
            );
        }

        #[test]
        fn single_status_progress_does_not_downgrade_round_status() {
            let mut draft = single_draft(owned_event_id!("$single:server"));
            push_single_progress_line(&mut draft, &thinking_status_text(1), 10);
            push_single_progress_line(&mut draft, &thinking_status_text(0), 10);

            assert_eq!(
                single_visible_text_with_budget(&draft, usize::MAX),
                format!("{THINKING_STATUS_PREFIX}Thinking (round 2)...")
            );
        }

        #[test]
        fn single_progress_zero_limit_keeps_all_entries() {
            let mut draft = single_draft(owned_event_id!("$single:server"));
            push_single_progress_line(&mut draft, "one", 0);
            push_single_progress_line(&mut draft, "two", 0);
            push_single_progress_line(&mut draft, "three", 0);

            assert_eq!(
                single_visible_text_with_budget(&draft, usize::MAX),
                "one\ntwo\nthree"
            );
        }

        #[test]
        fn single_progress_byte_budget_drops_oldest_entries_after_line_limit() {
            let mut draft = single_draft(owned_event_id!("$single:server"));
            push_single_progress_line(&mut draft, "one", 0);
            push_single_progress_line(&mut draft, "two", 0);
            push_single_progress_line(&mut draft, "three", 0);

            assert_eq!(
                single_visible_text_with_budget(&draft, "two\nthree".len()),
                "two\nthree"
            );
        }

        #[test]
        fn single_progress_edit_budget_replaces_oversized_utf8_line_with_alert() {
            let mut draft = single_draft(owned_event_id!("$single:server"));
            push_single_progress_line(&mut draft, &"😀".repeat(300), 0);

            let visible = single_visible_text_with_edit_budget(&draft, 512);

            assert!(visible.contains("too large to fit"));
            assert!(
                outbound::serialized_edit_content_len(&visible, &draft.event_id)
                    .is_some_and(|actual| actual <= 512)
            );
        }

        #[test]
        fn single_progress_edit_budget_preserves_tool_identity_in_oversized_alert() {
            let mut draft = single_draft(owned_event_id!("$single:server"));
            push_single_progress_line(&mut draft, &format!("✅ browser: {}", "😀".repeat(300)), 0);

            let visible = single_visible_text_with_edit_budget(&draft, 512);

            assert!(visible.starts_with("✅ browser: "));
            assert!(visible.contains("too large to fit"));
            assert!(
                outbound::serialized_edit_content_len(&visible, &draft.event_id)
                    .is_some_and(|actual| actual <= 512)
            );
        }

        #[test]
        fn single_progress_normalizes_vertical_whitespace_for_matrix_only() {
            assert_eq!(
                normalize_matrix_progress_line("shell: printf 'a\\nb'\nnext\r\n"),
                "shell\\: printf \\'a\\\\nb\\'␊next"
            );
            assert_eq!(
                normalize_matrix_progress_line("delegate: prompt=Check **service**\nthen _report_"),
                "delegate\\: prompt\\=Check \\*\\*service\\*\\*␊then \\_report\\_"
            );
            assert_eq!(
                normalize_matrix_progress_line(&format!(
                    "{REASONING_FULL_PREFIX}Check **service**\nthen _report_"
                )),
                format!("{REASONING_FULL_PREFIX}Check \\*\\*service\\*\\*\nthen \\_report\\_")
            );
        }

        #[test]
        fn single_progress_uses_literal_matrix_markdown_transport_once() {
            let raw = "    **bold** <div data-x=\"1\">&amp;</div> `code`\t\u{001b}[31m";
            let encoded = normalize_matrix_progress_line(raw);

            assert!(encoded.starts_with('\u{00a0}'));
            assert!(
                encoded.starts_with("\u{00a0}   "),
                "indentation width is preserved"
            );
            assert!(encoded.contains("\\*\\*bold\\*\\*"));
            assert!(encoded.contains("\\<div data\\-x\\=\\\"1\\\"\\>\\&amp\\;\\<\\/div\\>"));
            assert!(encoded.contains('␉'));
            assert!(encoded.contains('␛'));

            let mut draft = single_draft(owned_event_id!("$single:server"));
            push_single_progress_line(&mut draft, raw, 0);
            let first_render = single_visible_text_with_budget(&draft, usize::MAX);
            let second_render = single_visible_text_with_budget(&draft, usize::MAX);
            assert_eq!(first_render, encoded);
            assert_eq!(
                second_render, encoded,
                "retained drafts must not double-escape"
            );

            let content = RoomMessageEventContent::text_markdown(&encoded);
            let serialized = serde_json::to_value(content).expect("Matrix content serializes");
            let formatted = serialized
                .get("formatted_body")
                .and_then(serde_json::Value::as_str)
                .expect("Markdown content has formatted body");
            assert!(formatted.contains("&lt;div"));
            assert!(!formatted.contains("<div data-x"));
            assert!(!formatted.contains("<strong>bold</strong>"));
            assert!(!formatted.contains("<pre><code>"));
        }

        #[test]
        fn single_edit_interval_can_skip_render_until_debounce_elapses() {
            let now = Instant::now();
            let mut draft = single_draft(owned_event_id!("$single:server"));
            draft.last_edit = now - Duration::from_millis(100);

            assert!(!single_edit_interval_elapsed(
                &draft,
                now,
                Duration::from_millis(500)
            ));

            draft.last_edit = now - Duration::from_millis(600);
            assert!(single_edit_interval_elapsed(
                &draft,
                now,
                Duration::from_millis(500)
            ));
        }

        #[test]
        fn single_render_changed_skips_duplicate_matrix_edits() {
            let mut draft = single_draft(owned_event_id!("$single:server"));
            draft.last_text = "one\ntwo".to_string();

            assert!(!single_render_changed(&draft, "one\ntwo"));
            assert!(single_render_changed(&draft, "two\nthree"));
        }

        #[test]
        fn failed_single_edit_leaves_retained_draft_flushable() {
            let mut draft = single_draft(owned_event_id!("$single:server"));
            push_single_progress_line(&mut draft, "one", 10);

            // Simulate a rendered edit body whose Matrix request fails: the
            // line buffer advanced, but the delivery checkpoint must not.
            assert_eq!(
                single_retained_draft_action(&draft, usize::MAX),
                SingleRetainedDraftAction::Flush("one".to_string())
            );

            assert!(mark_single_edit_delivered(
                &mut draft,
                &owned_event_id!("$single:server"),
                "one".to_string(),
                Instant::now(),
            ));
            assert_eq!(
                single_retained_draft_action(&draft, usize::MAX),
                SingleRetainedDraftAction::KeepCurrent
            );
        }

        #[test]
        fn single_edit_delivery_ignores_replaced_draft_event() {
            let mut draft = single_draft(owned_event_id!("$current:server"));

            assert!(!mark_single_edit_delivered(
                &mut draft,
                &owned_event_id!("$old:server"),
                "old progress".to_string(),
                Instant::now(),
            ));
            assert_eq!(draft.last_text, DRAFT_PLACEHOLDER);
        }

        #[test]
        fn single_finalize_plan_deletes_draft_before_sending_final_when_enabled() {
            assert_eq!(
                single_finalize_plan(true, true, true),
                streaming::SingleFinalizePlan::DeleteDraftThenSendFinal
            );
        }

        #[test]
        fn single_finalize_plan_keeps_draft_but_still_sends_final_when_disabled() {
            assert_eq!(
                single_finalize_plan(true, false, true),
                streaming::SingleFinalizePlan::KeepDraftThenSendFinal
            );
            assert_eq!(
                single_finalize_plan(false, true, true),
                streaming::SingleFinalizePlan::SendFinalOnly
            );
            assert_eq!(
                single_finalize_plan(false, true, false),
                streaming::SingleFinalizePlan::Noop
            );
        }

        #[test]
        fn single_retained_draft_action_flushes_latest_unflushed_progress() {
            let mut draft = single_draft(owned_event_id!("$single:server"));
            draft.last_text = "one".to_string();
            push_single_progress_line(&mut draft, "one", 10);
            push_single_progress_line(&mut draft, "two", 10);

            assert_eq!(
                single_retained_draft_action(&draft, usize::MAX),
                SingleRetainedDraftAction::Flush("one\ntwo".to_string())
            );
        }

        #[test]
        fn single_retained_draft_action_uses_the_serialized_edit_budget() {
            let mut draft = single_draft(owned_event_id!("$single:server"));
            push_single_progress_line(&mut draft, "old", 0);
            push_single_progress_line(&mut draft, &"😀".repeat(300), 0);

            let action = single_retained_draft_action(&draft, 512);
            let SingleRetainedDraftAction::Flush(visible) = action else {
                panic!("unflushed progress must produce a Matrix edit");
            };

            assert!(visible.contains("too large to fit"));
            assert!(
                outbound::serialized_edit_content_len(&visible, &draft.event_id)
                    .is_some_and(|actual| actual <= 512)
            );
        }

        #[test]
        fn single_retained_draft_action_keeps_current_visible_text() {
            let mut draft = single_draft(owned_event_id!("$single:server"));
            push_single_progress_line(&mut draft, "one", 10);
            draft.last_text = "one".to_string();

            assert_eq!(
                single_retained_draft_action(&draft, usize::MAX),
                SingleRetainedDraftAction::KeepCurrent
            );
        }

        #[test]
        fn single_retained_draft_action_deletes_placeholder_without_progress() {
            let draft = single_draft(owned_event_id!("$single:server"));

            assert_eq!(
                single_retained_draft_action(&draft, usize::MAX),
                SingleRetainedDraftAction::DeletePlaceholder
            );
        }

        #[test]
        fn single_retained_draft_action_deletes_placeholder_when_progress_cannot_fit() {
            let mut draft = single_draft(owned_event_id!("$single:server"));
            push_single_progress_line(&mut draft, "unpublished progress", 10);

            assert_eq!(
                single_retained_draft_action(&draft, 0),
                SingleRetainedDraftAction::DeletePlaceholder
            );
        }

        #[test]
        fn single_cancel_deletes_placeholder_but_retains_durable_progress() {
            let mut draft = single_draft(owned_event_id!("$single:server"));
            assert!(single_cancel_deletes_draft(&draft, false));

            push_single_progress_line(&mut draft, "one", 10);
            assert!(single_cancel_deletes_draft(&draft, false));

            draft.last_text = "one".to_string();
            assert!(!single_cancel_deletes_draft(&draft, false));
            assert!(single_cancel_deletes_draft(&draft, true));
        }

        #[tokio::test]
        async fn single_message_update_draft_ignores_answer_text() {
            let state_dir = TempDir::new().expect("temp state dir");
            let channel = MatrixChannel::new(
                MatrixConfig {
                    homeserver: "https://matrix.invalid".to_string(),
                    access_token: Some("test-token".to_string()),
                    stream_mode: MatrixStreamMode::SingleMessage,
                    ..MatrixConfig::default()
                },
                "matrix",
                Arc::new(Vec::<String>::new),
                state_dir.path().to_path_buf(),
            )
            .expect("matrix channel");
            let key =
                super::super::streaming_key("!room:server", "$draft:server").expect("draft key");

            {
                let mut state = channel.streaming_state.write().await;
                insert_single(
                    &mut state,
                    key.clone(),
                    single_draft(owned_event_id!("$draft:server")),
                )
                .expect("single-message state accepts draft");
            }

            Channel::update_draft(&channel, "!room:server", "$draft:server", "final prose")
                .await
                .expect("single_message answer deltas are ignored without Matrix I/O");

            let mut state = channel.streaming_state.write().await;
            let draft =
                streaming::single_for_update(&mut state, &key).expect("draft remains active");
            assert!(draft.lines.is_empty());
            assert_eq!(draft.last_text, DRAFT_PLACEHOLDER);
        }

        async fn assert_retained_single_draft_flush_failure(
            last_text: &str,
            expect_placeholder_cleanup: bool,
        ) {
            let server = MockServer::start().await;
            let room_id = "!room:server";
            let draft_id = owned_event_id!("$draft:server");

            Mock::given(method("GET"))
                .and(path_regex(r"^/_matrix/client/versions$"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "versions": ["r0.6.0", "v1.1", "v1.2", "v1.3", "v1.4", "v1.5"],
                    "unstable_features": {}
                })))
                .expect(1..)
                .mount(&server)
                .await;

            Mock::given(method("GET"))
                .and(path_regex(
                    r"^/_matrix/client/(v3|r0)/profile/.*/displayname$",
                ))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "displayname": "ClawCrew Test"
                })))
                .mount(&server)
                .await;

            Mock::given(method("GET"))
                .and(path_regex(
                    r"^/_matrix/client/(v3|r0)/user/.*/account_data/m\.secret_storage\.default_key$",
                ))
                .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                    "errcode": "M_NOT_FOUND",
                    "error": "not found"
                })))
                .mount(&server)
                .await;

            Mock::given(method("POST"))
                .and(path_regex(r"^/_matrix/client/(v3|r0)/keys/upload$"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "one_time_key_counts": {}
                })))
                .mount(&server)
                .await;

            Mock::given(method("POST"))
                .and(path_regex(r"^/_matrix/client/(v3|r0)/keys/query$"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "device_keys": {}
                })))
                .mount(&server)
                .await;

            Mock::given(method("GET"))
                .and(path_regex(r"^/_matrix/client/(v3|r0)/sync$"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "next_batch": "s1",
                    "rooms": {
                        "join": {
                            room_id: {
                                "state": { "events": [] },
                                "timeline": {
                                    "limited": false,
                                    "prev_batch": "t0",
                                    "events": [{
                                        "type": "m.room.message",
                                        "sender": "@bot:server",
                                        "event_id": draft_id.as_str(),
                                        "origin_server_ts": 1,
                                        "content": {
                                            "msgtype": "m.text",
                                            "body": last_text
                                        }
                                    }]
                                }
                            }
                        }
                    }
                })))
                .expect(1)
                .mount(&server)
                .await;

            Mock::given(method("GET"))
                .and(path_regex(r"^/_matrix/client/(v3|r0)/rooms/.*/event/.*$"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "type": "m.room.message",
                    "sender": "@bot:server",
                    "event_id": draft_id.as_str(),
                    "origin_server_ts": 1,
                    "room_id": room_id,
                    "content": {
                        "msgtype": "m.text",
                        "body": last_text
                    }
                })))
                .expect(1)
                .mount(&server)
                .await;

            if expect_placeholder_cleanup {
                Mock::given(method("PUT"))
                    .and(path_regex(
                        r"^/_matrix/client/(v3|r0)/rooms/.*/redact/.*/.*$",
                    ))
                    .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                        "event_id": "$redaction:server"
                    })))
                    .expect(1)
                    .mount(&server)
                    .await;
            }

            Mock::given(method("GET"))
                .and(path_regex(
                    r"^/_matrix/client/(v3|r0)/rooms/.*/state/m\.room\.encryption/?$",
                ))
                .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                    "errcode": "M_NOT_FOUND",
                    "error": "room is not encrypted"
                })))
                .mount(&server)
                .await;

            Mock::given(method("PUT"))
                .and(path_regex(
                    r"^/_matrix/client/(v3|r0)/rooms/.*/send/m\.room\.message/.*$",
                ))
                .and(body_partial_json(serde_json::json!({
                    "m.relates_to": {
                        "rel_type": "m.replace",
                        "event_id": draft_id.as_str()
                    }
                })))
                .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                    "errcode": "M_BAD_JSON",
                    "error": "edit failed"
                })))
                .expect(1)
                .mount(&server)
                .await;

            Mock::given(method("PUT"))
                .and(path_regex(
                    r"^/_matrix/client/(v3|r0)/rooms/.*/send/m\.room\.message/.*$",
                ))
                .and(body_partial_json(serde_json::json!({
                    "body": "final answer"
                })))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "event_id": "$final:server"
                })))
                .expect(1)
                .mount(&server)
                .await;

            let state_dir = TempDir::new().expect("temp state dir");
            let channel = MatrixChannel::new(
                MatrixConfig {
                    homeserver: server.uri(),
                    access_token: Some("secret-token".to_string()),
                    user_id: Some("@bot:server".to_string()),
                    device_id: Some("DEVICE".to_string()),
                    allowed_rooms: vec![room_id.to_string()],
                    stream_mode: MatrixStreamMode::SingleMessage,
                    stream_draft_delete: false,
                    reply_in_thread: false,
                    ack_reactions: Some(false),
                    ..MatrixConfig::default()
                },
                "matrix",
                Arc::new(Vec::<String>::new),
                state_dir.path().to_path_buf(),
            )
            .expect("matrix channel");

            let client = channel.ensure_client().await.expect("matrix client");
            if let Err(err) = client.sync_once(SyncSettings::default()).await {
                let paths = server
                    .received_requests()
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .map(|request| request.url.path().to_string())
                    .collect::<Vec<_>>();
                panic!("mock sync populates joined room: {err}; received paths: {paths:?}");
            }

            let key = super::super::streaming_key(room_id, draft_id.as_str()).expect("draft key");
            {
                let mut draft = single_draft(draft_id.clone());
                draft.last_text = last_text.to_string();
                push_single_progress_line(&mut draft, "new progress", 10);
                let mut state = channel.streaming_state.write().await;
                insert_single(&mut state, key, draft).expect("single-message state accepts draft");
            }

            match tokio::time::timeout(
                Duration::from_secs(5),
                channel.finalize_draft(room_id, draft_id.as_str(), "final answer", false),
            )
            .await
            {
                Ok(Ok(_)) => {}
                Ok(Err(err)) => {
                    let paths = server
                        .received_requests()
                        .await
                        .unwrap_or_default()
                        .into_iter()
                        .map(|request| request.url.path().to_string())
                        .collect::<Vec<_>>();
                    panic!(
                        "retained draft flush failure must not block final send: {err}; received paths: {paths:?}"
                    );
                }
                Err(_) => {
                    let paths = server
                        .received_requests()
                        .await
                        .unwrap_or_default()
                        .into_iter()
                        .map(|request| request.url.path().to_string())
                        .collect::<Vec<_>>();
                    panic!("retained draft finalize timed out; received paths: {paths:?}");
                }
            }

            let requests = server
                .received_requests()
                .await
                .expect("requests captured by Matrix mock");
            let redaction_index = requests
                .iter()
                .position(|request| request.url.path().contains("/redact/"));
            assert_eq!(
                redaction_index.is_some(),
                expect_placeholder_cleanup,
                "placeholder cleanup must match the Matrix delivery checkpoint"
            );
            if let Some(redaction_index) = redaction_index {
                let final_index = requests
                    .iter()
                    .position(|request| {
                        request.url.path().contains("/send/m.room.message/")
                            && String::from_utf8_lossy(&request.body).contains("final answer")
                    })
                    .expect("final answer request captured");
                assert!(
                    redaction_index < final_index,
                    "placeholder cleanup must be attempted before the final answer"
                );
            }
        }

        #[tokio::test]
        async fn retained_single_draft_flush_failure_still_sends_final() {
            assert_retained_single_draft_flush_failure("old progress", false).await;
        }

        #[tokio::test]
        async fn retained_placeholder_draft_flush_failure_redacts_before_final() {
            assert_retained_single_draft_flush_failure(DRAFT_PLACEHOLDER, true).await;
        }

        async fn assert_single_redact_failure_still_sends_budgeted_final(
            stream_draft_delete: bool,
        ) {
            let server = MockServer::start().await;
            let room_id = "!room:server";
            let draft_id = owned_event_id!("$draft:server");

            Mock::given(method("GET"))
                .and(path_regex(r"^/_matrix/client/versions$"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "versions": ["r0.6.0", "v1.1", "v1.2", "v1.3", "v1.4", "v1.5"],
                    "unstable_features": {}
                })))
                .expect(1..)
                .mount(&server)
                .await;

            Mock::given(method("GET"))
                .and(path_regex(
                    r"^/_matrix/client/(v3|r0)/profile/.*/displayname$",
                ))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "displayname": "ClawCrew Test"
                })))
                .mount(&server)
                .await;

            Mock::given(method("GET"))
                .and(path_regex(
                    r"^/_matrix/client/(v3|r0)/user/.*/account_data/m\.secret_storage\.default_key$",
                ))
                .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                    "errcode": "M_NOT_FOUND",
                    "error": "not found"
                })))
                .mount(&server)
                .await;

            Mock::given(method("POST"))
                .and(path_regex(r"^/_matrix/client/(v3|r0)/keys/upload$"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "one_time_key_counts": {}
                })))
                .mount(&server)
                .await;

            Mock::given(method("POST"))
                .and(path_regex(r"^/_matrix/client/(v3|r0)/keys/query$"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "device_keys": {}
                })))
                .mount(&server)
                .await;

            Mock::given(method("GET"))
                .and(path_regex(r"^/_matrix/client/(v3|r0)/sync$"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "next_batch": "s1",
                    "rooms": {
                        "join": {
                            room_id: {
                                "state": { "events": [] },
                                "timeline": {
                                    "limited": false,
                                    "prev_batch": "t0",
                                    "events": [{
                                        "type": "m.room.message",
                                        "sender": "@bot:server",
                                        "event_id": draft_id.as_str(),
                                        "origin_server_ts": 1,
                                        "content": {
                                            "msgtype": "m.text",
                                            "body": "old progress"
                                        }
                                    }]
                                }
                            }
                        }
                    }
                })))
                .expect(1)
                .mount(&server)
                .await;

            Mock::given(method("GET"))
                .and(path_regex(
                    r"^/_matrix/client/(v3|r0)/rooms/.*/state/m\.room\.encryption/?$",
                ))
                .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                    "errcode": "M_NOT_FOUND",
                    "error": "room is not encrypted"
                })))
                .mount(&server)
                .await;

            Mock::given(method("PUT"))
                .and(path_regex(
                    r"^/_matrix/client/(v3|r0)/rooms/.*/redact/.*/.*$",
                ))
                .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({
                    "errcode": "M_FORBIDDEN",
                    "error": "redact forbidden"
                })))
                .expect(1)
                .mount(&server)
                .await;

            Mock::given(method("PUT"))
                .and(path_regex(
                    r"^/_matrix/client/(v3|r0)/rooms/.*/send/m\.room\.message/.*$",
                ))
                .and(body_partial_json(serde_json::json!({
                    "body": "😀😀"
                })))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "event_id": "$final:server"
                })))
                .expect(2)
                .mount(&server)
                .await;

            let approval_prompt =
                "Approval required: reply approve or deny with token approval-12345";
            Mock::given(method("PUT"))
                .and(path_regex(
                    r"^/_matrix/client/(v3|r0)/rooms/.*/send/m\.room\.message/.*$",
                ))
                .and(body_partial_json(serde_json::json!({
                    "body": approval_prompt
                })))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "event_id": "$approval:server"
                })))
                .expect(1)
                .mount(&server)
                .await;

            let state_dir = TempDir::new().expect("temp state dir");
            let channel = MatrixChannel::new(
                MatrixConfig {
                    homeserver: server.uri(),
                    access_token: Some("secret-token".to_string()),
                    user_id: Some("@bot:server".to_string()),
                    device_id: Some("DEVICE".to_string()),
                    allowed_rooms: vec![room_id.to_string()],
                    stream_mode: MatrixStreamMode::SingleMessage,
                    stream_draft_delete,
                    message_max_bytes: 5,
                    reply_in_thread: false,
                    ack_reactions: Some(false),
                    ..MatrixConfig::default()
                },
                "matrix",
                Arc::new(Vec::<String>::new),
                state_dir.path().to_path_buf(),
            )
            .expect("matrix channel");

            let client = channel.ensure_client().await.expect("matrix client");
            match tokio::time::timeout(
                Duration::from_secs(5),
                client.sync_once(SyncSettings::default()),
            )
            .await
            {
                Ok(Ok(_)) => {}
                Ok(Err(err)) => {
                    let paths = server
                        .received_requests()
                        .await
                        .unwrap_or_default()
                        .into_iter()
                        .map(|request| request.url.path().to_string())
                        .collect::<Vec<_>>();
                    panic!("mock sync populates joined room: {err}; received paths: {paths:?}");
                }
                Err(_) => {
                    let paths = server
                        .received_requests()
                        .await
                        .unwrap_or_default()
                        .into_iter()
                        .map(|request| request.url.path().to_string())
                        .collect::<Vec<_>>();
                    panic!("mock sync timed out; received paths: {paths:?}");
                }
            }

            channel
                .send(&SendMessage::new(approval_prompt, room_id))
                .await
                .expect("ordinary approval-style send remains unbounded");
            channel
                .send_final(&SendMessage::new("😀😀", room_id))
                .await
                .expect("no-draft final uses the single-message budget");

            let key = super::super::streaming_key(room_id, draft_id.as_str()).expect("draft key");
            {
                let draft = single_draft(draft_id.clone());
                let mut state = channel.streaming_state.write().await;
                insert_single(&mut state, key, draft).expect("single-message state accepts draft");
            }

            match tokio::time::timeout(
                Duration::from_secs(5),
                channel.finalize_draft(room_id, draft_id.as_str(), "😀😀", false),
            )
            .await
            {
                Ok(Ok(())) => {}
                Ok(Err(err)) => {
                    let paths = server
                        .received_requests()
                        .await
                        .unwrap_or_default()
                        .into_iter()
                        .map(|request| request.url.path().to_string())
                        .collect::<Vec<_>>();
                    panic!(
                        "redaction failure must not block final send: {err}; received paths: {paths:?}"
                    );
                }
                Err(_) => {
                    let paths = server
                        .received_requests()
                        .await
                        .unwrap_or_default()
                        .into_iter()
                        .map(|request| request.url.path().to_string())
                        .collect::<Vec<_>>();
                    panic!("single-message finalize timed out; received paths: {paths:?}");
                }
            }
        }

        #[tokio::test]
        async fn single_delete_redact_failure_still_sends_utf8_budgeted_final() {
            assert_single_redact_failure_still_sends_budgeted_final(true).await;
        }

        #[tokio::test]
        async fn retained_placeholder_redact_failure_still_sends_utf8_budgeted_final() {
            assert_single_redact_failure_still_sends_budgeted_final(false).await;
        }

        #[test]
        fn marker_only_partial_finalize_redacts_placeholder_after_upload() {
            assert_eq!(
                decide_partial_finalize_action(true, true),
                PartialFinalizeAction::RedactDraft
            );
        }

        #[test]
        fn text_partial_finalize_keeps_editing_draft_after_upload() {
            assert_eq!(
                decide_partial_finalize_action(false, true),
                PartialFinalizeAction::EditDraft
            );
        }

        #[test]
        fn text_only_partial_finalize_keeps_editing_draft() {
            assert_eq!(
                decide_partial_finalize_action(false, false),
                PartialFinalizeAction::EditDraft
            );
        }

        #[test]
        fn empty_partial_finalize_without_upload_reports_empty_error() {
            assert_eq!(
                decide_partial_finalize_action(true, false),
                PartialFinalizeAction::EmptyError
            );
        }

        #[test]
        fn draft_keys_include_message_id_for_same_room_concurrency() {
            let room = owned_room_id!("!room:server");
            let first = streaming::draft_key(room.clone(), "$draft-a:server").unwrap();
            let second = streaming::draft_key(room.clone(), "$draft-b:server").unwrap();

            assert_ne!(first, second);

            let mut state = streaming::State::for_stream_mode(MatrixStreamMode::Partial);
            insert_partial(
                &mut state,
                first.clone(),
                PartialDraft {
                    event_id: owned_event_id!("$draft-a:server"),
                    thread_anchor: None,
                    last_text: "first".to_string(),
                    last_edit: Instant::now(),
                },
            )
            .expect("partial state accepts first draft");
            insert_partial(
                &mut state,
                second.clone(),
                PartialDraft {
                    event_id: owned_event_id!("$draft-b:server"),
                    thread_anchor: None,
                    last_text: "second".to_string(),
                    last_edit: Instant::now(),
                },
            )
            .expect("partial state accepts second draft");

            assert_eq!(partial_len(&state), 2);
            assert_eq!(
                streaming::take_partial(&mut state, &second).map(|draft| draft.event_id),
                Some(owned_event_id!("$draft-b:server"))
            );
            assert!(partial_contains(&state, &first));
        }

        #[test]
        fn partial_lifecycle_lookup_isolates_update_finalize_and_cancel_by_message_id() {
            let recipient = "!room:server";
            let first = super::super::streaming_key(recipient, "$draft-a:server").unwrap();
            let second = super::super::streaming_key(recipient, "$draft-b:server").unwrap();
            let canceled = super::super::streaming_key(recipient, "$draft-c:server").unwrap();

            let mut state = State::for_stream_mode(MatrixStreamMode::Partial);
            insert_partial(
                &mut state,
                first.clone(),
                partial_draft(owned_event_id!("$draft-a:server"), "first"),
            )
            .expect("partial state accepts first draft");
            insert_partial(
                &mut state,
                second.clone(),
                partial_draft(owned_event_id!("$draft-b:server"), "second"),
            )
            .expect("partial state accepts second draft");

            streaming::partial_for_update(&mut state, &second)
                .expect("second draft remains addressable")
                .last_text = "second updated".to_string();

            assert_eq!(
                streaming::partial_for_update(&mut state, &first)
                    .expect("first draft remains isolated")
                    .last_text,
                "first"
            );

            let finalized = streaming::take_partial(&mut state, &second)
                .expect("finalize removes only the addressed draft");
            assert_eq!(finalized.event_id, owned_event_id!("$draft-b:server"));
            assert!(partial_contains(&state, &first));
            assert!(!partial_contains(&state, &second));

            insert_partial(
                &mut state,
                canceled.clone(),
                partial_draft(owned_event_id!("$draft-c:server"), "cancel me"),
            )
            .expect("partial state accepts canceled draft");
            let canceled_draft = streaming::take_partial(&mut state, &canceled)
                .expect("cancel removes only the addressed draft");
            assert_eq!(canceled_draft.event_id, owned_event_id!("$draft-c:server"));
            assert!(partial_contains(&state, &first));
            assert!(!partial_contains(&state, &canceled));
        }

        #[test]
        fn single_message_lifecycle_lookup_isolates_update_finalize_and_cancel_by_message_id() {
            let recipient = "!room:server";
            let first = super::super::streaming_key(recipient, "$draft-a:server").unwrap();
            let second = super::super::streaming_key(recipient, "$draft-b:server").unwrap();
            let canceled = super::super::streaming_key(recipient, "$draft-c:server").unwrap();

            let mut state = State::for_stream_mode(MatrixStreamMode::SingleMessage);
            insert_single(
                &mut state,
                first.clone(),
                single_draft(owned_event_id!("$draft-a:server")),
            )
            .expect("single state accepts first draft");
            insert_single(
                &mut state,
                second.clone(),
                single_draft(owned_event_id!("$draft-b:server")),
            )
            .expect("single state accepts second draft");

            push_single_progress_line(
                streaming::single_for_update(&mut state, &second)
                    .expect("second draft remains addressable"),
                "second updated",
                10,
            );

            assert_eq!(
                streaming::single_for_update(&mut state, &first)
                    .expect("first draft remains isolated")
                    .lines
                    .len(),
                0
            );

            let finalized = streaming::take_single(&mut state, &second)
                .expect("finalize removes only the addressed draft");
            assert_eq!(finalized.event_id, owned_event_id!("$draft-b:server"));
            assert!(single_contains(&state, &first));
            assert!(!single_contains(&state, &second));

            insert_single(
                &mut state,
                canceled.clone(),
                single_draft(owned_event_id!("$draft-c:server")),
            )
            .expect("single state accepts canceled draft");
            let canceled_draft = streaming::take_single(&mut state, &canceled)
                .expect("cancel removes only the addressed draft");
            assert_eq!(canceled_draft.event_id, owned_event_id!("$draft-c:server"));
            assert!(single_contains(&state, &first));
            assert!(!single_contains(&state, &canceled));
        }

        #[test]
        fn multi_message_lifecycle_lookup_isolates_update_finalize_and_cancel_by_message_id() {
            let recipient = "!room:server";
            let first =
                super::super::streaming_key(recipient, "multi_message_synthetic:first").unwrap();
            let second =
                super::super::streaming_key(recipient, "multi_message_synthetic:second").unwrap();
            let canceled =
                super::super::streaming_key(recipient, "multi_message_synthetic:cancel").unwrap();

            let mut state = State::for_stream_mode(MatrixStreamMode::MultiMessage);
            insert_multi(
                &mut state,
                first.clone(),
                MultiDraft {
                    thread_anchor: None,
                    sent_so_far: 5,
                },
            )
            .expect("multi state accepts first draft");
            insert_multi(
                &mut state,
                second.clone(),
                MultiDraft {
                    thread_anchor: None,
                    sent_so_far: 0,
                },
            )
            .expect("multi state accepts second draft");

            streaming::multi_for_update(&mut state, &second)
                .expect("second multi-message draft remains addressable")
                .sent_so_far = 12;

            assert_eq!(
                streaming::multi_for_update(&mut state, &first)
                    .expect("first multi-message draft remains isolated")
                    .sent_so_far,
                5
            );

            let finalized = streaming::take_multi(&mut state, &second)
                .expect("finalize removes only the addressed multi-message draft");
            assert_eq!(finalized.sent_so_far, 12);
            assert!(multi_contains(&state, &first));
            assert!(!multi_contains(&state, &second));

            insert_multi(
                &mut state,
                canceled.clone(),
                MultiDraft {
                    thread_anchor: None,
                    sent_so_far: 3,
                },
            )
            .expect("multi state accepts canceled draft");
            let canceled_draft = streaming::take_multi(&mut state, &canceled)
                .expect("cancel removes only the addressed multi-message draft");
            assert_eq!(canceled_draft.sent_so_far, 3);
            assert!(multi_contains(&state, &first));
            assert!(!multi_contains(&state, &canceled));
        }

        #[test]
        fn multi_message_synthetic_draft_ids_are_unique() {
            let first = streaming::new_multi_message_draft_id();
            let second = streaming::new_multi_message_draft_id();

            assert_ne!(first, second);
            assert!(first.starts_with("multi_message_synthetic:"));
            assert!(second.starts_with("multi_message_synthetic:"));
        }
    }

    mod live_smoke {
        use std::{
            env,
            sync::Arc,
            time::{Duration, Instant, SystemTime, UNIX_EPOCH},
        };

        use matrix_sdk::config::SyncSettings;
        use tempfile::TempDir;
        use clawcrew_api::channel::{Channel, SendMessage};
        use clawcrew_config::schema::{MatrixConfig, MatrixStreamMode};

        use super::super::{
            MatrixChannel, inbound::SYNC_LONGPOLL_TIMEOUT, streaming, streaming_key,
        };

        fn env_first(primary: &str, fallback: &str) -> String {
            env::var(primary)
                .or_else(|_| env::var(fallback))
                .unwrap_or_else(|_| panic!("set {primary} or {fallback} to run Matrix live smoke"))
        }

        #[tokio::test]
        #[ignore = "requires Matrix smoke credentials and a disposable test room"]
        async fn same_room_partial_draft_lifecycle_uses_real_draft_ids() {
            let homeserver = env_first(
                "CLAWCREW_MATRIX_SMOKE_HOMESERVER",
                "CLAWCREW_MATRIX_HOMESERVER",
            );
            let room_id = env_first("CLAWCREW_MATRIX_SMOKE_ROOM_ID", "CLAWCREW_MATRIX_ROOM_ID");
            let access_token = env_first(
                "CLAWCREW_MATRIX_SMOKE_ACCESS_TOKEN",
                "CLAWCREW_MATRIX_ACCESS_TOKEN",
            );
            let device_id = env_first(
                "CLAWCREW_MATRIX_SMOKE_DEVICE_ID",
                "CLAWCREW_MATRIX_DEVICE_ID",
            );
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time before unix epoch")
                .as_secs();

            let config = MatrixConfig {
                enabled: true,
                homeserver,
                access_token: Some(access_token),
                device_id: Some(device_id),
                allowed_rooms: vec![room_id.clone()],
                stream_mode: MatrixStreamMode::Partial,
                draft_update_interval_ms: 50,
                multi_message_delay_ms: 0,
                stream_draft_lines: 10,
                message_max_bytes: 48_000,
                stream_draft_delete: true,
                reply_in_thread: false,
                ack_reactions: Some(false),
                approval_timeout_secs: 1,
                ..MatrixConfig::default()
            };
            let state_dir = TempDir::new().expect("temp state dir");
            let channel = MatrixChannel::new(
                config,
                "matrix",
                Arc::new(Vec::<String>::new),
                state_dir.path().to_path_buf(),
            )
            .expect("matrix channel");

            let client = channel.ensure_client().await.expect("matrix client");
            client
                .sync_once(SyncSettings::default().timeout(SYNC_LONGPOLL_TIMEOUT))
                .await
                .expect("initial Matrix sync");

            let first = channel
                .send_draft(&SendMessage::new(
                    format!("clawcrew draft lifecycle smoke {stamp} first"),
                    &room_id,
                ))
                .await
                .expect("send first draft")
                .expect("partial mode returns first draft event id");
            let second = channel
                .send_draft(&SendMessage::new(
                    format!("clawcrew draft lifecycle smoke {stamp} second"),
                    &room_id,
                ))
                .await
                .expect("send second draft")
                .expect("partial mode returns second draft event id");
            assert_ne!(first, second);

            let first_key = streaming_key(&room_id, &first).expect("first draft key");
            let second_key = streaming_key(&room_id, &second).expect("second draft key");
            {
                let state = channel.streaming_state.read().await;
                assert!(streaming::partial_contains(&state, &first_key));
                assert!(streaming::partial_contains(&state, &second_key));
            }

            tokio::time::sleep(Duration::from_millis(60)).await;
            let first_update = format!("clawcrew draft lifecycle smoke {stamp} first update");
            channel
                .update_draft(&room_id, &first, &first_update)
                .await
                .expect("update first draft by id");
            {
                let mut state = channel.streaming_state.write().await;
                assert_eq!(
                    streaming::partial_for_update(&mut state, &first_key)
                        .map(|draft| draft.last_text.as_str()),
                    Some(first_update.as_str())
                );
                assert!(streaming::partial_contains(&state, &second_key));
            }

            channel
                .finalize_draft(
                    &room_id,
                    &second,
                    &format!("clawcrew draft lifecycle smoke {stamp} second final"),
                    false,
                )
                .await
                .expect("finalize second draft by id");
            {
                let state = channel.streaming_state.read().await;
                assert!(streaming::partial_contains(&state, &first_key));
                assert!(!streaming::partial_contains(&state, &second_key));
            }

            channel
                .cancel_draft(&room_id, &first)
                .await
                .expect("cancel first draft by id");
            {
                let state = channel.streaming_state.read().await;
                assert_eq!(streaming::partial_len(&state), 0);
            }
        }

        #[tokio::test]
        #[ignore = "requires Matrix smoke credentials and a disposable test room"]
        async fn same_room_single_message_draft_edits_one_real_event() {
            let homeserver = env_first(
                "CLAWCREW_MATRIX_SMOKE_HOMESERVER",
                "CLAWCREW_MATRIX_HOMESERVER",
            );
            let room_id = env_first("CLAWCREW_MATRIX_SMOKE_ROOM_ID", "CLAWCREW_MATRIX_ROOM_ID");
            let access_token = env_first(
                "CLAWCREW_MATRIX_SMOKE_ACCESS_TOKEN",
                "CLAWCREW_MATRIX_ACCESS_TOKEN",
            );
            let device_id = env_first(
                "CLAWCREW_MATRIX_SMOKE_DEVICE_ID",
                "CLAWCREW_MATRIX_DEVICE_ID",
            );
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time before unix epoch")
                .as_secs();

            let config = MatrixConfig {
                enabled: true,
                homeserver,
                access_token: Some(access_token),
                device_id: Some(device_id),
                allowed_rooms: vec![room_id.clone()],
                stream_mode: MatrixStreamMode::SingleMessage,
                draft_update_interval_ms: 50,
                stream_draft_lines: 5,
                message_max_bytes: 512,
                stream_draft_delete: true,
                reply_in_thread: false,
                ack_reactions: Some(false),
                approval_timeout_secs: 1,
                ..MatrixConfig::default()
            };
            let state_dir = TempDir::new().expect("temp state dir");
            let channel = MatrixChannel::new(
                config,
                "matrix",
                Arc::new(Vec::<String>::new),
                state_dir.path().to_path_buf(),
            )
            .expect("matrix channel");

            let client = channel.ensure_client().await.expect("matrix client");
            client
                .sync_once(SyncSettings::default().timeout(SYNC_LONGPOLL_TIMEOUT))
                .await
                .expect("initial Matrix sync");

            let draft = channel
                .send_draft(&SendMessage::new(
                    format!("clawcrew single-message smoke {stamp} draft"),
                    &room_id,
                ))
                .await
                .expect("send single-message draft")
                .expect("single-message mode returns a real draft event id");
            let key = streaming_key(&room_id, &draft).expect("draft key");

            tokio::time::sleep(Duration::from_millis(60)).await;
            channel
                .update_draft_progress(
                    &room_id,
                    &draft,
                    &format!("💭 clawcrew single-message smoke {stamp} **literal** <tag>"),
                )
                .await
                .expect("edit the same single-message draft event");
            {
                let state = channel.streaming_state.read().await;
                let active = streaming::single_contains(&state, &key);
                assert!(active, "the edited draft remains one active event");
            }

            channel
                .finalize_draft(
                    &room_id,
                    &draft,
                    &format!("clawcrew single-message smoke {stamp} final"),
                    false,
                )
                .await
                .expect("finalize after editing the single-message draft");
            let state = channel.streaming_state.read().await;
            assert!(
                !streaming::single_contains(&state, &key),
                "finalization removes the local single-message draft state"
            );
        }

        #[tokio::test]
        #[ignore = "requires Matrix smoke credentials and a disposable idle test room"]
        async fn idle_sync_does_not_error_at_30s_cadence() {
            let homeserver = env_first(
                "CLAWCREW_MATRIX_SMOKE_HOMESERVER",
                "CLAWCREW_MATRIX_HOMESERVER",
            );
            let room_id = env_first("CLAWCREW_MATRIX_SMOKE_ROOM_ID", "CLAWCREW_MATRIX_ROOM_ID");
            let access_token = env_first(
                "CLAWCREW_MATRIX_SMOKE_ACCESS_TOKEN",
                "CLAWCREW_MATRIX_ACCESS_TOKEN",
            );
            let device_id = env_first(
                "CLAWCREW_MATRIX_SMOKE_DEVICE_ID",
                "CLAWCREW_MATRIX_DEVICE_ID",
            );

            let idle_secs: u64 = env::var("CLAWCREW_MATRIX_SMOKE_IDLE_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(35);
            assert!(
                idle_secs > 30,
                "idle soak must exceed 30s to exercise the pre-fix failure window; got {idle_secs}s"
            );
            let min_longpoll_ms: u64 = env::var("CLAWCREW_MATRIX_SMOKE_MIN_LONGPOLL_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1_000);

            let config = MatrixConfig {
                enabled: true,
                homeserver,
                access_token: Some(access_token),
                device_id: Some(device_id),
                allowed_rooms: vec![room_id.clone()],
                stream_mode: MatrixStreamMode::Off,
                reply_in_thread: false,
                ack_reactions: Some(false),
                ..MatrixConfig::default()
            };
            let state_dir = TempDir::new().expect("temp state dir");
            let channel = MatrixChannel::new(
                config,
                "matrix",
                Arc::new(Vec::<String>::new),
                state_dir.path().to_path_buf(),
            )
            .expect("matrix channel");

            // Building the client exercises `CLIENT_REQUEST_TIMEOUT` on the
            // underlying `RequestConfig`. If that constant ever regresses below
            // `SYNC_LONGPOLL_TIMEOUT`, the very first long-poll below will
            // error out at the HTTP deadline.
            let client = channel.ensure_client().await.expect("matrix client");

            // Prime the sync token with a single bounded sync_once so the
            // subsequent loop measures true idle long-poll behavior rather
            // than the initial state-fetch round-trip.
            client
                .sync_once(SyncSettings::default().timeout(SYNC_LONGPOLL_TIMEOUT))
                .await
                .expect("initial Matrix sync");

            let soak = Duration::from_secs(idle_secs);
            let min_longpoll = Duration::from_millis(min_longpoll_ms);
            let deadline = Instant::now() + soak;
            let mut call_count: u32 = 0;
            let mut short_longpoll_count: u32 = 0;
            let mut max_call: Duration = Duration::from_millis(0);

            while Instant::now() < deadline {
                let started = Instant::now();
                let result = client
                    .sync_once(SyncSettings::default().timeout(SYNC_LONGPOLL_TIMEOUT))
                    .await;
                let elapsed = started.elapsed();

                // Primary reviewer assertion: idle `/sync` must not error out.
                // The pre-fix bug surfaced as a request-deadline error at ~30s
                // when the HTTP timeout fired before the long-poll returned.
                result.unwrap_or_else(|e| {
                    panic!(
                        "idle sync_once errored after {elapsed:?} (call #{call_count}); this is the 30s-cadence regression \
                         the PR aims to fix: {e}"
                    )
                });

                call_count += 1;
                if elapsed > max_call {
                    max_call = elapsed;
                }
                if elapsed < min_longpoll {
                    short_longpoll_count += 1;
                }
            }

            // Defense-in-depth against the other half of the pre-fix bug: a
            // missing `?timeout=` made the homeserver reply instantly, so the
            // SDK would busy-poll. With `SYNC_LONGPOLL_TIMEOUT` set, an idle
            // room should produce only a handful of round-trips per 30s.
            assert!(
                call_count > 0,
                "expected at least one sync_once call during the {idle_secs}s soak"
            );
            assert!(
                max_call >= min_longpoll,
                "every sync_once call returned in <{min_longpoll:?} (max observed: {max_call:?}); \
                 homeserver appears to be replying without honoring `?timeout=` — likely the pre-fix \
                 busy-poll regression. call_count={call_count}"
            );
            // Allow a couple of legitimate early returns (e.g. presence pings)
            // but flag anything that smells like a tight busy-poll loop.
            let busy_poll_budget = ((idle_secs / 5).max(2)) as u32;
            assert!(
                short_longpoll_count <= busy_poll_budget,
                "{short_longpoll_count} of {call_count} sync_once calls returned in <{min_longpoll:?} \
                 (budget for an idle room over {idle_secs}s is {busy_poll_budget}); this matches the \
                 pre-fix busy-poll pattern"
            );

            // Mirror the validation-evidence shape requested on the PR: emit a
            // concise note so a captured `cargo test -- --nocapture` run reads
            // like the reviewer's "short Matrix smoke result" ask.
            eprintln!(
                "matrix idle-sync smoke: soak={idle_secs}s, sync_once_calls={call_count}, \
                 max_call={max_call:?}, short_calls={short_longpoll_count}, no errors at 30s cadence"
            );
        }
    }

    mod session {
        use super::super::session::{SessionBlob, load, save};
        use tempfile::TempDir;

        #[test]
        fn round_trip() {
            let dir = TempDir::new().unwrap();
            let blob = SessionBlob {
                user_id: "@bot:example.org".to_string(),
                device_id: "DEV1".to_string(),
                access_token: "secret".to_string(),
                refresh_token: Some("refresh".to_string()),
            };
            save(dir.path(), &blob).unwrap();
            let loaded = load(dir.path()).unwrap().unwrap();
            assert_eq!(blob, loaded);
        }

        #[test]
        fn missing_returns_none() {
            let dir = TempDir::new().unwrap();
            assert!(load(dir.path()).unwrap().is_none());
        }

        #[test]
        fn corrupt_returns_none() {
            let dir = TempDir::new().unwrap();
            let p = dir.path().join("session.json");
            std::fs::write(p, "{not valid json").unwrap();
            assert!(load(dir.path()).unwrap().is_none());
        }

        #[cfg(unix)]
        #[test]
        fn save_creates_owner_only_perms() {
            // session.json holds the access token in plaintext. On Unix
            // it must be 0o600 regardless of umask so other local users
            // can't read it.
            use std::os::unix::fs::PermissionsExt;
            let dir = TempDir::new().unwrap();
            let blob = SessionBlob {
                user_id: "@bot:example.org".to_string(),
                device_id: "DEV1".to_string(),
                access_token: "secret".to_string(),
                refresh_token: None,
            };
            save(dir.path(), &blob).unwrap();
            let meta = std::fs::metadata(dir.path().join("session.json")).unwrap();
            let mode = meta.permissions().mode() & 0o777;
            assert_eq!(
                mode, 0o600,
                "expected 0o600, got {mode:o}; session.json must be owner-only"
            );
        }
    }

    mod auth_gating {
        //! Pure-logic tests for the auth-flow gating helpers — keeps
        //! corruption-recovery decisions verifiable without touching the SDK.

        use super::super::client::{
            can_password_relogin, resolve_access_token_identity, saved_session_is_foreign,
            store_has_orphan_data,
        };
        use tempfile::TempDir;
        use wiremock::{
            Mock, MockServer, ResponseTemplate,
            matchers::{header, method, path},
        };
        use clawcrew_config::schema::MatrixConfig;

        const WHOAMI_PATH: &str = "/_matrix/client/v3/account/whoami";

        fn cfg(password: Option<&str>, user_id: Option<&str>) -> MatrixConfig {
            MatrixConfig {
                enabled: true,
                homeserver: "https://m.org".into(),
                access_token: None,
                user_id: user_id.map(String::from),
                device_id: None,
                allowed_rooms: vec![],
                interrupt_on_new_message: false,
                stream_mode: Default::default(),
                stream_tool_arguments: vec![],
                draft_update_interval_ms: 1500,
                multi_message_delay_ms: 800,
                stream_draft_lines: 10,
                message_max_bytes: 48_000,
                stream_draft_delete: true,
                stream_reasoning: clawcrew_config::schema::StreamReasoningMode::Status,
                mention_only: false,
                recovery_key: None,
                password: password.map(String::from),
                approval_timeout_secs: 300,
                reply_in_thread: true,
                ack_reactions: Some(true),
                excluded_tools: vec![],
                reply_min_interval_secs: 0,
                reply_queue_depth_max: 0,
            }
        }

        fn access_token_cfg(homeserver: String) -> MatrixConfig {
            MatrixConfig {
                homeserver,
                access_token: Some("secret-token".into()),
                ..cfg(None, None)
            }
        }

        fn resolved_homeserver(url: &str) -> reqwest::Url {
            reqwest::Url::parse(url).unwrap()
        }

        #[test]
        fn relogin_requires_both_password_and_user_id() {
            assert!(can_password_relogin(&cfg(Some("pw"), Some("@bot:m"))));
            assert!(!can_password_relogin(&cfg(None, Some("@bot:m"))));
            assert!(!can_password_relogin(&cfg(Some("pw"), None)));
            assert!(!can_password_relogin(&cfg(None, None)));
        }

        #[test]
        fn relogin_rejects_empty_strings() {
            assert!(!can_password_relogin(&cfg(Some(""), Some("@bot:m"))));
            assert!(!can_password_relogin(&cfg(Some("pw"), Some(""))));
        }

        fn blob_for(user_id: &str) -> super::super::session::SessionBlob {
            super::super::session::SessionBlob {
                user_id: user_id.to_string(),
                device_id: "DEV1".to_string(),
                access_token: "secret".to_string(),
                refresh_token: None,
            }
        }

        #[test]
        fn foreign_session_detected_when_user_ids_differ() {
            let cfg = cfg(Some("pw"), Some("@clamps-bot:matrix.org"));
            let foreign = blob_for("@bender-bending-rodriguez-clawcrew:matrix.org");
            assert!(saved_session_is_foreign(&cfg, &foreign));
        }

        #[test]
        fn matching_session_not_foreign() {
            let cfg = cfg(Some("pw"), Some("@clamps-bot:matrix.org"));
            let own = blob_for("@clamps-bot:matrix.org");
            assert!(!saved_session_is_foreign(&cfg, &own));
        }

        #[test]
        fn unset_or_bare_user_id_never_flags() {
            // No configured user_id, or a bare localpart that cannot be
            // compared against the canonical MXID, must not false-positive.
            let any = blob_for("@whoever:matrix.org");
            assert!(!saved_session_is_foreign(&cfg(Some("pw"), None), &any));
            assert!(!saved_session_is_foreign(&cfg(Some("pw"), Some("")), &any));
            assert!(!saved_session_is_foreign(
                &cfg(Some("pw"), Some("clamps-bot")),
                &any
            ));
        }

        #[test]
        fn orphan_detection_no_state_dir() {
            let dir = TempDir::new().unwrap();
            // store/ does not exist
            assert!(!store_has_orphan_data(dir.path()));
        }

        #[test]
        fn orphan_detection_empty_store() {
            let dir = TempDir::new().unwrap();
            std::fs::create_dir_all(dir.path().join("store")).unwrap();
            assert!(!store_has_orphan_data(dir.path()));
        }

        #[test]
        fn orphan_detection_populated_store() {
            let dir = TempDir::new().unwrap();
            let store = dir.path().join("store");
            std::fs::create_dir_all(&store).unwrap();
            std::fs::write(store.join("matrix-sdk-crypto.sqlite3"), b"x").unwrap();
            assert!(store_has_orphan_data(dir.path()));
        }

        #[tokio::test]
        async fn access_token_identity_fetches_missing_user_and_device_from_whoami() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path(WHOAMI_PATH))
                .and(header("authorization", "Bearer secret-token"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "user_id": "@bot:example.org",
                    "device_id": "DEVICE42"
                })))
                .mount(&server)
                .await;

            let identity = resolve_access_token_identity(
                &access_token_cfg(server.uri()),
                &resolved_homeserver(&server.uri()),
            )
            .await
            .unwrap();

            assert_eq!(identity.user_id, "@bot:example.org");
            assert_eq!(identity.device_id.as_deref(), Some("DEVICE42"));
        }

        #[tokio::test]
        async fn access_token_whoami_uses_discovered_delegated_homeserver() {
            let root = MockServer::start().await;
            let delegated = MockServer::start().await;

            Mock::given(method("GET"))
                .and(path("/.well-known/matrix/client"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "m.homeserver": { "base_url": delegated.uri() }
                })))
                .expect(1)
                .mount(&root)
                .await;
            Mock::given(method("GET"))
                .and(path("/_matrix/client/versions"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "versions": ["v1.1"],
                    "unstable_features": {}
                })))
                .mount(&delegated)
                .await;
            Mock::given(method("GET"))
                .and(path(WHOAMI_PATH))
                .and(header("authorization", "Bearer secret-token"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "user_id": "@bot:example.org",
                    "device_id": "DEVICE42"
                })))
                .expect(1)
                .mount(&delegated)
                .await;

            let server_name =
                matrix_sdk::ruma::ServerName::parse(root.address().to_string()).unwrap();
            let client = matrix_sdk::Client::builder()
                .insecure_server_name_no_tls(&server_name)
                .build()
                .await
                .unwrap();
            assert_eq!(
                client.homeserver().as_str().trim_end_matches('/'),
                delegated.uri()
            );

            let identity = resolve_access_token_identity(
                &access_token_cfg(server_name.to_string()),
                &client.homeserver(),
            )
            .await
            .unwrap();

            assert_eq!(identity.user_id, "@bot:example.org");
            assert_eq!(identity.device_id.as_deref(), Some("DEVICE42"));
            assert!(
                root.received_requests()
                    .await
                    .unwrap()
                    .iter()
                    .all(|request| request.url.path() != WHOAMI_PATH)
            );
        }

        #[tokio::test]
        async fn access_token_whoami_preserves_direct_homeserver_url_and_base_path() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/matrix/_matrix/client/versions"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "versions": ["v1.1"],
                    "unstable_features": {}
                })))
                .expect(1)
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path(format!("/matrix{WHOAMI_PATH}")))
                .and(header("authorization", "Bearer secret-token"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "user_id": "@bot:example.org",
                    "device_id": "DEVICE42"
                })))
                .expect(1)
                .mount(&server)
                .await;

            let homeserver = format!("{}/matrix", server.uri());
            let client = matrix_sdk::Client::builder()
                .server_name_or_homeserver_url(&homeserver)
                .build()
                .await
                .unwrap();
            let identity =
                resolve_access_token_identity(&access_token_cfg(homeserver), &client.homeserver())
                    .await
                    .unwrap();

            assert_eq!(identity.user_id, "@bot:example.org");
            assert_eq!(identity.device_id.as_deref(), Some("DEVICE42"));
        }

        #[tokio::test]
        async fn access_token_identity_rejects_whoami_without_device_when_not_configured() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path(WHOAMI_PATH))
                .and(header("authorization", "Bearer secret-token"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "user_id": "@bot:example.org"
                })))
                .mount(&server)
                .await;

            let err = resolve_access_token_identity(
                &access_token_cfg(server.uri()),
                &resolved_homeserver(&server.uri()),
            )
            .await
            .unwrap_err();

            assert!(
                err.to_string()
                    .contains("whoami response did not include device_id"),
                "{err}"
            );
        }

        #[tokio::test]
        async fn access_token_identity_uses_complete_config_without_whoami() {
            let mut config = access_token_cfg("http://127.0.0.1:9".into());
            config.user_id = Some(" @bot:example.org ".into());
            config.device_id = Some(" DEVICE42 ".into());

            let identity =
                resolve_access_token_identity(&config, &resolved_homeserver("http://127.0.0.1:9"))
                    .await
                    .unwrap();

            assert_eq!(identity.user_id, "@bot:example.org");
            assert_eq!(identity.device_id.as_deref(), Some("DEVICE42"));
        }

        #[tokio::test]
        async fn access_token_identity_rejects_configured_user_mismatch() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path(WHOAMI_PATH))
                .and(header("authorization", "Bearer secret-token"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "user_id": "@actual:example.org",
                    "device_id": "DEVICE42"
                })))
                .mount(&server)
                .await;
            let mut config = access_token_cfg(server.uri());
            config.user_id = Some("@configured:example.org".into());

            let err = resolve_access_token_identity(&config, &resolved_homeserver(&server.uri()))
                .await
                .unwrap_err();

            assert!(
                err.to_string()
                    .contains("does not match Matrix whoami user_id"),
                "{err}"
            );
        }

        #[tokio::test]
        async fn access_token_identity_reports_matrix_error_envelope_without_raw_body() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path(WHOAMI_PATH))
                .and(header("authorization", "Bearer secret-token"))
                .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({
                    "errcode": "M_FORBIDDEN",
                    "error": "token rejected",
                    "access_token": "secret-token"
                })))
                .mount(&server)
                .await;

            let err = resolve_access_token_identity(
                &access_token_cfg(server.uri()),
                &resolved_homeserver(&server.uri()),
            )
            .await
            .unwrap_err();
            let message = err.to_string();

            assert!(message.contains("M_FORBIDDEN: token rejected"), "{message}");
            assert!(!message.contains("access_token"), "{message}");
            assert!(!message.contains("secret-token"), "{message}");
        }

        #[tokio::test]
        async fn access_token_identity_rejects_configured_device_mismatch() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path(WHOAMI_PATH))
                .and(header("authorization", "Bearer secret-token"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "user_id": "@bot:example.org",
                    "device_id": "ACTUAL_DEVICE"
                })))
                .mount(&server)
                .await;
            let mut config = access_token_cfg(server.uri());
            config.device_id = Some("CONFIGURED_DEVICE".into());

            let err = resolve_access_token_identity(&config, &resolved_homeserver(&server.uri()))
                .await
                .unwrap_err();

            assert!(
                err.to_string()
                    .contains("does not match Matrix whoami device_id"),
                "{err}"
            );
        }
    }

    mod voice {
        use super::super::inbound::is_voice_message;
        use matrix_sdk::event_handler::RawEvent;
        use matrix_sdk::ruma::serde::Raw;

        fn raw(json: serde_json::Value) -> RawEvent {
            let raw: Raw<serde_json::Value> = Raw::new(&json).expect("raw");
            RawEvent(raw.into_json())
        }

        #[test]
        fn audio_with_voice_flag_detected() {
            let r = raw(serde_json::json!({
                "content": {
                    "msgtype": "m.audio",
                    "body": "voice.ogg",
                    "org.matrix.msc3245.voice": {},
                }
            }));
            assert!(is_voice_message(&r));
        }

        #[test]
        fn plain_audio_not_voice() {
            let r = raw(serde_json::json!({
                "content": {
                    "msgtype": "m.audio",
                    "body": "song.mp3",
                }
            }));
            assert!(!is_voice_message(&r));
        }
    }

    mod thread_extraction {
        use super::super::inbound::{
            extract_mentions_user_ids, extract_thread_id, interruption_scope_from_anchor,
            resolve_outbound_anchor,
        };
        use matrix_sdk::event_handler::RawEvent;
        use matrix_sdk::ruma::serde::Raw;

        fn raw(json: serde_json::Value) -> RawEvent {
            let raw: Raw<serde_json::Value> = Raw::new(&json).expect("raw");
            RawEvent(raw.into_json())
        }

        #[test]
        fn thread_relation_pulls_root_id() {
            let r = raw(serde_json::json!({
                "content": {
                    "msgtype": "m.text",
                    "body": "reply",
                    "m.relates_to": {
                        "rel_type": "m.thread",
                        "event_id": "$root:server",
                    }
                }
            }));
            let id = extract_thread_id(&r).expect("some");
            assert_eq!(id.as_str(), "$root:server");
        }

        #[test]
        fn no_relation_returns_none() {
            let r = raw(serde_json::json!({
                "content": { "msgtype": "m.text", "body": "hi" }
            }));
            assert!(extract_thread_id(&r).is_none());
        }

        #[test]
        fn non_thread_relation_returns_none() {
            let r = raw(serde_json::json!({
                "content": {
                    "msgtype": "m.text",
                    "body": "hi",
                    "m.relates_to": { "rel_type": "m.replace", "event_id": "$x:s" }
                }
            }));
            assert!(extract_thread_id(&r).is_none());
        }

        #[test]
        fn root_inbound_starts_new_thread_when_reply_in_thread_enabled() {
            let event_id = "$root:server".parse().expect("event id");
            assert_eq!(
                resolve_outbound_anchor(None, &event_id, true).as_deref(),
                Some("$root:server")
            );
        }

        #[test]
        fn root_inbound_stays_root_when_reply_in_thread_disabled() {
            let event_id = "$root:server".parse().expect("event id");
            assert_eq!(resolve_outbound_anchor(None, &event_id, false), None);
        }

        #[test]
        fn threaded_inbound_keeps_existing_thread_root() {
            let event_id = "$reply:server".parse().expect("event id");
            let thread_root = "$root:server".parse().expect("thread id");
            assert_eq!(
                resolve_outbound_anchor(Some(&thread_root), &event_id, true).as_deref(),
                Some("$root:server")
            );
            assert_eq!(
                resolve_outbound_anchor(Some(&thread_root), &event_id, false).as_deref(),
                Some("$root:server")
            );
        }

        // ── interruption_scope_from_anchor ──────────────────────────

        #[test]
        fn self_anchored_root_strips_interruption_scope() {
            // when reply_in_thread anchors on the inbound event
            // itself the anchor is a delivery detail, not a conversation
            // boundary — interruption_scope_id should be None so
            // cancellation keys match sender+room.
            let event_id = "$ev:server".parse().expect("event id");
            let outbound = resolve_outbound_anchor(None, &event_id, true);
            // thread_ts stays set to the event_id
            assert_eq!(outbound.as_deref(), Some("$ev:server"));
            // interruption_scope_id is stripped
            assert_eq!(
                interruption_scope_from_anchor(outbound.as_deref(), &event_id),
                None
            );
        }

        #[test]
        fn real_thread_reply_preserves_interruption_scope() {
            // A reply inside an existing thread: outbound anchor is the
            // thread root, not the inbound event itself.
            // interruption_scope_id must stay set to the thread root.
            let event_id = "$reply:server".parse().expect("event id");
            let thread_root = "$root:server".parse().expect("thread root");
            let outbound = resolve_outbound_anchor(Some(&thread_root), &event_id, true);
            assert_eq!(outbound.as_deref(), Some("$root:server"));
            assert_eq!(
                interruption_scope_from_anchor(outbound.as_deref(), &event_id).as_deref(),
                Some("$root:server")
            );
        }

        #[test]
        fn no_anchor_yields_no_interruption_scope() {
            // reply_in_thread disabled on a root event: no anchor at all.
            let event_id = "$ev:server".parse().expect("event id");
            let outbound = resolve_outbound_anchor(None, &event_id, false);
            assert_eq!(outbound, None);
            assert_eq!(
                interruption_scope_from_anchor(outbound.as_deref(), &event_id),
                None
            );
        }

        #[test]
        fn mentions_user_ids_extracted() {
            let r = raw(serde_json::json!({
                "content": {
                    "msgtype": "m.text",
                    "body": "hi",
                    "m.mentions": { "user_ids": ["@a:b", "@c:d"] }
                }
            }));
            let ids = extract_mentions_user_ids(&r).expect("some");
            assert_eq!(ids, vec!["@a:b", "@c:d"]);
        }

        #[test]
        fn no_mentions_field_returns_none() {
            let r = raw(serde_json::json!({
                "content": { "msgtype": "m.text", "body": "hi" }
            }));
            assert!(extract_mentions_user_ids(&r).is_none());
        }
    }

    mod multi_streaming {
        //! `next_paragraph_break` is the heart of MultiMessage streaming —
        //! getting the code-fence detection wrong means agent code blocks
        //! get split mid-block. These cover the corner cases.

        use super::super::streaming::next_paragraph_break;

        #[test]
        fn no_break_returns_none() {
            assert_eq!(next_paragraph_break("hello world"), None);
        }

        #[test]
        fn single_break_at_offset() {
            assert_eq!(next_paragraph_break("first\n\nsecond"), Some(5));
        }

        #[test]
        fn first_break_when_multiple_present() {
            // Caller is expected to consume +2 past the break, so reporting
            // the *first* break is the correct contract — the loop emits one
            // paragraph per iteration.
            assert_eq!(next_paragraph_break("a\n\nb\n\nc"), Some(1));
        }

        #[test]
        fn break_inside_code_fence_ignored() {
            // The `\n\n` after "let x = 1;" is inside ```rust ... ``` and
            // must not be treated as a paragraph boundary.
            let text = "before\n\n```rust\nlet x = 1;\n\nlet y = 2;\n```\n\nafter";
            let break_at = next_paragraph_break(text).expect("first break");
            // First real break is the one between "before" and the fence.
            assert_eq!(&text[..break_at], "before");
        }

        #[test]
        fn break_after_closed_fence_detected() {
            // Once the fence closes, subsequent `\n\n` should be detected.
            let text = "```\ncode\n```\n\nafter";
            assert_eq!(next_paragraph_break(text), Some(12));
        }

        #[test]
        fn fence_must_be_at_line_start() {
            // ``` mid-line is not a fence open — paragraph break still applies.
            let text = "inline ``` not a fence\n\nafter";
            assert!(next_paragraph_break(text).is_some());
        }

        #[test]
        fn unicode_safe() {
            // Byte offset must be on a char boundary so the caller's
            // `text[..break_at]` slice doesn't panic.
            let text = "héllo\n\nwörld";
            let break_at = next_paragraph_break(text).expect("break");
            assert!(text.is_char_boundary(break_at));
            assert_eq!(&text[..break_at], "héllo");
        }
    }

    mod in_reply_to {
        //! Coverage for the mention-only "@bot can you see this image?"
        //! flow: the inbound text event has no media of its own but its
        //! `m.relates_to.m.in_reply_to.event_id` points at an earlier
        //! media-only event the bot ignored.

        use super::super::inbound::{extract_in_reply_to, parent_media_info};
        use matrix_sdk::event_handler::RawEvent;
        use matrix_sdk::ruma::events::AnySyncTimelineEvent;
        use matrix_sdk::ruma::serde::Raw;

        fn raw(json: serde_json::Value) -> RawEvent {
            let r: Raw<serde_json::Value> = Raw::new(&json).expect("raw");
            RawEvent(r.into_json())
        }

        fn parent_raw(json: serde_json::Value) -> Raw<AnySyncTimelineEvent> {
            Raw::new(&json).expect("parent raw").cast_unchecked()
        }

        #[test]
        fn in_reply_to_extracted_from_plain_reply() {
            let r = raw(serde_json::json!({
                "content": {
                    "msgtype": "m.text",
                    "body": "@bot can you see this?",
                    "m.relates_to": {
                        "m.in_reply_to": { "event_id": "$parent:server" }
                    }
                }
            }));
            let id = extract_in_reply_to(&r).expect("some");
            assert_eq!(id.as_str(), "$parent:server");
        }

        #[test]
        fn in_reply_to_extracted_from_threaded_reply() {
            // Modern threaded replies nest m.in_reply_to *inside* the
            // m.thread relation — extract_in_reply_to should handle both.
            let r = raw(serde_json::json!({
                "content": {
                    "msgtype": "m.text",
                    "body": "...",
                    "m.relates_to": {
                        "rel_type": "m.thread",
                        "event_id": "$root:server",
                        "m.in_reply_to": { "event_id": "$parent:server" }
                    }
                }
            }));
            let id = extract_in_reply_to(&r).expect("some");
            assert_eq!(id.as_str(), "$parent:server");
        }

        #[test]
        fn no_relation_returns_none() {
            let r = raw(serde_json::json!({
                "content": { "msgtype": "m.text", "body": "hi" }
            }));
            assert!(extract_in_reply_to(&r).is_none());
        }

        #[test]
        fn parent_image_plain_url() {
            let p = parent_raw(serde_json::json!({
                "content": {
                    "msgtype": "m.image",
                    "body": "cat.jpg",
                    "url": "mxc://example.org/abc",
                    "info": { "mimetype": "image/jpeg" }
                }
            }));
            let info = parent_media_info(p).expect("media info");
            assert!(matches!(
                info.kind,
                super::super::inbound::MediaCategory::Image
            ));
            assert_eq!(info.file_name, "cat.jpg");
            assert_eq!(info.mime.as_deref(), Some("image/jpeg"));
        }

        #[test]
        fn parent_voice_distinguished_from_audio() {
            let p = parent_raw(serde_json::json!({
                "content": {
                    "msgtype": "m.audio",
                    "body": "voice.ogg",
                    "url": "mxc://example.org/v",
                    "org.matrix.msc3245.voice": {}
                }
            }));
            let info = parent_media_info(p).expect("media info");
            assert!(matches!(
                info.kind,
                super::super::inbound::MediaCategory::Voice
            ));
        }

        #[test]
        fn parent_audio_without_voice_flag_is_audio() {
            let p = parent_raw(serde_json::json!({
                "content": {
                    "msgtype": "m.audio",
                    "body": "song.mp3",
                    "url": "mxc://example.org/m"
                }
            }));
            let info = parent_media_info(p).expect("media info");
            assert!(matches!(
                info.kind,
                super::super::inbound::MediaCategory::Audio
            ));
        }

        #[test]
        fn parent_audio_real_filename_survives_even_with_unrecognized_extension() {
            let p = parent_raw(serde_json::json!({
                "content": {
                    "msgtype": "m.audio",
                    "body": "Meeting recap.",
                    "filename": "recording.bin",
                    "url": "mxc://example.org/m",
                    "info": { "mimetype": "audio/ogg" }
                }
            }));
            let info = parent_media_info(p).expect("media info");
            assert_eq!(info.file_name, "recording.bin");
        }

        #[test]
        fn parent_voice_filename_preferred_over_caption_body() {
            let p = parent_raw(serde_json::json!({
                "content": {
                    "msgtype": "m.audio",
                    "body": "Voice message",
                    "filename": "voice.ogg",
                    "url": "mxc://example.org/v",
                    "org.matrix.msc3245.voice": {},
                    "info": { "mimetype": "audio/ogg" }
                }
            }));
            let info = parent_media_info(p).expect("media info");
            assert_eq!(info.file_name, "voice.ogg");
        }

        #[test]
        fn parent_encrypted_file_decoded() {
            // The `file` key (instead of `url`) signals encrypted media —
            // parent_media_info must decode it as MediaSource::Encrypted.
            let p = parent_raw(serde_json::json!({
                "content": {
                    "msgtype": "m.image",
                    "body": "secret.jpg",
                    "info": { "mimetype": "image/jpeg" },
                    "file": {
                        "url": "mxc://example.org/enc",
                        "v": "v2",
                        "key": {
                            "kty": "oct",
                            "alg": "A256CTR",
                            "ext": true,
                            "k": "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8",
                            "key_ops": ["encrypt", "decrypt"]
                        },
                        "iv": "AAAAAAAAAAAAAAAAAAAAAA",
                        "hashes": { "sha256": "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8" }
                    }
                }
            }));
            let info = parent_media_info(p).expect("media info");
            assert!(matches!(
                info.kind,
                super::super::inbound::MediaCategory::Image
            ));
            assert!(matches!(
                info.source,
                matrix_sdk::ruma::events::room::MediaSource::Encrypted(_)
            ));
        }

        #[test]
        fn parent_text_event_returns_none() {
            let p = parent_raw(serde_json::json!({
                "content": { "msgtype": "m.text", "body": "hi" }
            }));
            assert!(parent_media_info(p).is_none());
        }
    }

    mod cron_recipient {
        //! Cron operators sometimes write `delivery.to` as `<sender>||<room>`.
        //! `client::normalize_recipient` extracts the last `!`/`#`-prefixed
        //! segment and signals whether it changed anything.

        use super::super::client::normalize_recipient;

        #[test]
        fn plain_room_id_unchanged() {
            let (out, normalized) = normalize_recipient("!abc:server");
            assert_eq!(out, "!abc:server");
            assert!(!normalized);
        }

        #[test]
        fn plain_alias_unchanged() {
            let (out, normalized) = normalize_recipient("#room:server");
            assert_eq!(out, "#room:server");
            assert!(!normalized);
        }

        #[test]
        fn sender_pipe_room_extracts_room() {
            let (out, normalized) = normalize_recipient("@bot:server||!abc:server");
            assert_eq!(out, "!abc:server");
            assert!(normalized);
        }

        #[test]
        fn whitespace_around_pipes_trimmed() {
            let (out, _) = normalize_recipient("@bot:server || !abc:server ");
            assert_eq!(out, "!abc:server");
        }

        #[test]
        fn no_room_segment_falls_through_to_input() {
            // If nothing in the split looks like a room, return the original
            // so resolve_room's downstream parser produces a clear error.
            let (out, normalized) = normalize_recipient("alice||bob");
            assert_eq!(out, "alice||bob");
            assert!(normalized);
        }

        #[test]
        fn last_room_segment_wins() {
            let (out, _) = normalize_recipient("!old:s||!new:s");
            assert_eq!(out, "!new:s");
        }
    }

    mod outbound_sandbox {
        //! Trust-boundary tests for `outbound::validate_marker_target`. The
        //! marker target string comes from agent text and is therefore
        //! untrusted; the sandbox must keep local reads inside `workspace_dir`
        //! and refuse non-http(s) schemes outright.

        use super::super::outbound::{MarkerTarget, validate_marker_target};
        use tempfile::TempDir;

        #[test]
        fn accepts_workspace_path() {
            let workspace = TempDir::new().unwrap();
            let inside = workspace.path().join("photo.jpg");
            std::fs::write(&inside, b"x").unwrap();
            let result = validate_marker_target(inside.to_str().unwrap(), Some(workspace.path()));
            match result.expect("validate") {
                MarkerTarget::Local(p) => {
                    assert!(p.starts_with(std::fs::canonicalize(workspace.path()).unwrap()));
                }
                _ => panic!("expected Local"),
            }
        }

        #[test]
        fn accepts_relative_workspace_path() {
            let workspace = TempDir::new().unwrap();
            let inside = workspace.path().join("photo.jpg");
            std::fs::write(&inside, b"x").unwrap();
            // Relative-to-workspace target — no `./` prefix; mimics the form
            // an agent emits when it knows the workspace as cwd.
            let result = validate_marker_target("photo.jpg", Some(workspace.path()));
            match result.expect("validate") {
                MarkerTarget::Local(_) => {}
                _ => panic!("expected Local"),
            }
        }

        #[test]
        fn rejects_absolute_outside_workspace() {
            let workspace = TempDir::new().unwrap();
            let outside = tempfile::NamedTempFile::new().unwrap();
            let result =
                validate_marker_target(outside.path().to_str().unwrap(), Some(workspace.path()));
            assert!(result.is_err(), "expected Err for outside target");
            let msg = result.unwrap_err().to_string();
            assert!(
                msg.contains("outside workspace_dir"),
                "expected 'outside workspace_dir' in error, got: {msg}"
            );
        }

        #[test]
        fn rejects_dotdot_traversal() {
            let workspace = TempDir::new().unwrap();
            let parent = workspace.path().parent().unwrap();
            let outside_dir = parent.join("clawcrew-test-outside");
            let _ = std::fs::create_dir(&outside_dir);
            let outside_file = outside_dir.join("secret");
            std::fs::write(&outside_file, b"x").unwrap();
            let traversal = format!(
                "../{}/secret",
                outside_dir.file_name().unwrap().to_str().unwrap()
            );
            let result = validate_marker_target(&traversal, Some(workspace.path()));
            let _ = std::fs::remove_file(&outside_file);
            let _ = std::fs::remove_dir(&outside_dir);
            assert!(
                result.is_err(),
                "expected Err for `..` traversal escaping workspace"
            );
        }

        #[test]
        fn rejects_file_scheme() {
            let workspace = TempDir::new().unwrap();
            let result = validate_marker_target("file:///etc/hostname", Some(workspace.path()));
            let msg = result.unwrap_err().to_string();
            assert!(
                msg.contains("disallowed scheme"),
                "expected scheme rejection, got: {msg}"
            );
        }

        #[test]
        fn rejects_data_scheme() {
            let workspace = TempDir::new().unwrap();
            let result =
                validate_marker_target("data:text/plain;base64,aGk=", Some(workspace.path()));
            let msg = result.unwrap_err().to_string();
            assert!(
                msg.contains("disallowed scheme"),
                "expected scheme rejection, got: {msg}"
            );
        }

        #[test]
        fn rejects_unknown_scheme() {
            let workspace = TempDir::new().unwrap();
            let result = validate_marker_target("ftp://example.com/x", Some(workspace.path()));
            let msg = result.unwrap_err().to_string();
            assert!(
                msg.contains("disallowed scheme"),
                "expected scheme rejection, got: {msg}"
            );
        }

        #[test]
        fn accepts_http_url() {
            let workspace = TempDir::new().unwrap();
            let result =
                validate_marker_target("http://example.com/photo.jpg", Some(workspace.path()));
            match result.expect("validate") {
                MarkerTarget::Http(u) => assert_eq!(u.scheme(), "http"),
                _ => panic!("expected Http"),
            }
        }

        #[test]
        fn accepts_https_url() {
            let workspace = TempDir::new().unwrap();
            let result =
                validate_marker_target("https://example.com/photo.jpg", Some(workspace.path()));
            match result.expect("validate") {
                MarkerTarget::Http(u) => assert_eq!(u.scheme(), "https"),
                _ => panic!("expected Http"),
            }
        }

        #[test]
        fn local_path_without_workspace_is_refused() {
            // Operator forgot to wire `with_workspace_dir`. Local marker
            // cannot be safely resolved — refuse rather than fall back to
            // process cwd (which would be the daemon working dir, not the
            // workspace).
            let result = validate_marker_target("photo.jpg", None);
            let msg = result.unwrap_err().to_string();
            assert!(
                msg.contains("without a workspace_dir"),
                "expected workspace_dir-not-configured error, got: {msg}"
            );
        }

        #[test]
        fn http_url_works_without_workspace() {
            // HTTP URLs don't depend on a workspace — they should succeed
            // even when workspace_dir is None.
            let result = validate_marker_target("https://example.com/x.jpg", None);
            assert!(matches!(result, Ok(MarkerTarget::Http(_))));
        }

        fn assert_ssr_refused(target: &str) {
            let workspace = TempDir::new().unwrap();
            let result = validate_marker_target(target, Some(workspace.path()));
            let msg = result
                .expect_err(&format!("expected SSRF refusal for {target}"))
                .to_string();
            assert!(
                msg.contains("private or local host"),
                "expected SSRF refusal message for {target}, got: {msg}"
            );
        }

        #[test]
        fn ssrf_refuses_loopback_v4() {
            assert_ssr_refused("http://127.0.0.1/admin");
        }

        #[test]
        fn ssrf_refuses_trailing_dot_local_hosts() {
            for target in [
                "http://localhost./admin",
                "http://printer.local./admin",
                "http://192.168.1.1../admin",
            ] {
                assert_ssr_refused(target);
            }
        }

        #[test]
        fn ssrf_refuses_loopback_v6() {
            assert_ssr_refused("http://[::1]/admin");
        }

        #[test]
        fn ssrf_refuses_rfc1918_v4() {
            for h in ["10.0.0.5", "172.16.0.1", "192.168.1.1"] {
                assert_ssr_refused(&format!("http://{h}/internal"));
            }
        }

        #[test]
        fn ssrf_refuses_link_local_v4() {
            // AWS / GCP / Azure cloud-metadata endpoint.
            assert_ssr_refused("http://169.254.169.254/latest/meta-data/");
        }

        #[test]
        fn ssrf_refuses_cgnat_v4() {
            // RFC 6598 shared address space (100.64.0.0/10).
            assert_ssr_refused("http://100.64.0.1/api");
        }

        #[test]
        fn ssrf_refuses_ipv4_mapped_loopback() {
            // ::ffff:127.0.0.1 — IPv4-mapped loopback, often missed by
            // naive IP-literal checks.
            assert_ssr_refused("http://[::ffff:127.0.0.1]/admin");
        }

        #[test]
        fn ssrf_refuses_localhost_name() {
            assert_ssr_refused("http://localhost/admin");
            assert_ssr_refused("http://foo.localhost/admin");
        }

        #[test]
        fn ssrf_refuses_local_suffix_name() {
            assert_ssr_refused("http://printer.local/");
        }

        #[test]
        fn ssrf_refuses_ipv6_unique_local() {
            // fc00::/7 — RFC 4193 unique local addresses.
            assert_ssr_refused("http://[fd00::1]/");
        }

        #[test]
        fn ssrf_refuses_ipv6_link_local() {
            // fe80::/10 — link-local.
            assert_ssr_refused("http://[fe80::1]/");
        }

        #[test]
        fn ssrf_refuses_https_same_as_http() {
            // The guard is scheme-agnostic; the same private host is
            // refused over https:// too.
            assert_ssr_refused("https://10.0.0.5/secret");
            assert_ssr_refused("https://[::1]/secret");
        }

        #[test]
        fn ssrf_refuses_with_userinfo_attempt() {
            // An attacker who controls the host string might smuggle a
            // private host through userinfo syntax; the SSRF guard fires
            // on the host portion regardless.
            assert_ssr_refused("http://attacker@127.0.0.1/");
        }

        #[test]
        fn accepts_public_host() {
            // Sanity: a normal public-looking host must still pass through.
            for h in ["example.com", "cdn.example.com", "1.1.1.1", "8.8.8.8"] {
                let workspace = TempDir::new().unwrap();
                let result = validate_marker_target(
                    &format!("https://{h}/photo.jpg"),
                    Some(workspace.path()),
                );
                assert!(
                    matches!(result, Ok(MarkerTarget::Http(_))),
                    "public host {h} must be accepted, got: {result:?}"
                );
            }
        }
    }

    mod outbound_redirect_ssrf {

        use super::super::outbound::fetch_http;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        fn private_redirect_body(location: &str) -> ResponseTemplate {
            ResponseTemplate::new(302).insert_header("Location", location)
        }

        #[tokio::test]
        async fn rejects_redirect_to_cloud_metadata_ip() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/photo.jpg"))
                .respond_with(private_redirect_body(
                    "http://169.254.169.254/latest/meta-data/iam/security-credentials/",
                ))
                .expect(1)
                .mount(&server)
                .await;

            let url = reqwest::Url::parse(&format!("{}/photo.jpg", server.uri())).unwrap();
            let err: anyhow::Error = fetch_http(url)
                .await
                .expect_err("redirect to cloud-metadata IP must be rejected");
            let msg = format!("{err:#}");
            assert!(
                msg.contains("private or local host") || msg.contains("PermissionDenied"),
                "expected SSRF redirect refusal, got: {msg}"
            );
            server.verify().await;
        }

        #[tokio::test]
        async fn rejects_redirect_to_loopback() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/photo.jpg"))
                .respond_with(private_redirect_body("http://127.0.0.1:9200/_cat/indices"))
                .expect(1)
                .mount(&server)
                .await;

            let url = reqwest::Url::parse(&format!("{}/photo.jpg", server.uri())).unwrap();
            let err: anyhow::Error = fetch_http(url)
                .await
                .expect_err("redirect to loopback must be rejected");
            let msg = format!("{err:#}");
            assert!(
                msg.contains("private or local host") || msg.contains("PermissionDenied"),
                "expected SSRF redirect refusal, got: {msg}"
            );
            server.verify().await;
        }

        #[tokio::test]
        async fn rejects_redirect_to_trailing_dot_localhost() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/photo.jpg"))
                .respond_with(private_redirect_body("http://localhost./secret"))
                .expect(1)
                .mount(&server)
                .await;

            let url = reqwest::Url::parse(&format!("{}/photo.jpg", server.uri())).unwrap();
            let err: anyhow::Error = fetch_http(url)
                .await
                .expect_err("redirect to trailing-dot localhost must be rejected");
            let msg = format!("{err:#}");
            assert!(
                msg.contains("private or local host") || msg.contains("PermissionDenied"),
                "expected SSRF redirect refusal, got: {msg}"
            );
            server.verify().await;
        }

        #[tokio::test]
        async fn follows_public_redirect_target() {
            assert!(
                !clawcrew_tools::helpers::domain_guard::is_private_or_local_host("example.com"),
                "public hostnames must not be classified as private by the per-hop guard"
            );
            assert!(
                !clawcrew_tools::helpers::domain_guard::is_private_or_local_host("cdn.example.com"),
                "public subdomains must not be classified as private"
            );
        }
    }

    mod transcription_gate {

        use super::super::inbound::{MediaCategory, should_transcribe};
        use super::super::legacy_transcription_resolver;
        use clawcrew_config::schema::TranscriptionConfig;

        fn enabled_cfg() -> TranscriptionConfig {
            // Construct via Default + struct update so we stay robust to
            // future field additions on TranscriptionConfig.
            TranscriptionConfig {
                enabled: true,
                ..TranscriptionConfig::default()
            }
        }

        fn disabled_cfg() -> TranscriptionConfig {
            TranscriptionConfig::default()
        }

        #[test]
        fn voice_transcribes() {
            assert!(should_transcribe(&MediaCategory::Voice));
        }

        #[test]
        fn audio_does_not_transcribe() {
            // Plain m.audio (no MSC3245 voice flag) is left as a regular
            // audio file — only voice notes get transcribed.
            assert!(!should_transcribe(&MediaCategory::Audio));
        }

        #[test]
        fn image_does_not_transcribe() {
            assert!(!should_transcribe(&MediaCategory::Image));
        }

        #[test]
        fn resolver_yields_nothing_when_disabled() {
            // The enabled gate moved from `should_transcribe` onto the
            // resolver, which reads it from live config on every message.
            let resolver = legacy_transcription_resolver(disabled_cfg());
            assert!(resolver().is_none());
        }

        #[test]
        fn resolver_yields_a_manager_when_enabled() {
            let resolver = legacy_transcription_resolver(TranscriptionConfig {
                local_whisper: Some(clawcrew_config::schema::LocalWhisperConfig {
                    url: "http://127.0.0.1:9999/v1/transcribe".to_string(),
                    bearer_token: None,
                    max_audio_bytes: 10 * 1024 * 1024,
                    timeout_secs: 30,
                }),
                ..enabled_cfg()
            });
            assert!(resolver().is_some_and(|manager| manager.is_ok()));
        }
    }

    mod outbound_send_outcome {
        //! Decision logic for what `outbound::send` does after attachment
        //! uploads complete. Marker-only messages used to error even though
        //! the attachment had landed; this captures the new contract.

        use super::super::outbound::{SendOutcome, decide_send_outcome};

        #[test]
        fn non_empty_text_with_attachment_sends_text() {
            assert_eq!(decide_send_outcome(false, true), SendOutcome::SendText);
        }

        #[test]
        fn non_empty_text_without_attachment_sends_text() {
            assert_eq!(decide_send_outcome(false, false), SendOutcome::SendText);
        }

        #[test]
        fn empty_text_with_attachment_returns_attachment() {
            // The bug fix: marker-only sends must surface the attachment's
            // event_id, not an error.
            assert_eq!(
                decide_send_outcome(true, true),
                SendOutcome::ReturnAttachment
            );
        }

        #[test]
        fn empty_text_without_attachment_is_error() {
            // True empty-message case: nothing to deliver, surface the error.
            assert_eq!(decide_send_outcome(true, false), SendOutcome::EmptyError);
        }
    }

    mod outbound_attachment_info {
        use super::super::outbound::{AttachmentKind, attachment_config_for};
        use matrix_sdk::{attachment::AttachmentInfo, ruma::UInt};
        use clawcrew_api::media::MediaAttachment;

        fn attachment(file_name: &str, mime_type: &str, len: usize) -> MediaAttachment {
            MediaAttachment {
                file_name: file_name.to_string(),
                data: vec![0; len],
                mime_type: Some(mime_type.to_string()),
                marker: None,
            }
        }

        fn info_size(info: AttachmentInfo) -> Option<UInt> {
            match info {
                AttachmentInfo::Image(info) => info.size,
                AttachmentInfo::Video(info) => info.size,
                AttachmentInfo::Audio(info) | AttachmentInfo::Voice(info) => info.size,
                AttachmentInfo::File(info) => info.size,
            }
        }

        #[test]
        fn structured_file_attachment_carries_matrix_size_info() {
            let att = attachment("report.pdf", "application/pdf", 4096);

            let mime = super::super::outbound::attachment_mime(&att);
            let config = attachment_config_for(&att, AttachmentKind::Auto, &mime, None);

            let info = config.info.expect("attachment info is populated");
            assert!(matches!(info, AttachmentInfo::File(_)));
            assert_eq!(info_size(info), UInt::try_from(4096usize).ok());
        }

        #[test]
        fn media_markers_use_type_specific_matrix_info_with_size() {
            let cases = [
                (
                    AttachmentKind::Image,
                    attachment("photo.png", "image/png", 17),
                    "image",
                ),
                (
                    AttachmentKind::Audio,
                    attachment("clip.ogg", "audio/ogg", 23),
                    "audio",
                ),
                (
                    AttachmentKind::Video,
                    attachment("movie.mp4", "video/mp4", 31),
                    "video",
                ),
            ];

            for (kind, att, expected_kind) in cases {
                let mime = super::super::outbound::attachment_mime(&att);
                let config = attachment_config_for(&att, kind, &mime, None);
                let info = config.info.expect("attachment info is populated");
                match (&info, expected_kind) {
                    (AttachmentInfo::Image(_), "image") => {}
                    (AttachmentInfo::Audio(_), "audio") => {}
                    (AttachmentInfo::Video(_), "video") => {}
                    _ => panic!("unexpected attachment info kind {info:?}"),
                }
                assert_eq!(info_size(info), UInt::try_from(att.data.len()).ok());
            }
        }

        #[test]
        fn attachment_info_kind_matches_final_mime_type() {
            let image_named_as_file = attachment("photo.png", "image/png", 47);
            let mime = super::super::outbound::attachment_mime(&image_named_as_file);
            let config =
                attachment_config_for(&image_named_as_file, AttachmentKind::File, &mime, None);
            let info = config.info.expect("attachment info is populated");
            assert!(
                matches!(info, AttachmentInfo::Image(_)),
                "info must match the MIME-selected Matrix event type"
            );
            assert_eq!(
                info_size(info),
                UInt::try_from(image_named_as_file.data.len()).ok()
            );

            let image_marker_with_file_mime = attachment("report.pdf", "application/pdf", 53);
            let mime = super::super::outbound::attachment_mime(&image_marker_with_file_mime);
            let config = attachment_config_for(
                &image_marker_with_file_mime,
                AttachmentKind::Image,
                &mime,
                None,
            );
            let info = config.info.expect("attachment info is populated");
            assert!(
                matches!(info, AttachmentInfo::File(_)),
                "file MIME should use file info so SDK preserves size"
            );
            assert_eq!(
                info_size(info),
                UInt::try_from(image_marker_with_file_mime.data.len()).ok()
            );
        }
    }

    /// A voice note has to carry its own length or clients render it as a
    /// `00:00` bubble with no seek bar. ClawCrew reads that length out of the
    /// Ogg container rather than decoding the audio, so these cover the parse
    /// itself, the layouts it refuses to guess at, and the event the send path
    /// actually puts on the wire.
    mod outbound_voice_duration {
        use std::time::Duration;

        use matrix_sdk::attachment::AttachmentInfo;
        use matrix_sdk::ruma::{event_id, mxc_uri, room_id};
        use matrix_sdk::test_utils::mocks::MatrixMockServer;
        use clawcrew_api::media::MediaAttachment;

        use super::super::outbound::{
            AttachmentKind, attachment_config_for, opus_duration, upload_attachment,
        };

        /// One second of tone encoded by `opusenc`, so the parser is measured
        /// against a real encoder rather than against our own idea of the
        /// format. `opusinfo` reports its playback length as `0m:01.000s`, and
        /// its `OpusTags` carry only encoder strings.
        const VOICE_NOTE: &[u8] = include_bytes!("testdata/voice_note.ogg");
        /// The fixture's length, on the 48 kHz granule clock.
        const VOICE_NOTE_SAMPLES: u64 = 48_000;

        // ---- helpers over real encoded bytes ------------------------------

        /// Ogg's page checksum: polynomial `0x04c1_1db7`, no reflection, zero
        /// initial value, no final inversion.
        fn ogg_crc(page: &[u8]) -> u32 {
            let mut crc = 0u32;
            for &byte in page {
                crc ^= u32::from(byte) << 24;
                for _ in 0..8 {
                    crc = if crc & 0x8000_0000 == 0 {
                        crc << 1
                    } else {
                        (crc << 1) ^ 0x04c1_1db7
                    };
                }
            }
            crc
        }

        /// Split a stream into its pages, as `(offset, end)` pairs.
        fn page_bounds(bytes: &[u8]) -> Vec<(usize, usize)> {
            let mut bounds = Vec::new();
            let mut cursor = 0usize;
            while cursor < bytes.len() {
                let segments = usize::from(bytes[cursor + 26]);
                let body_start = cursor + 27 + segments;
                let body_len: usize = bytes[cursor + 27..body_start]
                    .iter()
                    .map(|&n| usize::from(n))
                    .sum();
                let end = body_start + body_len;
                bounds.push((cursor, end));
                cursor = end;
            }
            bounds
        }

        /// Rebuild every page with a new serial and/or a shifted granule,
        /// restoring each checksum so the result is a file a decoder would
        /// accept, not merely one this parser happens to walk.
        fn rewrite_pages(bytes: &[u8], serial: Option<u32>, granule_offset: u64) -> Vec<u8> {
            let mut out = Vec::with_capacity(bytes.len());
            for (start, end) in page_bounds(bytes) {
                let mut page = bytes[start..end].to_vec();
                let granule =
                    u64::from_le_bytes(page[6..14].try_into().expect("eight granule bytes"));
                if granule != u64::MAX {
                    page[6..14].copy_from_slice(&(granule + granule_offset).to_le_bytes());
                }
                if let Some(serial) = serial {
                    page[14..18].copy_from_slice(&serial.to_le_bytes());
                }
                page[22..26].copy_from_slice(&0u32.to_le_bytes());
                let crc = ogg_crc(&page);
                page[22..26].copy_from_slice(&crc.to_le_bytes());
                out.extend_from_slice(&page);
            }
            out
        }

        /// Whether every page in a stream carries the checksum of its contents.
        fn checksums_hold(bytes: &[u8]) -> bool {
            page_bounds(bytes).into_iter().all(|(start, end)| {
                let mut page = bytes[start..end].to_vec();
                let stored =
                    u32::from_le_bytes(page[22..26].try_into().expect("four checksum bytes"));
                page[22..26].copy_from_slice(&0u32.to_le_bytes());
                ogg_crc(&page) == stored
            })
        }

        /// Rebuild a stream with all of its audio on one page, keeping the
        /// final granule and recomputing the checksum.
        ///
        /// This is the layout `opusenc` emits for any clip shorter than about a
        /// second: a single audio page that also ends the stream, whose granule
        /// trims the tail of the last packet and so sits *below* the samples
        /// completing on it. `header_type` lets a test build the same page
        /// without the end-of-stream flag, which is the invalid form.
        fn repaged_onto_one_audio_page(bytes: &[u8], header_type: u8, granule: u64) -> Vec<u8> {
            let bounds = page_bounds(bytes);
            let (headers, audio) = bounds.split_at(2);
            let mut out: Vec<u8> = headers
                .iter()
                .flat_map(|&(start, end)| bytes[start..end].to_vec())
                .collect();

            let mut table = Vec::new();
            let mut body = Vec::new();
            for &(start, end) in audio {
                let page = &bytes[start..end];
                assert_eq!(
                    page[5] & 0x01,
                    0,
                    "a packet spanning pages cannot be merged naively"
                );
                let segments = usize::from(page[26]);
                table.extend_from_slice(&page[27..27 + segments]);
                body.extend_from_slice(&page[27 + segments..]);
            }
            assert!(table.len() <= 255, "one page holds at most 255 segments");

            let first = &bytes[audio[0].0..audio[0].1];
            let mut page = Vec::from(*b"OggS");
            page.push(0);
            page.push(header_type);
            page.extend_from_slice(&granule.to_le_bytes());
            page.extend_from_slice(&first[14..22]); // serial and sequence
            page.extend_from_slice(&0u32.to_le_bytes()); // checksum
            page.push(u8::try_from(table.len()).expect("segment count below 256"));
            page.extend_from_slice(&table);
            page.extend_from_slice(&body);
            let crc = ogg_crc(&page);
            page[22..26].copy_from_slice(&crc.to_le_bytes());

            out.extend_from_slice(&page);
            out
        }

        // ---- helpers building streams byte by byte ------------------------
        //
        // These carry zero checksums. The parser does not read them, and these
        // fixtures exist to pin down which byte makes a stream unreadable.

        /// Header-type bit for a page opening mid-packet.
        const CONTINUED: u8 = 0x01;
        /// Header-type bit for a page beginning a logical stream.
        const BOS: u8 = 0x02;
        /// Header-type bit for a page ending a logical stream.
        const EOS: u8 = 0x04;
        const PRE_SKIP: u16 = 312;
        const SERIAL: u32 = 0x5a43_0001;
        /// Table of contents for 20 ms of SILK wideband, one frame per packet.
        const AUDIO_TOC: u8 = 0x08;
        /// Samples one `AUDIO_TOC` packet decodes to at 48 kHz.
        const PACKET_SAMPLES: u64 = 960;

        /// `OpusHead` identification packet. Only the magic and `pre_skip`
        /// matter to the parser; the rest is spec-shaped filler.
        fn opus_head(pre_skip: u16) -> Vec<u8> {
            let mut head = Vec::from(*b"OpusHead");
            head.push(1); // version
            head.push(1); // channel count
            head.extend_from_slice(&pre_skip.to_le_bytes());
            head.extend_from_slice(&48_000u32.to_le_bytes()); // input sample rate
            head.extend_from_slice(&0u16.to_le_bytes()); // output gain
            head.push(0); // channel mapping family
            head
        }

        /// `OpusTags` comment packet with an empty vendor string and no
        /// comments.
        fn opus_tags() -> Vec<u8> {
            let mut tags = Vec::from(*b"OpusTags");
            tags.extend_from_slice(&0u32.to_le_bytes()); // vendor string length
            tags.extend_from_slice(&0u32.to_le_bytes()); // user comment count
            tags
        }

        /// An audio packet worth `PACKET_SAMPLES`.
        fn audio_packet() -> Vec<u8> {
            vec![AUDIO_TOC, 0x00, 0x00, 0x00]
        }

        /// One Ogg page with a segment table that really describes `packets`.
        fn page(granule: u64, header_type: u8, packets: &[Vec<u8>]) -> Vec<u8> {
            let mut table = Vec::new();
            let mut body = Vec::new();
            for packet in packets {
                let mut remaining = packet.len();
                while remaining >= 255 {
                    table.push(255u8);
                    remaining -= 255;
                }
                table.push(u8::try_from(remaining).expect("lacing value below 255"));
                body.extend_from_slice(packet);
            }
            raw_page(granule, header_type, SERIAL, &table, &body)
        }

        /// A page with an arbitrary segment table, so tests can claim a body
        /// length the buffer does not actually hold.
        fn raw_page(
            granule: u64,
            header_type: u8,
            serial: u32,
            table: &[u8],
            body: &[u8],
        ) -> Vec<u8> {
            let mut out = Vec::from(*b"OggS");
            out.push(0); // stream structure version
            out.push(header_type);
            out.extend_from_slice(&granule.to_le_bytes());
            out.extend_from_slice(&serial.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes()); // page sequence
            out.extend_from_slice(&0u32.to_le_bytes()); // checksum
            out.push(u8::try_from(table.len()).expect("segment count below 256"));
            out.extend_from_slice(table);
            out.extend_from_slice(body);
            out
        }

        /// The two header pages every Opus stream opens with.
        fn header_pages() -> Vec<u8> {
            let mut bytes = page(0, BOS, &[opus_head(PRE_SKIP)]);
            bytes.extend_from_slice(&page(0, 0, &[opus_tags()]));
            bytes
        }

        /// A spec-shaped stream running `samples` of audio: `OpusHead` alone on
        /// a beginning-of-stream page, then `OpusTags`, then audio. The final
        /// page's granule trims the tail of its packet, which is how an encoder
        /// expresses a length that does not land on a packet boundary.
        fn stream(samples: u64) -> Vec<u8> {
            let final_granule = samples + u64::from(PRE_SKIP);
            let whole_packets = final_granule / PACKET_SAMPLES;
            let packets: Vec<Vec<u8>> = (0..whole_packets).map(|_| audio_packet()).collect();

            let mut bytes = header_pages();
            bytes.extend_from_slice(&page(whole_packets * PACKET_SAMPLES, 0, &packets));
            bytes.extend_from_slice(&page(final_granule, EOS, &[audio_packet()]));
            bytes
        }

        // ---- the parse ----------------------------------------------------

        #[test]
        fn a_real_encoded_stream_measures_its_playback_length() {
            assert!(
                checksums_hold(VOICE_NOTE),
                "the fixture must be a checksum-valid Ogg stream"
            );
            assert_eq!(opus_duration(VOICE_NOTE), Some(Duration::from_secs(1)));
        }

        #[test]
        fn elapsed_time_is_measured_from_the_clip_start_not_the_timeline_origin() {
            // A clip cropped out of a longer recording keeps the granule
            // positions of its source, so the final granule is a timestamp
            // rather than a sample count. One minute of origin must not become
            // one minute of playback.
            let origin = 60 * 48_000;
            let cropped = rewrite_pages(VOICE_NOTE, None, origin);

            assert!(
                checksums_hold(&cropped),
                "the shifted stream must stay checksum-valid"
            );
            assert_eq!(opus_duration(&cropped), opus_duration(VOICE_NOTE));
            assert_eq!(opus_duration(&cropped), Some(Duration::from_secs(1)));
        }

        #[test]
        fn a_lone_end_of_stream_audio_page_may_trim_below_its_packet_samples() {
            // `opusenc` emits this for any clip under about a second: one audio
            // page that also ends the stream, whose granule sits below the
            // samples completing on it because the tail is trimmed. The origin
            // is zero rather than something to derive by working backwards.
            let trimmed = repaged_onto_one_audio_page(VOICE_NOTE, 0x04, 48_312);

            assert!(checksums_hold(&trimmed), "the repaged stream stays valid");
            assert_eq!(opus_duration(&trimmed), opus_duration(VOICE_NOTE));
            assert_eq!(opus_duration(&trimmed), Some(Duration::from_secs(1)));
        }

        #[test]
        fn chained_streams_with_distinct_serials_report_no_duration() {
            // Concatenated logical streams each restart their own granule
            // timeline, so no single duration describes the result.
            let mut chained = VOICE_NOTE.to_vec();
            chained.extend_from_slice(&rewrite_pages(VOICE_NOTE, Some(0x0bad_f00d), 0));

            assert!(checksums_hold(&chained));
            assert_eq!(opus_duration(&chained), None);
        }

        /// Kept apart from the distinct-serial case on purpose: with both in
        /// one test the serial check answers first and this guard is never
        /// the assertion that fires.
        #[test]
        fn a_chain_reusing_the_serial_is_still_a_chain() {
            let mut chained = VOICE_NOTE.to_vec();
            chained.extend_from_slice(VOICE_NOTE);

            // Every page carries the opening stream's serial, so only the
            // second stream's beginning-of-stream page marks the boundary.
            assert!(checksums_hold(&chained));
            assert_eq!(opus_duration(&chained), None);
        }

        #[test]
        fn duration_spans_the_granule_positions_less_the_priming_samples() {
            assert_eq!(
                opus_duration(&stream(144_000)),
                Some(Duration::from_secs(3))
            );
            assert_eq!(
                opus_duration(&stream(72_000)),
                Some(Duration::from_millis(1_500))
            );
            assert_eq!(opus_duration(&stream(720)), Some(Duration::from_millis(15)));
        }

        #[test]
        fn a_length_between_milliseconds_lands_exactly() {
            // A lone audio page holds 960 samples, 312 of which are priming:
            // 648 samples, or 13.5 ms.
            let mut bytes = header_pages();
            bytes.extend_from_slice(&page(PACKET_SAMPLES, EOS, &[audio_packet()]));

            assert_eq!(opus_duration(&bytes), Some(Duration::from_micros(13_500)));
        }

        #[test]
        fn last_page_granule_wins() {
            let mut bytes = header_pages();
            bytes.extend_from_slice(&page(PACKET_SAMPLES, 0, &[audio_packet()]));
            bytes.extend_from_slice(&page(96_312, 0, &[audio_packet()]));
            bytes.extend_from_slice(&page(240_312, EOS, &[audio_packet()]));

            assert_eq!(opus_duration(&bytes), Some(Duration::from_secs(5)));
        }

        #[test]
        fn unknown_granule_pages_are_skipped_but_still_advance() {
            let mut bytes = header_pages();
            bytes.extend_from_slice(&page(PACKET_SAMPLES, 0, &[audio_packet()]));
            // `u64::MAX` means "no granule for this page", not "zero length".
            bytes.extend_from_slice(&page(u64::MAX, 0, &[audio_packet()]));
            bytes.extend_from_slice(&page(48_312, EOS, &[audio_packet()]));

            assert_eq!(opus_duration(&bytes), Some(Duration::from_secs(1)));
        }

        #[test]
        fn empty_body_page_does_not_stall_the_walk() {
            let mut bytes = header_pages();
            bytes.extend_from_slice(&page(PACKET_SAMPLES, 0, &[audio_packet()]));
            bytes.extend_from_slice(&page(u64::MAX, 0, &[]));
            bytes.extend_from_slice(&raw_page(u64::MAX, 0, SERIAL, &[], b""));
            bytes.extend_from_slice(&page(96_312, EOS, &[audio_packet()]));

            assert_eq!(opus_duration(&bytes), Some(Duration::from_secs(2)));
        }

        #[test]
        fn unreadable_streams_report_no_duration_instead_of_guessing() {
            let valid = stream(144_000);

            let mut short_header = Vec::from(*b"OggS");
            short_header.extend_from_slice(&[0u8; 8]);

            let mut table_overruns = header_pages();
            table_overruns.extend_from_slice(&raw_page(48_312, 0, SERIAL, &[255], b"five!"));

            let mut below_pre_skip = header_pages();
            below_pre_skip.extend_from_slice(&page(PACKET_SAMPLES, 0, &[audio_packet()]));
            below_pre_skip.extend_from_slice(&page(
                u64::from(PRE_SKIP) - 1,
                EOS,
                &[audio_packet()],
            ));

            let mut head_too_short = page(0, BOS, &[opus_head(PRE_SKIP)[..9].to_vec()]);
            head_too_short.extend_from_slice(&page(0, 0, &[opus_tags()]));
            head_too_short.extend_from_slice(&page(48_312, EOS, &[audio_packet()]));

            let mut opens_mid_packet = header_pages();
            opens_mid_packet.extend_from_slice(&page(48_312, CONTINUED, &[audio_packet()]));

            let mut first_audio_granule_unknown = header_pages();
            first_audio_granule_unknown.extend_from_slice(&page(u64::MAX, 0, &[audio_packet()]));
            first_audio_granule_unknown.extend_from_slice(&page(48_312, EOS, &[audio_packet()]));

            let mut zero_frame_count = header_pages();
            // Table-of-contents code 3 reads its frame count from the next
            // byte, and a packet of no frames has no length.
            zero_frame_count.extend_from_slice(&page(48_312, EOS, &[vec![AUDIO_TOC | 0x03, 0x00]]));

            let mut packet_too_long = header_pages();
            // 60 ms frames, 48 of them: four times what a packet may hold.
            packet_too_long.extend_from_slice(&page(48_312, EOS, &[vec![0x1b, 48]]));

            let mut audio_shares_the_tags_page = page(0, BOS, &[opus_head(PRE_SKIP)]);
            audio_shares_the_tags_page.extend_from_slice(&page(
                48_312,
                EOS,
                &[opus_tags(), audio_packet()],
            ));

            // The same trimmed page without the end-of-stream flag: a granule
            // below the page's samples is only legal on a page ending the
            // stream.
            let trims_without_ending_the_stream =
                repaged_onto_one_audio_page(VOICE_NOTE, 0, 48_312);
            // Ending the stream does not license a granule below `pre_skip`:
            // that would skip more samples than the stream contains.
            let trimmed_below_pre_skip =
                repaged_onto_one_audio_page(VOICE_NOTE, 0x04, u64::from(PRE_SKIP) - 1);

            let mut head_shares_its_page = page(0, BOS, &[opus_head(PRE_SKIP), opus_tags()]);
            head_shares_its_page.extend_from_slice(&page(48_312, EOS, &[audio_packet()]));

            let mut not_beginning_of_stream = page(0, 0, &[opus_head(PRE_SKIP)]);
            not_beginning_of_stream.extend_from_slice(&page(0, 0, &[opus_tags()]));
            not_beginning_of_stream.extend_from_slice(&page(48_312, EOS, &[audio_packet()]));

            let cases: [(&str, Vec<u8>); 20] = [
                ("empty", Vec::new()),
                ("garbage", b"not an ogg file, just some plain text".to_vec()),
                ("header shorter than a page header", short_header),
                ("truncated mid body", valid[..valid.len() - 3].to_vec()),
                ("trailing junk after the last page", {
                    let mut b = valid.clone();
                    b.extend_from_slice(b"tail");
                    b
                }),
                ("first packet is not OpusHead", {
                    let mut b = page(0, BOS, &[b"VorbisHead padding bytes".to_vec()]);
                    b.extend_from_slice(&page(48_312, EOS, &[audio_packet()]));
                    b
                }),
                ("OpusHead truncated before pre_skip", head_too_short),
                ("OpusHead sharing its page", head_shares_its_page),
                (
                    "audio sharing the OpusTags page",
                    audio_shares_the_tags_page,
                ),
                (
                    "first page not marked beginning-of-stream",
                    not_beginning_of_stream,
                ),
                ("segment table claims more than exists", table_overruns),
                ("span shorter than pre_skip", below_pre_skip),
                (
                    "granule below the page's samples without ending the stream",
                    trims_without_ending_the_stream,
                ),
                (
                    "end-of-stream granule below pre_skip",
                    trimmed_below_pre_skip,
                ),
                ("first audio page opens mid-packet", opens_mid_packet),
                (
                    "first audio page has no granule",
                    first_audio_granule_unknown,
                ),
                ("packet claiming no frames", zero_frame_count),
                ("packet claiming more than 120 ms", packet_too_long),
                (
                    "every granule unknown",
                    page(u64::MAX, BOS, &[opus_head(PRE_SKIP)]),
                ),
                ("header pages but no audio", header_pages()),
            ];

            for (label, bytes) in cases {
                assert_eq!(
                    opus_duration(&bytes),
                    None,
                    "{label} must not yield a duration"
                );
            }
        }

        // ---- the event that ships -----------------------------------------

        fn voice_attachment(data: Vec<u8>) -> MediaAttachment {
            MediaAttachment {
                file_name: "voice.ogg".to_string(),
                data,
                mime_type: Some("audio/ogg".to_string()),
                marker: None,
            }
        }

        fn info_duration(info: AttachmentInfo) -> Option<Duration> {
            match info {
                AttachmentInfo::Audio(info) | AttachmentInfo::Voice(info) => info.duration,
                _ => panic!("unexpected attachment info kind {info:?}"),
            }
        }

        /// The voice arm of `attachment_info_for` is what the SDK send path
        /// hands to `send_attachment`, so the measured length has to land here.
        #[test]
        fn voice_attachment_info_reports_the_measured_length() {
            let att = voice_attachment(VOICE_NOTE.to_vec());
            let mime = super::super::outbound::attachment_mime(&att);

            let config = attachment_config_for(&att, AttachmentKind::Voice, &mime, None);
            let info = config.info.expect("attachment info is populated");

            assert!(matches!(info, AttachmentInfo::Voice(_)));
            assert_eq!(info_duration(info), Some(Duration::from_secs(1)));
        }

        #[test]
        fn unmeasurable_voice_attachment_keeps_the_zero_fallback() {
            let att = voice_attachment(b"OggS-fake-opus-payload".to_vec());
            let mime = super::super::outbound::attachment_mime(&att);

            let config = attachment_config_for(&att, AttachmentKind::Voice, &mime, None);
            let info = config.info.expect("attachment info is populated");

            // Still `Some`: the SDK only emits `org.matrix.msc1767.audio` when
            // duration and waveform are both present.
            assert_eq!(info_duration(info), Some(Duration::ZERO));
        }

        /// Voice attachments go out through the SDK's shared attachment path,
        /// with `AttachmentInfo::Voice` carrying the measured length, so the
        /// proof belongs on the event that reaches the homeserver: both
        /// duration fields must carry the clip's length, next to the
        /// `org.matrix.msc3245.voice` flag that marks it a voice note.
        #[tokio::test]
        async fn the_sent_event_carries_the_measured_length() {
            let matrix = MatrixMockServer::new().await;
            let client = matrix.client_builder().build().await;
            matrix.mock_room_state_encryption().plain().mount().await;
            let room = matrix
                .sync_joined_room(&client, room_id!("!room:localhost"))
                .await;

            matrix
                .mock_authenticated_media_config()
                .ok_default()
                .mount()
                .await;
            matrix
                .mock_upload()
                .ok(mxc_uri!("mxc://localhost/voicenote"))
                .mount()
                .await;
            matrix
                .mock_room_send()
                .ok(event_id!("$voicenote"))
                .expect(1)
                .mount()
                .await;

            let att = voice_attachment(VOICE_NOTE.to_vec());
            upload_attachment(&room, &att, AttachmentKind::Voice, None)
                .await
                .expect("the voice note is sent");

            let sent = matrix
                .server()
                .received_requests()
                .await
                .expect("the mock server records requests")
                .into_iter()
                .filter(|req| req.url.path().contains("/send/"))
                .map(|req| req.body_json::<serde_json::Value>().expect("a JSON event"))
                .next_back()
                .expect("the voice event reached the homeserver");

            assert_eq!(sent["msgtype"], serde_json::json!("m.audio"));
            assert!(sent.get("org.matrix.msc3245.voice").is_some());
            assert_eq!(sent["info"]["duration"], serde_json::json!(1_000));
            assert_eq!(
                sent["org.matrix.msc1767.audio"]["duration"],
                serde_json::json!(1_000)
            );
        }

        #[test]
        fn the_fixture_is_the_length_the_tests_assert() {
            // Guards the fixture against being replaced by a clip of another
            // length without the expectations moving with it.
            assert_eq!(
                opus_duration(VOICE_NOTE),
                Some(Duration::new(
                    VOICE_NOTE_SAMPLES / 48_000,
                    u32::try_from((VOICE_NOTE_SAMPLES % 48_000) * 1_000_000_000 / 48_000)
                        .expect("a remainder below one second")
                ))
            );
        }
    }
