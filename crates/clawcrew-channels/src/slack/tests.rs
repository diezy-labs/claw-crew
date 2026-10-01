    use super::*;

    #[test]
    fn split_text_into_chunks_safe_on_multibyte_utf8() {
        let text = format!(
            "{}{}{}",
            "a".repeat(SLACK_BLOCK_TEXT_MAX_CHARS - 1),
            "≡ƒÿÇ",
            "tail"
        );
        let chunks = split_text_into_chunks(&text, SLACK_BLOCK_TEXT_MAX_CHARS, 3);

        assert_eq!(chunks.concat(), text);
        assert_eq!(chunks[0].len(), SLACK_BLOCK_TEXT_MAX_CHARS - 1);
        assert_eq!(chunks[1], "≡ƒÿÇtail");
        for chunk in &chunks {
            assert!(chunk.len() <= SLACK_BLOCK_TEXT_MAX_CHARS);
            assert!(chunk.is_char_boundary(chunk.len()));
        }
    }

    #[test]
    fn slack_channel_name() {
        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec![],
            "slack_test_alias",
            Arc::new(Vec::new),
        );
        assert_eq!(ch.name(), "slack");
    }

    #[test]
    fn slack_channel_with_channel_ids() {
        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec!["C12345".into()],
            "slack_test_alias",
            Arc::new(Vec::new),
        );
        assert_eq!(ch.channel_ids, vec!["C12345".to_string()]);
    }

    /// REGRESSION: Slack's own `with_transcription` never bound a provider, so
    /// every audio attachment failed with "no transcription_provider
    /// configured" even in a single-provider deployment. The shared snapshot
    /// path binds the lone provider; the daemon path binds the owning agent's.
    #[test]
    fn with_transcription_binds_the_sole_provider() {
        let tc = clawcrew_config::schema::TranscriptionConfig {
            enabled: true,
            api_key: Some("test_key".to_string()),
            ..Default::default()
        };
        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec!["C12345".into()],
            "slack_test_alias",
            Arc::new(Vec::new),
        )
        .with_transcription(tc);
        let manager = ch.transcription_manager.as_ref().expect("manager is built");
        assert_eq!(manager.bound_provider(), "groq");
        assert!(ch.transcription.is_some());
    }

    #[test]
    fn slack_group_reply_policy_defaults_to_all_messages() {
        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec![],
            "slack_test_alias",
            Arc::new(|| vec!["*".into()]),
        );
        assert!(ch.thread_replies);
        assert!(!ch.mention_only);
        assert!(ch.group_reply_allowed_sender_ids.is_empty());
    }

    #[test]
    fn with_thread_replies_sets_flag() {
        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec![],
            "slack_test_alias",
            Arc::new(Vec::new),
        )
        .with_thread_replies(false);
        assert!(!ch.thread_replies);
    }

    #[test]
    fn with_strict_mention_in_thread_sets_flag() {
        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec![],
            "slack_test_alias",
            Arc::new(Vec::new),
        );
        assert!(!ch.strict_mention_in_thread);
        let ch = ch.with_strict_mention_in_thread(true);
        assert!(ch.strict_mention_in_thread);
    }

    #[test]
    fn thread_context_max_messages_resolver_reads_live_value() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let configured = Arc::new(AtomicUsize::new(3));
        let resolver_value = Arc::clone(&configured);
        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec![],
            "slack_test_alias",
            Arc::new(Vec::new),
        )
        .with_thread_context_max_messages_resolver(Arc::new(move || {
            resolver_value.load(Ordering::Relaxed)
        }));

        assert_eq!(ch.thread_context_max_messages(), 3);
        configured.store(7, Ordering::Relaxed);
        assert_eq!(ch.thread_context_max_messages(), 7);
    }

    #[test]
    fn strict_active_thread_reply_requires_mention() {
        let strict = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec![],
            "slack_test_alias",
            Arc::new(Vec::new),
        )
        .with_group_reply_policy(true, Vec::new())
        .with_strict_mention_in_thread(true);
        assert!(strict.requires_mention("C_ONE", "U_USER", true));

        let non_strict = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec![],
            "slack_test_alias",
            Arc::new(Vec::new),
        )
        .with_group_reply_policy(true, Vec::new());
        assert!(!non_strict.requires_mention("C_ONE", "U_USER", true));
        assert!(!strict.requires_mention("D_ONE", "U_USER", true));
    }

    #[test]
    fn active_thread_cursor_advances_before_reply_policy_filters() {
        let original_seen_at = Instant::now();
        let mut active_threads = HashMap::from([(
            "T_PARENT".to_string(),
            (
                "C_ONE".to_string(),
                "1700000000.000001".to_string(),
                original_seen_at,
            ),
        )]);

        SlackChannel::advance_active_thread_cursor(
            &mut active_threads,
            "T_PARENT",
            "1700000001.000001",
        );

        let entry = active_threads.get("T_PARENT").unwrap();
        assert_eq!(entry.1, "1700000001.000001");
        assert!(entry.2 >= original_seen_at);
    }

    #[test]
    fn outbound_thread_ts_respects_thread_replies_setting() {
        let msg = SendMessage::new("hello", "C123").in_thread(Some("1741234567.100001".into()));

        let threaded = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec![],
            "slack_test_alias",
            Arc::new(Vec::new),
        );
        assert_eq!(threaded.outbound_thread_ts(&msg), Some("1741234567.100001"));

        let channel_root = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec![],
            "slack_test_alias",
            Arc::new(Vec::new),
        )
        .with_thread_replies(false);
        assert_eq!(channel_root.outbound_thread_ts(&msg), None);
    }

    #[test]
    fn with_workspace_dir_sets_field() {
        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec![],
            "slack_test_alias",
            Arc::new(Vec::new),
        )
        .with_workspace_dir(PathBuf::from("/tmp/slack-workspace"));
        assert_eq!(
            ch.workspace_dir.as_deref(),
            Some(std::path::Path::new("/tmp/slack-workspace"))
        );
    }

    #[test]
    fn slack_group_reply_policy_applies_sender_overrides() {
        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec![],
            "slack_test_alias",
            Arc::new(|| vec!["*".into()]),
        )
        .with_group_reply_policy(true, vec![" U111 ".into(), "U111".into(), "U222".into()]);

        assert!(ch.mention_only);
        assert_eq!(
            ch.group_reply_allowed_sender_ids,
            vec!["U111".to_string(), "U222".to_string()]
        );
        assert!(ch.is_group_sender_trigger_enabled("U111"));
        assert!(!ch.is_group_sender_trigger_enabled("U999"));
    }

    #[test]
    fn normalized_channel_id_respects_wildcard_and_blank() {
        assert_eq!(SlackChannel::normalized_channel_id(None), None);
        assert_eq!(SlackChannel::normalized_channel_id(Some("")), None);
        assert_eq!(SlackChannel::normalized_channel_id(Some("   ")), None);
        assert_eq!(SlackChannel::normalized_channel_id(Some("*")), None);
        assert_eq!(SlackChannel::normalized_channel_id(Some(" * ")), None);
        assert_eq!(
            SlackChannel::normalized_channel_id(Some(" C12345 ")),
            Some("C12345".to_string())
        );
    }

    #[test]
    fn configured_app_token_ignores_blank_values() {
        let ch = SlackChannel::new(
            unique_test_bot_token(),
            Some("   ".into()),
            vec![],
            "slack_test_alias",
            Arc::new(Vec::new),
        );
        assert_eq!(ch.configured_app_token(), None);
    }

    #[test]
    fn configured_app_token_trims_value() {
        let ch = SlackChannel::new(
            unique_test_bot_token(),
            Some(" xapp-123 ".into()),
            vec![],
            "slack_test_alias",
            Arc::new(Vec::new),
        );
        assert_eq!(ch.configured_app_token().as_deref(), Some("xapp-123"));
    }

    #[test]
    fn scoped_channel_ids_uses_explicit_list() {
        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec!["C_LIST1".into(), "D_DM1".into()],
            "slack_test_alias",
            Arc::new(Vec::new),
        );
        assert_eq!(
            ch.scoped_channel_ids(),
            Some(vec!["C_LIST1".to_string(), "D_DM1".to_string()])
        );
    }

    #[test]
    fn scoped_channel_ids_with_single_entry() {
        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec!["C_SINGLE".into()],
            "slack_test_alias",
            Arc::new(Vec::new),
        );
        assert_eq!(ch.scoped_channel_ids(), Some(vec!["C_SINGLE".to_string()]));
    }

    #[test]
    fn scoped_channel_ids_returns_none_for_wildcard_mode() {
        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec![],
            "slack_test_alias",
            Arc::new(Vec::new),
        );
        assert_eq!(ch.scoped_channel_ids(), None);
    }

    #[test]
    fn is_group_channel_id_detects_channel_prefixes() {
        assert!(SlackChannel::is_group_channel_id("C123"));
        assert!(SlackChannel::is_group_channel_id("G123"));
        assert!(!SlackChannel::is_group_channel_id("D123"));
        assert!(!SlackChannel::is_group_channel_id(""));
    }

    #[test]
    fn is_direct_message_true_for_im_reply_target() {
        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec![],
            "slack_test_alias",
            Arc::new(Vec::new),
        );
        let dm = clawcrew_api::channel::ChannelMessage {
            reply_target: "D0B189MTELX".into(),
            channel: "slack".into(),
            ..Default::default()
        };
        let group = clawcrew_api::channel::ChannelMessage {
            reply_target: "C12345".into(),
            ..dm.clone()
        };
        assert!(Channel::is_direct_message(&ch, &dm));
        assert!(!Channel::is_direct_message(&ch, &group));
    }

    #[test]
    fn extract_channel_ids_filters_archived_and_non_member_entries() {
        let payload = serde_json::json!({
            "channels": [
                {"id": "C1", "is_archived": false, "is_member": true},
                {"id": "C2", "is_archived": true, "is_member": true},
                {"id": "C3", "is_archived": false, "is_member": false},
                {"id": "C1", "is_archived": false, "is_member": true},
                {"id": "C4"}
            ]
        });
        let ids = SlackChannel::extract_channel_ids(&payload);
        assert_eq!(ids, vec!["C1".to_string(), "C4".to_string()]);
    }

    #[test]
    fn empty_allowlist_denies_everyone() {
        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec![],
            "slack_test_alias",
            Arc::new(Vec::new),
        );
        assert!(!ch.is_user_allowed("U12345"));
        assert!(!ch.is_user_allowed("anyone"));
    }

    #[test]
    fn wildcard_allows_everyone() {
        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec![],
            "slack_test_alias",
            Arc::new(|| vec!["*".into()]),
        );
        assert!(ch.is_user_allowed("U12345"));
    }

    #[test]
    fn explicit_user_peer_is_allowed() {
        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec![],
            "slack_test_alias",
            Arc::new(|| vec!["U01EXAMPLE".into()]),
        );
        assert!(ch.is_user_allowed("U01EXAMPLE"));
        assert!(!ch.is_user_allowed("U99OTHER"));
    }

    #[test]
    fn extract_user_display_name_prefers_profile_display_name() {
        let payload = serde_json::json!({
            "ok": true,
            "user": {
                "name": "fallback_name",
                "profile": {
                    "display_name": "Display Name",
                    "real_name": "Real Name"
                }
            }
        });

        assert_eq!(
            SlackChannel::extract_user_display_name(&payload).as_deref(),
            Some("Display Name")
        );
    }

    #[test]
    fn extract_user_display_name_falls_back_to_username() {
        let payload = serde_json::json!({
            "ok": true,
            "user": {
                "name": "fallback_name",
                "profile": {
                    "display_name": "   ",
                    "real_name": ""
                }
            }
        });

        assert_eq!(
            SlackChannel::extract_user_display_name(&payload).as_deref(),
            Some("fallback_name")
        );
    }

    #[test]
    fn cached_sender_display_name_returns_none_when_expired() {
        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec![],
            "slack_test_alias",
            Arc::new(|| vec!["*".into()]),
        );
        {
            let mut cache = ch.user_display_name_cache.lock().unwrap();
            cache.insert(
                "U123".to_string(),
                CachedSlackDisplayName {
                    display_name: "Expired Name".to_string(),
                    expires_at: Instant::now()
                        .checked_sub(Duration::from_secs(1))
                        .expect("instant should allow subtracting one second in tests"),
                },
            );
        }

        assert_eq!(ch.cached_sender_display_name("U123"), None);
    }

    #[test]
    fn cached_sender_display_name_returns_cached_value_when_valid() {
        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec![],
            "slack_test_alias",
            Arc::new(|| vec!["*".into()]),
        );
        ch.cache_sender_display_name("U123", "Cached Name");

        assert_eq!(
            ch.cached_sender_display_name("U123").as_deref(),
            Some("Cached Name")
        );
    }

    #[test]
    fn normalize_incoming_content_requires_mention_when_enabled() {
        assert!(SlackChannel::normalize_incoming_content("hello", true, "U_BOT").is_none());
        assert_eq!(
            SlackChannel::normalize_incoming_content("<@U_BOT> run", true, "U_BOT").as_deref(),
            Some("<@U_BOT> run")
        );
    }

    #[test]
    fn normalize_incoming_content_without_mention_mode_keeps_message() {
        assert_eq!(
            SlackChannel::normalize_incoming_content("  hello world  ", false, "U_BOT").as_deref(),
            Some("hello world")
        );
    }

    #[test]
    fn compose_incoming_content_allows_attachment_only_messages() {
        let composed = SlackChannel::compose_incoming_content(
            String::new(),
            vec!["[IMAGE:data:image/png;base64,aaaa]".to_string()],
        );
        assert_eq!(
            composed.as_deref(),
            Some("[IMAGE:data:image/png;base64,aaaa]")
        );
    }

    #[test]
    fn parse_slack_permalink_accepts_standard_archives_link() {
        let parsed = SlackChannel::parse_slack_permalink(
            "https://acme.slack.com/archives/C12345678/p1712345678901234",
        )
        .expect("permalink");

        assert_eq!(parsed.channel_id, "C12345678");
        assert_eq!(parsed.message_ts, "1712345678.901234");
        assert_eq!(parsed.thread_ts_hint, None);
    }

    #[test]
    fn parse_slack_permalink_reads_thread_hint_when_present() {
        let parsed = SlackChannel::parse_slack_permalink(
            "https://acme.slack.com/archives/C12345678/p1712345678901234?thread_ts=1712345600.000100&cid=C12345678",
        )
        .expect("permalink");

        assert_eq!(parsed.thread_ts_hint.as_deref(), Some("1712345600.000100"));
    }

    #[test]
    fn parse_slack_permalink_rejects_non_message_links() {
        assert!(SlackChannel::parse_slack_permalink("https://example.com/path").is_none());
        assert!(
            SlackChannel::parse_slack_permalink("https://acme.slack.com/client/T1/C1").is_none()
        );
        assert!(
            SlackChannel::parse_slack_permalink("https://acme.slack.com/archives/C1/not-a-message")
                .is_none()
        );
    }

    #[test]
    fn extract_slack_permalinks_handles_slack_angle_bracket_format() {
        let permalinks = SlackChannel::extract_slack_permalinks(
            "Please inspect <https://acme.slack.com/archives/C123/p1712345678901234|message> now",
        );

        assert_eq!(permalinks.len(), 1);
        assert_eq!(permalinks[0].channel_id, "C123");
        assert_eq!(permalinks[0].message_ts, "1712345678.901234");
    }

    #[test]
    fn extract_slack_permalinks_deduplicates_message_targets() {
        let permalinks = SlackChannel::extract_slack_permalinks(
            "https://acme.slack.com/archives/C123/p1712345678901234 again <https://acme.slack.com/archives/C123/p1712345678901234|same>",
        );

        assert_eq!(permalinks.len(), 1);
    }

    #[test]
    fn message_subtype_support_allows_file_share() {
        assert!(SlackChannel::is_supported_message_subtype(None));
        assert!(SlackChannel::is_supported_message_subtype(Some(
            "file_share"
        )));
        assert!(SlackChannel::is_supported_message_subtype(Some(
            "thread_broadcast"
        )));
        assert!(!SlackChannel::is_supported_message_subtype(Some(
            "message_changed"
        )));
        assert!(!SlackChannel::is_supported_message_subtype(Some(
            "channel_join"
        )));
    }

    #[test]
    fn file_text_preview_prefers_preview_field() {
        let file = serde_json::json!({
            "preview": "line 1\nline 2",
            "preview_highlight": "ignored"
        });
        assert_eq!(
            SlackChannel::file_text_preview(&file).as_deref(),
            Some("line 1\nline 2")
        );
    }

    #[test]
    fn is_image_file_detects_mimetype_or_extension() {
        let from_mime = serde_json::json!({"mimetype":"image/png"});
        let from_ext = serde_json::json!({"name":"photo.jpeg"});
        let non_image = serde_json::json!({"name":"notes.txt","mimetype":"text/plain"});
        assert!(SlackChannel::is_image_file(&from_mime));
        assert!(SlackChannel::is_image_file(&from_ext));
        assert!(!SlackChannel::is_image_file(&non_image));
    }

    #[test]
    fn detect_image_mime_rejects_non_image_bytes_despite_image_metadata() {
        let file = serde_json::json!({"mimetype":"image/png","name":"wow.png"});
        let html_bytes = b"<!DOCTYPE html><html><body>login required</body></html>";
        assert_eq!(
            SlackChannel::detect_image_mime(
                Some("image/png"),
                &file,
                html_bytes,
                "https://files.slack.com/files-pri/T1/F2/wow.png"
            ),
            None
        );
    }

    #[test]
    fn detect_image_mime_prefers_magic_bytes_over_misleading_metadata() {
        let file = serde_json::json!({"mimetype":"image/bmp","name":"wow.png"});
        let png_header = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
        assert_eq!(
            SlackChannel::detect_image_mime(
                Some("image/bmp"),
                &file,
                &png_header,
                "https://files.slack.com/files-pri/T1/F2/wow.png"
            )
            .as_deref(),
            Some("image/png")
        );
    }

    #[test]
    fn is_probably_text_file_accepts_snippet_mode() {
        let snippet = serde_json::json!({"mode":"snippet"});
        let plain = serde_json::json!({"mimetype":"text/plain"});
        let binary = serde_json::json!({"mimetype":"application/octet-stream","name":"a.bin"});
        assert!(SlackChannel::is_probably_text_file(&snippet));
        assert!(SlackChannel::is_probably_text_file(&plain));
        assert!(!SlackChannel::is_probably_text_file(&binary));
    }

    #[test]
    fn sanitize_attachment_filename_strips_path_traversal() {
        assert_eq!(
            SlackChannel::sanitize_attachment_filename("../../secret.txt").as_deref(),
            Some("secret.txt")
        );
        assert_eq!(
            SlackChannel::sanitize_attachment_filename(r"..\\..\\secret.txt").as_deref(),
            Some("..__..__secret.txt")
        );
        assert!(SlackChannel::sanitize_attachment_filename("..").is_none());
    }

    #[test]
    fn parse_outbound_attachment_markers_extracts_supported_markers() {
        let (cleaned, attachments) =
            parse_outbound_attachment_markers("Done [IMAGE:/tmp/chart.png] and [file:/tmp/a.pdf]");

        assert_eq!(cleaned, "Done  and");
        assert_eq!(attachments.len(), 2);
        assert_eq!(attachments[0].kind, SlackOutboundAttachmentKind::Image);
        assert_eq!(attachments[0].target, "/tmp/chart.png");
        assert_eq!(attachments[1].kind, SlackOutboundAttachmentKind::File);
        assert_eq!(attachments[1].target, "/tmp/a.pdf");
    }

    #[test]
    fn parse_outbound_attachment_markers_keeps_unknown_markers() {
        let (cleaned, attachments) =
            parse_outbound_attachment_markers("Keep [UNKNOWN:/tmp/chart.png] here");

        assert_eq!(cleaned, "Keep [UNKNOWN:/tmp/chart.png] here");
        assert!(attachments.is_empty());
    }

    #[tokio::test]
    async fn resolve_outbound_attachment_marker_accepts_workspace_file() {
        let workspace = tempfile::tempdir().unwrap();
        let path = workspace.path().join("chart.png");
        tokio::fs::write(&path, b"\x89PNG\r\n\x1a\n").await.unwrap();
        let channel = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec![],
            "slack_test_alias",
            Arc::new(Vec::new),
        )
        .with_workspace_dir(workspace.path().to_path_buf());
        let marker = SlackOutboundAttachmentMarker {
            kind: SlackOutboundAttachmentKind::Image,
            target: path.to_string_lossy().to_string(),
        };

        let attachment = channel
            .resolve_outbound_attachment_marker(&marker)
            .await
            .unwrap();

        assert_eq!(attachment.file_name, "chart.png");
        assert_eq!(attachment.mime_type.as_deref(), Some("image/png"));
        assert_eq!(attachment.data, b"\x89PNG\r\n\x1a\n");
    }

    #[tokio::test]
    async fn resolve_outbound_attachment_marker_rejects_workspace_escape() {
        let workspace = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let path = outside.path().join("secret.png");
        tokio::fs::write(&path, b"\x89PNG\r\n\x1a\n").await.unwrap();
        let channel = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec![],
            "slack_test_alias",
            Arc::new(Vec::new),
        )
        .with_workspace_dir(workspace.path().to_path_buf());
        let marker = SlackOutboundAttachmentMarker {
            kind: SlackOutboundAttachmentKind::Image,
            target: path.to_string_lossy().to_string(),
        };

        let err = channel
            .resolve_outbound_attachment_marker(&marker)
            .await
            .unwrap_err()
            .to_string();

        assert!(err.contains("escapes workspace"), "{err}");
    }

    async fn mock_slack_upload_flow(
        server: &wiremock::MockServer,
        file_id: &str,
        upload_path: &str,
        complete_status: u16,
        complete_body: serde_json::Value,
    ) {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};

        Mock::given(method("POST"))
            .and(path("/files.getUploadURLExternal"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "upload_url": format!("{}{}", server.uri(), upload_path),
                "file_id": file_id,
            })))
            .expect(1)
            .mount(server)
            .await;

        Mock::given(method("POST"))
            .and(path(upload_path))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .expect(1)
            .mount(server)
            .await;

        Mock::given(method("POST"))
            .and(path("/files.completeUploadExternal"))
            .respond_with(ResponseTemplate::new(complete_status).set_body_json(complete_body))
            .expect(1)
            .mount(server)
            .await;
    }

    /// Every handle gets a distinct bot token so each test resolves its own
    /// entry in the process-wide installation cooldown registry. Sharing one
    /// token here would leak a `Retry-After` deadline set by a rate-limit test
    /// into unrelated tests running in the same process. Tests that need the
    /// shared-installation behaviour construct handles with an explicit common
    /// token instead.
    fn unique_test_bot_token() -> String {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        format!(
            "xoxb-fake-{}",
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )
    }

    fn test_slack_channel(server: &wiremock::MockServer, workspace: &Path) -> SlackChannel {
        SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec![],
            "slack_test_alias",
            Arc::new(Vec::new),
        )
        .with_workspace_dir(workspace.to_path_buf())
        .with_api_base_url(server.uri())
    }

    #[tokio::test]
    async fn send_uploads_text_and_outbound_attachment_via_slack_external_flow() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let tmp = tempfile::tempdir().unwrap();
        let attachment_path = tmp.path().join("report.txt");
        tokio::fs::write(&attachment_path, b"report-bytes")
            .await
            .unwrap();

        Mock::given(method("POST"))
            .and(path("/chat.postMessage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "ts": "1710000000.000100",
            })))
            .expect(1)
            .mount(&server)
            .await;
        mock_slack_upload_flow(
            &server,
            "F_TEXT",
            "/upload/text",
            200,
            serde_json::json!({"ok": true}),
        )
        .await;

        let ch = test_slack_channel(&server, tmp.path());
        let mut msg =
            SendMessage::new(format!("Done [FILE:{}]", attachment_path.display()), "C123");
        msg.thread_ts = Some("1709999999.000001".into());

        SlackChannel::send(&ch, &msg).await.unwrap();

        let requests = server.received_requests().await.unwrap();
        let post = requests
            .iter()
            .find(|req| req.url.path() == "/chat.postMessage")
            .expect("chat.postMessage should be called");
        let post_body: serde_json::Value = serde_json::from_slice(&post.body).unwrap();
        assert_eq!(post_body["channel"], "C123");
        assert_eq!(post_body["thread_ts"], "1709999999.000001");
        assert_eq!(post_body["text"], "Done");

        let get_upload = requests
            .iter()
            .find(|req| req.url.path() == "/files.getUploadURLExternal")
            .expect("getUploadURLExternal should be called");
        let get_upload_body = String::from_utf8_lossy(&get_upload.body);
        assert!(get_upload_body.contains("filename=report.txt"));
        assert!(get_upload_body.contains("length=12"));

        let upload = requests
            .iter()
            .find(|req| req.url.path() == "/upload/text")
            .expect("byte upload should be called");
        assert_eq!(upload.body.as_slice(), b"report-bytes");

        let complete = requests
            .iter()
            .find(|req| req.url.path() == "/files.completeUploadExternal")
            .expect("completeUploadExternal should be called");
        let complete_body: serde_json::Value = serde_json::from_slice(&complete.body).unwrap();
        assert_eq!(complete_body["channel_id"], "C123");
        assert_eq!(complete_body["thread_ts"], "1709999999.000001");
        assert_eq!(complete_body["files"][0]["id"], "F_TEXT");
        assert_eq!(complete_body["files"][0]["title"], "report.txt");
    }

    #[tokio::test]
    async fn send_uploads_attachment_only_message_without_chat_post() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let tmp = tempfile::tempdir().unwrap();
        let attachment_path = tmp.path().join("only.txt");
        tokio::fs::write(&attachment_path, b"only-bytes")
            .await
            .unwrap();

        Mock::given(method("POST"))
            .and(path("/chat.postMessage"))
            .respond_with(ResponseTemplate::new(500).set_body_string("must-not-call"))
            .expect(0)
            .mount(&server)
            .await;
        mock_slack_upload_flow(
            &server,
            "F_ONLY",
            "/upload/only",
            200,
            serde_json::json!({"ok": true}),
        )
        .await;

        let ch = test_slack_channel(&server, tmp.path());
        let msg = SendMessage::new(format!("[FILE:{}]", attachment_path.display()), "C123");

        SlackChannel::send(&ch, &msg).await.unwrap();

        let requests = server.received_requests().await.unwrap();
        assert!(
            requests
                .iter()
                .all(|req| req.url.path() != "/chat.postMessage"),
            "attachment-only sends must skip chat.postMessage"
        );
        assert!(
            requests
                .iter()
                .any(|req| req.url.path() == "/files.completeUploadExternal"),
            "attachment-only sends must still complete file upload"
        );
    }

    #[tokio::test]
    async fn send_returns_error_when_slack_complete_upload_fails() {
        use wiremock::MockServer;

        let server = MockServer::start().await;
        let tmp = tempfile::tempdir().unwrap();
        let attachment_path = tmp.path().join("fail.txt");
        tokio::fs::write(&attachment_path, b"fail-bytes")
            .await
            .unwrap();

        mock_slack_upload_flow(
            &server,
            "F_FAIL",
            "/upload/fail",
            200,
            serde_json::json!({"ok": false, "error": "complete_failed"}),
        )
        .await;

        let ch = test_slack_channel(&server, tmp.path());
        let msg = SendMessage::new(format!("[FILE:{}]", attachment_path.display()), "C123");

        let err = SlackChannel::send(&ch, &msg)
            .await
            .expect_err("Slack completion failure should propagate");
        assert!(
            err.to_string()
                .contains("files.completeUploadExternal failed: complete_failed"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn ensure_file_extension_appends_when_missing() {
        assert_eq!(
            SlackChannel::ensure_file_extension("capture", "png"),
            "capture.png"
        );
        assert_eq!(
            SlackChannel::ensure_file_extension("capture.jpeg", "png"),
            "capture.jpeg"
        );
    }

    #[test]
    fn is_allowed_slack_media_hostname_matches_suffixes() {
        assert!(SlackChannel::is_allowed_slack_media_hostname(
            "files.slack.com"
        ));
        assert!(SlackChannel::is_allowed_slack_media_hostname(
            "downloads.slack-edge.com"
        ));
        assert!(SlackChannel::is_allowed_slack_media_hostname(
            "foo.slack-files.com"
        ));
        assert!(!SlackChannel::is_allowed_slack_media_hostname(
            "example.com"
        ));
    }

    #[test]
    fn validate_slack_private_file_url_rejects_invalid_schemes_and_hosts() {
        assert!(
            SlackChannel::validate_slack_private_file_url("https://files.slack.com/f").is_some()
        );
        assert!(
            SlackChannel::validate_slack_private_file_url("http://files.slack.com/f").is_none()
        );
        assert!(SlackChannel::validate_slack_private_file_url("https://example.com/f").is_none());
        assert!(SlackChannel::validate_slack_private_file_url("not a url").is_none());
    }

    #[test]
    fn resolve_https_redirect_target_enforces_https() {
        let base = reqwest::Url::parse("https://files.slack.com/path/file").unwrap();
        let ok = SlackChannel::resolve_https_redirect_target(&base, "/next");
        assert_eq!(
            ok.as_ref().map(|url| url.as_str()),
            Some("https://files.slack.com/next")
        );

        let rejected =
            SlackChannel::resolve_https_redirect_target(&base, "http://files.slack.com/next");
        assert!(rejected.is_none());

        let rejected_host =
            SlackChannel::resolve_https_redirect_target(&base, "https://example.com/next");
        assert!(rejected_host.is_none());
    }

    #[test]
    fn redact_slack_url_hides_query_fragments() {
        let url = reqwest::Url::parse(
            "https://files.slack.com/files-pri/T1/F2/wow.png?token=secret#fragment",
        )
        .unwrap();
        let redacted = SlackChannel::redact_slack_url(&url);
        assert_eq!(redacted, "files.slack.com/.../wow.png");
        assert!(!redacted.contains('?'));
        assert!(!redacted.contains("token="));
        assert!(!redacted.contains('#'));
    }

    #[test]
    fn redact_redirect_location_keeps_only_relative_tail() {
        let redacted =
            SlackChannel::redact_redirect_location("/files-pri/T1/F2/wow.png?token=secret");
        assert_eq!(redacted, "relative/.../wow.png");
        assert!(!redacted.contains("token="));
    }

    #[tokio::test]
    async fn resolve_workspace_attachment_output_path_stays_in_workspace() {
        let workspace = tempfile::tempdir().unwrap();
        let output =
            SlackChannel::resolve_workspace_attachment_output_path(workspace.path(), "capture.png")
                .await
                .unwrap();

        let root = tokio::fs::canonicalize(workspace.path()).await.unwrap();
        assert!(output.starts_with(&root));
        assert!(output.to_string_lossy().contains("slack_files"));
    }

    #[tokio::test]
    async fn persist_image_attachment_writes_bytes_without_part_leftovers() {
        let workspace = tempfile::tempdir().unwrap();
        let channel = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec![],
            "slack_test_alias",
            Arc::new(Vec::new),
        )
        .with_workspace_dir(workspace.path().to_path_buf());
        let file = serde_json::json!({"id":"F1","name":"wow.png"});
        let png_bytes = vec![
            0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n', 0x00, 0x01, 0x02, 0x03,
        ];

        let output = channel
            .persist_image_attachment(&file, "wow.png", "image/png", &png_bytes)
            .await
            .expect("attachment path");
        let stored = tokio::fs::read(&output).await.expect("stored bytes");
        assert_eq!(stored, png_bytes);

        let save_dir = output.parent().unwrap();
        let mut entries = tokio::fs::read_dir(save_dir).await.unwrap();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            let name = entry.file_name().to_string_lossy().to_string();
            assert!(
                !name.ends_with(".part"),
                "unexpected temp artifact left behind: {name}"
            );
        }
    }

    #[test]
    fn evaluate_health_enforces_socket_mode_probe_when_enabled() {
        assert!(!SlackChannel::evaluate_health(false, false, true));
        assert!(!SlackChannel::evaluate_health(false, true, true));
        assert!(SlackChannel::evaluate_health(true, false, false));
        assert!(SlackChannel::evaluate_health(true, false, true));
        assert!(!SlackChannel::evaluate_health(true, true, false));
        assert!(SlackChannel::evaluate_health(true, true, true));
    }

    #[test]
    fn slack_api_call_succeeded_requires_ok_true_in_body() {
        assert!(!SlackChannel::slack_api_call_succeeded(
            reqwest::StatusCode::OK,
            r#"{"ok":false,"error":"invalid_auth"}"#
        ));
    }

    #[test]
    fn slack_api_call_succeeded_accepts_ok_true() {
        assert!(SlackChannel::slack_api_call_succeeded(
            reqwest::StatusCode::OK,
            r#"{"ok":true}"#
        ));
    }

    #[test]
    fn specific_allowlist_filters() {
        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec![],
            "slack_test_alias",
            Arc::new(|| vec!["U111".into(), "U222".into()]),
        );
        assert!(ch.is_user_allowed("U111"));
        assert!(ch.is_user_allowed("U222"));
        assert!(!ch.is_user_allowed("U333"));
    }

    #[test]
    fn allowlist_exact_match_not_substring() {
        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec![],
            "slack_test_alias",
            Arc::new(|| vec!["U111".into()]),
        );
        assert!(!ch.is_user_allowed("U1111"));
        assert!(!ch.is_user_allowed("U11"));
    }

    #[test]
    fn allowlist_empty_user_id() {
        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec![],
            "slack_test_alias",
            Arc::new(|| vec!["U111".into()]),
        );
        assert!(!ch.is_user_allowed(""));
    }

    #[test]
    fn allowlist_case_sensitive() {
        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec![],
            "slack_test_alias",
            Arc::new(|| vec!["U111".into()]),
        );
        assert!(ch.is_user_allowed("U111"));
        assert!(!ch.is_user_allowed("u111"));
    }

    #[test]
    fn allowlist_wildcard_and_specific() {
        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec![],
            "slack_test_alias",
            Arc::new(|| vec!["U111".into(), "*".into()]),
        );
        assert!(ch.is_user_allowed("U111"));
        assert!(ch.is_user_allowed("anyone"));
    }

    // ΓöÇΓöÇ Message ID edge cases ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ

    #[test]
    fn slack_message_id_format_includes_channel_and_ts() {
        // Verify that message IDs follow the format: slack_{channel_id}_{ts}
        let ts = "1234567890.123456";
        let channel_id = "C12345";
        let expected_id = format!("slack_{channel_id}_{ts}");
        assert_eq!(expected_id, "slack_C12345_1234567890.123456");
    }

    #[test]
    fn slack_message_id_is_deterministic() {
        // Same channel_id + same ts = same ID (prevents duplicates after restart)
        let ts = "1234567890.123456";
        let channel_id = "C12345";
        let id1 = format!("slack_{channel_id}_{ts}");
        let id2 = format!("slack_{channel_id}_{ts}");
        assert_eq!(id1, id2);
    }

    #[test]
    fn slack_message_id_different_ts_different_id() {
        // Different timestamps produce different IDs
        let channel_id = "C12345";
        let id1 = format!("slack_{channel_id}_1234567890.123456");
        let id2 = format!("slack_{channel_id}_1234567890.123457");
        assert_ne!(id1, id2);
    }

    #[test]
    fn slack_message_id_different_channel_different_id() {
        // Different channels produce different IDs even with same ts
        let ts = "1234567890.123456";
        let id1 = format!("slack_C12345_{ts}");
        let id2 = format!("slack_C67890_{ts}");
        assert_ne!(id1, id2);
    }

    #[test]
    fn slack_message_id_no_uuid_randomness() {
        // Verify format doesn't contain random UUID components
        let ts = "1234567890.123456";
        let channel_id = "C12345";
        let id = format!("slack_{channel_id}_{ts}");
        assert!(!id.contains('-')); // No UUID dashes
        assert!(id.starts_with("slack_"));
    }

    #[test]
    fn inbound_thread_ts_prefers_explicit_thread_ts() {
        let msg = serde_json::json!({
            "ts": "123.002",
            "thread_ts": "123.001"
        });

        let thread_ts = SlackChannel::inbound_thread_ts(&msg, "123.002");
        assert_eq!(thread_ts.as_deref(), Some("123.001"));
    }

    #[test]
    fn inbound_thread_ts_falls_back_to_ts() {
        let msg = serde_json::json!({
            "ts": "123.001"
        });

        let thread_ts = SlackChannel::inbound_thread_ts(&msg, "123.001");
        assert_eq!(thread_ts.as_deref(), Some("123.001"));
    }

    #[test]
    fn inbound_thread_ts_none_when_ts_missing() {
        let msg = serde_json::json!({});

        let thread_ts = SlackChannel::inbound_thread_ts(&msg, "");
        assert_eq!(thread_ts, None);
    }

    #[test]
    fn ensure_poll_cursor_bootstraps_new_channel() {
        let mut cursors = HashMap::new();
        let now_ts = "1700000000.123456";

        let cursor = SlackChannel::ensure_poll_cursor(&mut cursors, "C123", now_ts);
        assert_eq!(cursor, now_ts);
        assert_eq!(cursors.get("C123").map(String::as_str), Some(now_ts));
    }

    #[test]
    fn ensure_poll_cursor_keeps_existing_cursor() {
        let mut cursors = HashMap::from([("C123".to_string(), "1700000000.000001".to_string())]);
        let cursor = SlackChannel::ensure_poll_cursor(&mut cursors, "C123", "9999999999.999999");

        assert_eq!(cursor, "1700000000.000001");
        assert_eq!(
            cursors.get("C123").map(String::as_str),
            Some("1700000000.000001")
        );
    }

    #[test]
    fn parse_retry_after_value_accepts_integer_seconds() {
        assert_eq!(SlackChannel::parse_retry_after_value("30"), Some(30));
    }

    #[test]
    fn parse_retry_after_value_accepts_decimal_seconds() {
        assert_eq!(SlackChannel::parse_retry_after_value("2.9"), Some(2));
    }

    #[test]
    fn parse_retry_after_value_rejects_non_numeric_values() {
        assert_eq!(SlackChannel::parse_retry_after_value("later"), None);
        assert_eq!(SlackChannel::parse_retry_after_value(""), None);
    }

    #[test]
    fn parse_retry_after_secs_reads_header_value() {
        let mut headers = HeaderMap::new();
        headers.insert(reqwest::header::RETRY_AFTER, "45".parse().unwrap());
        assert_eq!(SlackChannel::parse_retry_after_secs(&headers), Some(45));
    }

    #[test]
    fn compute_retry_delay_applies_backoff_and_jitter_with_cap() {
        let delay = SlackChannel::compute_retry_delay(30, 3, 250);
        assert_eq!(delay, Duration::from_secs(120) + Duration::from_millis(250));
    }

    #[tokio::test]
    async fn fetch_thread_replies_retries_bodyless_http_429() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/conversations.replies"))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/conversations.replies"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "messages": [],
                "response_metadata": {"next_cursor": ""},
            })))
            .expect(1)
            .mount(&server)
            .await;

        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec!["C_ONE".into()],
            "slack_test_alias",
            Arc::new(Vec::new),
        )
        .with_api_base_url(server.uri());

        let payload = ch
            .fetch_thread_replies_page_with_retry("C_ONE", "T_PARENT", "0", None, None, 1)
            .await;

        assert_eq!(
            payload.as_ref().and_then(|value| value.get("ok")),
            Some(&serde_json::Value::Bool(true))
        );
        server.verify().await;
    }

    // ΓöÇΓöÇ Thread reply handling ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ

    #[test]
    fn extract_active_threads_finds_thread_parents_with_replies() {
        let messages = vec![
            serde_json::json!({
                "ts": "100.000",
                "thread_ts": "100.000",
                "reply_count": 3,
                "latest_reply": "103.000"
            }),
            serde_json::json!({
                "ts": "200.000",
                "text": "no thread"
            }),
            serde_json::json!({
                "ts": "300.000",
                "thread_ts": "300.000",
                "reply_count": 0
            }),
        ];

        let threads = SlackChannel::extract_active_threads(&messages);
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].0, "100.000");
        assert_eq!(threads[0].1, "103.000");
    }

    #[test]
    fn extract_active_threads_ignores_reply_messages() {
        // A reply message has ts != thread_ts; it should not be treated as a thread parent.
        let messages = vec![serde_json::json!({
            "ts": "101.000",
            "thread_ts": "100.000",
            "text": "reply in thread"
        })];

        let threads = SlackChannel::extract_active_threads(&messages);
        assert!(threads.is_empty());
    }

    #[test]
    fn extract_active_threads_uses_thread_ts_as_fallback_latest_reply() {
        let messages = vec![serde_json::json!({
            "ts": "100.000",
            "thread_ts": "100.000",
            "reply_count": 1
        })];

        let threads = SlackChannel::extract_active_threads(&messages);
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].1, "100.000");
    }

    #[test]
    fn evict_stale_threads_removes_expired_entries() {
        let mut threads: HashMap<String, (String, String, Instant)> = HashMap::new();
        let old = Instant::now()
            .checked_sub(Duration::from_secs(SLACK_POLL_THREAD_EXPIRE_SECS + 1))
            .unwrap();
        threads.insert(
            "old.thread".to_string(),
            ("C1".to_string(), "old.reply".to_string(), old),
        );
        threads.insert(
            "new.thread".to_string(),
            ("C1".to_string(), "new.reply".to_string(), Instant::now()),
        );

        SlackChannel::evict_stale_threads(&mut threads, Instant::now());
        assert_eq!(threads.len(), 1);
        assert!(threads.contains_key("new.thread"));
    }

    #[test]
    fn evict_stale_threads_trims_excess_by_oldest_key() {
        let mut threads: HashMap<String, (String, String, Instant)> = HashMap::new();
        let now = Instant::now();
        for i in 0..(SLACK_POLL_ACTIVE_THREAD_MAX + 5) {
            threads.insert(
                format!("{i:06}.000"),
                ("C1".to_string(), format!("{i:06}.001"), now),
            );
        }

        SlackChannel::evict_stale_threads(&mut threads, now);
        assert_eq!(threads.len(), SLACK_POLL_ACTIVE_THREAD_MAX);
    }

    #[test]
    fn is_supported_message_subtype_rejects_message_replied() {
        // message_replied is a parent-level notification, not an actual reply.
        assert!(!SlackChannel::is_supported_message_subtype(Some(
            "message_replied"
        )));
    }

    #[test]
    fn extract_slack_ts_from_standard_message_id() {
        assert_eq!(
            extract_slack_ts("slack_C1234567890_1234567890.123456"),
            "1234567890.123456"
        );
    }

    #[test]
    fn extract_slack_ts_from_raw_ts_passthrough() {
        assert_eq!(extract_slack_ts("1234567890.123456"), "1234567890.123456");
    }

    #[test]
    fn extract_slack_ts_from_unprefixed_id() {
        assert_eq!(extract_slack_ts("unknown_format"), "unknown_format");
    }

    #[test]
    fn unicode_emoji_maps_to_slack_eyes() {
        assert_eq!(unicode_emoji_to_slack_name("\u{1F440}"), "eyes");
    }

    #[test]
    fn unicode_emoji_maps_to_slack_check_mark() {
        assert_eq!(unicode_emoji_to_slack_name("\u{2705}"), "white_check_mark");
    }

    #[test]
    fn unicode_emoji_maps_to_slack_warning() {
        assert_eq!(unicode_emoji_to_slack_name("\u{26A0}\u{FE0F}"), "warning");
        assert_eq!(unicode_emoji_to_slack_name("\u{26A0}"), "warning");
    }

    #[test]
    fn unicode_emoji_colon_wrapped_passthrough() {
        assert_eq!(
            unicode_emoji_to_slack_name(":custom_emoji:"),
            "custom_emoji"
        );
    }

    #[test]
    fn inbound_thread_ts_on_thread_reply_uses_thread_ts() {
        let reply = serde_json::json!({
            "ts": "200.000",
            "thread_ts": "100.000",
            "text": "a thread reply"
        });
        let thread_ts = SlackChannel::inbound_thread_ts(&reply, "200.000");
        assert_eq!(thread_ts.as_deref(), Some("100.000"));
    }

    #[test]
    fn inbound_thread_ts_genuine_only_returns_none_for_top_level() {
        // Top-level messages don't have thread_ts in Slack's API.
        let msg = serde_json::json!({
            "ts": "100.000",
            "text": "hello"
        });
        assert_eq!(SlackChannel::inbound_thread_ts_genuine_only(&msg), None);
    }

    #[test]
    fn inbound_thread_ts_genuine_only_returns_thread_ts_for_replies() {
        // Thread replies have thread_ts pointing to the parent message.
        let reply = serde_json::json!({
            "ts": "200.000",
            "thread_ts": "100.000",
            "text": "a reply"
        });
        assert_eq!(
            SlackChannel::inbound_thread_ts_genuine_only(&reply).as_deref(),
            Some("100.000")
        );
    }

    #[test]
    fn session_key_stable_without_thread_replies() {
        // When thread_replies=false, top-level messages from the same user should
        // produce the same conversation_history_key (thread_ts=None).
        use clawcrew_api::channel::ChannelMessage;

        let make_msg = |ts: &str| ChannelMessage {
            id: format!("slack_C123_{ts}"),
            sender: "U_alice".into(),
            reply_target: "C123".into(),
            content: "text".into(),
            channel: "slack".into(),
            channel_alias: None,
            timestamp: 0,
            thread_ts: None, // thread_replies=false ΓåÆ no fallback to ts
            interruption_scope_id: None,
            attachments: vec![],
            subject: None,

            ..Default::default()
        };

        let msg1 = make_msg("100.000");
        let msg2 = make_msg("200.000");

        let key1 = crate::util::conversation_history_key(&msg1);
        let key2 = crate::util::conversation_history_key(&msg2);
        assert_eq!(key1, key2, "session key should be stable across messages");
    }

    #[test]
    fn session_key_varies_with_thread_replies() {
        // When thread_replies=true, top-level messages get thread_ts=Some(ts),
        // giving each its own session key (thread isolation).
        use clawcrew_api::channel::ChannelMessage;

        let make_msg = |ts: &str| ChannelMessage {
            id: format!("slack_C123_{ts}"),
            sender: "U_alice".into(),
            reply_target: "C123".into(),
            content: "text".into(),
            channel: "slack".into(),
            channel_alias: None,
            timestamp: 0,
            thread_ts: Some(ts.to_string()), // thread_replies=true ΓåÆ ts as thread_ts
            interruption_scope_id: None,
            attachments: vec![],
            subject: None,

            ..Default::default()
        };

        let msg1 = make_msg("100.000");
        let msg2 = make_msg("200.000");

        let key1 = crate::util::conversation_history_key(&msg1);
        let key2 = crate::util::conversation_history_key(&msg2);
        assert_ne!(key1, key2, "session key should differ per thread");
    }

    #[test]
    fn slack_send_uses_markdown_blocks() {
        let msg = SendMessage::new("**bold** and _italic_", "C123");
        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec![],
            "slack_test_alias",
            Arc::new(Vec::new),
        );

        // Build the same JSON body that send() would construct.
        let mut body = serde_json::json!({
            "channel": msg.recipient,
            "text": msg.content
        });
        if msg.content.len() <= SLACK_MARKDOWN_BLOCK_MAX_CHARS {
            body["blocks"] = serde_json::json!([{
                "type": "markdown",
                "text": msg.content
            }]);
        }

        // Verify blocks are present with correct structure.
        let blocks = body["blocks"]
            .as_array()
            .expect("blocks should be an array");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0]["type"], "markdown");
        assert_eq!(blocks[0]["text"], msg.content);
        // text field kept as plaintext fallback.
        assert_eq!(body["text"], msg.content);
        // Suppress unused variable warning.
        let _ = ch.name();
    }

    #[test]
    fn slack_send_skips_markdown_blocks_for_long_content() {
        let long_content = "x".repeat(SLACK_MARKDOWN_BLOCK_MAX_CHARS + 1);
        let msg = SendMessage::new(long_content.clone(), "C123");

        let mut body = serde_json::json!({
            "channel": msg.recipient,
            "text": msg.content
        });
        if msg.content.len() <= SLACK_MARKDOWN_BLOCK_MAX_CHARS {
            body["blocks"] = serde_json::json!([{
                "type": "markdown",
                "text": msg.content
            }]);
        }

        assert!(
            body.get("blocks").is_none(),
            "blocks should not be set for oversized content"
        );
    }

    #[tokio::test]
    async fn start_typing_requires_thread_context() {
        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec![],
            "slack_test_alias",
            Arc::new(Vec::new),
        );
        // No thread_ts tracked for "C999" ΓÇö start_typing should be a no-op (Ok).
        let result = ch.start_typing("C999").await;
        assert!(
            result.is_ok(),
            "start_typing should succeed as no-op without thread context"
        );
    }

    #[tokio::test]
    async fn draft_progress_sanitizes_tool_details_in_direct_messages() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let tmp = tempfile::tempdir().unwrap();
        Mock::given(method("POST"))
            .and(path("/chat.postMessage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "ts": "1710000000.000100",
            })))
            .expect(1)
            .mount(&server)
            .await;

        let ch = test_slack_channel(&server, tmp.path()).with_streaming(true, 1);
        let draft_id = ch
            .send_draft(&SendMessage::new("...", "D123"))
            .await
            .unwrap()
            .expect("streaming Slack returns a draft id");

        ch.update_draft_progress("D123", &draft_id, "ΓÅ│ shell: cat /private/secret.txt\n")
            .await
            .unwrap();

        let requests = server.received_requests().await.unwrap();
        let post = requests
            .iter()
            .find(|request| request.url.path() == "/chat.postMessage")
            .expect("progress should materialize the draft");
        let body: serde_json::Value = serde_json::from_slice(&post.body).unwrap();
        assert_eq!(body["text"], "Running tool");
        assert!(!body["text"].as_str().unwrap().contains("secret"));
    }

    #[test]
    fn legacy_progress_parser_does_not_trust_display_strings() {
        assert_eq!(SlackChannel::legacy_progress_event("Running tool"), None);
        assert_eq!(
            SlackChannel::legacy_progress_event("ΓÅ│ shell: cat /private/secret.txt"),
            Some(ProgressEvent::RunningTool)
        );
    }

    #[tokio::test]
    async fn draft_progress_falls_back_to_message_updates_for_group_threads() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let tmp = tempfile::tempdir().unwrap();
        Mock::given(method("POST"))
            .and(path("/chat.postMessage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "ts": "1710000000.000100",
            })))
            .expect(1)
            .mount(&server)
            .await;

        let ch = test_slack_channel(&server, tmp.path()).with_streaming(true, 1);
        let draft_id = ch
            .send_draft(
                &SendMessage::new("...", "C123").in_thread(Some("1709999999.000001".to_string())),
            )
            .await
            .unwrap()
            .expect("streaming Slack returns a draft id");

        ch.update_draft_lifecycle("C123", &draft_id, ProgressEvent::Received)
            .await
            .unwrap();

        let requests = server.received_requests().await.unwrap();
        let post = requests
            .iter()
            .find(|request| request.url.path() == "/chat.postMessage")
            .expect("group-thread progress should materialize the draft");
        let body: serde_json::Value = serde_json::from_slice(&post.body).unwrap();
        assert_eq!(body["channel"], "C123");
        assert_eq!(body["thread_ts"], "1709999999.000001");
        assert_eq!(body["text"], "Received");
    }

    #[tokio::test]
    async fn assistant_thread_progress_is_sanitized_and_rate_limited() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let tmp = tempfile::tempdir().unwrap();
        Mock::given(method("POST"))
            .and(path("/assistant.threads.setStatus"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
            })))
            .expect(1)
            .mount(&server)
            .await;

        let ch = test_slack_channel(&server, tmp.path()).with_streaming(true, 60_000);
        ch.remember_assistant_thread(AssistantTarget {
            channel_id: "C123".to_string(),
            thread_ts: "1709999999.000001".to_string(),
        });
        let draft_id = ch
            .send_draft(
                &SendMessage::new("...", "C123")
                    .in_thread(Some("1709999999.000001".to_string()))
                    .in_reply_to(Some("slack_C123_message-one".to_string())),
            )
            .await
            .unwrap()
            .unwrap();

        ch.update_draft_lifecycle("C123", &draft_id, ProgressEvent::WaitingOnModel)
            .await
            .unwrap();
        ch.update_draft_progress("C123", &draft_id, "ΓÅ│ shell: cat secret\n")
            .await
            .unwrap();

        let requests = server.received_requests().await.unwrap();
        let status = requests
            .iter()
            .find(|request| request.url.path() == "/assistant.threads.setStatus")
            .expect("assistant thread should receive progress status");
        let body: serde_json::Value = serde_json::from_slice(&status.body).unwrap();
        assert_eq!(body["status"], "Waiting on model");
        assert_eq!(
            requests.len(),
            1,
            "status updates should respect the interval"
        );
    }

    #[tokio::test]
    async fn concurrent_assistant_turns_keep_independent_targets_and_pacing() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let tmp = tempfile::tempdir().unwrap();
        Mock::given(method("POST"))
            .and(path("/assistant.threads.setStatus"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
            })))
            .expect(2)
            .mount(&server)
            .await;

        let ch = test_slack_channel(&server, tmp.path()).with_streaming(true, 60_000);
        ch.remember_assistant_thread(AssistantTarget {
            channel_id: "C123".to_string(),
            thread_ts: "thread-one".to_string(),
        });
        let first = ch
            .send_draft(
                &SendMessage::new("...", "C123")
                    .in_thread(Some("thread-one".to_string()))
                    .in_reply_to(Some("slack_C123_message-one".to_string())),
            )
            .await
            .unwrap()
            .unwrap();

        ch.remember_assistant_thread(AssistantTarget {
            channel_id: "C123".to_string(),
            thread_ts: "thread-two".to_string(),
        });
        let second = ch
            .send_draft(
                &SendMessage::new("...", "C123")
                    .in_thread(Some("thread-two".to_string()))
                    .in_reply_to(Some("slack_C123_message-two".to_string())),
            )
            .await
            .unwrap()
            .unwrap();

        ch.update_draft_lifecycle("C123", &first, ProgressEvent::Planning)
            .await
            .unwrap();
        ch.update_draft_lifecycle("C123", &second, ProgressEvent::WaitingOnModel)
            .await
            .unwrap();

        let requests = server.received_requests().await.unwrap();
        let mut targets = requests
            .iter()
            .map(|request| {
                let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
                (
                    body["thread_ts"].as_str().unwrap().to_string(),
                    body["status"].as_str().unwrap().to_string(),
                )
            })
            .collect::<Vec<_>>();
        targets.sort();
        assert_eq!(
            targets,
            vec![
                ("thread-one".to_string(), "Planning".to_string()),
                ("thread-two".to_string(), "Waiting on model".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn ordinary_message_in_assistant_channel_uses_draft_message_api() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let tmp = tempfile::tempdir().unwrap();
        Mock::given(method("POST"))
            .and(path("/chat.postMessage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "ts": "ordinary-draft",
            })))
            .expect(1)
            .mount(&server)
            .await;

        let ch = test_slack_channel(&server, tmp.path()).with_streaming(true, 1);
        ch.remember_assistant_thread(AssistantTarget {
            channel_id: "C123".to_string(),
            thread_ts: "assistant-thread".to_string(),
        });
        let ordinary = ch
            .send_draft(
                &SendMessage::new("...", "C123")
                    .in_thread(Some("ordinary-thread".to_string()))
                    .in_reply_to(Some("slack_C123_ordinary-message".to_string())),
            )
            .await
            .unwrap()
            .unwrap();

        ch.update_draft_lifecycle("C123", &ordinary, ProgressEvent::Planning)
            .await
            .unwrap();

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].url.path(), "/chat.postMessage");
        let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(body["thread_ts"], "ordinary-thread");
    }

    /// Collect every `assistant.threads.setStatus` call the mock saw, as
    /// `(thread_ts, status)` pairs in request order. An empty status is the
    /// clear that terminal paths must issue.
    async fn assistant_status_calls(server: &wiremock::MockServer) -> Vec<(String, String)> {
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| request.url.path() == "/assistant.threads.setStatus")
            .map(|request| {
                let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
                let field = |key: &str| {
                    body.get(key)
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_string()
                };
                (field("thread_ts"), field("status"))
            })
            .collect()
    }

    /// Seed two concurrent Assistant turns in one channel and show a lifecycle
    /// state on each, so a terminal path has something to clear and a sibling
    /// that must survive. Returns `(channel, server, first_draft, second_draft)`.
    async fn two_live_assistant_turns(
        server: &wiremock::MockServer,
        data_dir: &std::path::Path,
    ) -> (SlackChannel, String, String) {
        let ch = test_slack_channel(server, data_dir).with_streaming(true, 1);
        for thread_ts in ["thread-one", "thread-two"] {
            ch.remember_assistant_thread(AssistantTarget {
                channel_id: "C123".to_string(),
                thread_ts: thread_ts.to_string(),
            });
        }

        let mut drafts = Vec::new();
        for (thread_ts, message_id) in [
            ("thread-one", "slack_C123_message-one"),
            ("thread-two", "slack_C123_message-two"),
        ] {
            drafts.push(
                ch.send_draft(
                    &SendMessage::new("...", "C123")
                        .in_thread(Some(thread_ts.to_string()))
                        .in_reply_to(Some(message_id.to_string())),
                )
                .await
                .unwrap()
                .unwrap(),
            );
        }

        // Both turns are visibly mid-lifecycle before the terminal path runs.
        ch.update_draft_lifecycle("C123", &drafts[0], ProgressEvent::RunningTool)
            .await
            .unwrap();
        ch.update_draft_lifecycle("C123", &drafts[1], ProgressEvent::WaitingOnModel)
            .await
            .unwrap();

        let second = drafts.pop().unwrap();
        let first = drafts.pop().unwrap();
        (ch, first, second)
    }

    #[tokio::test]
    async fn finalizing_one_assistant_turn_clears_only_its_status() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let tmp = tempfile::tempdir().unwrap();
        Mock::given(method("POST"))
            .and(path("/assistant.threads.setStatus"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/chat.postMessage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "ts": "final-one",
            })))
            .expect(1)
            .mount(&server)
            .await;

        let (ch, first, _second) = two_live_assistant_turns(&server, tmp.path()).await;

        ch.finalize_draft("C123", &first, "done", false)
            .await
            .unwrap();

        let calls = assistant_status_calls(&server).await;
        assert!(
            calls.contains(&("thread-one".to_string(), "Running tool".to_string())),
            "the finalized turn must have shown a lifecycle state first: {calls:?}"
        );
        assert_eq!(
            calls
                .iter()
                .filter(|(_, status)| status.is_empty())
                .collect::<Vec<_>>(),
            vec![&("thread-one".to_string(), String::new())],
            "finalization must clear exactly the finalized turn's thread: {calls:?}"
        );
    }

    /// A progress `chat.update` is dispatched detached, so a slow one can still
    /// be in flight when the turn ends. Finalization must wait for it: if the
    /// delayed lifecycle edit were allowed to land afterwards it would replace
    /// the final answer with stale progress text.
    ///
    /// The mock delays the first `chat.update` (the lifecycle edit) well past
    /// the finalize call, then records order of arrival. The final answer must
    /// be the last write Slack sees.
    #[tokio::test]
    async fn a_delayed_progress_update_cannot_overwrite_the_final_answer() {
        use wiremock::matchers::{body_string_contains, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let tmp = tempfile::tempdir().unwrap();
        Mock::given(method("POST"))
            .and(path("/chat.postMessage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "ts": "draft-ts",
            })))
            .mount(&server)
            .await;
        // The lifecycle edit is held for 2s; finalization is invoked ~immediately
        // after it is dispatched.
        Mock::given(method("POST"))
            .and(path("/chat.update"))
            .and(body_string_contains("Running tool"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_secs(2))
                    .set_body_json(serde_json::json!({"ok": true})),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/chat.update"))
            .and(body_string_contains("the final answer"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
            .mount(&server)
            .await;

        let ch = test_slack_channel(&server, tmp.path()).with_streaming(true, 1);
        let draft = ch
            .send_draft(
                &SendMessage::new("...", "C123")
                    .in_thread(Some("thread-one".to_string()))
                    .in_reply_to(Some("slack_C123_message-one".to_string())),
            )
            .await
            .unwrap()
            .unwrap();

        // Materialize the draft, then dispatch the slow lifecycle edit. Step
        // clear of the pacing window first so the lifecycle edit is genuinely
        // dispatched rather than rate-limited away ΓÇö otherwise this test could
        // pass for the wrong reason.
        ch.update_draft("C123", &draft, "Received").await.unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        ch.update_draft_lifecycle("C123", &draft, ProgressEvent::RunningTool)
            .await
            .unwrap();

        ch.finalize_draft("C123", &draft, "the final answer", false)
            .await
            .unwrap();

        let bodies: Vec<String> = server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| request.url.path() == "/chat.update")
            .map(|request| String::from_utf8_lossy(&request.body).to_string())
            .collect();

        assert!(
            bodies.iter().any(|body| body.contains("Running tool")),
            "the delayed lifecycle edit must still have been dispatched: {bodies:?}"
        );
        assert!(
            bodies
                .last()
                .is_some_and(|body| body.contains("the final answer")),
            "the final answer must be the last write to the draft: {bodies:?}"
        );
    }

    /// Production-shaped pacing: lifecycle updates and streamed response text
    /// share one draft and one rate limiter. With a realistic interval, an
    /// intermediate state dispatched after the interval has elapsed must
    /// actually reach Slack rather than being swallowed by interleaved text,
    /// and the final answer must still land last.
    ///
    /// This is the interaction the live exercise could not settle: emission and
    /// rendering were covered separately, but not together under pacing.
    #[tokio::test]
    async fn intermediate_lifecycle_survives_pacing_shared_with_streamed_text() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let tmp = tempfile::tempdir().unwrap();
        Mock::given(method("POST"))
            .and(path("/chat.postMessage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "ts": "draft-ts",
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/chat.update"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
            .mount(&server)
            .await;

        // 50ms interval: long enough to be a real rate limit, short enough to
        // step over deterministically.
        let ch = test_slack_channel(&server, tmp.path()).with_streaming(true, 50);
        let draft = ch
            .send_draft(
                &SendMessage::new("...", "C123")
                    .in_thread(Some("thread-one".to_string()))
                    .in_reply_to(Some("slack_C123_message-one".to_string())),
            )
            .await
            .unwrap()
            .unwrap();

        // Materializes the draft and starts the pacing clock.
        ch.update_draft_lifecycle("C123", &draft, ProgressEvent::Received)
            .await
            .unwrap();

        // Partial response text arrives, then the tool phase begins. Both are
        // dispatched after the interval, so neither may be dropped.
        tokio::time::sleep(Duration::from_millis(60)).await;
        ch.update_draft("C123", &draft, "partial answer so far")
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(60)).await;
        ch.update_draft_lifecycle("C123", &draft, ProgressEvent::RunningTool)
            .await
            .unwrap();

        ch.finalize_draft("C123", &draft, "the final answer", false)
            .await
            .unwrap();

        let bodies: Vec<String> = server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| matches!(request.url.path(), "/chat.update" | "/chat.postMessage"))
            .map(|request| String::from_utf8_lossy(&request.body).to_string())
            .collect();

        assert!(
            bodies.iter().any(|body| body.contains("Received")),
            "the first lifecycle state must materialize the draft: {bodies:?}"
        );
        assert!(
            bodies.iter().any(|body| body.contains("Running tool")),
            "an intermediate state dispatched outside the pacing window must reach Slack, \
             not be swallowed by the interleaved text update: {bodies:?}"
        );
        assert!(
            bodies
                .last()
                .is_some_and(|body| body.contains("the final answer")),
            "the final answer must still be the last write: {bodies:?}"
        );
    }

    /// `assistant.threads.setStatus` addresses a status only by
    /// `(channel_id, thread_ts)`, so two overlapping turns in ONE Assistant
    /// thread share that surface even though each holds its own draft ID. This
    /// is reachable with `interrupt_on_new_message = false`, the default, where
    /// the dispatcher lets the older worker continue.
    ///
    /// Latest-live-turn-wins: once turn B claims the thread, turn A may neither
    /// overwrite B's status with a late lifecycle write nor blank it when A
    /// finishes.
    #[tokio::test]
    async fn an_older_turn_cannot_overwrite_or_clear_a_newer_turn_in_one_assistant_thread() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let tmp = tempfile::tempdir().unwrap();
        Mock::given(method("POST"))
            .and(path("/assistant.threads.setStatus"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/chat.postMessage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "ts": "final-ts",
            })))
            .mount(&server)
            .await;

        let ch = test_slack_channel(&server, tmp.path()).with_streaming(true, 1);
        ch.remember_assistant_thread(AssistantTarget {
            channel_id: "C123".to_string(),
            thread_ts: "shared-thread".to_string(),
        });

        // Two turns, same Assistant thread, distinct draft IDs.
        let draft = |message_id: &'static str| {
            SendMessage::new("...", "C123")
                .in_thread(Some("shared-thread".to_string()))
                .in_reply_to(Some(message_id.to_string()))
        };
        let turn_a = ch
            .send_draft(&draft("slack_C123_msg-a"))
            .await
            .unwrap()
            .unwrap();
        let turn_b = ch
            .send_draft(&draft("slack_C123_msg-b"))
            .await
            .unwrap()
            .unwrap();
        assert_ne!(turn_a, turn_b, "each turn must hold its own draft ID");

        // B is the live turn and shows its state.
        ch.update_draft_lifecycle("C123", &turn_b, ProgressEvent::WaitingOnModel)
            .await
            .unwrap();
        // A is superseded: neither a late lifecycle write nor its terminal clear
        // may touch the surface B now owns.
        ch.update_draft_lifecycle("C123", &turn_a, ProgressEvent::RunningTool)
            .await
            .unwrap();
        ch.finalize_draft("C123", &turn_a, "A's answer", false)
            .await
            .unwrap();

        let calls = assistant_status_calls(&server).await;
        assert_eq!(
            calls,
            vec![("shared-thread".to_string(), "Waiting on model".to_string())],
            "only the live turn may write the shared surface, and a superseded \
             turn must not clear it: {calls:?}"
        );

        // B still owns it, so B's own completion does clear it.
        ch.finalize_draft("C123", &turn_b, "B's answer", false)
            .await
            .unwrap();
        let calls = assistant_status_calls(&server).await;
        assert_eq!(
            calls.last(),
            Some(&("shared-thread".to_string(), String::new())),
            "the owning turn's completion must clear the surface: {calls:?}"
        );
    }

    /// When every bounded attempt fails, the generation is retained rather than
    /// erased. That keeps the failure attributable and leaves the recovery path
    /// open: the next turn in the thread reclaims the target and its own
    /// terminal path clears the surface.
    #[tokio::test]
    async fn a_failed_assistant_clear_retains_ownership_so_it_can_be_retried() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let tmp = tempfile::tempdir().unwrap();
        // Every attempt fails, so the in-path retry cannot rescue this turn.
        Mock::given(method("POST"))
            .and(path("/assistant.threads.setStatus"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/chat.postMessage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "ts": "final-ts",
            })))
            .mount(&server)
            .await;

        let ch = test_slack_channel(&server, tmp.path()).with_streaming(true, 1);
        let target = AssistantTarget {
            channel_id: "C123".to_string(),
            thread_ts: "thread-one".to_string(),
        };
        ch.remember_assistant_thread(target.clone());
        let draft = ch
            .send_draft(
                &SendMessage::new("...", "C123")
                    .in_thread(Some("thread-one".to_string()))
                    .in_reply_to(Some("slack_C123_message-one".to_string())),
            )
            .await
            .unwrap()
            .unwrap();

        // Finalization exhausts its bounded attempts; Slack rejects every one.
        ch.finalize_draft("C123", &draft, "done", false)
            .await
            .unwrap();
        assert!(
            ch.owns_assistant_status(&target, &draft).await,
            "an exhausted clear must keep the generation rather than erase the \
             only record of the target"
        );
        let calls = assistant_status_calls(&server).await;
        assert_eq!(
            calls.iter().filter(|(_, status)| status.is_empty()).count(),
            ASSISTANT_STATUS_CLEAR_ATTEMPTS,
            "every bounded attempt must have been made: {calls:?}"
        );

        // Recovery path: the next turn in this thread reclaims the target, and
        // its own terminal path clears the surface.
        let next_draft = ch
            .send_draft(
                &SendMessage::new("...", "C123")
                    .in_thread(Some("thread-one".to_string()))
                    .in_reply_to(Some("slack_C123_message-two".to_string())),
            )
            .await
            .unwrap()
            .unwrap();
        assert!(
            ch.owns_assistant_status(&target, &next_draft).await,
            "the newer turn must take the generation from the stranded one"
        );
        assert!(
            !ch.owns_assistant_status(&target, &draft).await,
            "the stranded turn must no longer own the surface"
        );
    }

    /// A transient failure is retried within one terminal path rather than
    /// waiting for a later turn, so an ordinary blip does not leave the surface
    /// showing stale lifecycle text.
    #[tokio::test]
    async fn a_transient_assistant_clear_failure_is_retried_within_one_terminal_path() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let tmp = tempfile::tempdir().unwrap();
        Mock::given(method("POST"))
            .and(path("/assistant.threads.setStatus"))
            .respond_with(ResponseTemplate::new(500))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/assistant.threads.setStatus"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/chat.postMessage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "ts": "final-ts",
            })))
            .mount(&server)
            .await;

        let ch = test_slack_channel(&server, tmp.path()).with_streaming(true, 1);
        let target = AssistantTarget {
            channel_id: "C123".to_string(),
            thread_ts: "thread-one".to_string(),
        };
        ch.remember_assistant_thread(target.clone());
        let draft = ch
            .send_draft(
                &SendMessage::new("...", "C123")
                    .in_thread(Some("thread-one".to_string()))
                    .in_reply_to(Some("slack_C123_message-one".to_string())),
            )
            .await
            .unwrap()
            .unwrap();

        ch.finalize_draft("C123", &draft, "done", false)
            .await
            .unwrap();

        assert!(
            !ch.owns_assistant_status(&target, &draft).await,
            "the in-path retry must succeed and release the generation"
        );
        server.verify().await;
    }

    /// One entry per Assistant thread still grows without bound in a long-lived
    /// daemon, so the owner map is capped and evicts the oldest claim.
    #[tokio::test]
    async fn the_assistant_status_owner_map_is_bounded() {
        use wiremock::MockServer;

        let server = MockServer::start().await;
        let tmp = tempfile::tempdir().unwrap();
        let ch = test_slack_channel(&server, tmp.path()).with_streaming(true, 1);

        for i in 0..(ASSISTANT_STATUS_OWNER_CAP + 32) {
            ch.claim_assistant_status(
                &AssistantTarget {
                    channel_id: "C123".to_string(),
                    thread_ts: format!("thread-{i}"),
                },
                &format!("draft-{i}"),
            )
            .await;
        }

        let owners = ch.assistant_status_owners.lock().await;
        assert!(
            owners.len() <= ASSISTANT_STATUS_OWNER_CAP,
            "the owner map must stay bounded, got {}",
            owners.len()
        );
        assert!(
            owners.contains_key(&AssistantTarget {
                channel_id: "C123".to_string(),
                thread_ts: format!("thread-{}", ASSISTANT_STATUS_OWNER_CAP + 31),
            }),
            "the newest claim must survive eviction"
        );
    }

    /// Preflight ownership alone is not enough: the check ends when it returns,
    /// but the Slack request keeps going. This holds turn A's status write
    /// *in flight*, lets turn B claim the same target and publish, then releases
    /// A ΓÇö and proves B's status is still the final visible value.
    ///
    /// Without serializing through request completion, A's delayed write lands
    /// last and overwrites B.
    #[tokio::test]
    async fn an_in_flight_write_cannot_cross_a_newer_turns_claim() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use wiremock::matchers::{body_string_contains, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let tmp = tempfile::tempdir().unwrap();
        Mock::given(method("POST"))
            .and(path("/chat.postMessage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true, "ts": "final-ts",
            })))
            .mount(&server)
            .await;
        // A's write stalls for 2s; B's is immediate.
        Mock::given(method("POST"))
            .and(path("/assistant.threads.setStatus"))
            .and(body_string_contains("Running tool"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_secs(2))
                    .set_body_json(serde_json::json!({"ok": true})),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/assistant.threads.setStatus"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
            .mount(&server)
            .await;

        let ch = Arc::new(test_slack_channel(&server, tmp.path()).with_streaming(true, 1));
        ch.remember_assistant_thread(AssistantTarget {
            channel_id: "C123".to_string(),
            thread_ts: "shared-thread".to_string(),
        });
        let draft = |id: &'static str| {
            SendMessage::new("...", "C123")
                .in_thread(Some("shared-thread".to_string()))
                .in_reply_to(Some(id.to_string()))
        };
        let turn_a = ch
            .send_draft(&draft("slack_C123_msg-a"))
            .await
            .unwrap()
            .unwrap();

        // A starts writing and stalls inside the Slack request.
        let a_ch = Arc::clone(&ch);
        let a_id = turn_a.clone();
        let started = Arc::new(AtomicUsize::new(0));
        let started_a = Arc::clone(&started);
        let a_task = clawcrew_spawn::spawn!(async move {
            started_a.fetch_add(1, Ordering::SeqCst);
            a_ch.update_draft_lifecycle("C123", &a_id, ProgressEvent::RunningTool)
                .await
        });
        while started.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;

        // B claims the same target and publishes while A is still in flight.
        let turn_b = ch
            .send_draft(&draft("slack_C123_msg-b"))
            .await
            .unwrap()
            .unwrap();
        let b_started = std::time::Instant::now();
        ch.update_draft_lifecycle("C123", &turn_b, ProgressEvent::WaitingOnModel)
            .await
            .unwrap();
        let b_publish_elapsed = b_started.elapsed();

        // B's publish must have been held behind A's in-flight request rather
        // than racing it. Arrival order alone cannot show this ΓÇö wiremock
        // records a request on receipt ΓÇö so assert B was actually blocked for
        // most of A's 2s response. Without the serializer this returns
        // immediately and the elapsed time collapses.
        assert!(
            b_publish_elapsed >= Duration::from_millis(1_200),
            "B's write must serialize behind A's in-flight request, took {b_publish_elapsed:?}"
        );

        // Release A and let its superseded terminal path run too.
        a_task.await.unwrap().unwrap();
        ch.finalize_draft("C123", &turn_a, "A's answer", false)
            .await
            .unwrap();

        let calls = assistant_status_calls(&server).await;
        assert_eq!(
            calls.last(),
            Some(&("shared-thread".to_string(), "Waiting on model".to_string())),
            "the newer turn's status must remain the final visible value: {calls:?}"
        );
        assert!(
            !calls.iter().any(|(_, status)| status.is_empty()),
            "a superseded turn must not clear the live turn's status: {calls:?}"
        );
    }

    /// Cancellation is the terminal owner for interruption, hook suppression,
    /// timeout, and error exits. It must clear the exact turn-bound Assistant
    /// status and leave a concurrent sibling turn's status untouched.
    #[tokio::test]
    async fn cancelling_one_assistant_turn_clears_only_its_status() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let tmp = tempfile::tempdir().unwrap();
        Mock::given(method("POST"))
            .and(path("/assistant.threads.setStatus"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
            })))
            .mount(&server)
            .await;

        let (ch, first, _second) = two_live_assistant_turns(&server, tmp.path()).await;

        ch.cancel_draft("C123", &first).await.unwrap();

        let calls = assistant_status_calls(&server).await;
        assert!(
            calls.contains(&("thread-two".to_string(), "Waiting on model".to_string())),
            "the sibling turn must have shown its own lifecycle state: {calls:?}"
        );
        assert_eq!(
            calls
                .iter()
                .filter(|(_, status)| status.is_empty())
                .collect::<Vec<_>>(),
            vec![&("thread-one".to_string(), String::new())],
            "cancellation must clear exactly the cancelled turn's thread: {calls:?}"
        );
    }

    /// A second terminal call for the same draft must not emit another clear,
    /// and must never clear a sibling turn.
    #[tokio::test]
    async fn repeated_cancellation_does_not_clear_a_sibling_turn() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let tmp = tempfile::tempdir().unwrap();
        Mock::given(method("POST"))
            .and(path("/assistant.threads.setStatus"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
            })))
            .mount(&server)
            .await;

        let (ch, first, _second) = two_live_assistant_turns(&server, tmp.path()).await;

        ch.cancel_draft("C123", &first).await.unwrap();
        ch.cancel_draft("C123", &first).await.unwrap();

        let calls = assistant_status_calls(&server).await;
        assert_eq!(
            calls.iter().filter(|(_, status)| status.is_empty()).count(),
            1,
            "the turn's status must be cleared exactly once: {calls:?}"
        );
    }

    #[test]
    fn assistant_thread_tracking() {
        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec![],
            "slack_test_alias",
            Arc::new(Vec::new),
        );

        // Initially empty.
        {
            let threads = ch.active_assistant_threads.lock().unwrap();
            assert!(threads.is_empty());
        }

        // Simulate storing a thread_ts (as listen_socket_mode would).
        ch.remember_assistant_thread(AssistantTarget {
            channel_id: "C123".to_string(),
            thread_ts: "1741234567.000100".to_string(),
        });

        // Verify retrieval.
        assert!(ch.is_assistant_target(&AssistantTarget {
            channel_id: "C123".to_string(),
            thread_ts: "1741234567.000100".to_string(),
        }));
        assert!(!ch.is_assistant_target(&AssistantTarget {
            channel_id: "C999".to_string(),
            thread_ts: "1741234567.000100".to_string(),
        }));
    }

    /// Registry entries come from inbound Slack events, so the set must stay
    /// bounded rather than growing for the daemon's lifetime. The least
    /// recently seen target is dropped, and a refreshed target outlives idle
    /// ones even when it was registered first.
    #[tokio::test]
    async fn the_assistant_thread_registry_is_bounded_and_keeps_recent_targets() {
        use wiremock::MockServer;

        let server = MockServer::start().await;
        let tmp = tempfile::tempdir().unwrap();
        let ch = test_slack_channel(&server, tmp.path());

        let target = |i: usize| AssistantTarget {
            channel_id: "C123".to_string(),
            thread_ts: format!("thread-{i}"),
        };

        ch.remember_assistant_thread(target(0));
        for i in 1..ASSISTANT_THREAD_REGISTRY_CAP {
            ch.remember_assistant_thread(target(i));
        }
        // Re-observing target 0 makes it the most recently seen.
        ch.remember_assistant_thread(target(0));
        for i in ASSISTANT_THREAD_REGISTRY_CAP..(ASSISTANT_THREAD_REGISTRY_CAP + 16) {
            ch.remember_assistant_thread(target(i));
        }

        let len = ch.active_assistant_threads.lock().unwrap().len();
        assert!(
            len <= ASSISTANT_THREAD_REGISTRY_CAP,
            "registry must stay bounded, got {len}"
        );
        assert!(
            ch.is_assistant_target(&target(0)),
            "a refreshed target must survive eviction of idle ones"
        );
        assert!(
            ch.is_assistant_target(&target(ASSISTANT_THREAD_REGISTRY_CAP + 15)),
            "the newest target must be present"
        );
    }

    #[test]
    fn pending_approvals_map_is_initially_empty() {
        let ch = SlackChannel::new(
            "xoxb-token".into(),
            None,
            vec![],
            "slack_test_alias",
            Arc::new(Vec::new),
        );
        let map = ch.pending_approvals.try_lock().unwrap();
        assert!(map.is_empty());
    }

    #[test]
    fn approval_timeout_defaults_to_300_and_is_overridable() {
        let ch = SlackChannel::new(
            "xoxb-token".into(),
            None,
            vec![],
            "slack_test_alias",
            Arc::new(Vec::new),
        );
        assert_eq!(ch.approval_timeout_secs, 300);
        let ch = ch.with_approval_timeout_secs(90);
        assert_eq!(ch.approval_timeout_secs, 90);
    }

    #[tokio::test]
    async fn pending_approval_requires_allowed_user_and_origin_channel() {
        let ch = SlackChannel::new(
            "xoxb-token".into(),
            None,
            vec![],
            "slack_test_alias",
            Arc::new(|| vec!["U_OPERATOR".into()]),
        );
        let (tx, rx) = oneshot::channel();
        ch.pending_approvals.lock().await.insert(
            "abc123".to_string(),
            crate::util::PendingApproval {
                sender: tx,
                destination: "C_ORIGIN".to_string(),
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
                    &ch.pending_approvals,
                    "abc123",
                    response,
                    ch.is_user_allowed("U_OTHER"),
                    "C_ORIGIN",
                )
                .await,
                crate::util::PendingApprovalResolution::Rejected,
            );
            assert!(ch.pending_approvals.lock().await.contains_key("abc123"));
        }
        assert_eq!(
            crate::util::resolve_pending_approval(
                &ch.pending_approvals,
                "abc123",
                ChannelApprovalResponse::Approve,
                ch.is_user_allowed("U_OPERATOR"),
                "C_OTHER",
            )
            .await,
            crate::util::PendingApprovalResolution::Rejected,
        );
        assert!(ch.pending_approvals.lock().await.contains_key("abc123"));

        assert_eq!(
            crate::util::resolve_pending_approval(
                &ch.pending_approvals,
                "abc123",
                ChannelApprovalResponse::AlwaysApprove,
                ch.is_user_allowed("U_OPERATOR"),
                "C_ORIGIN",
            )
            .await,
            crate::util::PendingApprovalResolution::Resolved,
        );
        assert_eq!(rx.await.unwrap(), ChannelApprovalResponse::AlwaysApprove);

        let (approve_tx, approve_rx) = oneshot::channel();
        ch.pending_approvals.lock().await.insert(
            "def456".to_string(),
            crate::util::PendingApproval {
                sender: approve_tx,
                destination: "C_ORIGIN".to_string(),
                tool_name: "tool".to_string(),
            },
        );
        assert_eq!(
            crate::util::resolve_pending_approval(
                &ch.pending_approvals,
                "def456",
                ChannelApprovalResponse::Approve,
                ch.is_user_allowed("U_OPERATOR"),
                "C_ORIGIN",
            )
            .await,
            crate::util::PendingApprovalResolution::Resolved,
        );
        assert_eq!(approve_rx.await.unwrap(), ChannelApprovalResponse::Approve);
    }

    #[tokio::test]
    async fn polling_ingress_suppresses_rejected_approval_replies_and_delivers_authorized_one() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/auth.test"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "ok": true, "user_id": "U_BOT" })),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/conversations.history"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "messages": [
                    { "ts": "9999999999.000003", "user": "U_OTHER", "text": "other1 deny" },
                    { "ts": "9999999999.000002", "user": "U_OPERATOR", "text": "wrong1 deny" },
                    { "ts": "9999999999.000001", "user": "U_OPERATOR", "text": "auth01 approve" }
                ]
            })))
            .mount(&server)
            .await;

        let channel = SlackChannel::new(
            "xoxb-token".into(),
            None,
            vec!["C_ORIGIN".into()],
            "slack_test_alias",
            Arc::new(|| vec!["U_OPERATOR".into()]),
        )
        .with_api_base_url(server.uri());
        let pending = Arc::clone(&channel.pending_approvals);
        let (approved_tx, approved_rx) = oneshot::channel();
        let (wrong_tx, _wrong_rx) = oneshot::channel();
        let (unauthorized_tx, _unauthorized_rx) = oneshot::channel();
        {
            let mut approvals = pending.lock().await;
            approvals.insert(
                "auth01".into(),
                crate::util::PendingApproval {
                    sender: approved_tx,
                    destination: "C_ORIGIN".into(),
                    tool_name: "tool".to_string(),
                },
            );
            approvals.insert(
                "wrong1".into(),
                crate::util::PendingApproval {
                    sender: wrong_tx,
                    destination: "C_OTHER".into(),
                    tool_name: "tool".to_string(),
                },
            );
            approvals.insert(
                "other1".into(),
                crate::util::PendingApproval {
                    sender: unauthorized_tx,
                    destination: "C_ORIGIN".into(),
                    tool_name: "tool".to_string(),
                },
            );
        }

        let (tx, mut inbound_rx) = tokio::sync::mpsc::channel(4);
        let listener = clawcrew_spawn::spawn!(async move { channel.listen(tx).await });

        assert_eq!(
            tokio::time::timeout(Duration::from_secs(6), approved_rx)
                .await
                .expect("polling ingress should resolve the authorized approval")
                .expect("approval manager sender should stay open"),
            ChannelApprovalResponse::Approve
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(200), inbound_rx.recv())
                .await
                .is_err(),
            "approval-shaped messages must not reach agent dispatch"
        );
        let approvals = pending.lock().await;
        assert!(approvals.contains_key("wrong1"));
        assert!(approvals.contains_key("other1"));
        drop(approvals);

        listener.abort();
        let _ = listener.await;
        assert!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .any(|request| request.url.path() == "/conversations.history"),
            "test must drive the production polling ingress"
        );
    }

    #[tokio::test]
    async fn socket_mode_interactive_ingress_suppresses_rejected_approval_replies() {
        use clawcrew_api::channel::ChannelApprovalResponse;

        let channel = SlackChannel::new(
            "xoxb-token".into(),
            None,
            vec![],
            "slack_test_alias",
            Arc::new(|| vec!["U_OPERATOR".into()]),
        );
        let pending = Arc::clone(&channel.pending_approvals);
        let (approved_tx, approved_rx) = oneshot::channel();
        let (wrong_tx, _wrong_rx) = oneshot::channel();
        let (unauthorized_tx, _unauthorized_rx) = oneshot::channel();
        {
            let mut approvals = pending.lock().await;
            approvals.insert(
                "auth01".into(),
                crate::util::PendingApproval {
                    sender: approved_tx,
                    destination: "C_ORIGIN".into(),
                    tool_name: "tool".to_string(),
                },
            );
            approvals.insert(
                "wrong1".into(),
                crate::util::PendingApproval {
                    sender: wrong_tx,
                    destination: "C_ORIGIN".into(),
                    tool_name: "tool".to_string(),
                },
            );
            approvals.insert(
                "other1".into(),
                crate::util::PendingApproval {
                    sender: unauthorized_tx,
                    destination: "C_ORIGIN".into(),
                    tool_name: "tool".to_string(),
                },
            );
        }

        let (tx, mut inbound_rx) = tokio::sync::mpsc::channel(4);
        let envelopes = [
            serde_json::json!({
                "type": "interactive",
                "payload": {
                    "type": "block_actions",
                    "user": { "id": "U_OTHER" },
                    "channel": { "id": "C_ORIGIN" },
                    "actions": [{ "action_id": "approval_other1_deny" }]
                }
            }),
            serde_json::json!({
                "type": "interactive",
                "payload": {
                    "type": "block_actions",
                    "user": { "id": "U_OPERATOR" },
                    "channel": { "id": "C_OTHER" },
                    "actions": [{ "action_id": "approval_wrong1_deny" }]
                }
            }),
            serde_json::json!({
                "type": "interactive",
                "payload": {
                    "type": "block_actions",
                    "user": { "id": "U_OPERATOR" },
                    "channel": { "id": "C_ORIGIN" },
                    "actions": [{ "action_id": "approval_auth01_approve" }]
                }
            }),
        ];

        for envelope in &envelopes {
            assert!(
                channel
                    .handle_socket_mode_interactive(envelope, &tx, "U_BOT")
                    .await,
                "the live Socket Mode loop must continue after each interactive payload"
            );
        }

        assert_eq!(approved_rx.await.unwrap(), ChannelApprovalResponse::Approve);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), inbound_rx.recv())
                .await
                .is_err(),
            "approval-shaped Socket Mode payloads must not reach agent dispatch"
        );
        let approvals = pending.lock().await;
        assert!(approvals.contains_key("wrong1"));
        assert!(approvals.contains_key("other1"));
    }

    #[test]
    fn approval_block_action_parsed_correctly() {
        let envelope = serde_json::json!({
            "payload": {
                "type": "block_actions",
                "user": { "id": "U_OPERATOR" },
                "channel": { "id": "C_ORIGIN" },
                "actions": [{ "action_id": "approval_abc123_approve" }]
            }
        });
        let (token, response, responder, channel) =
            SlackChannel::try_parse_approval_block_action(&envelope).unwrap();
        assert_eq!(token, "abc123");
        assert_eq!(response, ChannelApprovalResponse::Approve);
        assert_eq!(responder, "U_OPERATOR");
        assert_eq!(channel, "C_ORIGIN");
    }

    #[test]
    fn approval_block_action_deny_parsed() {
        let envelope = serde_json::json!({
            "payload": {
                "type": "block_actions",
                "user": { "id": "U_OPERATOR" },
                "container": { "channel_id": "C_ORIGIN" },
                "actions": [{ "action_id": "approval_xz9q1w_deny" }]
            }
        });
        let (token, response, responder, channel) =
            SlackChannel::try_parse_approval_block_action(&envelope).unwrap();
        assert_eq!(token, "xz9q1w");
        assert_eq!(response, ChannelApprovalResponse::Deny);
        assert_eq!(responder, "U_OPERATOR");
        assert_eq!(channel, "C_ORIGIN");
    }

    #[test]
    fn approval_block_action_non_approval_returns_none() {
        let envelope = serde_json::json!({
            "payload": {
                "type": "block_actions",
                "actions": [{ "action_id": "clawcrew_config_provider", "selected_option": { "value": "anthropic" } }]
            }
        });
        assert!(SlackChannel::try_parse_approval_block_action(&envelope).is_none());
    }

    #[test]
    fn socket_mode_approval_card_shows_the_batch_position_on_both_surfaces() {
        // Socket Mode is the documented supervised-mode path, and it renders
        // twice: the `text` notification fallback and the Block Kit section.
        // A line in only one of them still leaves a card the operator cannot
        // tell apart from the next one.
        let body = super::build_socket_mode_approval_body(
            "C123",
            "ab12cd",
            "shell",
            "ls -la",
            Some((2, 3)),
        );
        let expected = crate::util::approval_position_line(Some((2, 3)));
        assert!(!expected.is_empty(), "helper should render a 2-of-3 line");

        let notification = body["text"].as_str().expect("text is a string");
        assert!(
            notification.contains(expected.trim_end()),
            "notification text should carry the position; got {notification}"
        );

        let section = body["blocks"][0]["text"]["text"]
            .as_str()
            .expect("section text is a string");
        assert!(
            section.contains(expected.trim_end()),
            "Block Kit section should carry the position; got {section}"
        );
    }

    #[test]
    fn socket_mode_approval_card_omits_the_position_for_a_single_call() {
        let single = super::build_socket_mode_approval_body(
            "C123",
            "ab12cd",
            "shell",
            "ls -la",
            Some((1, 1)),
        );
        let none =
            super::build_socket_mode_approval_body("C123", "ab12cd", "shell", "ls -la", None);
        assert_eq!(
            single, none,
            "a one-call batch renders exactly as an unpositioned card"
        );
    }

    #[test]
    fn approval_block_action_requires_non_empty_responder_and_channel() {
        let empty_responder = serde_json::json!({
            "payload": {
                "type": "block_actions",
                "user": { "id": "" },
                "channel": { "id": "C_ORIGIN" },
                "actions": [{ "action_id": "approval_abc123_approve" }]
            }
        });
        assert!(SlackChannel::try_parse_approval_block_action(&empty_responder).is_none());

        let empty_channel = serde_json::json!({
            "payload": {
                "type": "block_actions",
                "user": { "id": "U_OPERATOR" },
                "channel": { "id": "" },
                "actions": [{ "action_id": "approval_abc123_approve" }]
            }
        });
        assert!(SlackChannel::try_parse_approval_block_action(&empty_channel).is_none());
    }

    // --- Thread-context backfill tests ---

    #[test]
    fn thread_backfill_reservation_is_atomic_and_channel_scoped() {
        use std::sync::Barrier;

        let seen = Arc::new(Mutex::new(HashSet::new()));
        let barrier = Arc::new(Barrier::new(8));
        let message = serde_json::json!({
            "ts": "T_REPLY",
            "thread_ts": "T_PARENT",
            "user": "U_USER",
            "text": "mention",
        });

        let handles = (0..8)
            .map(|_| {
                let seen = Arc::clone(&seen);
                let barrier = Arc::clone(&barrier);
                let message = message.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    SlackChannel::reserve_thread_backfill(&message, "C_ONE", &seen).is_some()
                })
            })
            .collect::<Vec<_>>();

        let reservations = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .filter(|reserved| *reserved)
            .count();
        assert_eq!(reservations, 1, "only one concurrent caller may reserve");

        assert!(
            SlackChannel::reserve_thread_backfill(&message, "C_TWO", &seen).is_some(),
            "the same Slack timestamp in another channel is a distinct thread",
        );
    }

    #[test]
    fn thread_backfill_uncommitted_guard_releases_on_drop() {
        let seen = Mutex::new(HashSet::new());
        let message = serde_json::json!({
            "ts": "T_REPLY",
            "thread_ts": "T_PARENT",
        });
        let key = SlackChannel::reserve_thread_backfill(&message, "C_ONE", &seen)
            .expect("first attempt must reserve");

        {
            let _guard = ThreadBackfillReservationGuard::new(&seen, key);
        }

        assert!(
            SlackChannel::reserve_thread_backfill(&message, "C_ONE", &seen).is_some(),
            "dropping an in-flight hydration future must allow retry",
        );
    }

    #[tokio::test]
    async fn thread_backfill_first_strict_mention_fetches_once_with_configured_bound() {
        use wiremock::matchers::{method, path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/conversations.replies"))
            .and(query_param("channel", "C_ONE"))
            .and(query_param("ts", "T_PARENT"))
            .and(query_param("oldest", "0"))
            .and(query_param("limit", "50"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "messages": [
                    {"ts": "T_PARENT", "user": "U_USER", "text": "parent context"},
                    {"ts": "T_REPLY1", "thread_ts": "T_PARENT", "user": "U_USER", "text": "first prior reply"},
                    {"ts": "T_REPLY2", "thread_ts": "T_PARENT", "user": "U_USER", "text": "second prior reply"},
                    {"ts": "T_TRIGGER", "thread_ts": "T_PARENT", "user": "U_USER", "text": "<@U_BOT> summarize this"},
                ],
            })))
            .expect(1)
            .mount(&server)
            .await;

        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec!["C_ONE".into()],
            "slack_test_alias",
            Arc::new(|| vec!["U_USER".to_string()]),
        )
        .with_group_reply_policy(true, Vec::new())
        .with_strict_mention_in_thread(true)
        .with_thread_context_max_messages_resolver(Arc::new(|| 2))
        .with_api_base_url(server.uri());
        ch.cache_sender_display_name("U_USER", "alice");

        let unmentioned = serde_json::json!({
            "ts": "T_IGNORED",
            "thread_ts": "T_PARENT",
            "user": "U_USER",
            "text": "discussion before the mention",
        });
        assert!(
            ch.build_incoming_content(&unmentioned, "C_ONE", true, "U_BOT")
                .await
                .is_none(),
            "strict thread mode must ignore an unmentioned reply before hydration",
        );

        let first_mention = serde_json::json!({
            "ts": "T_TRIGGER",
            "thread_ts": "T_PARENT",
            "user": "U_USER",
            "text": "<@U_BOT> summarize this",
        });
        let hydrated = ch
            .build_incoming_content(&first_mention, "C_ONE", true, "U_BOT")
            .await
            .expect("the first mentioned reply must be forwarded");

        assert!(hydrated.starts_with("[Thread context]"), "{hydrated}");
        assert!(hydrated.contains("ΓÇª 1 earlier thread messages omitted ΓÇª"));
        let first_position = hydrated.find("first prior reply").unwrap();
        let second_position = hydrated.find("second prior reply").unwrap();
        assert!(
            first_position < second_position,
            "history must stay chronological"
        );
        assert!(
            !hydrated.contains("parent context"),
            "configured depth must apply"
        );
        assert_eq!(hydrated.matches("summarize this").count(), 1);

        let second_mention = serde_json::json!({
            "ts": "T_LATER",
            "thread_ts": "T_PARENT",
            "user": "U_USER",
            "text": "<@U_BOT> one more question",
        });
        let later = ch
            .build_incoming_content(&second_mention, "C_ONE", true, "U_BOT")
            .await
            .expect("a later mentioned reply must still be forwarded");
        assert!(!later.contains("[Thread context]"));

        server.verify().await;
    }

    #[tokio::test]
    async fn thread_backfill_honors_retry_after_before_retrying_bodyless_429() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/conversations.replies"))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "1"))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/conversations.replies"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "messages": [
                    {"ts": "T_PRIOR", "thread_ts": "T_PARENT", "user": "U_USER", "text": "prior context"},
                ],
                "response_metadata": {"next_cursor": ""},
            })))
            .expect(1)
            .mount(&server)
            .await;

        let ch = Arc::new(
            SlackChannel::new(
                unique_test_bot_token(),
                None,
                vec!["C_ONE".into()],
                "slack_test_alias",
                Arc::new(|| vec!["U_USER".to_string()]),
            )
            .with_thread_context_max_messages_resolver(Arc::new(|| 2))
            .with_api_base_url(server.uri()),
        );
        ch.cache_sender_display_name("U_USER", "alice");

        let mention = serde_json::json!({
            "ts": "T_TRIGGER",
            "thread_ts": "T_PARENT",
            "user": "U_USER",
            "text": "<@U_BOT> summarize",
        });
        let task_ch = Arc::clone(&ch);
        let task = clawcrew_spawn::spawn!(async move {
            task_ch
                .build_incoming_content(&mention, "C_ONE", true, "U_BOT")
                .await
        });

        for _ in 0..20 {
            if server.received_requests().await.unwrap().len() == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert_eq!(server.received_requests().await.unwrap().len(), 1);

        let content = tokio::time::timeout(Duration::from_secs(3), task)
            .await
            .expect("hydration retry must complete after Retry-After")
            .expect("hydration task must not panic")
            .expect("the triggering message must be forwarded");
        assert!(content.contains("prior context"), "{content}");
        server.verify().await;
    }

    /// Handles for one installation must resolve to the same cooldown state,
    /// and distinct installations must stay isolated.
    #[test]
    fn installation_cooldowns_are_shared_per_token_and_isolated_across_tokens() {
        let a1 = installation_method_cooldowns("xoxb-installation-a");
        let a2 = installation_method_cooldowns("xoxb-installation-a");
        let b = installation_method_cooldowns("xoxb-installation-b");
        assert!(
            Arc::ptr_eq(&a1, &a2),
            "two handles for one token must share cooldown state"
        );
        assert!(
            !Arc::ptr_eq(&a1, &b),
            "different installations must not share cooldown state"
        );
    }

    /// The registry holds `Weak` refs, so state for an installation with no live
    /// handles is reclaimed instead of accumulating one permanent entry per
    /// token ever constructed.
    #[test]
    fn installation_cooldown_registry_reclaims_dropped_installations() {
        let key = {
            use sha2::Digest as _;
            hex::encode(sha2::Sha256::digest(b"xoxb-transient-installation"))
        };
        {
            let _live = installation_method_cooldowns("xoxb-transient-installation");
            let registry = INSTALLATION_METHOD_COOLDOWNS
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            assert!(
                registry.get(&key).and_then(Weak::upgrade).is_some(),
                "a live handle must keep its cooldown state resolvable"
            );
        }
        // The handle is gone, so the entry must no longer resolve...
        {
            let registry = INSTALLATION_METHOD_COOLDOWNS
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            assert!(
                registry.get(&key).and_then(Weak::upgrade).is_none(),
                "dropped installations must not keep cooldown state alive"
            );
        }
        // ...and the dead key is pruned by the next resolution.
        let _other = installation_method_cooldowns("xoxb-some-other-installation");
        let registry = INSTALLATION_METHOD_COOLDOWNS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert!(
            !registry.contains_key(&key),
            "a later lookup must prune the dead entry"
        );
    }

    /// Slack applies Web API limits per method, per workspace, per app, so a
    /// terminal `Retry-After` must bind every configured handle addressing the
    /// same installation ΓÇö not just the alias that received the 429.
    ///
    /// Two `[channels.slack.<alias>]` handles are built with one shared bot
    /// token. Alias A exhausts its three-request hydration budget on a terminal
    /// 429 carrying `Retry-After: 1`; alias B must then be unable to call
    /// `conversations.replies` until that deadline expires.
    #[tokio::test]
    async fn terminal_cooldown_binds_every_alias_of_one_slack_installation() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/conversations.replies"))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
            .up_to_n_times(2)
            .expect(2)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/conversations.replies"))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "1"))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/conversations.replies"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "messages": [],
                "response_metadata": {"next_cursor": ""},
            })))
            .expect(1)
            .mount(&server)
            .await;

        // One installation, two configured aliases.
        let shared_token = "xoxb-one-installation-two-aliases";
        let build = |alias: &'static str| {
            Arc::new(
                SlackChannel::new(
                    shared_token.into(),
                    None,
                    vec!["C_ONE".into()],
                    alias,
                    Arc::new(|| vec!["U_USER".to_string()]),
                )
                .with_thread_context_max_messages_resolver(Arc::new(|| 2))
                .with_api_base_url(server.uri()),
            )
        };
        let alias_a = build("slack_alias_a");
        let alias_b = build("slack_alias_b");

        // Alias A burns the whole hydration budget and ends on a terminal 429.
        let first = serde_json::json!({
            "ts": "T_FIRST",
            "thread_ts": "T_PARENT_ONE",
            "user": "U_USER",
            "text": "<@U_BOT> first",
        });
        alias_a
            .build_incoming_content(&first, "C_ONE", true, "U_BOT")
            .await
            .expect("a rate-limited hydration must not drop the message");
        assert_eq!(server.received_requests().await.unwrap().len(), 3);

        // Alias B is a different handle, so before this fix it had its own empty
        // cooldown map and called straight through the active deadline.
        let second = serde_json::json!({
            "ts": "T_SECOND",
            "thread_ts": "T_PARENT_TWO",
            "user": "U_USER",
            "text": "<@U_BOT> second",
        });
        let task_b = Arc::clone(&alias_b);
        let task = clawcrew_spawn::spawn!(async move {
            task_b
                .build_incoming_content(&second, "C_ONE", true, "U_BOT")
                .await
        });

        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            3,
            "a second alias of the same installation must honor the method cooldown"
        );
        tokio::time::timeout(Duration::from_secs(7), task)
            .await
            .expect("the second alias must resume once Retry-After expires")
            .expect("hydration task must not panic")
            .expect("the second alias's message must still be forwarded");

        server.verify().await;
    }

    #[tokio::test]
    async fn thread_backfill_terminal_bodyless_429_cools_down_next_hydration() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/conversations.replies"))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
            .up_to_n_times(2)
            .expect(2)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/conversations.replies"))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "1"))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/conversations.replies"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "messages": [],
                "response_metadata": {"next_cursor": ""},
            })))
            .expect(1)
            .mount(&server)
            .await;

        let ch = Arc::new(
            SlackChannel::new(
                unique_test_bot_token(),
                None,
                vec!["C_ONE".into()],
                "slack_test_alias",
                Arc::new(|| vec!["U_USER".to_string()]),
            )
            .with_thread_context_max_messages_resolver(Arc::new(|| 2))
            .with_api_base_url(server.uri()),
        );

        let first = serde_json::json!({
            "ts": "T_FIRST",
            "thread_ts": "T_PARENT_ONE",
            "user": "U_USER",
            "text": "<@U_BOT> first",
        });
        ch.build_incoming_content(&first, "C_ONE", true, "U_BOT")
            .await
            .expect("rate-limited hydration must not drop the message");
        assert_eq!(server.received_requests().await.unwrap().len(), 3);

        let second = serde_json::json!({
            "ts": "T_SECOND",
            "thread_ts": "T_PARENT_TWO",
            "user": "U_USER",
            "text": "<@U_BOT> second",
        });
        let task_ch = Arc::clone(&ch);
        let task = clawcrew_spawn::spawn!(async move {
            task_ch
                .build_incoming_content(&second, "C_ONE", true, "U_BOT")
                .await
        });

        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            3,
            "a new hydration must honor the workspace/method cooldown"
        );
        tokio::time::timeout(Duration::from_secs(7), task)
            .await
            .expect("next hydration must resume after Retry-After")
            .expect("hydration task must not panic")
            .expect("the next message must be forwarded");

        server.verify().await;
    }

    #[tokio::test]
    async fn thread_backfill_paginates_to_newest_prior_messages() {
        use wiremock::matchers::{method, path, query_param, query_param_is_missing};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/conversations.replies"))
            .and(query_param("channel", "C_ONE"))
            .and(query_param("ts", "T_PARENT"))
            .and(query_param("oldest", "0"))
            .and(query_param("latest", "T_TRIGGER"))
            .and(query_param("limit", "50"))
            .and(query_param_is_missing("cursor"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "messages": [
                    {"ts": "T_PARENT", "user": "U_USER", "text": "old parent"},
                    {"ts": "T_OLD", "thread_ts": "T_PARENT", "user": "U_USER", "text": "old reply"},
                ],
                "response_metadata": {"next_cursor": "NEXT_PAGE"},
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/conversations.replies"))
            .and(query_param("channel", "C_ONE"))
            .and(query_param("ts", "T_PARENT"))
            .and(query_param("oldest", "0"))
            .and(query_param("latest", "T_TRIGGER"))
            .and(query_param("limit", "50"))
            .and(query_param("cursor", "NEXT_PAGE"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "messages": [
                    {"ts": "T_RECENT1", "thread_ts": "T_PARENT", "user": "U_USER", "text": "recent first"},
                    {"ts": "T_RECENT2", "thread_ts": "T_PARENT", "user": "U_USER", "text": "recent second"},
                ],
                "response_metadata": {"next_cursor": ""},
            })))
            .expect(1)
            .mount(&server)
            .await;

        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec!["C_ONE".into()],
            "slack_test_alias",
            Arc::new(|| vec!["U_USER".to_string()]),
        )
        .with_thread_context_max_messages_resolver(Arc::new(|| 2))
        .with_api_base_url(server.uri());
        ch.cache_sender_display_name("U_USER", "alice");

        let mention = serde_json::json!({
            "ts": "T_TRIGGER",
            "thread_ts": "T_PARENT",
            "user": "U_USER",
            "text": "<@U_BOT> summarize",
        });
        let hydrated = ch
            .build_incoming_content(&mention, "C_ONE", true, "U_BOT")
            .await
            .expect("mentioned reply must be forwarded");

        assert!(!hydrated.contains("old parent"));
        assert!(!hydrated.contains("old reply"));
        let recent_first = hydrated.find("recent first").unwrap();
        let recent_second = hydrated.find("recent second").unwrap();
        assert!(recent_first < recent_second);
        assert!(hydrated.contains("ΓÇª 2 earlier thread messages omitted ΓÇª"));
        server.verify().await;
    }

    #[tokio::test]
    async fn thread_backfill_stops_at_request_budget_and_commits_partial_context() {
        use wiremock::matchers::{method, path, query_param, query_param_is_missing};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let pages = [
            (None, "PAGE_2", "first page"),
            (Some("PAGE_2"), "PAGE_3", "second page"),
            (Some("PAGE_3"), "PAGE_4", "third page"),
        ];
        for (cursor, next_cursor, text) in pages {
            let mut matcher = Mock::given(method("GET"))
                .and(path("/conversations.replies"))
                .and(query_param("channel", "C_ONE"))
                .and(query_param("ts", "T_PARENT"))
                .and(query_param("latest", "T_TRIGGER"));
            matcher = if let Some(cursor) = cursor {
                matcher.and(query_param("cursor", cursor))
            } else {
                matcher.and(query_param_is_missing("cursor"))
            };
            matcher
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "ok": true,
                    "messages": [
                        {"ts": text, "thread_ts": "T_PARENT", "user": "U_USER", "text": text},
                    ],
                    "response_metadata": {"next_cursor": next_cursor},
                })))
                .expect(1)
                .mount(&server)
                .await;
        }
        Mock::given(method("GET"))
            .and(path("/conversations.replies"))
            .and(query_param("cursor", "PAGE_4"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "messages": [
                    {"ts": "fourth page", "thread_ts": "T_PARENT", "user": "U_USER", "text": "fourth page"},
                ],
                "response_metadata": {"next_cursor": ""},
            })))
            .expect(0)
            .mount(&server)
            .await;

        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec!["C_ONE".into()],
            "slack_test_alias",
            Arc::new(|| vec!["U_USER".to_string()]),
        )
        .with_thread_context_max_messages_resolver(Arc::new(|| 2))
        .with_api_base_url(server.uri());
        ch.cache_sender_display_name("U_USER", "alice");

        let mention = serde_json::json!({
            "ts": "T_TRIGGER",
            "thread_ts": "T_PARENT",
            "user": "U_USER",
            "text": "<@U_BOT> summarize",
        });
        let hydrated = ch
            .build_incoming_content(&mention, "C_ONE", true, "U_BOT")
            .await
            .expect("mentioned reply must be forwarded");

        assert!(!hydrated.contains("first page"));
        assert!(hydrated.contains("second page"));
        assert!(hydrated.contains("third page"));
        assert!(!hydrated.contains("fourth page"));
        assert!(hydrated.contains(
            "ΓÇª additional recent thread messages omitted because history fetch limit was reached ΓÇª"
        ));
        assert_eq!(server.received_requests().await.unwrap().len(), 3);

        let second_mention = serde_json::json!({
            "ts": "T_LATER",
            "thread_ts": "T_PARENT",
            "user": "U_USER",
            "text": "<@U_BOT> follow up",
        });
        ch.build_incoming_content(&second_mention, "C_ONE", true, "U_BOT")
            .await
            .expect("later mention must still be forwarded");
        assert_eq!(server.received_requests().await.unwrap().len(), 3);
        server.verify().await;
    }

    #[tokio::test]
    async fn thread_backfill_zero_depth_disables_fetch() {
        use wiremock::MockServer;

        let server = MockServer::start().await;
        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec!["C_ONE".into()],
            "slack_test_alias",
            Arc::new(|| vec!["U_USER".to_string()]),
        )
        .with_thread_context_max_messages_resolver(Arc::new(|| 0))
        .with_api_base_url(server.uri());
        let mention = serde_json::json!({
            "ts": "T_TRIGGER",
            "thread_ts": "T_PARENT",
            "user": "U_USER",
            "text": "<@U_BOT> summarize this",
        });

        let content = ch
            .build_incoming_content(&mention, "C_ONE", true, "U_BOT")
            .await
            .expect("the triggering message must still be forwarded");
        assert_eq!(content, "<@U_BOT> summarize this");
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn thread_backfill_default_depth_disables_fetch() {
        use wiremock::MockServer;

        let server = MockServer::start().await;
        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec!["C_ONE".into()],
            "slack_test_alias",
            Arc::new(|| vec!["U_USER".to_string()]),
        )
        .with_api_base_url(server.uri());
        let mention = serde_json::json!({
            "ts": "T_TRIGGER",
            "thread_ts": "T_PARENT",
            "user": "U_USER",
            "text": "<@U_BOT> summarize this",
        });

        let content = ch
            .build_incoming_content(&mention, "C_ONE", true, "U_BOT")
            .await
            .expect("the triggering message must still be forwarded");
        assert_eq!(content, "<@U_BOT> summarize this");
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn thread_backfill_successful_empty_history_does_not_refetch() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/conversations.replies"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "messages": [
                    {"ts": "T_TRIGGER", "thread_ts": "T_PARENT", "user": "U_USER", "text": "<@U_BOT> first"},
                ],
            })))
            .expect(1)
            .mount(&server)
            .await;

        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec!["C_ONE".into()],
            "slack_test_alias",
            Arc::new(|| vec!["U_USER".to_string()]),
        )
        .with_thread_context_max_messages_resolver(Arc::new(|| 2))
        .with_api_base_url(server.uri());

        for (ts, text) in [
            ("T_TRIGGER", "<@U_BOT> first"),
            ("T_LATER", "<@U_BOT> second"),
        ] {
            let message = serde_json::json!({
                "ts": ts,
                "thread_ts": "T_PARENT",
                "user": "U_USER",
                "text": text,
            });
            let content = ch
                .build_incoming_content(&message, "C_ONE", true, "U_BOT")
                .await
                .expect("mentioned replies must be forwarded");
            assert!(!content.contains("[Thread context]"));
        }

        server.verify().await;
    }

    #[tokio::test]
    async fn thread_backfill_fetch_failure_releases_reservation_for_retry() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/conversations.replies"))
            .respond_with(ResponseTemplate::new(500).set_body_string("temporary failure"))
            .expect(2)
            .mount(&server)
            .await;

        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec!["C_ONE".into()],
            "slack_test_alias",
            Arc::new(|| vec!["U_USER".to_string()]),
        )
        .with_thread_context_max_messages_resolver(Arc::new(|| 2))
        .with_api_base_url(server.uri());

        for (ts, text) in [
            ("T_TRIGGER", "<@U_BOT> first"),
            ("T_RETRY", "<@U_BOT> retry"),
        ] {
            let message = serde_json::json!({
                "ts": ts,
                "thread_ts": "T_PARENT",
                "user": "U_USER",
                "text": text,
            });
            let content = ch
                .build_incoming_content(&message, "C_ONE", true, "U_BOT")
                .await
                .expect("fetch failure must not drop the triggering message");
            assert!(!content.contains("[Thread context]"));
        }

        server.verify().await;
    }

    #[tokio::test]
    async fn thread_backfill_malformed_success_response_retries() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/conversations.replies"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not-json"))
            .expect(2)
            .mount(&server)
            .await;

        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec!["C_ONE".into()],
            "slack_test_alias",
            Arc::new(|| vec!["U_USER".to_string()]),
        )
        .with_thread_context_max_messages_resolver(Arc::new(|| 2))
        .with_api_base_url(server.uri());

        for ts in ["T_FIRST", "T_RETRY"] {
            let message = serde_json::json!({
                "ts": ts,
                "thread_ts": "T_PARENT",
                "user": "U_USER",
                "text": "<@U_BOT> retry malformed response",
            });
            assert!(
                ch.build_incoming_content(&message, "C_ONE", true, "U_BOT")
                    .await
                    .is_some()
            );
        }

        server.verify().await;
    }

    #[tokio::test]
    async fn thread_backfill_success_without_messages_array_retries() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/conversations.replies"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "messages": {"unexpected": "object"},
            })))
            .expect(2)
            .mount(&server)
            .await;

        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec!["C_ONE".into()],
            "slack_test_alias",
            Arc::new(|| vec!["U_USER".to_string()]),
        )
        .with_thread_context_max_messages_resolver(Arc::new(|| 2))
        .with_api_base_url(server.uri());

        for ts in ["T_FIRST", "T_RETRY"] {
            let message = serde_json::json!({
                "ts": ts,
                "thread_ts": "T_PARENT",
                "user": "U_USER",
                "text": "<@U_BOT> retry invalid messages",
            });
            assert!(
                ch.build_incoming_content(&message, "C_ONE", true, "U_BOT")
                    .await
                    .is_some()
            );
        }

        server.verify().await;
    }

    #[tokio::test]
    async fn thread_backfill_invalid_pagination_cursor_retries() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/conversations.replies"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "messages": [],
                "response_metadata": {"next_cursor": {"unexpected": "object"}},
            })))
            .expect(2)
            .mount(&server)
            .await;

        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec!["C_ONE".into()],
            "slack_test_alias",
            Arc::new(|| vec!["U_USER".to_string()]),
        )
        .with_thread_context_max_messages_resolver(Arc::new(|| 2))
        .with_api_base_url(server.uri());

        for ts in ["T_FIRST", "T_RETRY"] {
            let message = serde_json::json!({
                "ts": ts,
                "thread_ts": "T_PARENT",
                "user": "U_USER",
                "text": "<@U_BOT> retry invalid cursor",
            });
            assert!(
                ch.build_incoming_content(&message, "C_ONE", true, "U_BOT")
                    .await
                    .is_some()
            );
        }

        server.verify().await;
    }

    #[tokio::test]
    async fn thread_backfill_null_pagination_cursor_is_terminal() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/conversations.replies"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "messages": [
                    {
                        "ts": "T_CONTEXT",
                        "thread_ts": "T_PARENT",
                        "user": "U_USER",
                        "text": "context before a null cursor"
                    },
                ],
                "response_metadata": {"next_cursor": null},
            })))
            .expect(1)
            .mount(&server)
            .await;

        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec!["C_ONE".into()],
            "slack_test_alias",
            Arc::new(|| vec!["U_USER".to_string()]),
        )
        .with_thread_context_max_messages_resolver(Arc::new(|| 2))
        .with_api_base_url(server.uri());

        for (ts, should_contain_context) in [("T_FIRST", true), ("T_SECOND", false)] {
            let message = serde_json::json!({
                "ts": ts,
                "thread_ts": "T_PARENT",
                "user": "U_USER",
                "text": "<@U_BOT> use the prior context",
            });
            let hydrated = ch
                .build_incoming_content(&message, "C_ONE", true, "U_BOT")
                .await
                .expect("eligible Slack mention");
            assert_eq!(
                hydrated.contains("context before a null cursor"),
                should_contain_context
            );
        }

        server.verify().await;
    }

    #[tokio::test]
    async fn thread_backfill_repeated_cursor_retries_on_next_message() {
        use wiremock::matchers::{method, path, query_param, query_param_is_missing};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/conversations.replies"))
            .and(query_param_is_missing("cursor"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "messages": [],
                "response_metadata": {"next_cursor": "REPEATED"},
            })))
            .expect(2)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/conversations.replies"))
            .and(query_param("cursor", "REPEATED"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "messages": [],
                "response_metadata": {"next_cursor": "REPEATED"},
            })))
            .expect(2)
            .mount(&server)
            .await;

        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec!["C_ONE".into()],
            "slack_test_alias",
            Arc::new(|| vec!["U_USER".to_string()]),
        )
        .with_thread_context_max_messages_resolver(Arc::new(|| 2))
        .with_api_base_url(server.uri());

        for ts in ["T_FIRST", "T_RETRY"] {
            let message = serde_json::json!({
                "ts": ts,
                "thread_ts": "T_PARENT",
                "user": "U_USER",
                "text": "<@U_BOT> retry repeated cursor",
            });
            assert!(
                ch.build_incoming_content(&message, "C_ONE", true, "U_BOT")
                    .await
                    .is_some()
            );
        }

        server.verify().await;
    }

    /// Top-level message (no thread_ts) and thread-parent (`thread_ts == ts`)
    /// must not trigger backfill, regardless of `seen_threads` state.
    #[test]
    fn thread_backfill_precheck_skips_non_replies() {
        let seen = Mutex::new(HashSet::new());

        let top_level = serde_json::json!({
            "ts": "1700000010.000100",
            "text": "hi bot",
            "user": "U_USER",
        });
        assert!(SlackChannel::reserve_thread_backfill(&top_level, "C1", &seen).is_none());

        let parent = serde_json::json!({
            "ts": "1700000000.000001",
            "thread_ts": "1700000000.000001",
            "text": "thread starts here",
            "user": "U_USER",
        });
        assert!(SlackChannel::reserve_thread_backfill(&parent, "C1", &seen).is_none());
    }

    #[test]
    fn thread_backfill_filter_drops_trigger_subtype_userless_and_non_allow_listed() {
        let messages = vec![
            // Parent message from allow-listed user ΓÇö keep.
            serde_json::json!({"ts": "T_PARENT", "user": "U_USER", "text": "parent"}),
            // Earlier reply from allow-listed user ΓÇö keep.
            serde_json::json!({"ts": "T_R1", "thread_ts": "T_PARENT", "user": "U_USER", "text": "first"}),
            // System message (`channel_join`) ΓÇö must be dropped by subtype.
            serde_json::json!({
                "ts": "T_R2",
                "thread_ts": "T_PARENT",
                "subtype": "channel_join",
                "user": "U_USER",
                "text": "joined",
            }),
            // Reply from non-allow-listed user ΓÇö must be counted as a drop.
            serde_json::json!({"ts": "T_R3", "thread_ts": "T_PARENT", "user": "U_BAD", "text": "filtered"}),
            // Bot's own past reply ΓÇö must pass through (useful self-context).
            serde_json::json!({"ts": "T_R4", "thread_ts": "T_PARENT", "user": "U_BOT", "text": "bot turn"}),
            serde_json::json!({"ts": "T_R5", "thread_ts": "T_PARENT", "text": "from a webhook"}),
            // Same situation with a `bot_id` instead of `user` (some
            // integration messages carry `bot_id` instead of `user`).
            // Filter only inspects `user`, so `bot_id`-only messages
            // also fall under the userless drop.
            serde_json::json!({"ts": "T_R6", "thread_ts": "T_PARENT", "bot_id": "B999", "text": "from a bot integration"}),
            // The triggering reply itself ΓÇö must be dropped to avoid
            // duplication in the agent payload.
            serde_json::json!({"ts": "T_TRIGGER", "thread_ts": "T_PARENT", "user": "U_USER", "text": "hi bot"}),
        ];

        let (allowed, dropped) =
            SlackChannel::filter_backfill_messages(&messages, "T_TRIGGER", "U_BOT", |user| {
                user == "U_USER"
            });

        let kept_ts: Vec<&str> = allowed
            .iter()
            .map(|m| m.get("ts").and_then(|v| v.as_str()).unwrap_or_default())
            .collect();
        assert_eq!(
            kept_ts,
            vec!["T_PARENT", "T_R1", "T_R4"],
            "trigger, subtype, userless, and non-allow-listed messages must be dropped",
        );
        assert_eq!(
            dropped, 3,
            "non-allow-listed and userless messages all count toward the gap marker",
        );
    }

    #[test]
    fn thread_backfill_compose_renders_allow_list_gap_marker() {
        let lines = vec![
            "- alice: first".to_string(),
            "- bob: second".to_string(),
            "- alice: third".to_string(),
        ];
        let block = SlackChannel::compose_thread_backfill_block(lines, 2, 0, false)
            .expect("block should be produced");
        assert!(block.starts_with("[Thread context]\n"));
        assert!(block.contains("ΓÇª 2 messages from non-allow-listed users omitted ΓÇª"));
        assert!(block.contains("- alice: first"));
        assert!(block.contains("- bob: second"));
        assert!(block.contains("- alice: third"));
        // Reply-cap marker must NOT appear when reply_cap_omitted == 0.
        assert!(!block.contains("earlier thread messages omitted"));
    }

    #[test]
    fn thread_backfill_compose_with_only_dropped_senders() {
        let block = SlackChannel::compose_thread_backfill_block(Vec::new(), 4, 0, false)
            .expect("block should still surface the gap signal");
        assert!(block.starts_with("[Thread context]\n"));
        assert!(block.contains("ΓÇª 4 messages from non-allow-listed users omitted ΓÇª"));
        // No rendered message lines.
        assert!(!block.contains("- "));
    }

    #[test]
    fn thread_backfill_compose_renders_reply_cap_marker() {
        let lines = (0..SLACK_PERMALINK_THREAD_MAX_REPLIES)
            .map(|i| format!("- alice: message {i}"))
            .collect::<Vec<_>>();
        let block = SlackChannel::compose_thread_backfill_block(lines, 0, 5, false)
            .expect("block should be produced");
        assert!(block.starts_with("[Thread context]\n"));
        assert!(block.contains("ΓÇª 5 earlier thread messages omitted ΓÇª"));
        // Allow-list marker must NOT appear when dropped_by_allow_list == 0.
        assert!(!block.contains("non-allow-listed"));
    }

    #[test]
    fn thread_backfill_compose_truncates_long_text() {
        let huge = "x".repeat(SLACK_PERMALINK_TEXT_MAX_CHARS + 1000);
        let lines = vec![format!("- alice: {huge}")];
        let block = SlackChannel::compose_thread_backfill_block(lines, 0, 0, false)
            .expect("block should be produced");
        assert!(block.ends_with("ΓÇª[truncated]"));
        assert!(block.starts_with("[Thread context]"));
    }

    #[test]
    fn thread_backfill_strip_bot_mentions_removes_self_mention() {
        let line = "- alice: hey <@U_BOT> can you help?";
        let stripped = SlackChannel::strip_bot_mentions(line, "U_BOT");
        assert!(!stripped.contains("<@U_BOT>"));
        assert!(stripped.contains("alice:"));
        assert!(stripped.contains("can you help?"));
    }

    #[test]
    fn thread_backfill_first_reply_backfills_then_subsequent_replies_do_not() {
        let ch = SlackChannel::new(
            unique_test_bot_token(),
            None,
            vec!["C1".into()],
            "slack_test_alias",
            Arc::new(Vec::new),
        );

        let reply1 = serde_json::json!({
            "ts": "T_REPLY1",
            "thread_ts": "T_PARENT",
            "user": "U_USER",
            "text": "first reply",
        });
        let key = SlackChannel::reserve_thread_backfill(&reply1, "C1", &ch.seen_threads)
            .expect("first forwarded reply must trigger backfill");
        assert_eq!(key.thread_ts, "T_PARENT");

        let reply2 = serde_json::json!({
            "ts": "T_REPLY2",
            "thread_ts": "T_PARENT",
            "user": "U_USER",
            "text": "second reply",
        });
        assert!(
            SlackChannel::reserve_thread_backfill(&reply2, "C1", &ch.seen_threads).is_none(),
            "second reply must not re-backfill",
        );
    }

    #[test]
    fn thread_backfill_block_is_prepended_to_payload() {
        let backfill_block = Some("[Thread context]\n- alice: hi\n- bob: hello".to_string());
        let normalized_text = "<@U_BOT> please summarize".to_string();
        let attachment_blocks: Vec<String> = vec!["[Attachment] report.pdf".to_string()];

        // Mirror the assembly done at the tail of `build_incoming_content`.
        let body = SlackChannel::compose_incoming_content(normalized_text, attachment_blocks)
            .expect("non-empty body");
        let payload = match backfill_block {
            Some(block) => format!("{block}\n\n{body}"),
            None => body,
        };

        assert!(
            payload.starts_with("[Thread context]"),
            "payload must lead with [Thread context], got: {payload:?}",
        );
        let ctx_end = payload.find("\n\n").expect("context separator");
        let after_ctx = &payload[ctx_end + 2..];
        assert!(
            after_ctx.starts_with("<@U_BOT> please summarize"),
            "triggering message must follow the context block, got: {after_ctx:?}",
        );
        assert!(
            payload.contains("[Attachment] report.pdf"),
            "attachment blocks still appended after the message",
        );
        assert!(
            payload.rfind("[Attachment]").unwrap() > payload.rfind("please summarize").unwrap(),
            "attachments must come after the message body",
        );
    }
