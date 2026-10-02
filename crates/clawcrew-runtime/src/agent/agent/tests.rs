    use super::*;
    use async_trait::async_trait;
    use chrono::TimeZone;
    use parking_lot::Mutex;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use clawcrew_api::observability_traits::ObserverMetric;

    #[test]
    fn build_session_model_provider_rejects_undotted_ref() {
        let config = Config::default();
        let err = match build_session_model_provider(&config, "anthropic", Some("m")) {
            Ok(_) => panic!("undotted ref must error"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("<type>.<alias>"), "got: {err}");
    }

    #[test]
    fn build_session_model_provider_requires_a_model() {
        // No configured entry and no override → cannot resolve a model name.
        let config = Config::default();
        let err = match build_session_model_provider(&config, "anthropic.default", None) {
            Ok(_) => panic!("missing model must error"),
            Err(e) => e,
        };
        assert!(
            err.to_string().contains("no `model` configured"),
            "got: {err}"
        );
    }

    #[test]
    fn build_session_model_provider_returns_full_canonical_ref() {
        use clawcrew_config::schema::{ModelProviderConfig, OpenAIModelProviderConfig};

        let mut config = Config::default();
        config.providers.models.openai.insert(
            "fast".to_string(),
            OpenAIModelProviderConfig {
                base: ModelProviderConfig {
                    model: Some("gpt-4o-mini".to_string()),
                    api_key: Some("test-key".to_string()),
                    ..Default::default()
                },
            },
        );

        let (_provider, provider_ref, model, _route_resolver) =
            build_session_model_provider(&config, "openai.fast", None).unwrap();

        assert_eq!(provider_ref, "openai.fast");
        assert_eq!(model, "gpt-4o-mini");
    }

    /// Regression: trim write-back must never persist the provider-only
    /// recalled-memory preamble the turn engine injects onto the last user
    /// message. `replay_loop_messages` feeds every durable-history write-back
    /// call site, so stripping it there covers both the buffered and
    /// streamed trim paths. The strip target carries the injected message's
    /// index within the replayed (mutated history) buffer.
    #[test]
    fn replay_loop_messages_strips_the_memory_context_preamble() {
        let preamble = format!(
            "{}\n- k: recalled fact\n{}\n\n",
            clawcrew_memory::MEMORY_CONTEXT_OPEN,
            clawcrew_memory::MEMORY_CONTEXT_CLOSE,
        );
        let with_preamble = ChatMessage::user(format!("{preamble}what's the weather like"));
        let assistant = ChatMessage::assistant("it's sunny".to_string());
        let replayed = Agent::replay_loop_messages(
            &[with_preamble, assistant],
            Some(MemoryPreambleTarget {
                preamble: preamble.as_str(),
                index: 0,
            }),
        );

        let ConversationMessage::Chat(user_msg) = &replayed[0] else {
            panic!(
                "expected the user message to replay as Chat, got {:?}",
                replayed[0]
            );
        };
        assert_eq!(user_msg.content, "what's the weather like");
        assert!(
            !user_msg
                .content
                .contains(clawcrew_memory::MEMORY_CONTEXT_OPEN),
            "durable history must never carry the recalled-memory preamble"
        );
    }

    /// Regression: a genuine user message that merely starts with the same
    /// marker text as a recalled-memory preamble must survive byte-for-byte
    /// when no length was recorded for it — provenance is the caller's own
    /// record of what it injected, never a match against the marker text.
    #[test]
    fn replay_loop_messages_preserves_a_user_message_that_looks_like_a_preamble() {
        let looks_like_a_preamble = ChatMessage::user(format!(
            "{}\n- k: a user-authored fact\n{}\n\nplease keep this text",
            clawcrew_memory::MEMORY_CONTEXT_OPEN,
            clawcrew_memory::MEMORY_CONTEXT_CLOSE,
        ));
        let original = looks_like_a_preamble.content.clone();
        let replayed = Agent::replay_loop_messages(&[looks_like_a_preamble], None);

        let ConversationMessage::Chat(user_msg) = &replayed[0] else {
            panic!(
                "expected the user message to replay as Chat, got {:?}",
                replayed[0]
            );
        };
        assert_eq!(
            user_msg.content, original,
            "a genuine user message must survive byte-for-byte without a recorded preamble length"
        );
    }

    /// Regression: the no-trim and streamed callers replay pre-injection
    /// canonical clones (`loop_new_messages` / `round_added`) that the
    /// loop's memory injection never touches. Those callers pass no strip
    /// target, so replay is byte-for-byte even when the user's original
    /// text starts with the exact recorded preamble — the case that
    /// content-discovered stripping corrupted.
    #[test]
    fn replay_loop_messages_never_strips_an_uninjected_clone_even_when_text_collides() {
        let preamble = format!(
            "{}\n- k: recalled fact\n{}\n\n",
            clawcrew_memory::MEMORY_CONTEXT_OPEN,
            clawcrew_memory::MEMORY_CONTEXT_CLOSE,
        );
        // The user's genuine text starts with the exact recorded block, in
        // a buffer the injector never touched.
        let genuine = ChatMessage::user(format!("{preamble}my original question"));
        let assistant = ChatMessage::assistant("answer 1".to_string());
        let original = genuine.content.clone();

        let replayed = Agent::replay_loop_messages(&[genuine, assistant], None);

        let ConversationMessage::Chat(user_msg) = &replayed[0] else {
            panic!(
                "expected the user message to replay as Chat, got {:?}",
                replayed[0]
            );
        };
        assert_eq!(
            user_msg.content, original,
            "an uninjected clone must survive intact: clones carry no strip target"
        );
    }

    /// Regression: an older genuine user message that happens to equal the
    /// exact rendered preamble must survive write-back. Replay strips only
    /// the recorded target position in the mutated history buffer — never a
    /// content match anywhere else in the buffer.
    #[test]
    fn replay_loop_messages_strips_only_the_injected_user_message_when_an_older_one_collides() {
        let preamble = format!(
            "{}\n- k: recalled fact\n{}\n\n",
            clawcrew_memory::MEMORY_CONTEXT_OPEN,
            clawcrew_memory::MEMORY_CONTEXT_CLOSE,
        );
        // A genuine older turn quoting the exact recalled block verbatim.
        let older_collision = ChatMessage::user(preamble.clone());
        let injected = ChatMessage::user(format!("{preamble}current question"));
        let assistant = ChatMessage::assistant("answer".to_string());
        let replayed = Agent::replay_loop_messages(
            &[older_collision, injected, assistant],
            Some(MemoryPreambleTarget {
                preamble: preamble.as_str(),
                index: 1,
            }),
        );

        assert_eq!(replayed.len(), 3);
        let ConversationMessage::Chat(older_msg) = &replayed[0] else {
            panic!(
                "expected the older user message to replay as Chat, got {:?}",
                replayed[0]
            );
        };
        assert_eq!(
            older_msg.content, preamble,
            "an older genuine message matching the preamble must survive byte-for-byte"
        );
        let ConversationMessage::Chat(current_msg) = &replayed[1] else {
            panic!(
                "expected the injected user message to replay as Chat, got {:?}",
                replayed[1]
            );
        };
        assert_eq!(
            current_msg.content, "current question",
            "the injected message must still be stripped"
        );
    }

    /// Regression: steering input appends newer user messages after the
    /// injected one. Replay must strip the recorded target position only —
    /// a later steering message starting with the same preamble block must
    /// survive, and the injected memory must still be removed from the
    /// original message.
    #[test]
    fn replay_loop_messages_preserves_a_later_steering_message_when_it_collides() {
        let preamble = format!(
            "{}\n- k: recalled fact\n{}\n\n",
            clawcrew_memory::MEMORY_CONTEXT_OPEN,
            clawcrew_memory::MEMORY_CONTEXT_CLOSE,
        );
        let injected = ChatMessage::user(format!("{preamble}current question"));
        let assistant = ChatMessage::assistant("working on it".to_string());
        // A steering follow-up that happens to start with the same block.
        let steering = ChatMessage::user(format!("{preamble}steering follow-up"));
        let steering_original = steering.content.clone();
        let replayed = Agent::replay_loop_messages(
            &[injected, assistant, steering],
            Some(MemoryPreambleTarget {
                preamble: preamble.as_str(),
                index: 0,
            }),
        );

        assert_eq!(replayed.len(), 3);
        let ConversationMessage::Chat(current_msg) = &replayed[0] else {
            panic!(
                "expected the injected user message to replay as Chat, got {:?}",
                replayed[0]
            );
        };
        assert_eq!(
            current_msg.content, "current question",
            "the injected message must still be stripped"
        );
        let ConversationMessage::Chat(steering_msg) = &replayed[2] else {
            panic!(
                "expected the steering message to replay as Chat, got {:?}",
                replayed[2]
            );
        };
        assert_eq!(
            steering_msg.content, steering_original,
            "a later steering message must survive even when it starts with the preamble"
        );
    }

    clawcrew_api::mock_tool_attribution!(
        CountingTool,
        NamedMockTool,
        MockTool,
        SlowTool,
        ModelSwitchTriggerTool,
    );

    struct MockModelProvider {
        responses: Mutex<Vec<clawcrew_providers::ChatResponse>>,
    }

    #[async_trait]
    impl ModelProvider for MockModelProvider {
        fn has_stable_request_identity(&self, _model: &str) -> bool {
            true
        }

        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<String> {
            Ok("ok".into())
        }

        async fn chat(
            &self,
            _request: ChatRequest<'_>,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<clawcrew_providers::ChatResponse> {
            let mut guard = self.responses.lock();
            if guard.is_empty() {
                return Ok(clawcrew_providers::ChatResponse {
                    text: Some("done".into()),
                    tool_calls: vec![],
                    usage: None,
                    reasoning_content: None,
                });
            }
            Ok(guard.remove(0))
        }
    }
    impl ::clawcrew_api::attribution::Attributable for MockModelProvider {
        fn role(&self) -> ::clawcrew_api::attribution::Role {
            ::clawcrew_api::attribution::Role::Provider(
                ::clawcrew_api::attribution::ProviderKind::Model(
                    ::clawcrew_api::attribution::ModelProviderKind::Custom,
                ),
            )
        }
        fn alias(&self) -> &str {
            "MockModelProvider"
        }
    }

    struct SafeguardNoticeProvider;

    #[async_trait]
    impl ModelProvider for SafeguardNoticeProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<String> {
            clawcrew_providers::commit_safeguard_fallback(Some(
                clawcrew_providers::SafeguardFallbackNotice {
                    kind: clawcrew_providers::SafeguardFallbackKind::ClientAndServer,
                    requested_model: "requested-model".into(),
                    served_model: "served-model".into(),
                    category: Some("private-category".into()),
                },
            ));
            Ok("accepted response".into())
        }

        async fn chat(
            &self,
            _request: ChatRequest<'_>,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<clawcrew_providers::ChatResponse> {
            clawcrew_providers::commit_safeguard_fallback(Some(
                clawcrew_providers::SafeguardFallbackNotice {
                    kind: clawcrew_providers::SafeguardFallbackKind::ClientAndServer,
                    requested_model: "requested-model".into(),
                    served_model: "served-model".into(),
                    category: Some("private-category".into()),
                },
            ));
            Ok(clawcrew_providers::ChatResponse {
                text: Some("accepted response".into()),
                tool_calls: Vec::new(),
                usage: None,
                reasoning_content: None,
            })
        }
    }

    impl ::clawcrew_api::attribution::Attributable for SafeguardNoticeProvider {
        fn role(&self) -> ::clawcrew_api::attribution::Role {
            ::clawcrew_api::attribution::Role::Provider(
                ::clawcrew_api::attribution::ProviderKind::Model(
                    ::clawcrew_api::attribution::ModelProviderKind::Custom,
                ),
            )
        }

        fn alias(&self) -> &str {
            "SafeguardNoticeProvider"
        }
    }

    struct RefusingCandidateProvider {
        /// Usage billed by the refusing attempt, when the provider reports it.
        usage: Option<clawcrew_providers::traits::TokenUsage>,
    }

    impl RefusingCandidateProvider {
        fn refusal(&self, model: &str) -> anyhow::Error {
            anyhow::Error::new(clawcrew_providers::AnthropicRefusalError {
                requested_model: model.into(),
                category: Some("private-category".into()),
                usage: self.usage.clone().map(Box::new),
                attempted_candidate: None,
                attempted_candidate_index: None,
            })
        }
    }

    #[async_trait]
    impl ModelProvider for RefusingCandidateProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            model: &str,
            _temperature: Option<f64>,
        ) -> Result<String> {
            Err(self.refusal(model))
        }

        async fn chat(
            &self,
            _request: ChatRequest<'_>,
            model: &str,
            _temperature: Option<f64>,
        ) -> Result<clawcrew_providers::ChatResponse> {
            Err(self.refusal(model))
        }
    }

    /// Candidate that fails with an ordinary transport error, never a refusal.
    struct UnavailableCandidateProvider;

    #[async_trait]
    impl ModelProvider for UnavailableCandidateProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<String> {
            anyhow::bail!("503 service unavailable")
        }

        async fn chat(
            &self,
            _request: ChatRequest<'_>,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<clawcrew_providers::ChatResponse> {
            anyhow::bail!("503 service unavailable")
        }
    }

    impl ::clawcrew_api::attribution::Attributable for UnavailableCandidateProvider {
        fn role(&self) -> ::clawcrew_api::attribution::Role {
            ::clawcrew_api::attribution::Role::Provider(
                ::clawcrew_api::attribution::ProviderKind::Model(
                    ::clawcrew_api::attribution::ModelProviderKind::Custom,
                ),
            )
        }

        fn alias(&self) -> &str {
            "UnavailableCandidateProvider"
        }
    }

    impl ::clawcrew_api::attribution::Attributable for RefusingCandidateProvider {
        fn role(&self) -> ::clawcrew_api::attribution::Role {
            ::clawcrew_api::attribution::Role::Provider(
                ::clawcrew_api::attribution::ProviderKind::Model(
                    ::clawcrew_api::attribution::ModelProviderKind::Custom,
                ),
            )
        }

        fn alias(&self) -> &str {
            "RefusingCandidateProvider"
        }
    }

    /// Keep the real Anthropic non-streaming implementation while making the
    /// runtime test enter the non-streaming turn path deterministically.
    struct NonStreamingAnthropicProvider {
        inner: clawcrew_providers::anthropic::AnthropicModelProvider,
    }

    #[async_trait]
    impl ModelProvider for NonStreamingAnthropicProvider {
        async fn chat_with_system(
            &self,
            system_prompt: Option<&str>,
            message: &str,
            model: &str,
            temperature: Option<f64>,
        ) -> Result<String> {
            self.inner
                .chat_with_system(system_prompt, message, model, temperature)
                .await
        }

        async fn chat(
            &self,
            request: ChatRequest<'_>,
            model: &str,
            temperature: Option<f64>,
        ) -> Result<clawcrew_providers::ChatResponse> {
            self.inner.chat(request, model, temperature).await
        }
    }

    impl ::clawcrew_api::attribution::Attributable for NonStreamingAnthropicProvider {
        fn role(&self) -> ::clawcrew_api::attribution::Role {
            ::clawcrew_api::attribution::Role::Provider(
                ::clawcrew_api::attribution::ProviderKind::Model(
                    ::clawcrew_api::attribution::ModelProviderKind::Anthropic,
                ),
            )
        }

        fn alias(&self) -> &str {
            "NonStreamingAnthropicProvider"
        }
    }

    #[derive(Clone)]
    struct SequencedAnthropicResponder {
        calls: Arc<AtomicUsize>,
        bodies: Arc<Vec<serde_json::Value>>,
    }

    impl wiremock::Respond for SequencedAnthropicResponder {
        fn respond(&self, _request: &wiremock::Request) -> wiremock::ResponseTemplate {
            let index = self.calls.fetch_add(1, Ordering::SeqCst);
            let body = self
                .bodies
                .get(index)
                .or_else(|| self.bodies.last())
                .expect("sequence must contain a response")
                .clone();
            wiremock::ResponseTemplate::new(200).set_body_json(body)
        }
    }

    const BLANK_TURN_ERROR: &str = "empty user message: refusing to dispatch a blank turn";

    fn blank_input_agent(model_provider: Box<dyn ModelProvider>) -> Agent {
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );
        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        Agent::builder()
            .model_provider(model_provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                Vec::new(),
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .build()
            .expect("agent builder should succeed with valid config")
    }

    #[tokio::test]
    async fn turn_rejects_blank_input() {
        let model_provider = Box::new(MockModelProvider {
            responses: Mutex::new(Vec::new()),
        });
        let mut agent = blank_input_agent(model_provider);
        let err = agent.turn("").await.expect_err("blank turn must fail");
        assert_eq!(err.to_string(), BLANK_TURN_ERROR);
    }

    #[tokio::test]
    async fn turn_rejects_whitespace_only_input() {
        let model_provider = Box::new(MockModelProvider {
            responses: Mutex::new(Vec::new()),
        });
        let mut agent = blank_input_agent(model_provider);
        let err = agent
            .turn("   \n\t")
            .await
            .expect_err("whitespace-only turn must fail");
        assert_eq!(err.to_string(), BLANK_TURN_ERROR);
    }

    // ── model-fallback notice (silent downgrade surfacing) ──────────────

    fn fallback_info(
        requested_provider: &str,
        requested_model: &str,
        actual_provider: &str,
        actual_model: &str,
    ) -> clawcrew_providers::reliable::ProviderFallbackInfo {
        clawcrew_providers::reliable::ProviderFallbackInfo {
            requested_provider: requested_provider.into(),
            requested_model: requested_model.into(),
            actual_provider: actual_provider.into(),
            actual_model: actual_model.into(),
        }
    }

    #[tokio::test]
    async fn model_fallback_notice_appended_and_streamed_on_model_downgrade() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        // Same provider family, different model — the case the channels
        // orchestrator's family check suppresses; direct-turn surfaces must
        // still see it.
        let info = fallback_info("anthropic", "model-requested", "anthropic", "model-served");
        let out =
            Agent::append_model_fallback_notice("hello".to_string(), Some(&info), None, &tx).await;
        assert!(
            out.starts_with("hello\n\n"),
            "reply text must be preserved ahead of the notice: {out}"
        );
        assert!(
            out.contains("model-requested") && out.contains("model-served"),
            "notice must name both models: {out}"
        );
        match rx.try_recv() {
            Ok(TurnEvent::Chunk { delta }) => {
                assert!(
                    delta.contains("model-served"),
                    "streamed chunk must carry the notice for delta-only consumers: {delta}"
                );
            }
            other => panic!("expected a trailing Chunk carrying the notice, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn model_fallback_notice_skipped_for_pure_retry() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        // The resilient wrapper records retries too (attempt > 0 on the
        // primary entry); an identical requested/served pair is not a
        // downgrade and must stay silent.
        let info = fallback_info("anthropic", "same-model", "anthropic", "same-model");
        let out =
            Agent::append_model_fallback_notice("hello".to_string(), Some(&info), None, &tx).await;
        assert_eq!(out, "hello");
        assert!(rx.try_recv().is_err(), "no chunk for a retry");
    }

    #[tokio::test]
    async fn model_fallback_notice_absent_without_fallback_info() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let out = Agent::append_model_fallback_notice("hello".to_string(), None, None, &tx).await;
        assert_eq!(out, "hello");
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn safeguard_notice_suppresses_generic_model_fallback_notice() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let info = fallback_info("anthropic", "requested-model", "openai", "served-model");
        let safeguard = clawcrew_providers::SafeguardFallbackNotice {
            kind: clawcrew_providers::SafeguardFallbackKind::ClientSide,
            requested_model: "requested-model".into(),
            served_model: "served-model".into(),
            category: None,
        };
        let out = Agent::append_model_fallback_notice(
            "hello".to_string(),
            Some(&info),
            Some(&safeguard),
            &tx,
        )
        .await;
        assert_eq!(out, "hello");
        assert!(rx.try_recv().is_err(), "no generic fallback chunk");
    }

    #[tokio::test]
    async fn server_side_safeguard_keeps_generic_model_fallback_notice() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        // Ordinary failure on model A moved the request to pinned model B;
        // Anthropic then served B's request with C. The safeguard notice
        // covers only B to C, so the generic A to B leg must stay visible.
        let info = fallback_info("anthropic", "model-a", "anthropic", "model-b");
        let safeguard = clawcrew_providers::SafeguardFallbackNotice {
            kind: clawcrew_providers::SafeguardFallbackKind::ServerSide,
            requested_model: "model-b".into(),
            served_model: "model-c".into(),
            category: None,
        };
        let out = Agent::append_model_fallback_notice(
            "hello".to_string(),
            Some(&info),
            Some(&safeguard),
            &tx,
        )
        .await;
        assert!(out.starts_with("hello\n\n"), "reply text preserved: {out}");
        assert!(
            out.contains("model-a") && out.contains("model-b"),
            "the ordinary leg must keep naming the original request: {out}"
        );
        assert!(
            !out.contains("model-c"),
            "the safeguard leg is rendered by the caller, not here: {out}"
        );
        match rx.try_recv() {
            Ok(TurnEvent::Chunk { delta }) => {
                assert!(
                    delta.contains("model-a"),
                    "streamed chunk carries the leg: {delta}"
                );
            }
            other => panic!("expected the generic notice chunk, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn direct_agent_turn_presents_one_safeguard_without_persisting_it() {
        let mut agent = blank_input_agent(Box::new(SafeguardNoticeProvider));
        let response = agent.turn("hello").await.expect("direct turn succeeds");

        assert_eq!(response.matches("Safety safeguards").count(), 1);
        assert!(response.contains("requested-model"));
        assert!(response.contains("served-model"));
        assert!(!response.contains("private-category"));
        let persisted = agent
            .history
            .iter()
            .rev()
            .find_map(|message| match message {
                ConversationMessage::Chat(message) if message.role == "assistant" => {
                    Some(message.content.as_str())
                }
                _ => None,
            })
            .expect("assistant response persisted");
        assert_eq!(persisted, "accepted response");
    }

    #[tokio::test]
    async fn direct_streamed_turn_returns_typed_safeguard_and_raw_transcript() {
        let mut agent = blank_input_agent(Box::new(SafeguardNoticeProvider));
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        let outcome = agent
            .turn_streamed_with_steering_state("hello", tx, None, None)
            .await
            .expect("streamed turn succeeds");

        assert_eq!(outcome.response, "accepted response");
        let notice = outcome
            .safeguard_fallback
            .expect("accepted safeguard attribution");
        assert_eq!(notice.requested_model, "requested-model");
        assert_eq!(notice.served_model, "served-model");
        assert!(outcome.new_messages.iter().all(|message| match message {
            ConversationMessage::Chat(message) => !message.content.contains("Safety safeguards"),
            _ => true,
        }));
    }

    #[tokio::test]
    async fn direct_streaming_api_emits_one_display_notice_for_cli_and_acp() {
        let mut agent = blank_input_agent(Box::new(SafeguardNoticeProvider));
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        let (response, messages) = agent
            .turn_streamed("hello", tx, None)
            .await
            .expect("streamed API succeeds");

        assert_eq!(response.matches("Safety safeguards").count(), 1);
        let mut streamed = String::new();
        while let Ok(event) = rx.try_recv() {
            if let TurnEvent::Chunk { delta } = event {
                streamed.push_str(&delta);
            }
        }
        assert_eq!(streamed.matches("Safety safeguards").count(), 1);
        assert!(messages.iter().all(|message| match message {
            ConversationMessage::Chat(message) => !message.content.contains("Safety safeguards"),
            _ => true,
        }));
    }

    #[tokio::test]
    async fn refusal_then_real_anthropic_server_fallback_reaches_publisher_once() {
        use wiremock::{Mock, MockServer, matchers::method};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "model": "served-c",
                    "content": [{"type": "text", "text": "accepted from c"}],
                    "stop_reason": "end_turn",
                    "usage": {
                        "input_tokens": 10,
                        "output_tokens": 5,
                        "iterations": [
                            {"type": "message"},
                            {"type": "fallback_message"}
                        ]
                    }
                })),
            )
            .expect(1)
            .mount(&server)
            .await;
        let anthropic =
            clawcrew_providers::anthropic::AnthropicModelProvider::builder("candidate-b")
                .credential(Some("synthetic-key"))
                .base_url(&server.uri())
                .server_fallback_models(vec!["served-c".into()])
                .build();
        let reliable = clawcrew_providers::reliable::ReliableModelProvider::new(
            "test",
            vec![
                (
                    "candidate-a".into(),
                    Box::new(RefusingCandidateProvider { usage: None }) as Box<dyn ModelProvider>,
                ),
                (
                    "candidate-b".into(),
                    Box::new(NonStreamingAnthropicProvider { inner: anthropic })
                        as Box<dyn ModelProvider>,
                ),
            ],
            0,
            1,
        );
        let mut agent = blank_input_agent(Box::new(reliable));
        agent.model_name = "requested-a".into();
        let (tx, _rx) = tokio::sync::mpsc::channel(16);

        let outcome = agent
            .turn_streamed_with_steering_state("hello", tx, None, None)
            .await
            .expect("candidate B and Anthropic server fallback C recover the turn");

        assert_eq!(outcome.response, "accepted from c");
        let notice = outcome
            .safeguard_fallback
            .as_ref()
            .expect("composed accepted-route attribution");
        assert_eq!(
            notice.kind,
            clawcrew_providers::SafeguardFallbackKind::ClientAndServer
        );
        assert_eq!(notice.requested_model, "requested-a");
        assert_eq!(notice.served_model, "served-c");
        assert_eq!(notice.category.as_deref(), Some("private-category"));

        let display =
            crate::agent::append_safeguard_fallback_notice(outcome.response.clone(), Some(notice));
        assert_eq!(display.matches("Safety safeguards").count(), 1);
        assert!(display.contains("requested-a"));
        assert!(display.contains("served-c"));
        assert!(!display.contains("private-category"));
        assert!(outcome.new_messages.iter().all(|message| match message {
            ConversationMessage::Chat(message) => !message.content.contains("Safety safeguards"),
            _ => true,
        }));
        assert!(agent.history.iter().all(|message| match message {
            ConversationMessage::Chat(message) => !message.content.contains("Safety safeguards"),
            _ => true,
        }));
    }

    #[tokio::test]
    async fn rejected_empty_server_fallback_does_not_leak_into_normal_retry() {
        use wiremock::{Mock, MockServer, matchers::method};

        let server = MockServer::start().await;
        let calls = Arc::new(AtomicUsize::new(0));
        let bodies = Arc::new(vec![
            serde_json::json!({
                "model": "served-c",
                "content": [],
                "stop_reason": "end_turn",
                "usage": {
                    "input_tokens": 10,
                    "output_tokens": 0,
                    "iterations": [{"type": "fallback_message"}]
                }
            }),
            serde_json::json!({
                "model": "requested-a",
                "content": [{"type": "text", "text": "normal retry"}],
                "stop_reason": "end_turn",
                "usage": {
                    "input_tokens": 11,
                    "output_tokens": 2,
                    "iterations": [{"type": "message"}]
                }
            }),
        ]);
        Mock::given(method("POST"))
            .respond_with(SequencedAnthropicResponder {
                calls: Arc::clone(&calls),
                bodies,
            })
            .expect(2)
            .mount(&server)
            .await;
        let anthropic =
            clawcrew_providers::anthropic::AnthropicModelProvider::builder("candidate-b")
                .credential(Some("synthetic-key"))
                .base_url(&server.uri())
                .server_fallback_models(vec!["served-c".into()])
                .build();
        let reliable = clawcrew_providers::reliable::ReliableModelProvider::new(
            "test",
            vec![(
                "candidate-b".into(),
                Box::new(NonStreamingAnthropicProvider { inner: anthropic })
                    as Box<dyn ModelProvider>,
            )],
            1,
            1,
        );
        let mut agent = blank_input_agent(Box::new(reliable));
        agent.model_name = "requested-a".into();
        let (tx, _rx) = tokio::sync::mpsc::channel(16);

        let outcome = agent
            .turn_streamed_with_steering_state("hello", tx, None, None)
            .await
            .expect("normal retry is accepted");

        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(outcome.response, "normal retry");
        assert!(
            outcome.safeguard_fallback.is_none(),
            "rejected server-fallback attribution must be cleared before retry"
        );
        assert!(outcome.new_messages.iter().all(|message| match message {
            ConversationMessage::Chat(message) => !message.content.contains("served-c"),
            _ => true,
        }));
    }

    /// One accepted native response served by Anthropic's server fallback C.
    async fn served_by_c_anthropic_server() -> wiremock::MockServer {
        use wiremock::{Mock, MockServer, matchers::method};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "model": "served-c",
                    "content": [{"type": "text", "text": "accepted from c"}],
                    "stop_reason": "end_turn",
                    "usage": {
                        "input_tokens": 10,
                        "output_tokens": 5,
                        "iterations": [
                            {"type": "message"},
                            {"type": "fallback_message"}
                        ]
                    }
                })),
            )
            .expect(1)
            .mount(&server)
            .await;
        server
    }

    /// Candidate A fails with an ordinary 503, Reliable advances to candidate
    /// B (the real Anthropic client), and Anthropic serves B's request with C.
    fn ordinary_failure_then_server_fallback_reliable(
        server_uri: &str,
    ) -> clawcrew_providers::reliable::ReliableModelProvider {
        let anthropic =
            clawcrew_providers::anthropic::AnthropicModelProvider::builder("candidate-b")
                .credential(Some("synthetic-key"))
                .base_url(server_uri)
                .server_fallback_models(vec!["served-c".into()])
                .build();
        clawcrew_providers::reliable::ReliableModelProvider::new(
            "test",
            vec![
                (
                    "candidate-a".into(),
                    Box::new(UnavailableCandidateProvider) as Box<dyn ModelProvider>,
                ),
                (
                    "candidate-b".into(),
                    Box::new(NonStreamingAnthropicProvider { inner: anthropic })
                        as Box<dyn ModelProvider>,
                ),
            ],
            0,
            1,
        )
    }

    fn last_assistant_content(messages: &[ConversationMessage]) -> Option<&str> {
        messages.iter().rev().find_map(|message| match message {
            ConversationMessage::Chat(message) if message.role == "assistant" => {
                Some(message.content.as_str())
            }
            _ => None,
        })
    }

    #[tokio::test]
    async fn ordinary_failure_then_real_anthropic_server_fallback_keeps_original_request_visible() {
        let server = served_by_c_anthropic_server().await;
        let mut agent = blank_input_agent(Box::new(
            ordinary_failure_then_server_fallback_reliable(&server.uri()),
        ));
        agent.model_name = "requested-a".into();
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);

        let outcome = agent
            .turn_streamed_with_steering_state("hello", tx, None, None)
            .await
            .expect("candidate B and Anthropic server fallback C recover the turn");

        let notice = outcome
            .safeguard_fallback
            .as_ref()
            .expect("server-side attribution for the accepted attempt");
        assert_eq!(
            notice.kind,
            clawcrew_providers::SafeguardFallbackKind::ServerSide,
            "an ordinary 503 is not a refusal-triggered client recovery"
        );
        assert_eq!(notice.requested_model, "requested-a");
        assert_eq!(notice.served_model, "served-c");

        // The ordinary A to B leg is delivered as the generic notice, both in
        // the returned text and as a streamed chunk, so the original route is
        // not erased by the safeguard notice.
        assert!(
            outcome.response.starts_with("accepted from c\n\n"),
            "reply text precedes the route notices: {}",
            outcome.response
        );
        assert!(
            outcome.response.contains("candidate-a") && outcome.response.contains("candidate-b"),
            "the ordinary leg must name the original route: {}",
            outcome.response
        );
        let mut streamed = String::new();
        while let Ok(event) = rx.try_recv() {
            if let TurnEvent::Chunk { delta } = event {
                streamed.push_str(&delta);
            }
        }
        assert!(
            streamed.contains("candidate-a"),
            "delta-only consumers must also receive the ordinary leg: {streamed}"
        );

        let display =
            crate::agent::append_safeguard_fallback_notice(outcome.response.clone(), Some(notice));
        assert_eq!(display.matches("Safety safeguards").count(), 1);
        assert!(display.contains("served-c"));
        assert!(
            display.find("candidate-a") < display.find("Safety safeguards"),
            "the ordinary leg precedes the safety leg: {display}"
        );
        assert!(
            !display.contains("fallback chain"),
            "the client leg was an ordinary failure, not a refusal chain: {display}"
        );
        assert_eq!(
            last_assistant_content(&outcome.new_messages),
            Some("accepted from c"),
            "persisted content stays undecorated"
        );
        assert_eq!(
            last_assistant_content(&agent.history),
            Some("accepted from c")
        );
    }

    #[tokio::test]
    async fn ordinary_failure_then_server_fallback_direct_turn_renders_both_legs_once() {
        let server = served_by_c_anthropic_server().await;
        let mut agent = blank_input_agent(Box::new(
            ordinary_failure_then_server_fallback_reliable(&server.uri()),
        ));
        agent.model_name = "requested-a".into();

        let response = agent
            .turn("hello")
            .await
            .expect("candidate B and Anthropic server fallback C recover the turn");

        assert!(
            response.starts_with("accepted from c\n\n"),
            "reply text precedes the route notices: {response}"
        );
        assert!(
            response.contains("candidate-a"),
            "the ordinary leg must keep the original request visible: {response}"
        );
        assert_eq!(response.matches("Safety safeguards").count(), 1);
        assert!(response.contains("served-c"));
        assert!(
            response.find("candidate-a") < response.find("Safety safeguards"),
            "the ordinary leg precedes the safety leg: {response}"
        );
        assert!(!response.contains("fallback chain"));
        assert_eq!(
            last_assistant_content(&agent.history),
            Some("accepted from c"),
            "persisted content stays undecorated"
        );
    }

    #[tokio::test]
    async fn exhausted_refusal_with_billed_usage_delivers_safety_guidance() {
        let reliable = clawcrew_providers::reliable::ReliableModelProvider::new(
            "test",
            vec![(
                "candidate-a".into(),
                Box::new(RefusingCandidateProvider {
                    usage: Some(clawcrew_providers::traits::TokenUsage {
                        input_tokens: Some(7),
                        output_tokens: Some(3),
                        cached_input_tokens: None,
                        cache_creation_input_tokens: None,
                    }),
                }) as Box<dyn ModelProvider>,
            )],
            0,
            1,
        );
        let mut agent = blank_input_agent(Box::new(reliable));
        agent.model_name = "requested-a".into();

        let error = agent
            .turn("hello")
            .await
            .expect_err("an unrescued refusal fails the turn");

        let usage = clawcrew_providers::rejected_attempt_usage_from_error(&error)
            .expect("billed refusal usage survives the terminal error");
        assert_eq!(usage.input_tokens, Some(7));
        assert_eq!(usage.output_tokens, Some(3));
        assert!(
            error
                .downcast_ref::<clawcrew_providers::AnthropicRefusalError>()
                .is_none(),
            "production shape keeps the refusal beneath Reliable's envelopes: {error:#}"
        );

        let message = crate::agent::terminal_completion_error_message(&error, None)
            .expect("an exhausted refusal projects a user-facing message");
        assert_eq!(
            message,
            crate::i18n::get_required_cli_string("cli-agent-error-provider-refusal")
        );
        assert!(message.contains("safety system"), "{message}");
        assert!(!message.contains("private-category"));
        assert!(
            clawcrew_providers::reliable::transient_error_hint(&error)
                .is_some_and(|hint| hint.contains("safety system")),
            "the channel hint fallback must also see the refusal"
        );
    }

    #[test]
    fn fallback_notice_fluent_key_stays_localized_without_new_keys() {
        let args = [
            ("requested_model", "requested-model"),
            ("requested_provider", "primary"),
            ("actual_model", "served-model"),
            ("actual_provider", "fallback"),
        ];
        let english =
            crate::i18n::get_english_cli_string_with_args("turn-model-fallback-notice", &args);
        let french = crate::i18n::get_disk_override_cli_string_for_test(
            "fr",
            include_str!("../../locales/fr/cli.ftl"),
            "turn-model-fallback-notice",
            &args,
        );

        assert_ne!(french, "{turn-model-fallback-notice}");
        assert_ne!(french, english, "French must not fall back to English");
        for (_, value) in args {
            assert!(
                french.contains(value),
                "localized fallback notice lost {value}"
            );
        }
    }

    #[derive(Clone, Copy)]
    enum RuntimeStreamPlan {
        Unsupported,
        Text(&'static str),
        EmptyWithUsage,
        Error,
    }

    struct RuntimeStreamingProbeProvider {
        stream: RuntimeStreamPlan,
        chat_text: Option<&'static str>,
    }

    #[async_trait]
    impl ModelProvider for RuntimeStreamingProbeProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<String> {
            Ok(self.chat_text.unwrap_or("ok").to_string())
        }

        async fn chat(
            &self,
            _request: ChatRequest<'_>,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<clawcrew_providers::ChatResponse> {
            let Some(text) = self.chat_text else {
                anyhow::bail!("chat path must not be used for this probe");
            };
            Ok(clawcrew_providers::ChatResponse {
                text: Some(text.to_string()),
                tool_calls: vec![],
                usage: None,
                reasoning_content: None,
            })
        }

        fn supports_streaming(&self) -> bool {
            !matches!(self.stream, RuntimeStreamPlan::Unsupported)
        }

        fn stream_chat(
            &self,
            _request: ChatRequest<'_>,
            _model: &str,
            _temperature: Option<f64>,
            _options: clawcrew_providers::traits::StreamOptions,
        ) -> futures_util::stream::BoxStream<
            'static,
            clawcrew_providers::traits::StreamResult<clawcrew_providers::traits::StreamEvent>,
        > {
            use futures_util::StreamExt as _;

            match self.stream {
                RuntimeStreamPlan::Unsupported => futures_util::stream::empty().boxed(),
                RuntimeStreamPlan::Text(text) => futures_util::stream::iter(vec![
                    Ok(clawcrew_providers::traits::StreamEvent::TextDelta(
                        clawcrew_providers::traits::StreamChunk::delta(text),
                    )),
                    Ok(clawcrew_providers::traits::StreamEvent::Final),
                ])
                .boxed(),
                RuntimeStreamPlan::EmptyWithUsage => futures_util::stream::iter(vec![
                    Ok(clawcrew_providers::traits::StreamEvent::Usage(
                        clawcrew_providers::traits::TokenUsage {
                            input_tokens: Some(13),
                            output_tokens: Some(7),
                            cached_input_tokens: None,
                            cache_creation_input_tokens: None,
                        },
                    )),
                    Ok(clawcrew_providers::traits::StreamEvent::Final),
                ])
                .boxed(),
                RuntimeStreamPlan::Error => futures_util::stream::iter(vec![Err(
                    clawcrew_providers::traits::StreamError::ModelProvider(
                        "stream failed before output".into(),
                    ),
                )])
                .boxed(),
            }
        }
    }

    impl ::clawcrew_api::attribution::Attributable for RuntimeStreamingProbeProvider {
        fn role(&self) -> ::clawcrew_api::attribution::Role {
            ::clawcrew_api::attribution::Role::Provider(
                ::clawcrew_api::attribution::ProviderKind::Model(
                    ::clawcrew_api::attribution::ModelProviderKind::Custom,
                ),
            )
        }
        fn alias(&self) -> &str {
            "RuntimeStreamingProbeProvider"
        }
    }

    fn streaming_probe_reliable_provider(
        primary: RuntimeStreamingProbeProvider,
        fallback: RuntimeStreamingProbeProvider,
    ) -> clawcrew_providers::reliable::ReliableModelProvider {
        clawcrew_providers::reliable::ReliableModelProvider::new(
            "test",
            vec![
                (
                    "provider-requested".to_string(),
                    Box::new(primary) as Box<dyn ModelProvider>,
                ),
                (
                    "provider-served".to_string(),
                    Box::new(fallback) as Box<dyn ModelProvider>,
                ),
            ],
            0,
            1,
        )
    }

    /// End-to-end: a resilient wrapper that fails over to a second entry
    /// mid-turn must surface the downgrade in BOTH the returned response and
    /// the event stream.
    #[tokio::test]
    async fn streamed_turn_surfaces_provider_fallback_notice() {
        struct FailingModelProvider;
        #[async_trait]
        impl ModelProvider for FailingModelProvider {
            async fn chat_with_system(
                &self,
                _system_prompt: Option<&str>,
                _message: &str,
                _model: &str,
                _temperature: Option<f64>,
            ) -> Result<String> {
                anyhow::bail!("primary provider is down")
            }
            async fn chat(
                &self,
                _request: ChatRequest<'_>,
                _model: &str,
                _temperature: Option<f64>,
            ) -> Result<clawcrew_providers::ChatResponse> {
                anyhow::bail!("primary provider is down")
            }
        }
        impl ::clawcrew_api::attribution::Attributable for FailingModelProvider {
            fn role(&self) -> ::clawcrew_api::attribution::Role {
                ::clawcrew_api::attribution::Role::Provider(
                    ::clawcrew_api::attribution::ProviderKind::Model(
                        ::clawcrew_api::attribution::ModelProviderKind::Custom,
                    ),
                )
            }
            fn alias(&self) -> &str {
                "FailingModelProvider"
            }
        }

        let reliable = clawcrew_providers::reliable::ReliableModelProvider::new(
            "test",
            vec![
                (
                    "provider-requested".to_string(),
                    Box::new(FailingModelProvider) as Box<dyn ModelProvider>,
                ),
                (
                    "provider-served".to_string(),
                    Box::new(MockModelProvider {
                        responses: Mutex::new(Vec::new()),
                    }) as Box<dyn ModelProvider>,
                ),
            ],
            0,
            50,
        );

        let mut agent = blank_input_agent(Box::new(reliable));
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let outcome = agent
            .turn_streamed_with_steering_state("hello", tx, None, None)
            .await
            .expect("turn must succeed via the fallback entry");

        assert!(
            outcome.response.contains("provider-served")
                && outcome.response.contains("provider-requested"),
            "final response must carry the fallback notice: {}",
            outcome.response
        );

        let mut chunk_carried_notice = false;
        while let Ok(event) = rx.try_recv() {
            if let TurnEvent::Chunk { delta } = event
                && delta.contains("provider-served")
            {
                chunk_carried_notice = true;
            }
        }
        assert!(
            chunk_carried_notice,
            "the notice must also be streamed for delta-only consumers (ZeroCode)"
        );
    }

    #[tokio::test]
    async fn streamed_turn_surfaces_streaming_provider_fallback_notice() {
        let reliable = streaming_probe_reliable_provider(
            RuntimeStreamingProbeProvider {
                stream: RuntimeStreamPlan::Unsupported,
                chat_text: None,
            },
            RuntimeStreamingProbeProvider {
                stream: RuntimeStreamPlan::Text("streamed fallback"),
                chat_text: None,
            },
        );

        let mut agent = blank_input_agent(Box::new(reliable));
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let outcome = agent
            .turn_streamed_with_steering_state("hello", tx, None, None)
            .await
            .expect("turn must succeed via the streaming fallback entry");

        assert!(
            outcome.response.contains("streamed fallback")
                && outcome.response.contains("provider-served"),
            "final response must include streamed text and fallback notice: {}",
            outcome.response
        );

        let mut streamed = String::new();
        while let Ok(event) = rx.try_recv() {
            if let TurnEvent::Chunk { delta } = event {
                streamed.push_str(&delta);
            }
        }
        assert!(
            streamed.contains("streamed fallback") && streamed.contains("provider-served"),
            "streamed chunks must include the live fallback output and notice: {streamed}"
        );
    }

    #[tokio::test]
    async fn streamed_turn_does_not_surface_stale_record_after_stream_error() {
        let reliable = streaming_probe_reliable_provider(
            RuntimeStreamingProbeProvider {
                stream: RuntimeStreamPlan::Unsupported,
                chat_text: Some("primary final"),
            },
            RuntimeStreamingProbeProvider {
                stream: RuntimeStreamPlan::Error,
                chat_text: None,
            },
        );

        let mut agent = blank_input_agent(Box::new(reliable));
        let (tx, _rx) = tokio::sync::mpsc::channel(64);
        let outcome = agent
            .turn_streamed_with_steering_state("hello", tx, None, None)
            .await
            .expect("pre-output stream error must fall back to primary chat");

        assert_eq!(
            outcome.response, "primary final",
            "failed fallback streams must not leave stale fallback notice state"
        );
    }

    /// A billed fallback stream is only a transport candidate. If its final
    /// response is semantically empty and Reliable recovers to primary chat,
    /// neither the Agent result nor its chunks may retain the fallback notice.
    #[tokio::test]
    async fn streamed_empty_fallback_recovery_does_not_leak_a_provider_notice() {
        let reliable = streaming_probe_reliable_provider(
            RuntimeStreamingProbeProvider {
                stream: RuntimeStreamPlan::Unsupported,
                chat_text: Some("primary recovery"),
            },
            RuntimeStreamingProbeProvider {
                stream: RuntimeStreamPlan::EmptyWithUsage,
                chat_text: None,
            },
        );

        let mut agent = blank_input_agent(Box::new(reliable));
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let outcome = agent
            .turn_streamed_with_steering_state("hello", tx, None, None)
            .await
            .expect("primary recovery must succeed after an empty fallback stream");

        assert_eq!(outcome.response, "primary recovery");
        let mut chunks = String::new();
        while let Ok(TurnEvent::Chunk { delta }) = rx.try_recv() {
            chunks.push_str(&delta);
        }
        assert!(
            !chunks.contains("provider-served") && !chunks.contains("provider-requested"),
            "rejected fallback must not leak its notice into streamed chunks: {chunks}"
        );
    }

    /// A tool-call response is accepted for this iteration, but its recovery
    /// record must be replaced by the route of the final accepted answer.
    #[tokio::test]
    async fn tool_call_then_final_fallback_surfaces_exactly_one_final_notice() {
        let reliable = clawcrew_providers::reliable::ReliableModelProvider::new(
            "test",
            vec![
                (
                    "primary".to_string(),
                    Box::new(ToolThenFailingModelProvider {
                        calls: std::sync::atomic::AtomicUsize::new(0),
                    }) as Box<dyn ModelProvider>,
                ),
                (
                    "fallback".to_string(),
                    Box::new(MockModelProvider {
                        responses: Mutex::new(vec![clawcrew_providers::ChatResponse {
                            text: Some("fallback final".to_string()),
                            tool_calls: vec![],
                            usage: None,
                            reasoning_content: None,
                        }]),
                    }) as Box<dyn ModelProvider>,
                ),
            ],
            0,
            0,
        );
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let memory: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("test memory must initialize"),
        );
        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = Agent::builder()
            .model_provider(Box::new(reliable))
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(memory)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .build()
            .expect("test agent must initialize");

        let response = agent.turn("run the tool").await.expect("turn must recover");
        assert_eq!(
            response.matches("fallback").count(),
            2,
            "final text plus one notice"
        );
        assert!(
            response.contains("primary"),
            "notice identifies the requested route"
        );
    }

    #[tokio::test]
    async fn fallback_tool_call_then_primary_final_clears_the_stale_notice() {
        struct PrimaryFailsOnceThenFinal(std::sync::atomic::AtomicUsize);
        struct FallbackToolCall;

        macro_rules! attributable {
            ($type:ty, $alias:literal) => {
                impl ::clawcrew_api::attribution::Attributable for $type {
                    fn role(&self) -> ::clawcrew_api::attribution::Role {
                        ::clawcrew_api::attribution::Role::Provider(
                            ::clawcrew_api::attribution::ProviderKind::Model(
                                ::clawcrew_api::attribution::ModelProviderKind::Custom,
                            ),
                        )
                    }
                    fn alias(&self) -> &str {
                        $alias
                    }
                }
            };
        }
        attributable!(PrimaryFailsOnceThenFinal, "PrimaryFailsOnceThenFinal");
        attributable!(FallbackToolCall, "FallbackToolCall");

        #[async_trait]
        impl ModelProvider for PrimaryFailsOnceThenFinal {
            async fn chat_with_system(
                &self,
                _: Option<&str>,
                _: &str,
                _: &str,
                _: Option<f64>,
            ) -> Result<String> {
                Ok("unused".into())
            }
            async fn chat(
                &self,
                _: ChatRequest<'_>,
                _: &str,
                _: Option<f64>,
            ) -> Result<clawcrew_providers::ChatResponse> {
                if self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                    anyhow::bail!("primary unavailable for first tool request");
                }
                Ok(clawcrew_providers::ChatResponse {
                    text: Some("primary final".into()),
                    tool_calls: vec![],
                    usage: None,
                    reasoning_content: None,
                })
            }
        }
        #[async_trait]
        impl ModelProvider for FallbackToolCall {
            async fn chat_with_system(
                &self,
                _: Option<&str>,
                _: &str,
                _: &str,
                _: Option<f64>,
            ) -> Result<String> {
                Ok("unused".into())
            }
            async fn chat(
                &self,
                _: ChatRequest<'_>,
                _: &str,
                _: Option<f64>,
            ) -> Result<clawcrew_providers::ChatResponse> {
                Ok(clawcrew_providers::ChatResponse {
                    text: Some("tool request".into()),
                    tool_calls: vec![clawcrew_providers::ToolCall {
                        id: "fallback-tool".into(),
                        name: "echo".into(),
                        arguments: "{}".into(),
                        extra_content: None,
                    }],
                    usage: None,
                    reasoning_content: None,
                })
            }
        }

        let reliable = clawcrew_providers::reliable::ReliableModelProvider::new(
            "test",
            vec![
                (
                    "primary".into(),
                    Box::new(PrimaryFailsOnceThenFinal(
                        std::sync::atomic::AtomicUsize::new(0),
                    )) as Box<dyn ModelProvider>,
                ),
                (
                    "fallback".into(),
                    Box::new(FallbackToolCall) as Box<dyn ModelProvider>,
                ),
            ],
            0,
            0,
        );
        let mut agent = Agent::builder()
            .model_provider(Box::new(reliable))
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(Arc::from(
                clawcrew_memory::create_memory(
                    &clawcrew_config::schema::MemoryConfig {
                        backend: "none".into(),
                        ..Default::default()
                    },
                    std::path::Path::new("/tmp"),
                    None,
                )
                .expect("test memory must initialize"),
            ))
            .observer(Arc::from(crate::observability::NoopObserver {}))
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .build()
            .expect("test agent must initialize");

        assert_eq!(
            agent.turn("run the tool").await.expect("turn must recover"),
            "primary final"
        );
    }

    #[tokio::test]
    async fn turn_streamed_rejects_blank_input() {
        let model_provider = Box::new(MockModelProvider {
            responses: Mutex::new(Vec::new()),
        });
        let mut agent = blank_input_agent(model_provider);
        let (event_tx, _event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(8);
        let err = agent
            .turn_streamed("", event_tx, None)
            .await
            .expect_err("blank streamed turn must fail");
        assert_eq!(err.to_string(), BLANK_TURN_ERROR);
    }

    #[tokio::test]
    async fn turn_streamed_rejects_whitespace_only_input() {
        let model_provider = Box::new(MockModelProvider {
            responses: Mutex::new(Vec::new()),
        });
        let mut agent = blank_input_agent(model_provider);
        let (event_tx, _event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(8);
        let err = agent
            .turn_streamed("  \n", event_tx, None)
            .await
            .expect_err("whitespace-only streamed turn must fail");
        assert_eq!(err.to_string(), BLANK_TURN_ERROR);
    }

    struct ModelCaptureModelProvider {
        responses: Mutex<Vec<clawcrew_providers::ChatResponse>>,
        seen_models: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl ModelProvider for ModelCaptureModelProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<String> {
            Ok("ok".into())
        }

        async fn chat(
            &self,
            _request: ChatRequest<'_>,
            model: &str,
            _temperature: Option<f64>,
        ) -> Result<clawcrew_providers::ChatResponse> {
            self.seen_models.lock().push(model.to_string());
            let mut guard = self.responses.lock();
            if guard.is_empty() {
                return Ok(clawcrew_providers::ChatResponse {
                    text: Some("done".into()),
                    tool_calls: vec![],
                    usage: None,
                    reasoning_content: None,
                });
            }
            Ok(guard.remove(0))
        }
    }
    impl ::clawcrew_api::attribution::Attributable for ModelCaptureModelProvider {
        fn role(&self) -> ::clawcrew_api::attribution::Role {
            ::clawcrew_api::attribution::Role::Provider(
                ::clawcrew_api::attribution::ProviderKind::Model(
                    ::clawcrew_api::attribution::ModelProviderKind::Custom,
                ),
            )
        }
        fn alias(&self) -> &str {
            "ModelCaptureModelProvider"
        }
    }

    struct TranscriptCaptureModelProvider {
        alias: String,
        responses: Mutex<Vec<clawcrew_providers::ChatResponse>>,
        seen_messages: Arc<Mutex<Vec<Vec<ChatMessage>>>>,
    }

    #[async_trait]
    impl ModelProvider for TranscriptCaptureModelProvider {
        fn has_stable_request_identity(&self, _model: &str) -> bool {
            true
        }

        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<String> {
            Ok("ok".into())
        }

        async fn chat(
            &self,
            request: ChatRequest<'_>,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<clawcrew_providers::ChatResponse> {
            self.seen_messages.lock().push(request.messages.to_vec());
            let mut responses = self.responses.lock();
            if responses.is_empty() {
                return Ok(clawcrew_providers::ChatResponse {
                    text: Some("done".into()),
                    tool_calls: vec![],
                    usage: None,
                    reasoning_content: None,
                });
            }
            Ok(responses.remove(0))
        }
    }

    impl ::clawcrew_api::attribution::Attributable for TranscriptCaptureModelProvider {
        fn role(&self) -> ::clawcrew_api::attribution::Role {
            ::clawcrew_api::attribution::Role::Provider(
                ::clawcrew_api::attribution::ProviderKind::Model(
                    ::clawcrew_api::attribution::ModelProviderKind::Custom,
                ),
            )
        }
        fn alias(&self) -> &str {
            &self.alias
        }
    }

    struct AlwaysFailModelProvider {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl ModelProvider for AlwaysFailModelProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            anyhow::bail!("synthetic primary failure")
        }
    }

    impl ::clawcrew_api::attribution::Attributable for AlwaysFailModelProvider {
        fn role(&self) -> ::clawcrew_api::attribution::Role {
            ::clawcrew_api::attribution::Role::Provider(
                ::clawcrew_api::attribution::ProviderKind::Model(
                    ::clawcrew_api::attribution::ModelProviderKind::Custom,
                ),
            )
        }

        fn alias(&self) -> &str {
            "always-fail"
        }
    }

    struct CountingAnswerModelProvider {
        calls: Arc<AtomicUsize>,
        answer: String,
    }

    struct ContextWindowModelProvider {
        calls: Arc<AtomicUsize>,
        answer: String,
        reject_full_context: bool,
    }

    #[async_trait]
    impl ModelProvider for ContextWindowModelProvider {
        fn has_stable_request_identity(&self, _model: &str) -> bool {
            true
        }

        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<String> {
            unreachable!("response-cache regression uses the structured chat path")
        }

        async fn chat(
            &self,
            request: ChatRequest<'_>,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<clawcrew_providers::ChatResponse> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.reject_full_context && request.messages.len() > 2 {
                anyhow::bail!("input exceeds the context window of this model")
            }
            Ok(clawcrew_providers::ChatResponse {
                text: Some(self.answer.clone()),
                tool_calls: vec![],
                usage: None,
                reasoning_content: None,
            })
        }
    }

    impl ::clawcrew_api::attribution::Attributable for ContextWindowModelProvider {
        fn role(&self) -> ::clawcrew_api::attribution::Role {
            ::clawcrew_api::attribution::Role::Provider(
                ::clawcrew_api::attribution::ProviderKind::Model(
                    ::clawcrew_api::attribution::ModelProviderKind::Custom,
                ),
            )
        }

        fn alias(&self) -> &str {
            "context-window-provider"
        }
    }

    #[async_trait]
    impl ModelProvider for CountingAnswerModelProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.answer.clone())
        }
    }

    impl ::clawcrew_api::attribution::Attributable for CountingAnswerModelProvider {
        fn role(&self) -> ::clawcrew_api::attribution::Role {
            ::clawcrew_api::attribution::Role::Provider(
                ::clawcrew_api::attribution::ProviderKind::Model(
                    ::clawcrew_api::attribution::ModelProviderKind::Custom,
                ),
            )
        }

        fn alias(&self) -> &str {
            "counting-answer"
        }
    }

    struct CountingSafeguardModelProvider {
        calls: Arc<AtomicUsize>,
        answer: String,
    }

    #[async_trait]
    impl ModelProvider for CountingSafeguardModelProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            clawcrew_providers::commit_safeguard_fallback(Some(
                clawcrew_providers::SafeguardFallbackNotice {
                    kind: clawcrew_providers::SafeguardFallbackKind::ServerSide,
                    requested_model: "claude-sonnet-4-6".to_string(),
                    served_model: "server-fallback-model".to_string(),
                    category: None,
                },
            ));
            Ok(self.answer.clone())
        }

        async fn chat(
            &self,
            _request: ChatRequest<'_>,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<clawcrew_providers::ChatResponse> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            clawcrew_providers::commit_safeguard_fallback(Some(
                clawcrew_providers::SafeguardFallbackNotice {
                    kind: clawcrew_providers::SafeguardFallbackKind::ServerSide,
                    requested_model: "claude-sonnet-4-6".to_string(),
                    served_model: "server-fallback-model".to_string(),
                    category: None,
                },
            ));
            Ok(clawcrew_providers::ChatResponse {
                text: Some(self.answer.clone()),
                tool_calls: vec![],
                usage: None,
                reasoning_content: None,
            })
        }

        fn supports_streaming(&self) -> bool {
            true
        }

        fn stream_chat(
            &self,
            _request: ChatRequest<'_>,
            _model: &str,
            _temperature: Option<f64>,
            _options: clawcrew_providers::traits::StreamOptions,
        ) -> futures_util::stream::BoxStream<
            'static,
            clawcrew_api::model_provider::StreamResult<clawcrew_api::model_provider::StreamEvent>,
        > {
            self.calls.fetch_add(1, Ordering::SeqCst);
            clawcrew_providers::commit_safeguard_fallback(Some(
                clawcrew_providers::SafeguardFallbackNotice {
                    kind: clawcrew_providers::SafeguardFallbackKind::ServerSide,
                    requested_model: "claude-sonnet-4-6".to_string(),
                    served_model: "server-fallback-model".to_string(),
                    category: None,
                },
            ));
            let delta = self.answer.clone();
            Box::pin(futures_util::stream::iter(vec![
                Ok(clawcrew_api::model_provider::StreamEvent::TextDelta(
                    clawcrew_api::model_provider::StreamChunk::delta(delta),
                )),
                Ok(clawcrew_api::model_provider::StreamEvent::Final),
            ]))
        }
    }

    impl ::clawcrew_api::attribution::Attributable for CountingSafeguardModelProvider {
        fn role(&self) -> ::clawcrew_api::attribution::Role {
            ::clawcrew_api::attribution::Role::Provider(
                ::clawcrew_api::attribution::ProviderKind::Model(
                    ::clawcrew_api::attribution::ModelProviderKind::Anthropic,
                ),
            )
        }

        fn alias(&self) -> &str {
            "counting-safeguard"
        }
    }

    struct CancellingBeforeLlmHook;

    #[async_trait]
    impl crate::hooks::HookHandler for CancellingBeforeLlmHook {
        fn name(&self) -> &str {
            "cancel-before-llm"
        }

        async fn before_llm_call(
            &self,
            _messages: &mut Vec<ChatMessage>,
            _model: &mut String,
        ) -> crate::hooks::HookResult<()> {
            crate::hooks::HookResult::Cancel("blocked by request policy".into())
        }
    }

    type CapturedLlmInputs = Arc<Mutex<Vec<(Vec<ChatMessage>, String)>>>;

    struct MutatingBeforeLlmHook {
        seen_inputs: CapturedLlmInputs,
    }

    #[async_trait]
    impl crate::hooks::HookHandler for MutatingBeforeLlmHook {
        fn name(&self) -> &str {
            "mutate-before-llm"
        }

        async fn before_llm_call(
            &self,
            messages: &mut Vec<ChatMessage>,
            model: &mut String,
        ) -> crate::hooks::HookResult<()> {
            let last_user = messages
                .iter_mut()
                .rev()
                .find(|message| message.role == "user")
                .expect("request must contain the current user message");
            last_user.content = "hook-only provider request".into();
            *model = "hook-selected-model".into();
            crate::hooks::HookResult::Continue(())
        }

        async fn on_llm_input(&self, messages: &[ChatMessage], model: &str) {
            self.seen_inputs
                .lock()
                .push((messages.to_vec(), model.to_string()));
        }
    }

    struct SelectingBeforeLlmHook {
        model: String,
        system_suffix: Option<String>,
    }

    struct UnchangedBeforeLlmHook;

    #[async_trait]
    impl crate::hooks::HookHandler for UnchangedBeforeLlmHook {
        fn name(&self) -> &str {
            "unchanged-before-llm"
        }

        async fn before_llm_call(
            &self,
            _messages: &mut Vec<ChatMessage>,
            _model: &mut String,
        ) -> crate::hooks::HookResult<()> {
            crate::hooks::HookResult::Continue(())
        }
    }

    #[async_trait]
    impl crate::hooks::HookHandler for SelectingBeforeLlmHook {
        fn name(&self) -> &str {
            "select-before-llm"
        }

        async fn before_llm_call(
            &self,
            messages: &mut Vec<ChatMessage>,
            model: &mut String,
        ) -> crate::hooks::HookResult<()> {
            if let Some(suffix) = &self.system_suffix
                && let Some(system) = messages.iter_mut().find(|message| message.role == "system")
            {
                system.content.push_str(suffix);
            }
            *model = self.model.clone();
            crate::hooks::HookResult::Continue(())
        }
    }

    type CapturedToolProtocolRequests = Arc<Mutex<Vec<(String, bool, Vec<ChatMessage>)>>>;

    struct HookProtocolCaptureProvider {
        supports_native: bool,
        requests: CapturedToolProtocolRequests,
    }

    #[async_trait]
    impl ModelProvider for HookProtocolCaptureProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<String> {
            Ok("unexpected legacy request".into())
        }

        async fn chat(
            &self,
            request: ChatRequest<'_>,
            model: &str,
            _temperature: Option<f64>,
        ) -> Result<clawcrew_providers::ChatResponse> {
            self.requests.lock().push((
                model.to_string(),
                request.tools.is_some(),
                request.messages.to_vec(),
            ));
            Ok(clawcrew_providers::ChatResponse {
                text: Some("routed response".into()),
                tool_calls: vec![],
                usage: None,
                reasoning_content: None,
            })
        }

        fn supports_native_tools(&self) -> bool {
            self.supports_native
        }
    }

    impl ::clawcrew_api::attribution::Attributable for HookProtocolCaptureProvider {
        fn role(&self) -> ::clawcrew_api::attribution::Role {
            ::clawcrew_api::attribution::Role::Provider(
                ::clawcrew_api::attribution::ProviderKind::Model(
                    ::clawcrew_api::attribution::ModelProviderKind::Custom,
                ),
            )
        }

        fn alias(&self) -> &str {
            "HookProtocolCaptureProvider"
        }
    }

    struct StreamingSteeringModelProvider {
        seen_messages: Arc<Mutex<Vec<Vec<ChatMessage>>>>,
        call_count: AtomicUsize,
        fail_on_call: Option<usize>,
        fail_chat_on_call: Option<usize>,
        fail_after_delta_on_call: Option<usize>,
        delay_chat_on_call: Option<usize>,
    }

    #[async_trait]
    impl ModelProvider for StreamingSteeringModelProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<String> {
            Ok("ok".into())
        }

        async fn chat(
            &self,
            request: ChatRequest<'_>,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<clawcrew_providers::ChatResponse> {
            let call = self.call_count.fetch_add(1, Ordering::SeqCst) + 1;
            self.seen_messages.lock().push(request.messages.to_vec());
            if self.delay_chat_on_call == Some(call) {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            }
            if self.fail_on_call == Some(call) {
                anyhow::bail!("synthetic provider failure on call {call}");
            }
            if self.fail_chat_on_call == Some(call) {
                anyhow::bail!("synthetic chat failure on call {call}");
            }
            if self.fail_after_delta_on_call == Some(call) {
                anyhow::bail!("synthetic provider failure after delta on call {call}");
            }
            Ok(clawcrew_providers::ChatResponse {
                text: Some(if call == 1 { "draft" } else { "final" }.into()),
                tool_calls: vec![],
                usage: None,
                reasoning_content: None,
            })
        }

        fn supports_streaming(&self) -> bool {
            true
        }

        fn stream_chat(
            &self,
            request: ChatRequest<'_>,
            _model: &str,
            _temperature: Option<f64>,
            _options: clawcrew_providers::traits::StreamOptions,
        ) -> futures_util::stream::BoxStream<
            'static,
            clawcrew_providers::traits::StreamResult<clawcrew_providers::traits::StreamEvent>,
        > {
            use futures_util::StreamExt as _;

            let call = self.call_count.fetch_add(1, Ordering::SeqCst) + 1;
            self.seen_messages.lock().push(request.messages.to_vec());
            let should_fail = self.fail_on_call == Some(call);
            let should_fail_after_delta = self.fail_after_delta_on_call == Some(call);
            let delta = if call == 1 { "draft" } else { "final" }.to_string();
            futures_util::stream::unfold(0, move |step| {
                let delta = delta.clone();
                async move {
                    match step {
                        0 if should_fail => Some((
                            Err(clawcrew_providers::traits::StreamError::ModelProvider(
                                "synthetic provider failure".into(),
                            )),
                            1,
                        )),
                        0 => Some((
                            Ok(clawcrew_providers::traits::StreamEvent::TextDelta(
                                clawcrew_providers::traits::StreamChunk {
                                    delta,
                                    is_final: false,
                                    reasoning: None,
                                    token_count: 0,
                                },
                            )),
                            1,
                        )),
                        1 if should_fail_after_delta => Some((
                            Err(clawcrew_providers::traits::StreamError::ModelProvider(
                                "synthetic provider failure after delta".into(),
                            )),
                            2,
                        )),
                        1 => {
                            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                            Some((Ok(clawcrew_providers::traits::StreamEvent::Final), 2))
                        }
                        _ => None,
                    }
                }
            })
            .boxed()
        }
    }

    impl ::clawcrew_api::attribution::Attributable for StreamingSteeringModelProvider {
        fn role(&self) -> ::clawcrew_api::attribution::Role {
            ::clawcrew_api::attribution::Role::Provider(
                ::clawcrew_api::attribution::ProviderKind::Model(
                    ::clawcrew_api::attribution::ModelProviderKind::Custom,
                ),
            )
        }
        fn alias(&self) -> &str {
            "StreamingSteeringModelProvider"
        }
    }

    #[derive(Default)]
    struct CapturingObserver {
        events: parking_lot::Mutex<Vec<ObserverEvent>>,
    }

    fn fixed_response_cache_turn_datetime() -> chrono::DateTime<chrono::Local> {
        chrono::Local
            .with_ymd_and_hms(2026, 6, 25, 12, 0, 0)
            .single()
            .expect("fixed local test timestamp")
    }

    impl Observer for CapturingObserver {
        fn record_event(&self, event: &ObserverEvent) {
            self.events.lock().push(event.clone());
        }
        fn record_metric(&self, _metric: &ObserverMetric) {}
        fn name(&self) -> &str {
            "capturing"
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn flush(&self) {}
    }

    struct MultimodalCaptureProvider {
        seen_user_messages: Arc<Mutex<Vec<String>>>,
        streamed: bool,
    }

    #[async_trait]
    impl ModelProvider for MultimodalCaptureProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<String> {
            Ok("ok".into())
        }

        async fn chat(
            &self,
            request: ChatRequest<'_>,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<clawcrew_providers::ChatResponse> {
            if let Some(message) = request.messages.iter().rfind(|msg| msg.role == "user") {
                self.seen_user_messages.lock().push(message.content.clone());
            }
            Ok(clawcrew_providers::ChatResponse {
                text: Some("done".into()),
                tool_calls: vec![],
                usage: None,
                reasoning_content: None,
            })
        }

        fn stream_chat(
            &self,
            request: ChatRequest<'_>,
            _model: &str,
            _temperature: Option<f64>,
            _options: clawcrew_providers::traits::StreamOptions,
        ) -> futures_util::stream::BoxStream<
            'static,
            clawcrew_providers::traits::StreamResult<clawcrew_providers::traits::StreamEvent>,
        > {
            use futures_util::stream::{self, StreamExt};

            if let Some(message) = request.messages.iter().rfind(|msg| msg.role == "user") {
                self.seen_user_messages.lock().push(message.content.clone());
            }

            if self.streamed {
                let chunk = clawcrew_providers::traits::StreamEvent::TextDelta(
                    clawcrew_providers::traits::StreamChunk {
                        delta: "stream-done".into(),
                        is_final: false,
                        reasoning: None,
                        token_count: 0,
                    },
                );
                stream::iter(vec![
                    Ok(chunk),
                    Ok(clawcrew_providers::traits::StreamEvent::Final),
                ])
                .boxed()
            } else {
                stream::iter(vec![Ok(clawcrew_providers::traits::StreamEvent::Final)]).boxed()
            }
        }

        fn supports_vision(&self) -> bool {
            true
        }
    }
    impl ::clawcrew_api::attribution::Attributable for MultimodalCaptureProvider {
        fn role(&self) -> ::clawcrew_api::attribution::Role {
            ::clawcrew_api::attribution::Role::Provider(
                ::clawcrew_api::attribution::ProviderKind::Model(
                    ::clawcrew_api::attribution::ModelProviderKind::Custom,
                ),
            )
        }
        fn alias(&self) -> &str {
            "MultimodalCaptureProvider"
        }
    }

    struct MockTool;

    #[async_trait]
    impl Tool for MockTool {
        fn name(&self) -> &str {
            "echo"
        }

        fn description(&self) -> &str {
            "echo"
        }

        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }

        async fn execute(&self, _args: serde_json::Value) -> Result<crate::tools::ToolResult> {
            Ok(crate::tools::ToolResult {
                success: true,
                output: "tool-out".into(),
                error: None,
            })
        }
    }

    #[test]
    fn direct_agent_turn_does_not_write_intermediate_native_text_to_stdout() {
        let current_exe = std::env::current_exe().expect("current test binary path");
        let output = std::process::Command::new(current_exe)
            .args([
                "direct_agent_turn_stdout_boundary_helper_4721",
                "--ignored",
                "--nocapture",
            ])
            .output()
            .expect("helper test process should run");

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert!(
            output.status.success(),
            "helper failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
            output.status.code(),
            stdout,
            stderr
        );
        assert!(
            !stdout.contains("intermediate native narration"),
            "intermediate native narration leaked to stdout:\n{stdout}"
        );
        assert!(
            stderr.contains("intermediate native narration"),
            "intermediate native narration was not routed to stderr:\n{stderr}"
        );
    }

    #[tokio::test]
    #[ignore = "subprocess helper for stdout/stderr boundary regression"]
    async fn direct_agent_turn_stdout_boundary_helper_4721() {
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );

        let model_provider = Box::new(MockModelProvider {
            responses: Mutex::new(vec![
                clawcrew_providers::ChatResponse {
                    text: Some("intermediate native narration".into()),
                    tool_calls: vec![clawcrew_providers::ToolCall {
                        id: "tc1".into(),
                        name: "echo".into(),
                        arguments: "{}".into(),
                        extra_content: None,
                    }],
                    usage: None,
                    reasoning_content: None,
                },
                clawcrew_providers::ChatResponse {
                    text: Some("final answer".into()),
                    tool_calls: vec![],
                    usage: None,
                    reasoning_content: None,
                },
            ]),
        });

        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = Agent::builder()
            .model_provider(model_provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .build()
            .expect("agent builder should succeed with valid config");

        let answer = agent
            .turn("run the tool")
            .await
            .expect("turn should finish");
        assert_eq!(answer, "final answer");
    }

    struct FailingModelProvider;

    #[async_trait]
    impl ModelProvider for FailingModelProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<String> {
            Err(anyhow::Error::msg("provider unavailable"))
        }

        async fn chat(
            &self,
            _request: ChatRequest<'_>,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<clawcrew_providers::ChatResponse> {
            Err(anyhow::Error::msg("provider unavailable"))
        }
    }

    impl ::clawcrew_api::attribution::Attributable for FailingModelProvider {
        fn role(&self) -> ::clawcrew_api::attribution::Role {
            ::clawcrew_api::attribution::Role::Provider(
                ::clawcrew_api::attribution::ProviderKind::Model(
                    ::clawcrew_api::attribution::ModelProviderKind::Custom,
                ),
            )
        }
        fn alias(&self) -> &str {
            "FailingModelProvider"
        }
    }

    struct FailingPromptSection;

    impl crate::agent::prompt::PromptSection for FailingPromptSection {
        fn name(&self) -> &str {
            "failing-test-section"
        }

        fn build(&self, _ctx: &PromptContext<'_>) -> Result<String> {
            Err(anyhow::Error::msg("synthetic prompt rebuild failure"))
        }
    }

    struct ToolThenFailingModelProvider {
        calls: std::sync::atomic::AtomicUsize,
    }

    #[async_trait]
    impl ModelProvider for ToolThenFailingModelProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<String> {
            Err(anyhow::Error::msg("provider unavailable after tool"))
        }

        async fn chat(
            &self,
            _request: ChatRequest<'_>,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<clawcrew_providers::ChatResponse> {
            if self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                return Ok(clawcrew_providers::ChatResponse {
                    text: Some("running tool".into()),
                    tool_calls: vec![clawcrew_providers::ToolCall {
                        id: "error-path-call".into(),
                        name: "echo".into(),
                        arguments: "{}".into(),
                        extra_content: None,
                    }],
                    usage: None,
                    reasoning_content: None,
                });
            }
            Err(anyhow::Error::msg("provider unavailable after tool"))
        }
    }

    impl ::clawcrew_api::attribution::Attributable for ToolThenFailingModelProvider {
        fn role(&self) -> ::clawcrew_api::attribution::Role {
            ::clawcrew_api::attribution::Role::Provider(
                ::clawcrew_api::attribution::ProviderKind::Model(
                    ::clawcrew_api::attribution::ModelProviderKind::Custom,
                ),
            )
        }

        fn alias(&self) -> &str {
            "ToolThenFailingModelProvider"
        }
    }

    struct SlowTool;

    #[async_trait]
    impl Tool for SlowTool {
        fn name(&self) -> &str {
            "echo"
        }

        fn description(&self) -> &str {
            "echo"
        }

        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }

        async fn execute(&self, _args: serde_json::Value) -> Result<crate::tools::ToolResult> {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            Ok(crate::tools::ToolResult {
                success: true,
                output: "tool-out".into(),
                error: None,
            })
        }
    }

    struct CountingTool {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl Tool for CountingTool {
        fn name(&self) -> &str {
            "echo"
        }

        fn description(&self) -> &str {
            "echo"
        }

        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }

        async fn execute(&self, _args: serde_json::Value) -> Result<crate::tools::ToolResult> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(crate::tools::ToolResult {
                success: true,
                output: "tool-out".into(),
                error: None,
            })
        }
    }

    #[tokio::test]
    async fn turn_without_tools_returns_text() {
        let model_provider = Box::new(MockModelProvider {
            responses: Mutex::new(vec![clawcrew_providers::ChatResponse {
                text: Some("hello".into()),
                tool_calls: vec![],
                usage: None,
                reasoning_content: None,
            }]),
        });

        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );

        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = Agent::builder()
            .model_provider(model_provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(XmlToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .build()
            .expect("agent builder should succeed with valid config");

        let response = agent.turn("hi").await.unwrap();
        assert_eq!(response, "hello");
    }

    #[tokio::test]
    async fn direct_agent_strict_tool_parsing_ignores_xml_dispatcher_calls() {
        let provider = Box::new(MockModelProvider {
            responses: Mutex::new(vec![clawcrew_providers::ChatResponse {
                text: Some(
                    r#"<tool_call>{"name":"echo","arguments":{"value":"ignored"}}</tool_call>"#
                        .into(),
                ),
                tool_calls: vec![],
                usage: None,
                reasoning_content: None,
            }]),
        });

        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );
        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let calls = Arc::new(AtomicUsize::new(0));
        let agent_config = clawcrew_config::schema::AliasedAgentConfig {
            resolved: clawcrew_config::schema::ResolvedRuntime {
                strict_tool_parsing: true,
                ..Default::default()
            },
            ..clawcrew_config::schema::AliasedAgentConfig::default()
        };
        let mut agent = Agent::builder()
            .model_provider(provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(CountingTool {
                    calls: Arc::clone(&calls),
                })],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(XmlToolDispatcher))
            .config(agent_config)
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .build()
            .expect("agent builder should succeed with valid config");

        let system_prompt = agent
            .build_system_prompt()
            .expect("system prompt should render");
        assert!(
            !system_prompt.contains("## Tools"),
            "strict parsing should not advertise text tool instructions"
        );
        assert!(
            !system_prompt.contains("<tool_call"),
            "strict parsing should not advertise XML tool calls"
        );

        let response = agent.turn("hi").await.unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(response.contains("<tool_call>"));
    }

    #[test]
    fn native_agent_prompt_omits_duplicate_tools_section() {
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let workspace = tempfile::TempDir::new().expect("temp dir");
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, workspace.path(), None)
                .expect("memory creation should succeed with valid config"),
        );
        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});

        let native_agent = Agent::builder()
            .model_provider(Box::new(MockModelProvider {
                responses: Mutex::new(vec![]),
            }))
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(Arc::clone(&mem))
            .observer(Arc::clone(&observer))
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(workspace.path().to_path_buf())
            .build()
            .expect("agent builder should succeed with valid config");
        let native_prompt = native_agent.build_system_prompt().unwrap();
        assert!(!native_prompt.contains("## Tools"));
        assert!(!native_prompt.contains("echo"));

        let xml_agent = Agent::builder()
            .model_provider(Box::new(MockModelProvider {
                responses: Mutex::new(vec![]),
            }))
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(XmlToolDispatcher))
            .workspace_dir(workspace.path().to_path_buf())
            .build()
            .expect("agent builder should succeed with valid config");
        let xml_prompt = xml_agent.build_system_prompt().unwrap();
        assert!(xml_prompt.contains("## Tools"));
        assert!(xml_prompt.contains("echo"));
        assert!(xml_prompt.contains("## Tool Use Protocol"));
    }

    #[test]
    fn approval_manager_policy_overrides_legacy_builder_autonomy() {
        let workspace = tempfile::TempDir::new().expect("temp dir");
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, workspace.path(), None)
                .expect("memory creation should succeed"),
        );
        let risk_profile = clawcrew_config::schema::RiskProfileConfig {
            level: crate::security::AutonomyLevel::Full,
            always_ask: vec!["shell".into()],
            ..Default::default()
        };
        let manager = Arc::new(ApprovalManager::for_non_interactive(&risk_profile));

        let agent = Agent::builder()
            .model_provider(Box::new(MockModelProvider {
                responses: Mutex::new(vec![]),
            }))
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![],
            ))
            .memory(Arc::clone(&mem))
            .observer(Arc::new(crate::observability::NoopObserver {}))
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .prompt_builder(SystemPromptBuilder::with_defaults())
            .workspace_dir(workspace.path().to_path_buf())
            // Deliberately conflict with the manager to prove this restored
            // public builder method is only a managerless fallback.
            .autonomy_level(crate::security::AutonomyLevel::Supervised)
            .approval_manager(Some(manager))
            .build()
            .expect("agent builder should succeed");

        let prompt = agent.build_system_prompt().expect("prompt should render");
        assert!(
            prompt.contains("Full autonomy auto-approves tools")
                && prompt.contains("shell")
                && prompt.contains("still require operator approval"),
            "manager-owned Full/always_ask policy must win: {prompt}"
        );
        assert!(
            !prompt.contains("Ask for approval when the runtime policy requires it"),
            "legacy Supervised fallback must not override the manager: {prompt}"
        );

        let managerless = Agent::builder()
            .model_provider(Box::new(MockModelProvider {
                responses: Mutex::new(vec![]),
            }))
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![],
            ))
            .memory(mem)
            .observer(Arc::new(crate::observability::NoopObserver {}))
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .prompt_builder(SystemPromptBuilder::with_defaults())
            .workspace_dir(workspace.path().to_path_buf())
            .autonomy_level(crate::security::AutonomyLevel::Full)
            .build()
            .expect("managerless agent builder should succeed");
        let prompt = managerless
            .build_system_prompt()
            .expect("managerless prompt should render");
        assert!(
            prompt.contains("Full autonomy auto-approves tools")
                && prompt.contains("No tools are listed in `always_ask`"),
            "legacy builder fallback must still render Full autonomy: {prompt}"
        );
    }

    mod surface2_tests {
        use super::*;
        use crate::agent::dispatcher::{NativeToolDispatcher, XmlToolDispatcher};

        /// Marker text produced by the section-based prompt builder when tools
        /// are advertised as XML/text instructions rather than native tool specs.
        const XML_TOOLS_MARKER: &str = "## Tools";
        type CapturedTranscripts = Arc<Mutex<Vec<Vec<ChatMessage>>>>;

        /// Test provider that captures the provider-visible transcript and
        /// reports a configurable native-tool capability.
        struct CapturingModelProvider {
            responses: Mutex<Vec<clawcrew_providers::ChatResponse>>,
            supports_native: bool,
            captured_messages: CapturedTranscripts,
        }

        #[async_trait]
        impl ModelProvider for CapturingModelProvider {
            async fn chat_with_system(
                &self,
                _system_prompt: Option<&str>,
                _message: &str,
                _model: &str,
                _temperature: Option<f64>,
            ) -> Result<String> {
                Ok("ok".into())
            }

            async fn chat(
                &self,
                request: ChatRequest<'_>,
                _model: &str,
                _temperature: Option<f64>,
            ) -> Result<clawcrew_providers::ChatResponse> {
                self.captured_messages
                    .lock()
                    .push(request.messages.to_vec());
                let mut guard = self.responses.lock();
                if guard.is_empty() {
                    return Ok(clawcrew_providers::ChatResponse {
                        text: Some("done".into()),
                        tool_calls: vec![],
                        usage: None,
                        reasoning_content: None,
                    });
                }
                Ok(guard.remove(0))
            }

            fn supports_native_tools(&self) -> bool {
                self.supports_native
            }
        }

        impl ::clawcrew_api::attribution::Attributable for CapturingModelProvider {
            fn role(&self) -> ::clawcrew_api::attribution::Role {
                ::clawcrew_api::attribution::Role::Provider(
                    ::clawcrew_api::attribution::ProviderKind::Model(
                        ::clawcrew_api::attribution::ModelProviderKind::Custom,
                    ),
                )
            }
            fn alias(&self) -> &str {
                "CapturingModelProvider"
            }
        }

        fn capturing_provider(
            supports_native: bool,
        ) -> (Box<dyn ModelProvider>, CapturedTranscripts) {
            let captured: CapturedTranscripts = Arc::new(Mutex::new(Vec::new()));
            (
                Box::new(CapturingModelProvider {
                    responses: Mutex::new(vec![]),
                    supports_native,
                    captured_messages: Arc::clone(&captured),
                }),
                captured,
            )
        }

        #[test]
        fn dispatcher_selection_uses_the_selected_routes_tool_capability() {
            let (native_default, _) = capturing_provider(true);
            let (text_route, _) = capturing_provider(false);
            let router = clawcrew_providers::router::RouterModelProvider::new(
                "test",
                vec![
                    ("default".to_string(), native_default),
                    ("text".to_string(), text_route),
                ],
                vec![(
                    "text".to_string(),
                    clawcrew_providers::router::Route {
                        provider_name: "text".to_string(),
                        model: "text-model".to_string(),
                    },
                )],
                "native-model".to_string(),
            );
            let config = clawcrew_config::schema::AliasedAgentConfig::default();

            assert!(
                tool_dispatcher_for_provider(&config, &router, "native-model")
                    .should_send_tool_specs(),
                "the default native route must select the native dispatcher"
            );
            assert!(
                !tool_dispatcher_for_provider(&config, &router, "hint:text")
                    .should_send_tool_specs(),
                "the hinted text-only route must select the XML dispatcher"
            );
        }

        fn test_agent_with_provider(
            provider: Box<dyn ModelProvider>,
            tools: Vec<Box<dyn Tool>>,
        ) -> Agent {
            test_agent_with_provider_and_multimodal(provider, tools, None, None)
        }

        fn test_agent_with_provider_and_multimodal(
            provider: Box<dyn ModelProvider>,
            tools: Vec<Box<dyn Tool>>,
            tool_dispatcher: Option<Box<dyn ToolDispatcher>>,
            multimodal_config: Option<clawcrew_config::schema::MultimodalConfig>,
        ) -> Agent {
            let memory_cfg = clawcrew_config::schema::MemoryConfig {
                backend: "none".into(),
                ..clawcrew_config::schema::MemoryConfig::default()
            };
            let workspace = tempfile::TempDir::new().expect("temp dir");
            let mem: Arc<dyn Memory> = Arc::from(
                clawcrew_memory::create_memory(&memory_cfg, workspace.path(), None)
                    .expect("memory creation should succeed"),
            );
            let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
            let mut builder = Agent::builder()
                .model_provider(provider)
                .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                    tools,
                ))
                .memory(mem)
                .observer(observer)
                .workspace_dir(workspace.path().to_path_buf());
            if let Some(dispatcher) = tool_dispatcher {
                builder = builder.tool_dispatcher(dispatcher);
            } else {
                builder = builder.tool_dispatcher(Box::new(NativeToolDispatcher));
            }
            if let Some(mm) = multimodal_config {
                builder = builder.multimodal_config(mm);
            }
            builder.build().expect("agent builder should succeed")
        }

        /// Builds an Agent over a temp workspace holding a `MEMORY.md` sentinel
        /// and a `SOUL.md` control, at the given `exclude_memory` setting, and
        /// returns its assembled system prompt.
        fn system_prompt_with_memory_sentinel(exclude_memory: bool) -> String {
            let workspace = tempfile::TempDir::new().expect("temp dir");
            std::fs::write(
                workspace.path().join("MEMORY.md"),
                "MEMORY_MD_SENTINEL_9341",
            )
            .expect("write MEMORY.md");
            std::fs::write(workspace.path().join("SOUL.md"), "SOUL_MD_CONTROL_9341")
                .expect("write SOUL.md");

            let memory_cfg = clawcrew_config::schema::MemoryConfig {
                backend: "none".into(),
                ..clawcrew_config::schema::MemoryConfig::default()
            };
            let mem: Arc<dyn Memory> = Arc::from(
                clawcrew_memory::create_memory(&memory_cfg, workspace.path(), None)
                    .expect("memory creation should succeed"),
            );
            let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
            let agent = Agent::builder()
                .model_provider(Box::new(MockModelProvider {
                    responses: Mutex::new(vec![]),
                }))
                .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                    vec![Box::new(MockTool)],
                ))
                .memory(mem)
                .observer(observer)
                .tool_dispatcher(Box::new(NativeToolDispatcher))
                .workspace_dir(workspace.path().to_path_buf())
                .agent_workspace_dir(workspace.path().to_path_buf())
                .exclude_memory(exclude_memory)
                .build()
                .expect("agent builder should succeed");

            agent
                .build_system_prompt_with_dispatcher(&NativeToolDispatcher as &dyn ToolDispatcher)
                .expect("system prompt builds")
        }

        /// `exclude_memory: true` is the ACP / isolated-session policy. It
        /// already strips memory tools, installs `NoneMemory` and forces
        /// `auto_save` off; it must also keep curated `MEMORY.md` *content* out
        /// of the provider-visible system prompt, otherwise the "persistent
        /// memory isolated" copy is a claim the prompt path violates.
        #[test]
        fn build_system_prompt_omits_memory_md_when_exclude_memory() {
            let prompt = system_prompt_with_memory_sentinel(true);

            assert!(
                !prompt.contains("MEMORY_MD_SENTINEL_9341"),
                "MEMORY.md must not reach the ACP system prompt"
            );
            assert!(
                prompt.contains("SOUL_MD_CONTROL_9341"),
                "other personality files must still load under exclude_memory"
            );
        }

        /// Chat sessions (`exclude_memory: false`) are unchanged.
        #[test]
        fn build_system_prompt_includes_memory_md_without_exclude_memory() {
            let prompt = system_prompt_with_memory_sentinel(false);

            assert!(
                prompt.contains("MEMORY_MD_SENTINEL_9341"),
                "MEMORY.md must still reach the Chat system prompt"
            );
            assert!(
                prompt.contains("SOUL_MD_CONTROL_9341"),
                "SOUL.md must still reach the Chat system prompt"
            );
        }

        #[tokio::test]
        async fn streamed_agent_request_pairs_timestamp_orientation_with_labeled_user_text() {
            let (provider, captured) = capturing_provider(true);
            let mut agent = test_agent_with_provider(provider, Vec::new());
            let (event_tx, _event_rx) = tokio::sync::mpsc::channel(64);

            agent
                .turn_streamed_with_steering_state("hi from zerocode", event_tx, None, None)
                .await
                .expect("streamed Agent turn should succeed");

            let captured = captured.lock();
            let first_request = captured.first().expect("provider request captured");
            let system = first_request
                .iter()
                .find(|message| message.role == "system")
                .expect("system prompt");
            let user = first_request
                .iter()
                .find(|message| message.role == "user")
                .expect("user message");

            assert!(
                system
                    .content
                    .contains("timestamp metadata added by the runtime"),
                "Agent prompt must explain its runtime-owned user envelope"
            );
            assert!(
                user.content.starts_with("[CURRENT DATE & TIME:")
                    && user.content.ends_with("\n\nhi from zerocode"),
                "provider must receive the labeled envelope and original text: {}",
                user.content
            );
        }

        #[test]
        fn build_system_prompt_with_dispatcher_reflects_dispatcher_mode() {
            let workspace = tempfile::TempDir::new().expect("temp dir");
            let memory_cfg = clawcrew_config::schema::MemoryConfig {
                backend: "none".into(),
                ..clawcrew_config::schema::MemoryConfig::default()
            };
            let mem: Arc<dyn Memory> = Arc::from(
                clawcrew_memory::create_memory(&memory_cfg, workspace.path(), None)
                    .expect("memory creation should succeed"),
            );
            let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
            let agent = Agent::builder()
                .model_provider(Box::new(MockModelProvider {
                    responses: Mutex::new(vec![]),
                }))
                .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                    vec![Box::new(MockTool)],
                ))
                .memory(mem)
                .observer(observer)
                .tool_dispatcher(Box::new(NativeToolDispatcher))
                .workspace_dir(workspace.path().to_path_buf())
                .build()
                .expect("agent builder should succeed");

            let native_prompt = agent
                .build_system_prompt_with_dispatcher(&NativeToolDispatcher as &dyn ToolDispatcher)
                .unwrap();
            assert!(
                !native_prompt.contains(XML_TOOLS_MARKER),
                "native dispatcher must not emit XML tool listing"
            );

            let xml_prompt = agent
                .build_system_prompt_with_dispatcher(&XmlToolDispatcher as &dyn ToolDispatcher)
                .unwrap();
            assert!(
                xml_prompt.contains(XML_TOOLS_MARKER),
                "xml dispatcher must emit XML tool listing"
            );
        }

        #[test]
        fn build_system_prompt_with_dispatcher_uses_assembled_skill_tool_names() {
            let workspace = tempfile::TempDir::new().expect("temp dir");
            let memory_cfg = clawcrew_config::schema::MemoryConfig {
                backend: "none".into(),
                ..clawcrew_config::schema::MemoryConfig::default()
            };
            let mem: Arc<dyn Memory> = Arc::from(
                clawcrew_memory::create_memory(&memory_cfg, workspace.path(), None)
                    .expect("memory creation should succeed"),
            );
            let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
            let agent = Agent::builder()
                .model_provider(Box::new(MockModelProvider {
                    responses: Mutex::new(vec![]),
                }))
                .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                    vec![Box::new(NamedMockTool::new("ops__shell"))],
                ))
                .skills(vec![make_skill("ops", &["fetch", "shell"])])
                .memory(mem)
                .observer(observer)
                .tool_dispatcher(Box::new(NativeToolDispatcher))
                .workspace_dir(workspace.path().to_path_buf())
                .build()
                .expect("agent builder should succeed");

            let prompt = agent
                .build_system_prompt_with_dispatcher(&NativeToolDispatcher)
                .expect("system prompt should build");
            let callable = prompt
                .split_once("<callable_tools")
                .and_then(|(_, rest)| rest.split_once("</callable_tools>"))
                .map(|(block, _)| block)
                .expect("surviving skill tool should create callable block");

            assert!(callable.contains("<name>ops__shell</name>"));
            assert!(!callable.contains("<name>ops__fetch</name>"));
            assert!(
                prompt.contains("<name>fetch</name>"),
                "unavailable tool metadata should remain descriptive"
            );
        }

        #[test]
        fn rebuild_system_prompt_switches_to_xml_when_active_provider_non_native() {
            let (provider, _) = capturing_provider(true);
            let mut agent = test_agent_with_provider(provider, vec![Box::new(MockTool)]);

            // Seed a native-style system prompt as if the agent was built
            // against a native-capable base provider.
            let native_prompt = agent
                .build_system_prompt_with_dispatcher(&NativeToolDispatcher as &dyn ToolDispatcher)
                .unwrap();
            agent.history = vec![ConversationMessage::Chat(ChatMessage::system(
                native_prompt,
            ))];

            // Active provider for this turn does not support native tools.
            agent
                .rebuild_system_prompt_for_dispatcher(&XmlToolDispatcher)
                .expect("rebuild should succeed");

            let prompt = match &agent.history[0] {
                ConversationMessage::Chat(msg) => msg.content.clone(),
                _ => panic!("history[0] should be a chat message"),
            };
            assert!(
                prompt.contains(XML_TOOLS_MARKER),
                "prompt must be rebuilt with XML tool listing"
            );
        }

        #[test]
        fn rebuild_system_prompt_switches_to_native_when_active_provider_native() {
            let (provider, _) = capturing_provider(false);
            let mut agent = test_agent_with_provider(provider, vec![Box::new(MockTool)]);

            let xml_prompt = agent
                .build_system_prompt_with_dispatcher(&XmlToolDispatcher as &dyn ToolDispatcher)
                .unwrap();
            agent.history = vec![ConversationMessage::Chat(ChatMessage::system(xml_prompt))];

            // Active provider for this turn supports native tools.
            agent
                .rebuild_system_prompt_for_dispatcher(&NativeToolDispatcher)
                .expect("rebuild should succeed");

            let prompt = match &agent.history[0] {
                ConversationMessage::Chat(msg) => msg.content.clone(),
                _ => panic!("history[0] should be a chat message"),
            };
            assert!(
                !prompt.contains(XML_TOOLS_MARKER),
                "prompt must be rebuilt without XML tool listing"
            );
        }

        #[test]
        fn streamed_provider_switch_refreshes_active_loop_skills_prompt() {
            let workspace = tempfile::TempDir::new().expect("temp dir");
            let memory_cfg = clawcrew_config::schema::MemoryConfig {
                backend: "none".into(),
                ..clawcrew_config::schema::MemoryConfig::default()
            };
            let mem: Arc<dyn Memory> = Arc::from(
                clawcrew_memory::create_memory(&memory_cfg, workspace.path(), None)
                    .expect("memory creation should succeed"),
            );
            let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
            let (provider, _) = capturing_provider(false);
            let config = clawcrew_config::schema::AliasedAgentConfig {
                resolved: clawcrew_config::schema::ResolvedRuntime {
                    strict_tool_parsing: true,
                    ..Default::default()
                },
                ..Default::default()
            };
            let skills = vec![crate::skills::Skill {
                name: "deploy".into(),
                description: "Release safely".into(),
                description_localizations: Default::default(),
                version: "1.0.0".into(),
                author: None,
                tags: vec![],
                tools: vec![],
                prompts: vec!["Run smoke tests before deploy.".into()],
                slash_options: Vec::new(),
                always: false,
                location: None,
            }];
            let mut agent = Agent::builder()
                .model_provider(provider)
                .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                    vec![Box::new(MockTool)],
                ))
                .memory(mem)
                .observer(observer)
                .tool_dispatcher(Box::new(NativeToolDispatcher))
                .config(config)
                .skills(skills)
                .skills_prompt_mode(clawcrew_config::schema::SkillsPromptInjectionMode::Compact)
                .workspace_dir(workspace.path().to_path_buf())
                .build()
                .expect("agent builder should succeed");
            agent.history = vec![ConversationMessage::Chat(ChatMessage::system(
                "stale compact prompt",
            ))];
            let mut loop_history = vec![ChatMessage::system("stale compact prompt")];

            agent
                .rebuild_streamed_system_prompt_for_active_provider(&mut loop_history)
                .expect("streamed prompt rebuild should succeed");

            assert!(
                loop_history[0]
                    .content
                    .contains("Run smoke tests before deploy."),
                "active loop history must receive the loader-safe inlined instructions"
            );
            assert!(!loop_history[0].content.contains("read_skill(name)"));
        }

        #[tokio::test]
        async fn turn_uses_active_provider_tool_mode_for_transcript() {
            let (provider, captured) = capturing_provider(false);
            let mut agent = test_agent_with_provider(provider, vec![Box::new(MockTool)]);

            // The base provider does not support native tools, so the active
            // provider resolved by the turn path must be non-native. The
            // provider-visible transcript should reflect that.
            agent.turn("hello").await.expect("turn should succeed");

            let messages = captured.lock();
            let first_call = messages
                .first()
                .expect("provider should have received a request");
            let system = first_call
                .iter()
                .find(|m| m.role == "system")
                .expect("transcript must contain a system message");
            assert!(
                system.content.contains(XML_TOOLS_MARKER),
                "system prompt must advertise XML tools when active provider is non-native"
            );
        }

        #[tokio::test]
        async fn turn_streamed_uses_active_provider_tool_mode_for_transcript() {
            let (provider, captured) = capturing_provider(false);
            let mut agent = test_agent_with_provider(provider, vec![Box::new(MockTool)]);
            let (event_tx, _event_rx) = tokio::sync::mpsc::channel(16);

            agent
                .turn_streamed("hello", event_tx, None)
                .await
                .expect("streamed turn should succeed");

            let messages = captured.lock();
            let first_call = messages
                .first()
                .expect("provider should have received a request");
            let system = first_call
                .iter()
                .find(|m| m.role == "system")
                .expect("transcript must contain a system message");
            assert!(
                system.content.contains(XML_TOOLS_MARKER),
                "streamed system prompt must advertise XML tools when active provider is non-native"
            );
        }

        #[tokio::test]
        async fn turn_rebuilds_prompt_for_vision_routed_xml_provider() {
            // Base provider supports native tools but not vision. The configured
            // vision provider is a custom OpenAI-compatible endpoint: it supports
            // vision but not native tools.
            let (base_provider, _captured) = capturing_provider(true);
            let mm_config = clawcrew_config::schema::MultimodalConfig {
                vision_model_provider: Some("custom:http://127.0.0.1:9".into()),
                ..Default::default()
            };
            let mut agent = test_agent_with_provider_and_multimodal(
                base_provider,
                vec![Box::new(MockTool)],
                Some(Box::new(NativeToolDispatcher)),
                Some(mm_config),
            );

            let msg = "describe this image [IMAGE:data:image/png;base64,iVBORw0KGgo=]";

            // The vision provider will fail to connect to localhost:9, but the
            // prompt rebuild and provider-visible transcript happen before the
            // network call.
            let result = agent.turn(msg).await;
            assert!(
                result.is_err(),
                "vision provider chat should fail to connect"
            );

            let system_content = match &agent.history[0] {
                ConversationMessage::Chat(m) => m.content.clone(),
                _ => panic!("history[0] should be a chat message"),
            };
            assert!(
                system_content.contains(XML_TOOLS_MARKER),
                "stored system prompt must be rebuilt for XML vision provider"
            );

            let provider_messages = XmlToolDispatcher.to_provider_messages(&agent.history);
            let system = provider_messages
                .iter()
                .find(|m| m.role == "system")
                .expect("transcript must contain a system message");
            assert!(
                system.content.contains(XML_TOOLS_MARKER),
                "provider-visible transcript must advertise XML tools for vision provider"
            );
        }

        #[tokio::test]
        async fn turn_streamed_rebuilds_prompt_for_vision_routed_native_provider() {
            // Base provider does not support native tools or vision. The
            // configured vision provider is an Anthropic-compatible endpoint:
            // it supports both vision and native tools.
            let (base_provider, _captured) = capturing_provider(false);
            let mm_config = clawcrew_config::schema::MultimodalConfig {
                vision_model_provider: Some("anthropic-custom:http://127.0.0.1:9".into()),
                ..Default::default()
            };
            let mut agent = test_agent_with_provider_and_multimodal(
                base_provider,
                vec![Box::new(MockTool)],
                Some(Box::new(XmlToolDispatcher)),
                Some(mm_config),
            );

            let msg = "describe this image [IMAGE:data:image/png;base64,iVBORw0KGgo=]";
            let (event_tx, _event_rx) = tokio::sync::mpsc::channel(16);

            let result = agent.turn_streamed(msg, event_tx, None).await;
            assert!(
                result.is_err(),
                "vision provider chat should fail to connect"
            );

            let system_content = match &agent.history[0] {
                ConversationMessage::Chat(m) => m.content.clone(),
                _ => panic!("history[0] should be a chat message"),
            };
            assert!(
                !system_content.contains(XML_TOOLS_MARKER),
                "stored system prompt must be rebuilt for native vision provider"
            );

            let provider_messages = NativeToolDispatcher.to_provider_messages(&agent.history);
            let system = provider_messages
                .iter()
                .find(|m| m.role == "system")
                .expect("transcript must contain a system message");
            assert!(
                !system.content.contains(XML_TOOLS_MARKER),
                "provider-visible transcript must advertise native tools for vision provider"
            );
        }
    }

    #[tokio::test]
    async fn turn_with_native_dispatcher_handles_tool_results_variant() {
        let model_provider = Box::new(MockModelProvider {
            responses: Mutex::new(vec![
                clawcrew_providers::ChatResponse {
                    text: Some(String::new()),
                    tool_calls: vec![clawcrew_providers::ToolCall {
                        id: "tc1".into(),
                        name: "echo".into(),
                        arguments: "{}".into(),
                        extra_content: None,
                    }],
                    usage: None,
                    reasoning_content: None,
                },
                clawcrew_providers::ChatResponse {
                    text: Some("done".into()),
                    tool_calls: vec![],
                    usage: None,
                    reasoning_content: None,
                },
            ]),
        });

        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );

        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = Agent::builder()
            .model_provider(model_provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .build()
            .expect("agent builder should succeed with valid config");

        let response = agent.turn("hi").await.unwrap();
        assert_eq!(response, "done");
        assert!(
            agent
                .history()
                .iter()
                .any(|msg| matches!(msg, ConversationMessage::ToolResults(_)))
        );
    }

    #[tokio::test]
    async fn turn_routes_with_hint_when_query_classification_matches() {
        let seen_models = Arc::new(Mutex::new(Vec::new()));
        let model_provider = Box::new(ModelCaptureModelProvider {
            responses: Mutex::new(vec![clawcrew_providers::ChatResponse {
                text: Some("classified".into()),
                tool_calls: vec![],
                usage: Some(clawcrew_providers::traits::TokenUsage {
                    input_tokens: Some(100),
                    cached_input_tokens: None,
                    cache_creation_input_tokens: None,
                    output_tokens: Some(20),
                }),
                reasoning_content: None,
            }]),
            seen_models: seen_models.clone(),
        });

        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );

        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let model_route_resolver = Arc::new(clawcrew_providers::router::ModelRouteResolver::new(
            vec![(
                "fast".to_string(),
                clawcrew_providers::router::Route {
                    provider_name: "anthropic.fast".to_string(),
                    model: "anthropic/claude-haiku-4-5".to_string(),
                },
            )],
            "custom.default".to_string(),
            "default-model".to_string(),
        ));
        let mut agent = Agent::builder()
            .model_provider(model_provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .classification_config(clawcrew_config::schema::QueryClassificationConfig {
                enabled: true,
                rules: vec![clawcrew_config::schema::ClassificationRule {
                    hint: "fast".to_string(),
                    keywords: vec!["quick".to_string()],
                    patterns: vec![],
                    min_length: None,
                    max_length: None,
                    priority: 10,
                }],
            })
            .model_route_resolver(model_route_resolver)
            .build()
            .expect("agent builder should succeed with valid config");
        let resolved_routes = Arc::new(Mutex::new(Vec::new()));
        let resolved_routes_capture = Arc::clone(&resolved_routes);
        agent.context_limits_resolver = Some(Arc::new(move |provider, model| {
            resolved_routes_capture
                .lock()
                .push((provider.to_string(), model.to_string()));
            clawcrew_config::schema::ResolvedContextLimits {
                model_context_window: 200_000,
                context_token_budget: 160_000,
                model_context_window_source:
                    clawcrew_config::schema::ModelContextWindowSource::Configured,
            }
        }));

        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(8);
        let outcome = agent
            .turn_streamed_with_steering_state("quick summary please", event_tx, None, None)
            .await
            .unwrap();
        assert_eq!(outcome.response, "classified");
        assert_eq!(outcome.provider_name, "anthropic.fast");
        assert_eq!(outcome.model, "anthropic/claude-haiku-4-5");
        let seen = seen_models.lock();
        assert_eq!(seen.as_slice(), &["hint:fast".to_string()]);
        assert_eq!(
            resolved_routes.lock().as_slice(),
            &[(
                "anthropic.fast".to_string(),
                "anthropic/claude-haiku-4-5".to_string(),
            )]
        );
        let usage = std::iter::from_fn(|| event_rx.try_recv().ok())
            .find(|event| matches!(event, TurnEvent::Usage { .. }))
            .expect("the routed call should emit usage");
        assert!(matches!(
            usage,
            TurnEvent::Usage {
                context_token_budget: Some(160_000),
                model_context_window: Some(200_000),
                ..
            }
        ));
    }

    #[tokio::test]
    async fn classified_route_keeps_selector_for_tool_protocol_and_unchanged_hook() {
        let default_requests: CapturedToolProtocolRequests = Arc::new(Mutex::new(Vec::new()));
        let text_requests: CapturedToolProtocolRequests = Arc::new(Mutex::new(Vec::new()));
        let router = clawcrew_providers::router::RouterModelProvider::new(
            "classifier-router",
            vec![
                (
                    "default".into(),
                    Box::new(HookProtocolCaptureProvider {
                        supports_native: true,
                        requests: Arc::clone(&default_requests),
                    }) as Box<dyn ModelProvider>,
                ),
                (
                    "text".into(),
                    Box::new(HookProtocolCaptureProvider {
                        supports_native: false,
                        requests: Arc::clone(&text_requests),
                    }) as Box<dyn ModelProvider>,
                ),
            ],
            vec![(
                "text".into(),
                clawcrew_providers::router::Route {
                    provider_name: "text".into(),
                    model: "text-model".into(),
                },
            )],
            "native-model".into(),
        );
        let route_resolver = Arc::new(clawcrew_providers::router::ModelRouteResolver::new(
            vec![(
                "text".into(),
                clawcrew_providers::router::Route {
                    provider_name: "text".into(),
                    model: "text-model".into(),
                },
            )],
            "default".into(),
            "native-model".into(),
        ));
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let workspace = tempfile::tempdir().expect("temp workspace");
        let memory: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, workspace.path(), None)
                .expect("memory creation should succeed"),
        );
        let mut hooks = crate::hooks::HookRunner::new();
        hooks.register(Box::new(UnchangedBeforeLlmHook));
        let mut agent = Agent::builder()
            .model_provider(Box::new(router))
            .model_provider_name("classifier-router".into())
            .model_name("native-model".into())
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(memory)
            .observer(Arc::from(crate::observability::NoopObserver {}))
            .classification_config(clawcrew_config::schema::QueryClassificationConfig {
                enabled: true,
                rules: vec![clawcrew_config::schema::ClassificationRule {
                    hint: "text".into(),
                    keywords: vec!["quick".into()],
                    patterns: vec![],
                    min_length: None,
                    max_length: None,
                    priority: 10,
                }],
            })
            .model_route_resolver(route_resolver)
            .hook_runner(Some(Arc::new(hooks)))
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(workspace.path().to_path_buf())
            .build()
            .expect("agent should build");

        assert_eq!(
            agent.turn("quick routed request").await.unwrap(),
            "routed response"
        );
        assert!(
            default_requests.lock().is_empty(),
            "an unchanged hook must not replace the classifier's route selector"
        );
        let requests = text_requests.lock();
        assert_eq!(
            requests.len(),
            1,
            "the classified route must receive the request"
        );
        let (model, sent_native_tools, messages) = &requests[0];
        assert_eq!(model, "text-model");
        assert!(
            !sent_native_tools,
            "tool capability must come from the routed text provider"
        );
        let system_prompt = messages
            .iter()
            .find(|message| message.role == "system")
            .expect("provider request must include a system prompt")
            .content
            .as_str();
        assert!(system_prompt.contains("## Tools"));
        assert!(system_prompt.contains("## Tool Use Protocol"));
        assert!(system_prompt.contains("<tool_call>"));
    }

    #[tokio::test]
    async fn from_config_passes_extra_headers_to_custom_provider() {
        use axum::{Json, Router, http::HeaderMap, routing::post};
        use tempfile::TempDir;
        use tokio::net::TcpListener;

        let captured_headers: Arc<std::sync::Mutex<Option<HashMap<String, String>>>> =
            Arc::new(std::sync::Mutex::new(None));
        let captured_headers_clone = captured_headers.clone();

        let app = Router::new().route(
            "/chat/completions",
            post(
                move |headers: HeaderMap, Json(_body): Json<serde_json::Value>| {
                    let captured_headers = captured_headers_clone.clone();
                    async move {
                        let collected = headers
                            .iter()
                            .filter_map(|(name, value)| {
                                value
                                    .to_str()
                                    .ok()
                                    .map(|value| (name.as_str().to_string(), value.to_string()))
                            })
                            .collect();
                        *captured_headers.lock().unwrap() = Some(collected);
                        Json(serde_json::json!({
                            "choices": [{
                                "message": {
                                    "content": "hello from mock"
                                }
                            }]
                        }))
                    }
                },
            ),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mock_addr = listener.local_addr().unwrap();
        let server_handle = clawcrew_spawn::spawn!(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let tmp = TempDir::new().expect("temp dir");
        let workspace_dir = tmp.path().join("workspace");
        std::fs::create_dir_all(&workspace_dir).unwrap();

        let mut config = clawcrew_config::schema::Config {
            data_dir: workspace_dir,
            config_path: tmp.path().join("config.toml"),
            ..Default::default()
        };
        {
            let entry = config
                .providers
                .models
                .ensure("custom", "default")
                .expect("custom model_provider type slot");
            entry.api_key = Some("test-key".to_string());
            entry.model = Some("test-model".to_string());
            entry.uri = Some(format!("http://{mock_addr}"));
            entry.extra_headers.insert(
                "User-Agent".to_string(),
                "clawcrew-web-test/1.0".to_string(),
            );
            entry
                .extra_headers
                .insert("X-Title".to_string(), "clawcrew-web".to_string());
        }
        config.memory.backend = "none".to_string();
        config.memory.auto_save = false;

        // An explicit agent is required. Wire up a minimal agent that
        // points at the synthesized model_provider entry, then construct
        // Agent::from_config against it.
        config.risk_profiles.insert(
            "test-profile".to_string(),
            clawcrew_config::schema::RiskProfileConfig::default(),
        );
        let agent_cfg = clawcrew_config::schema::AliasedAgentConfig {
            model_provider: "custom.default".into(),
            risk_profile: "test-profile".into(),
            ..clawcrew_config::schema::AliasedAgentConfig::default()
        };
        config.agents.insert("test-agent".to_string(), agent_cfg);

        let mut agent = Agent::from_config(&config, "test-agent")
            .await
            .expect("agent from config");
        let response = agent.turn("hello").await.expect("agent turn");

        assert_eq!(response, "hello from mock");

        let headers = captured_headers
            .lock()
            .unwrap()
            .clone()
            .expect("captured headers");
        assert_eq!(
            headers.get("user-agent").map(String::as_str),
            Some("clawcrew-web-test/1.0")
        );
        assert_eq!(
            headers.get("x-title").map(String::as_str),
            Some("clawcrew-web")
        );

        server_handle.abort();
    }

    #[tokio::test]
    async fn from_config_accepts_openai_alias_with_requires_openai_auth() {
        use tempfile::TempDir;
        use clawcrew_config::schema::{
            AliasedAgentConfig, Config, ModelProviderConfig, OpenAIModelProviderConfig,
            RiskProfileConfig, WireApi,
        };

        let tmp = TempDir::new().expect("temp dir");
        let workspace_dir = tmp.path().join("workspace");
        std::fs::create_dir_all(&workspace_dir).expect("workspace dir");

        let mut config = Config {
            data_dir: workspace_dir,
            config_path: tmp.path().join("config.toml"),
            ..Default::default()
        };
        config.memory.backend = "none".to_string();
        config.memory.auto_save = false;
        config
            .risk_profiles
            .insert("test-profile".to_string(), RiskProfileConfig::default());
        config.providers.models.openai.insert(
            "codex".to_string(),
            OpenAIModelProviderConfig {
                base: ModelProviderConfig {
                    model: Some("gpt-5.4".to_string()),
                    requires_openai_auth: true,
                    wire_api: Some(WireApi::Responses),
                    ..ModelProviderConfig::default()
                },
            },
        );
        config.agents.insert(
            "test-agent".to_string(),
            AliasedAgentConfig {
                model_provider: "openai.codex".into(),
                risk_profile: "test-profile".into(),
                ..AliasedAgentConfig::default()
            },
        );

        let result = Agent::from_config(&config, "test-agent").await;

        assert!(
            result.is_ok(),
            "openai alias with requires_openai_auth should construct via Codex OAuth path: {}",
            result.err().unwrap()
        );
    }

    #[tokio::test]
    async fn acp_agent_session_tools_receive_the_owned_store_view() {
        use tempfile::TempDir;
        use clawcrew_config::schema::{
            AliasedAgentConfig, Config, ModelProviderConfig, OpenAIModelProviderConfig,
            RiskProfileConfig,
        };
        use clawcrew_infra::acp_session_store::AcpSessionStore;

        let tmp = TempDir::new().expect("temp dir");
        let data_dir = tmp.path().join("data");
        std::fs::create_dir_all(&data_dir).expect("data dir");
        let mut config = Config {
            data_dir: data_dir.clone(),
            config_path: tmp.path().join("config.toml"),
            ..Default::default()
        };
        config.memory.backend = "none".to_string();
        config.memory.auto_save = false;
        config
            .risk_profiles
            .insert("test-profile".to_string(), RiskProfileConfig::default());
        config.providers.models.openai.insert(
            "default".to_string(),
            OpenAIModelProviderConfig {
                base: ModelProviderConfig {
                    model: Some("gpt-4o-mini".to_string()),
                    api_key: Some("test-key".to_string()),
                    ..Default::default()
                },
            },
        );
        config.agents.insert(
            "test-agent".to_string(),
            AliasedAgentConfig {
                model_provider: "openai.default".into(),
                risk_profile: "test-profile".into(),
                ..Default::default()
            },
        );

        let store = Arc::new(AcpSessionStore::new(tmp.path()).expect("ACP store"));
        let current = "11111111-1111-4111-8111-111111111111";
        let previous = "22222222-2222-4222-8222-222222222222";
        store
            .create_session(current, "test-agent", "/current")
            .unwrap();
        store
            .create_session(previous, "test-agent", "/previous")
            .unwrap();
        store
            .append_turn(
                previous,
                &[ConversationMessage::Chat(ChatMessage::assistant(
                    "durable previous answer",
                ))],
            )
            .unwrap();

        let agent = Agent::from_config_with_session_cwd_and_mcp_backchannel_and_acp_sessions(
            &config,
            "test-agent",
            Some(&data_dir),
            false,
            true,
            true,
            None,
            None,
            None,
            Arc::clone(&store),
        )
        .await
        .expect("ACP agent construction");

        clawcrew_api::TOOL_LOOP_SESSION_KEY
            .scope(Some(current.to_string()), async {
                let listed = agent
                    .execute_tool_for_test("sessions_list", serde_json::json!({}))
                    .await
                    .expect("sessions_list registered")
                    .unwrap();
                assert!(listed.success);
                assert!(listed.output.contains(current));
                assert!(listed.output.contains(previous));

                let history = agent
                    .execute_tool_for_test(
                        "sessions_history",
                        serde_json::json!({"session_id": previous}),
                    )
                    .await
                    .expect("sessions_history registered")
                    .unwrap();
                assert!(history.success);
                assert!(history.output.contains("durable previous answer"));
            })
            .await;
    }

    #[tokio::test]
    async fn from_config_preserves_alias_for_cost_recording() {
        use crate::agent::cost::{
            TOOL_LOOP_COST_TRACKING_CONTEXT, ToolLoopCostTrackingContext,
            build_model_provider_pricing, record_tool_loop_cost_usage,
        };
        use crate::cost::CostTracker;
        use tempfile::TempDir;
        use clawcrew_config::schema::{
            AliasedAgentConfig, Config, ModelProviderConfig, OpenAIModelProviderConfig,
            RiskProfileConfig,
        };

        let tmp = TempDir::new().unwrap();
        let mut config = Config {
            data_dir: tmp.path().join("data"),
            config_path: tmp.path().join("config.toml"),
            ..Default::default()
        };
        config.memory.backend = "none".to_string();
        config.memory.auto_save = false;
        config.cost.enabled = true;
        config
            .risk_profiles
            .insert("test-profile".to_string(), RiskProfileConfig::default());
        config.providers.models.openai.insert(
            "fast".to_string(),
            OpenAIModelProviderConfig {
                base: ModelProviderConfig {
                    model: Some("gpt-4o-mini".to_string()),
                    api_key: Some("test-key".to_string()),
                    pricing: HashMap::from([
                        ("gpt-4o-mini.input".to_string(), 0.15),
                        ("gpt-4o-mini.output".to_string(), 0.60),
                    ]),
                    ..Default::default()
                },
            },
        );
        config.providers.models.openai.insert(
            "smart".to_string(),
            OpenAIModelProviderConfig {
                base: ModelProviderConfig {
                    model: Some("gpt-4o".to_string()),
                    api_key: Some("test-key".to_string()),
                    pricing: HashMap::from([
                        ("gpt-4o.input".to_string(), 2.50),
                        ("gpt-4o.output".to_string(), 10.00),
                    ]),
                    ..Default::default()
                },
            },
        );
        config.agents.insert(
            "test-agent".to_string(),
            AliasedAgentConfig {
                model_provider: "openai.fast".into(),
                risk_profile: "test-profile".into(),
                ..Default::default()
            },
        );

        let agent = Agent::from_config(&config, "test-agent").await.unwrap();
        assert_eq!(agent.model_provider_name, "openai.fast");

        let tracker = Arc::new(CostTracker::new(config.cost.clone(), &config.data_dir).unwrap());
        let context = ToolLoopCostTrackingContext::new(
            Arc::clone(&tracker),
            Arc::new(build_model_provider_pricing(&config)),
        );
        let usage = clawcrew_providers::traits::TokenUsage {
            input_tokens: Some(1_000_000),
            output_tokens: Some(1_000_000),
            cached_input_tokens: Some(0),
            cache_creation_input_tokens: None,
        };

        let (_, cost_usd) = TOOL_LOOP_COST_TRACKING_CONTEXT
            .scope(Some(context), async {
                record_tool_loop_cost_usage(&agent.model_provider_name, &agent.model_name, &usage)
            })
            .await
            .unwrap();

        assert!(
            (cost_usd - 0.75).abs() < 1e-12,
            "selected alias must charge its own rates, not the sibling's $12.50"
        );
        let summary = tracker.get_summary().unwrap();
        assert!((summary.daily_cost_usd - 0.75).abs() < 1e-12);
    }

    #[test]
    fn builder_allowed_tools_none_keeps_all_tools() {
        let model_provider = Box::new(MockModelProvider {
            responses: Mutex::new(vec![]),
        });

        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );

        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let agent = Agent::builder()
            .model_provider(model_provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .allowed_tools(None)
            .build()
            .expect("agent builder should succeed with valid config");

        assert_eq!(agent.tools.len(), 1);
        assert_eq!(agent.tools[0].name(), "echo");
    }

    #[test]
    fn builder_allowed_tools_some_filters_tools() {
        let model_provider = Box::new(MockModelProvider {
            responses: Mutex::new(vec![]),
        });

        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );

        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let agent = Agent::builder()
            .model_provider(model_provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .allowed_tools(Some(vec!["nonexistent".to_string()]))
            .build()
            .expect("agent builder should succeed with valid config");

        assert!(
            agent.tools.is_empty(),
            "No tools should match a non-existent allowlist entry"
        );
    }

    #[test]
    fn session_cwd_keeps_workspace_in_allowed_roots() {
        let workspace = std::env::temp_dir().join("clawcrew_test_session_cwd_workspace");
        let session = std::env::temp_dir().join("clawcrew_test_session_cwd_session");
        let _ = std::fs::create_dir_all(&workspace);
        let _ = std::fs::create_dir_all(&session);

        let skill_file = workspace.join("SKILL.md");
        let _ = std::fs::write(&skill_file, "body");
        // is_resolved_path_allowed expects a canonicalized path (symlinks resolved).
        let skill_resolved = std::fs::canonicalize(&skill_file).unwrap_or(skill_file);

        let risk_profile = clawcrew_config::schema::RiskProfileConfig::default();

        // Policy WITH the fix: workspace pushed into allowed_roots.
        let mut policy = SecurityPolicy::from_risk_profile(&risk_profile, &session);
        policy.allowed_roots.push(workspace.clone());
        assert!(
            policy.is_resolved_path_allowed(&skill_resolved),
            "workspace skills must remain readable when session_cwd differs"
        );

        // Without the push the same path must be denied, confirming the push
        // is the load-bearing fix rather than an incidental side-effect.
        let policy_no_push = SecurityPolicy::from_risk_profile(&risk_profile, &session);
        assert!(
            !policy_no_push.is_resolved_path_allowed(&skill_resolved),
            "without allowed_roots.push, workspace files must be outside the sandbox"
        );
    }

    #[test]
    fn seed_history_prepends_system_and_skips_system_from_seed() {
        let model_provider = Box::new(MockModelProvider {
            responses: Mutex::new(vec![]),
        });

        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );

        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = Agent::builder()
            .model_provider(model_provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .build()
            .expect("agent builder should succeed with valid config");

        let seed = vec![
            ChatMessage::system("old system prompt"),
            ChatMessage::user("hello"),
            ChatMessage::assistant("hi there"),
        ];
        agent.seed_history(&seed);

        let history = agent.history();
        // First message should be a freshly built system prompt (not the seed one)
        assert!(matches!(&history[0], ConversationMessage::Chat(m) if m.role == "system"));
        // System message from seed should be skipped, so next is user
        assert!(
            matches!(&history[1], ConversationMessage::Chat(m) if m.role == "user" && m.content == "hello")
        );
        assert!(
            matches!(&history[2], ConversationMessage::Chat(m) if m.role == "assistant" && m.content == "hi there")
        );
        assert_eq!(history.len(), 3);
    }

    #[test]
    fn set_tool_dispatcher_refreshes_existing_system_prompt() {
        use clawcrew_api::model_provider::{ChatMessage, ConversationMessage};

        let model_provider = Box::new(MockModelProvider {
            responses: Mutex::new(vec![]),
        });
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );
        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = Agent::builder()
            .model_provider(model_provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(XmlToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .build()
            .expect("agent builder should succeed with valid config");

        agent.seed_history(&[ChatMessage::user("hello")]);
        let before = match agent.history().first() {
            Some(ConversationMessage::Chat(m)) if m.role == "system" => m.content.clone(),
            other => panic!("expected a system prompt first, got {other:?}"),
        };
        assert!(
            before.contains("Tool Use Protocol"),
            "xml dispatcher system prompt should carry the xml tool protocol"
        );

        agent.set_tool_dispatcher(Box::new(NativeToolDispatcher));
        let after = match agent.history().first() {
            Some(ConversationMessage::Chat(m)) if m.role == "system" => m.content.clone(),
            other => panic!("expected a system prompt first, got {other:?}"),
        };
        assert!(
            !after.contains("Tool Use Protocol"),
            "native dispatcher system prompt must not carry the xml tool protocol after swap"
        );
    }

    #[test]
    fn seed_conversation_history_preserves_tool_call_variants() {
        use clawcrew_api::model_provider::{
            ChatMessage, ConversationMessage, ToolCall, ToolResultMessage,
        };

        let provider = Box::new(MockModelProvider {
            responses: Mutex::new(vec![]),
        });

        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );

        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = Agent::builder()
            .model_provider(provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .build()
            .expect("agent builder should succeed with valid config");

        let messages = vec![
            ConversationMessage::Chat(ChatMessage::user("run it")),
            ConversationMessage::AssistantToolCalls {
                text: None,
                tool_calls: vec![ToolCall {
                    id: "tc-1".into(),
                    name: "shell".into(),
                    arguments: r#"{"command":"ls"}"#.into(),
                    extra_content: None,
                }],
                reasoning_content: None,
            },
            ConversationMessage::ToolResults(vec![ToolResultMessage {
                tool_call_id: "tc-1".into(),
                content: "ok".into(),
                tool_name: String::new(),
            }]),
            ConversationMessage::Chat(ChatMessage::assistant("done")),
        ];

        agent.seed_conversation_history(messages);

        // System prompt may have been prepended; find non-system messages
        let non_system: Vec<_> = agent
            .history()
            .iter()
            .filter(|m| !matches!(m, ConversationMessage::Chat(c) if c.role == "system"))
            .collect();

        assert_eq!(non_system.len(), 4);
        assert!(
            matches!(non_system[1], ConversationMessage::AssistantToolCalls { tool_calls, .. } if tool_calls[0].id == "tc-1")
        );
        assert!(
            matches!(non_system[2], ConversationMessage::ToolResults(r) if r[0].tool_call_id == "tc-1")
        );
    }

    #[test]
    fn seed_history_trims_over_cap_restore_and_returns_transport_event() {
        let capturing = Arc::new(CapturingObserver::default());
        let observer: Arc<dyn Observer> = capturing.clone();
        let mut agent = trim_history_test_agent(2, observer);

        let event = agent.seed_history_with_event(&[
            ChatMessage::user("old request"),
            ChatMessage::assistant("old answer"),
            ChatMessage::user("new request"),
            ChatMessage::assistant("new answer"),
        ]);

        assert!(matches!(
            event,
            Some(TurnEvent::HistoryTrimmed {
                dropped_messages: 2,
                kept_turns: 1,
                ..
            })
        ));
        assert!(agent.history_has_trim_breadcrumb);
        assert!(matches!(
            agent.history.get(2),
            Some(ConversationMessage::Chat(message))
                if message.role == "user" && message.content == "new request"
        ));
        assert_eq!(
            capturing
                .events
                .lock()
                .iter()
                .filter(|event| matches!(event, ObserverEvent::HistoryTrimmed { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn seed_conversation_history_trims_over_cap_restore_without_splitting_tools() {
        use clawcrew_providers::{ToolCall, ToolResultMessage};

        let capturing = Arc::new(CapturingObserver::default());
        let observer: Arc<dyn Observer> = capturing.clone();
        let mut agent = trim_history_test_agent(4, observer);
        let event = agent.seed_conversation_history_with_event(vec![
            ConversationMessage::Chat(ChatMessage::user("old request")),
            ConversationMessage::Chat(ChatMessage::assistant("old answer")),
            ConversationMessage::Chat(ChatMessage::user("new request")),
            ConversationMessage::AssistantToolCalls {
                text: Some("running".into()),
                tool_calls: vec![ToolCall {
                    id: "seed-call".into(),
                    name: "echo".into(),
                    arguments: "{}".into(),
                    extra_content: None,
                }],
                reasoning_content: None,
            },
            ConversationMessage::ToolResults(vec![ToolResultMessage {
                tool_call_id: "seed-call".into(),
                content: "result".into(),
                tool_name: "echo".into(),
            }]),
            ConversationMessage::Chat(ChatMessage::assistant("new answer")),
        ]);

        assert!(matches!(
            event,
            Some(TurnEvent::HistoryTrimmed {
                dropped_messages: 2,
                kept_turns: 1,
                ..
            })
        ));
        assert!(matches!(
            (&agent.history[3], &agent.history[4]),
            (
                ConversationMessage::AssistantToolCalls { tool_calls, .. },
                ConversationMessage::ToolResults(results),
            ) if tool_calls[0].id == "seed-call" && results[0].tool_call_id == "seed-call"
        ));
        assert_eq!(
            capturing
                .events
                .lock()
                .iter()
                .filter(|event| matches!(event, ObserverEvent::HistoryTrimmed { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn clear_history_resets_trim_breadcrumb_provenance_before_reuse() {
        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = trim_history_test_agent(2, observer);
        agent.history = vec![
            ConversationMessage::Chat(ChatMessage::system("system")),
            ConversationMessage::Chat(ChatMessage::user("old user")),
            ConversationMessage::Chat(ChatMessage::assistant("old assistant")),
            ConversationMessage::Chat(ChatMessage::user("new user")),
            ConversationMessage::Chat(ChatMessage::assistant("new assistant")),
        ];
        let _ = agent.trim_history(None);
        assert!(agent.history_has_trim_breadcrumb);

        agent.clear_history();
        assert!(!agent.history_has_trim_breadcrumb);

        let breadcrumb = crate::i18n::get_required_cli_string("history-trim-breadcrumb");
        agent.seed_history(&[
            ChatMessage::user(breadcrumb.clone()),
            ChatMessage::assistant("user-authored marker reply"),
        ]);
        assert!(!agent.history_has_trim_breadcrumb);

        agent.seed_history(&[
            ChatMessage::user("later user"),
            ChatMessage::assistant("later assistant"),
        ]);
        assert!(agent.history_has_trim_breadcrumb);
        assert_eq!(
            agent
                .history
                .iter()
                .filter(|message| matches!(
                    message,
                    ConversationMessage::Chat(chat) if chat.content == breadcrumb
                ))
                .count(),
            1,
            "the user-authored marker must be dropped as an ordinary old turn before one synthetic breadcrumb is inserted"
        );
        assert!(agent.history.iter().any(|message| matches!(
            message,
            ConversationMessage::Chat(chat)
                if chat.role == "user" && chat.content == "later user"
        )));
    }

    #[test]
    fn append_seed_history_preserves_existing_trim_breadcrumb_provenance() {
        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = trim_history_test_agent(2, observer);
        agent.seed_history(&[
            ChatMessage::user("old user"),
            ChatMessage::assistant("old assistant"),
            ChatMessage::user("kept user"),
            ChatMessage::assistant("kept assistant"),
        ]);
        assert!(agent.history_has_trim_breadcrumb);

        agent.seed_history(&[
            ChatMessage::user("appended user"),
            ChatMessage::assistant("appended assistant"),
        ]);

        let breadcrumb = crate::i18n::get_required_cli_string("history-trim-breadcrumb");
        assert!(agent.history_has_trim_breadcrumb);
        assert_eq!(
            agent
                .history
                .iter()
                .filter(|message| matches!(
                    message,
                    ConversationMessage::Chat(chat) if chat.content == breadcrumb
                ))
                .count(),
            1
        );
        assert!(agent.history.iter().any(|message| matches!(
            message,
            ConversationMessage::Chat(chat)
                if chat.role == "user" && chat.content == "appended user"
        )));
    }

    #[test]
    fn append_conversation_seed_preserves_existing_trim_breadcrumb_provenance() {
        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = trim_history_test_agent(2, observer);
        agent.seed_conversation_history(vec![
            ConversationMessage::Chat(ChatMessage::user("old user")),
            ConversationMessage::Chat(ChatMessage::assistant("old assistant")),
            ConversationMessage::Chat(ChatMessage::user("kept user")),
            ConversationMessage::Chat(ChatMessage::assistant("kept assistant")),
        ]);
        assert!(agent.history_has_trim_breadcrumb);

        agent.seed_conversation_history(vec![
            ConversationMessage::Chat(ChatMessage::user("appended user")),
            ConversationMessage::Chat(ChatMessage::assistant("appended assistant")),
        ]);

        let breadcrumb = crate::i18n::get_required_cli_string("history-trim-breadcrumb");
        assert!(agent.history_has_trim_breadcrumb);
        assert_eq!(
            agent
                .history
                .iter()
                .filter(|message| matches!(
                    message,
                    ConversationMessage::Chat(chat) if chat.content == breadcrumb
                ))
                .count(),
            1
        );
        assert!(agent.history.iter().any(|message| matches!(
            message,
            ConversationMessage::Chat(chat)
                if chat.role == "user" && chat.content == "appended user"
        )));
    }

    /// Mock provider that captures whether tool specs were passed to `stream_chat`
    /// and returns a tool call followed by a text response through the stream.
    struct StreamToolCaptureModelProvider {
        tools_received: Arc<Mutex<Vec<bool>>>,
        call_count: Arc<Mutex<usize>>,
    }

    #[async_trait]
    impl ModelProvider for StreamToolCaptureModelProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<String> {
            Ok("ok".into())
        }

        async fn chat(
            &self,
            request: ChatRequest<'_>,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<clawcrew_providers::ChatResponse> {
            self.tools_received.lock().push(request.tools.is_some());
            let mut count = self.call_count.lock();
            *count += 1;
            if *count == 1 {
                Ok(clawcrew_providers::ChatResponse {
                    text: Some(String::new()),
                    tool_calls: vec![clawcrew_providers::ToolCall {
                        id: "00000000-0000-0000-0000-000000000001".into(),
                        name: "echo".into(),
                        arguments: "{}".into(),
                        extra_content: None,
                    }],
                    usage: None,
                    reasoning_content: None,
                })
            } else {
                Ok(clawcrew_providers::ChatResponse {
                    text: Some("stream-done".into()),
                    tool_calls: vec![],
                    usage: None,
                    reasoning_content: None,
                })
            }
        }

        fn supports_native_tools(&self) -> bool {
            true
        }

        fn stream_chat(
            &self,
            request: ChatRequest<'_>,
            _model: &str,
            _temperature: Option<f64>,
            _options: clawcrew_providers::traits::StreamOptions,
        ) -> futures_util::stream::BoxStream<
            'static,
            clawcrew_providers::traits::StreamResult<clawcrew_providers::traits::StreamEvent>,
        > {
            use futures_util::stream::{self, StreamExt};
            self.tools_received.lock().push(request.tools.is_some());
            let mut count = self.call_count.lock();
            *count += 1;
            if *count == 1 {
                let tc = clawcrew_providers::traits::StreamEvent::ToolCall(
                    clawcrew_providers::ToolCall {
                        id: "00000000-0000-0000-0000-000000000001".into(),
                        name: "echo".into(),
                        arguments: "{}".into(),
                        extra_content: None,
                    },
                );
                stream::iter(vec![
                    Ok(tc),
                    Ok(clawcrew_providers::traits::StreamEvent::Final),
                ])
                .boxed()
            } else {
                let chunk = clawcrew_providers::traits::StreamEvent::TextDelta(
                    clawcrew_providers::traits::StreamChunk {
                        delta: "stream-done".into(),
                        is_final: false,
                        reasoning: None,
                        token_count: 0,
                    },
                );
                stream::iter(vec![
                    Ok(chunk),
                    Ok(clawcrew_providers::traits::StreamEvent::Final),
                ])
                .boxed()
            }
        }
    }
    impl ::clawcrew_api::attribution::Attributable for StreamToolCaptureModelProvider {
        fn role(&self) -> ::clawcrew_api::attribution::Role {
            ::clawcrew_api::attribution::Role::Provider(
                ::clawcrew_api::attribution::ProviderKind::Model(
                    ::clawcrew_api::attribution::ModelProviderKind::Custom,
                ),
            )
        }
        fn alias(&self) -> &str {
            "StreamToolCaptureModelProvider"
        }
    }

    #[tokio::test]
    async fn turn_streamed_passes_tool_specs_to_provider() {
        let tools_received = Arc::new(Mutex::new(Vec::new()));
        let model_provider = Box::new(StreamToolCaptureModelProvider {
            tools_received: tools_received.clone(),
            call_count: Arc::new(Mutex::new(0)),
        });

        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );

        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = Agent::builder()
            .model_provider(model_provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .build()
            .expect("agent builder should succeed with valid config");

        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(64);
        let (response, _) = agent
            .turn_streamed("use the echo tool", event_tx, None)
            .await
            .unwrap();
        assert_eq!(response, "stream-done");

        // Verify tools were passed in both stream_chat calls
        let received = tools_received.lock();
        assert!(
            received.len() >= 2,
            "Expected at least 2 stream_chat calls, got {}",
            received.len()
        );
        assert!(
            received[0],
            "First stream_chat call should have received tool specs"
        );
        assert!(
            received[1],
            "Second stream_chat call should have received tool specs"
        );

        // Collect events and verify tool call + tool result were emitted
        let mut events = Vec::new();
        while let Ok(ev) = event_rx.try_recv() {
            events.push(ev);
        }
        let has_tool_call = events
            .iter()
            .any(|e| matches!(e, TurnEvent::ToolCall { name, .. } if name == "echo"));
        let has_tool_result = events
            .iter()
            .any(|e| matches!(e, TurnEvent::ToolResult { name, .. } if name == "echo"));
        assert!(
            has_tool_call,
            "Should have emitted a ToolCall event for 'echo'"
        );
        assert!(
            has_tool_result,
            "Should have emitted a ToolResult event for 'echo'"
        );

        // Verify ID correlation
        let call_id = events
            .iter()
            .find_map(|e| {
                if let TurnEvent::ToolCall { id, .. } = e {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .expect("ToolCall should have an ID");

        let result_id = events
            .iter()
            .find_map(|e| {
                if let TurnEvent::ToolResult { id, .. } = e {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .expect("ToolResult should have an ID");

        assert_eq!(
            call_id, result_id,
            "ToolCall and ToolResult should share the same ID for correlation"
        );

        // Verify it's a valid UUID
        assert!(
            uuid::Uuid::parse_str(&call_id).is_ok(),
            "Generated ID should be a valid UUID: got '{}'",
            call_id
        );
    }

    fn tool_receipts_enabled_config(enabled: bool) -> clawcrew_config::schema::AliasedAgentConfig {
        clawcrew_config::schema::AliasedAgentConfig {
            resolved: clawcrew_config::schema::ResolvedRuntime {
                tool_receipts: clawcrew_config::schema::ToolReceiptsConfig {
                    enabled,
                    ..Default::default()
                },
                ..Default::default()
            },
            ..clawcrew_config::schema::AliasedAgentConfig::default()
        }
    }

    fn streamed_agent_with_receipts(enabled: bool) -> Agent {
        let model_provider = Box::new(StreamToolCaptureModelProvider {
            tools_received: Arc::new(Mutex::new(Vec::new())),
            call_count: Arc::new(Mutex::new(0)),
        });
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );
        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        Agent::builder()
            .model_provider(model_provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .config(tool_receipts_enabled_config(enabled))
            .build()
            .expect("agent builder should succeed with valid config")
    }

    fn history_has_receipt(agent: &Agent) -> bool {
        agent.history().iter().any(|m| match m {
            ConversationMessage::ToolResults(results) => results
                .iter()
                .any(|r| r.content.contains("[receipt: zc-receipt-")),
            _ => false,
        })
    }

    // RED on upstream/master: the streamed turn path (ACP, gateway WS) hardcoded
    // `receipt_generator: None`, so an enabled config produced zero receipts.
    // GREEN once `turn_streamed` derives the scope from its own config through
    // the shared `ReceiptScope::from_config` seam.
    #[tokio::test]
    async fn turn_streamed_signs_tool_results_when_receipts_enabled() {
        let mut agent = streamed_agent_with_receipts(true);
        let (event_tx, _event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(64);
        agent
            .turn_streamed("use the echo tool", event_tx, None)
            .await
            .expect("streamed turn should succeed");
        assert!(
            history_has_receipt(&agent),
            "enabled receipts must sign tool results on the streamed path"
        );
    }

    // GREEN control: disabled config produces no receipts on the same path.
    #[tokio::test]
    async fn turn_streamed_omits_receipts_when_disabled() {
        let mut agent = streamed_agent_with_receipts(false);
        let (event_tx, _event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(64);
        agent
            .turn_streamed("use the echo tool", event_tx, None)
            .await
            .expect("streamed turn should succeed");
        assert!(
            !history_has_receipt(&agent),
            "disabled receipts must not sign tool results"
        );
    }

    fn show_in_response_config(show: bool) -> clawcrew_config::schema::AliasedAgentConfig {
        clawcrew_config::schema::AliasedAgentConfig {
            resolved: clawcrew_config::schema::ResolvedRuntime {
                tool_receipts: clawcrew_config::schema::ToolReceiptsConfig {
                    enabled: true,
                    show_in_response: show,
                    ..Default::default()
                },
                ..Default::default()
            },
            ..clawcrew_config::schema::AliasedAgentConfig::default()
        }
    }

    fn streamed_agent_with_config(config: clawcrew_config::schema::AliasedAgentConfig) -> Agent {
        let model_provider = Box::new(StreamToolCaptureModelProvider {
            tools_received: Arc::new(Mutex::new(Vec::new())),
            call_count: Arc::new(Mutex::new(0)),
        });
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );
        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        Agent::builder()
            .model_provider(model_provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .config(config)
            .build()
            .expect("agent builder should succeed with valid config")
    }

    // RED on the pre-fix branch: `show_in_response` was read only in the channel
    // orchestrator, so ACP/WS/CLI turns never appended the auditable block.
    // GREEN once the turn paths route the collector through
    // `append_receipts_block`.
    #[tokio::test]
    async fn turn_streamed_appends_receipts_block_when_show_in_response() {
        let mut agent = streamed_agent_with_config(show_in_response_config(true));
        let (event_tx, _event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(64);
        let (response, _msgs) = agent
            .turn_streamed("use the echo tool", event_tx, None)
            .await
            .expect("streamed turn should succeed");
        assert!(
            response.contains("---\nTool receipts:") && response.contains("zc-receipt-"),
            "show_in_response must append the Tool receipts block to the reply, got: {response}"
        );
    }

    // Control: with show_in_response off the reply carries no receipts block,
    // even though receipts are still signed into history.
    #[tokio::test]
    async fn turn_streamed_omits_receipts_block_when_show_in_response_off() {
        let mut agent = streamed_agent_with_config(show_in_response_config(false));
        let (event_tx, _event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(64);
        let (response, _msgs) = agent
            .turn_streamed("use the echo tool", event_tx, None)
            .await
            .expect("streamed turn should succeed");
        assert!(
            !response.contains("Tool receipts:"),
            "no receipts block when show_in_response is off, got: {response}"
        );
        assert!(
            history_has_receipt(&agent),
            "receipts are still signed into history when only the reply block is off"
        );
    }

    // The receipt-echo system-prompt addendum is added on the turn path when
    // inject_system_prompt is on (default), matching the channel orchestrator.
    #[test]
    fn build_system_prompt_injects_receipt_addendum_when_enabled() {
        let agent = streamed_agent_with_config(show_in_response_config(true));
        let prompt = agent
            .build_system_prompt()
            .expect("system prompt should build");
        assert!(
            prompt.contains("## Tool Execution Receipts"),
            "enabled receipts with inject_system_prompt must add the addendum"
        );
    }

    /// then finishes. Used to verify serial dispatch ordering.
    struct TwoToolCallStreamModelProvider {
        call_count: Arc<Mutex<usize>>,
    }

    #[async_trait]
    impl ModelProvider for TwoToolCallStreamModelProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<String> {
            Ok("ok".into())
        }

        async fn chat(
            &self,
            _request: ChatRequest<'_>,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<clawcrew_providers::ChatResponse> {
            Ok(clawcrew_providers::ChatResponse {
                text: Some("done".into()),
                tool_calls: vec![],
                usage: None,
                reasoning_content: None,
            })
        }

        fn supports_native_tools(&self) -> bool {
            true
        }

        fn supports_streaming(&self) -> bool {
            true
        }

        fn supports_streaming_tool_events(&self) -> bool {
            true
        }

        fn stream_chat(
            &self,
            _request: ChatRequest<'_>,
            _model: &str,
            _temperature: Option<f64>,
            _options: clawcrew_providers::traits::StreamOptions,
        ) -> futures_util::stream::BoxStream<
            'static,
            clawcrew_providers::traits::StreamResult<clawcrew_providers::traits::StreamEvent>,
        > {
            use futures_util::stream::{self, StreamExt};
            let mut count = self.call_count.lock();
            *count += 1;
            if *count == 1 {
                stream::iter(vec![
                    Ok(clawcrew_providers::traits::StreamEvent::ToolCall(
                        clawcrew_providers::ToolCall {
                            id: "00000000-0000-0000-0000-000000000001".into(),
                            name: "echo".into(),
                            arguments: "{}".into(),
                            extra_content: None,
                        },
                    )),
                    Ok(clawcrew_providers::traits::StreamEvent::ToolCall(
                        clawcrew_providers::ToolCall {
                            id: "00000000-0000-0000-0000-000000000002".into(),
                            name: "echo".into(),
                            arguments: "{}".into(),
                            extra_content: None,
                        },
                    )),
                    Ok(clawcrew_providers::traits::StreamEvent::Final),
                ])
                .boxed()
            } else {
                stream::iter(vec![
                    Ok(clawcrew_providers::traits::StreamEvent::TextDelta(
                        clawcrew_providers::traits::StreamChunk {
                            delta: "stream-done".into(),
                            is_final: false,
                            reasoning: None,
                            token_count: 0,
                        },
                    )),
                    Ok(clawcrew_providers::traits::StreamEvent::Final),
                ])
                .boxed()
            }
        }
    }
    impl ::clawcrew_api::attribution::Attributable for TwoToolCallStreamModelProvider {
        fn role(&self) -> ::clawcrew_api::attribution::Role {
            ::clawcrew_api::attribution::Role::Provider(
                ::clawcrew_api::attribution::ProviderKind::Model(
                    ::clawcrew_api::attribution::ModelProviderKind::Custom,
                ),
            )
        }
        fn alias(&self) -> &str {
            "TwoToolCallStreamModelProvider"
        }
    }

    #[tokio::test]
    async fn turn_streamed_dispatches_multiple_tools_serially_when_parallel_disabled() {
        let model_provider = Box::new(TwoToolCallStreamModelProvider {
            call_count: Arc::new(Mutex::new(0)),
        });

        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );

        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = Agent::builder()
            .model_provider(model_provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .build()
            .expect("agent builder should succeed with valid config");

        // Default resolved config has parallel_tools = false; this is the
        // serial path under test.
        assert!(
            !agent.config.resolved.parallel_tools,
            "test precondition: parallel_tools must be disabled"
        );

        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(64);
        let (response, _) = agent
            .turn_streamed("use echo twice", event_tx, None)
            .await
            .unwrap();
        assert_eq!(response, "stream-done");

        // Reduce events to the call/result sequence, tagged by id.
        let mut seq: Vec<(&'static str, String)> = Vec::new();
        while let Ok(ev) = event_rx.try_recv() {
            match ev {
                TurnEvent::ToolCall { id, .. } => seq.push(("call", id)),
                TurnEvent::ToolResult { id, .. } => seq.push(("result", id)),
                _ => {}
            }
        }

        let id1 = "00000000-0000-0000-0000-000000000001";
        let id2 = "00000000-0000-0000-0000-000000000002";
        assert_eq!(
            seq,
            vec![
                ("call", id1.to_string()),
                ("result", id1.to_string()),
                ("call", id2.to_string()),
                ("result", id2.to_string()),
            ],
            "serial dispatch must interleave call->result per tool, not batch all \
             starts then all results; got {seq:?}"
        );
    }

    struct PreExecutedToolModelProvider;

    #[async_trait]
    impl ModelProvider for PreExecutedToolModelProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<String> {
            Ok(String::new())
        }

        async fn chat(
            &self,
            _request: ChatRequest<'_>,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<clawcrew_providers::ChatResponse> {
            Ok(clawcrew_providers::ChatResponse {
                text: Some(String::new()),
                tool_calls: vec![],
                usage: None,
                reasoning_content: None,
            })
        }

        fn supports_streaming(&self) -> bool {
            true
        }

        fn stream_chat(
            &self,
            _request: ChatRequest<'_>,
            _model: &str,
            _temperature: Option<f64>,
            _options: clawcrew_providers::traits::StreamOptions,
        ) -> futures_util::stream::BoxStream<
            'static,
            clawcrew_providers::traits::StreamResult<clawcrew_providers::traits::StreamEvent>,
        > {
            use futures_util::stream::{self, StreamExt};

            stream::iter(vec![
                Ok(
                    clawcrew_providers::traits::StreamEvent::PreExecutedToolCall {
                        name: "file_read".into(),
                        args: "{\"path\":\"a.txt\"}".into(),
                    },
                ),
                Ok(
                    clawcrew_providers::traits::StreamEvent::PreExecutedToolCall {
                        name: "shell".into(),
                        args: "{\"command\":\"pwd\"}".into(),
                    },
                ),
                Ok(
                    clawcrew_providers::traits::StreamEvent::PreExecutedToolResult {
                        name: "file_read".into(),
                        output: "a".into(),
                    },
                ),
                Ok(
                    clawcrew_providers::traits::StreamEvent::PreExecutedToolResult {
                        name: "shell".into(),
                        output: "b".into(),
                    },
                ),
                Ok(clawcrew_providers::traits::StreamEvent::TextDelta(
                    clawcrew_providers::traits::StreamChunk::delta("done"),
                )),
                Ok(clawcrew_providers::traits::StreamEvent::Final),
            ])
            .boxed()
        }
    }
    impl ::clawcrew_api::attribution::Attributable for PreExecutedToolModelProvider {
        fn role(&self) -> ::clawcrew_api::attribution::Role {
            ::clawcrew_api::attribution::Role::Provider(
                ::clawcrew_api::attribution::ProviderKind::Model(
                    ::clawcrew_api::attribution::ModelProviderKind::Custom,
                ),
            )
        }
        fn alias(&self) -> &str {
            "PreExecutedToolModelProvider"
        }
    }

    #[tokio::test]
    async fn pre_executed_tool_results_keep_ids_when_calls_overlap() {
        let model_provider = Box::new(PreExecutedToolModelProvider);

        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );

        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = Agent::builder()
            .model_provider(model_provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .build()
            .expect("agent builder should succeed with valid config");

        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(64);
        let _ = agent
            .turn_streamed("use pre-executed tools", event_tx, None)
            .await
            .unwrap();

        let mut call_ids = HashMap::new();
        let mut result_ids = HashMap::new();
        while let Ok(event) = event_rx.try_recv() {
            match event {
                TurnEvent::ToolCall { id, name, .. } => {
                    call_ids.insert(name, id);
                }
                TurnEvent::ToolResult { id, name, .. } => {
                    result_ids.insert(name, id);
                }
                _ => {}
            }
        }

        assert_eq!(call_ids.len(), 2, "expected two pre-executed tool calls");
        assert_eq!(
            result_ids.len(),
            2,
            "expected two pre-executed tool results"
        );
        assert_eq!(call_ids.get("file_read"), result_ids.get("file_read"));
        assert_eq!(call_ids.get("shell"), result_ids.get("shell"));
    }

    #[tokio::test]
    async fn turn_normalizes_user_image_markers_before_provider_call() {
        let seen_user_messages = Arc::new(Mutex::new(Vec::new()));
        let provider = Box::new(MultimodalCaptureProvider {
            seen_user_messages: seen_user_messages.clone(),
            streamed: false,
        });

        let temp = tempfile::tempdir().expect("tempdir");
        let image_path = temp.path().join("agent-turn.png");
        std::fs::write(
            &image_path,
            [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'],
        )
        .expect("write fixture");

        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );

        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = Agent::builder()
            .model_provider(provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .multimodal_config(clawcrew_config::schema::MultimodalConfig::default())
            .build()
            .expect("agent builder should succeed with valid config");

        agent
            .turn(&format!(
                "inspect [IMAGE:{}]",
                image_path.display().to_string()
            ))
            .await
            .expect("turn should succeed");

        let seen = seen_user_messages.lock();
        let last = seen.last().expect("provider should receive a user message");
        assert!(
            last.contains("data:image/png;base64,"),
            "expected normalized data URI in provider request, got: {last}"
        );
    }

    #[tokio::test]
    async fn turn_streamed_normalizes_user_image_markers_before_provider_call() {
        let seen_user_messages = Arc::new(Mutex::new(Vec::new()));
        let provider = Box::new(MultimodalCaptureProvider {
            seen_user_messages: seen_user_messages.clone(),
            streamed: true,
        });

        let temp = tempfile::tempdir().expect("tempdir");
        let image_path = temp.path().join("agent-stream.png");
        std::fs::write(
            &image_path,
            [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'],
        )
        .expect("write fixture");

        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );

        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = Agent::builder()
            .model_provider(provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .multimodal_config(clawcrew_config::schema::MultimodalConfig::default())
            .build()
            .expect("agent builder should succeed with valid config");

        let (event_tx, _event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(8);
        agent
            .turn_streamed(
                &format!("inspect [IMAGE:{}]", image_path.display().to_string()),
                event_tx,
                None,
            )
            .await
            .expect("turn_streamed should succeed");

        let seen = seen_user_messages.lock();
        let last = seen.last().expect("provider should receive a user message");
        assert!(
            last.contains("data:image/png;base64,"),
            "expected normalized data URI in provider request, got: {last}"
        );
    }

    fn trim_history_test_agent(max_history_messages: usize, observer: Arc<dyn Observer>) -> Agent {
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );
        let agent_config = clawcrew_config::schema::AliasedAgentConfig {
            resolved: clawcrew_config::schema::ResolvedRuntime::default(),
            ..clawcrew_config::schema::AliasedAgentConfig::default()
        };

        Agent::builder()
            .model_provider(Box::new(MockModelProvider {
                responses: Mutex::new(vec![]),
            }))
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .config(agent_config)
            .structured_max_history_messages(max_history_messages)
            .build()
            .expect("agent builder should succeed with valid config")
    }

    fn seed_old_trim_test_turn(agent: &mut Agent) {
        agent.history = vec![
            ConversationMessage::Chat(ChatMessage::system("system")),
            ConversationMessage::Chat(ChatMessage::user("old user")),
            ConversationMessage::Chat(ChatMessage::assistant("old assistant")),
        ];
    }

    fn assert_old_trim_test_turn_was_removed(agent: &Agent) {
        assert!(agent.history_has_trim_breadcrumb);
        assert!(!agent.history.iter().any(|message| matches!(
            message,
            ConversationMessage::Chat(chat)
                if chat.content == "old user" || chat.content == "old assistant"
        )));
    }

    fn drain_history_trim_events(event_rx: &mut tokio::sync::mpsc::Receiver<TurnEvent>) -> usize {
        let mut count = 0;
        while let Ok(event) = event_rx.try_recv() {
            if matches!(event, TurnEvent::HistoryTrimmed { .. }) {
                count += 1;
            }
        }
        count
    }

    fn push_trim_history_tool_exchange(agent: &mut Agent, index: usize) {
        use clawcrew_providers::{ToolCall, ToolResultMessage};

        let tool_call_id = format!("trim-history-call-{index}");
        agent.history.push(ConversationMessage::AssistantToolCalls {
            text: Some(format!("Calling tool {index}")),
            tool_calls: vec![ToolCall {
                id: tool_call_id.clone(),
                name: "mock".into(),
                arguments: "{}".into(),
                extra_content: None,
            }],
            reasoning_content: None,
        });
        agent
            .history
            .push(ConversationMessage::ToolResults(vec![ToolResultMessage {
                tool_call_id,
                content: format!("result {index}"),
                tool_name: "mock".into(),
            }]));
    }

    #[test]
    fn trim_history_preserves_single_tool_heavy_turn_over_message_cap() {
        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = trim_history_test_agent(50, observer);
        agent
            .history
            .push(ConversationMessage::Chat(ChatMessage::user("start")));
        for index in 1..=31 {
            push_trim_history_tool_exchange(&mut agent, index);
        }
        agent
            .history
            .push(ConversationMessage::Chat(ChatMessage::assistant("done")));

        let _ = agent.trim_history(None);

        assert_eq!(
            agent.history.len(),
            64,
            "the newest complete turn must survive even when it exceeds the message cap"
        );
        assert!(matches!(
            agent.history.first(),
            Some(ConversationMessage::Chat(message))
                if message.role == "user" && message.content == "start"
        ));
        assert!(matches!(
            agent.history.last(),
            Some(ConversationMessage::Chat(message))
                if message.role == "assistant" && message.content == "done"
        ));
        for (index, pair) in agent.history[1..63].as_chunks::<2>().0.iter().enumerate() {
            let expected_id = format!("trim-history-call-{}", index + 1);
            match pair {
                [
                    ConversationMessage::AssistantToolCalls { tool_calls, .. },
                    ConversationMessage::ToolResults(results),
                ] => {
                    assert_eq!(tool_calls.len(), 1);
                    assert_eq!(results.len(), 1);
                    assert_eq!(tool_calls[0].id, expected_id);
                    assert_eq!(results[0].tool_call_id, expected_id);
                }
                _ => panic!("tool exchange {} was split or reordered", index + 1),
            }
        }
    }

    #[test]
    fn trim_history_drops_old_turn_with_breadcrumb_and_observer_event() {
        let capturing = Arc::new(CapturingObserver::default());
        let observer: Arc<dyn Observer> = capturing.clone();
        let mut agent = trim_history_test_agent(2, observer);
        agent.history = vec![
            ConversationMessage::Chat(ChatMessage::system("system")),
            ConversationMessage::Chat(ChatMessage::user("old user")),
            ConversationMessage::Chat(ChatMessage::assistant("old assistant")),
            ConversationMessage::Chat(ChatMessage::user("new user")),
            ConversationMessage::Chat(ChatMessage::assistant("new assistant")),
        ];

        let _ = agent.trim_history(None);

        let breadcrumb = crate::i18n::get_required_cli_string("history-trim-breadcrumb");
        assert!(matches!(
            agent.history.first(),
            Some(ConversationMessage::Chat(message))
                if message.role == "system"
        ));
        assert!(matches!(
            agent.history.get(1),
            Some(ConversationMessage::Chat(message))
                if message.role == "user" && message.content == breadcrumb
        ));
        assert_eq!(
            agent
                .history
                .iter()
                .filter(|message| matches!(
                    message,
                    ConversationMessage::Chat(chat) if chat.content == breadcrumb
                ))
                .count(),
            1,
            "trim breadcrumb must be inserted exactly once"
        );
        assert!(matches!(
            agent.history.get(2),
            Some(ConversationMessage::Chat(message))
                if message.role == "user" && message.content == "new user"
        ));
        assert!(matches!(
            agent.history.get(3),
            Some(ConversationMessage::Chat(message))
                if message.role == "assistant" && message.content == "new assistant"
        ));
        assert_eq!(
            agent.history.len(),
            4,
            "only the complete newest turn remains"
        );

        let trim_events: Vec<_> = capturing
            .events
            .lock()
            .iter()
            .filter_map(|event| match event {
                ObserverEvent::HistoryTrimmed {
                    dropped_messages,
                    kept_turns,
                    reason,
                    ..
                } => Some((*dropped_messages, *kept_turns, reason.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(trim_events.len(), 1, "one observer trim event is required");
        assert_eq!(trim_events[0].0, 2);
        assert_eq!(trim_events[0].1, 1);
        assert_eq!(
            trim_events[0].2,
            crate::i18n::get_required_cli_string("history-trim-reason-message-cap")
        );
    }

    #[tokio::test]
    async fn trim_history_runs_after_direct_tool_loop_provider_error() {
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );
        let capturing = Arc::new(CapturingObserver::default());
        let observer: Arc<dyn Observer> = capturing.clone();
        let config = clawcrew_config::schema::AliasedAgentConfig {
            resolved: clawcrew_config::schema::ResolvedRuntime::default(),
            ..Default::default()
        };
        let mut agent = Agent::builder()
            .model_provider(Box::new(ToolThenFailingModelProvider {
                calls: std::sync::atomic::AtomicUsize::new(0),
            }))
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .model_name("test-model".into())
            .config(config)
            .structured_max_history_messages(2)
            .build()
            .expect("agent builder should succeed with valid config");
        agent.history = vec![
            ConversationMessage::Chat(ChatMessage::system("system")),
            ConversationMessage::Chat(ChatMessage::user("old request")),
            ConversationMessage::Chat(ChatMessage::assistant("old answer")),
        ];

        let error = agent
            .turn("new request")
            .await
            .expect_err("second provider call should fail");

        assert!(
            error
                .to_string()
                .contains("provider unavailable after tool")
        );
        assert!(agent.history_has_trim_breadcrumb);
        assert!(!agent.history.iter().any(|message| matches!(
            message,
            ConversationMessage::Chat(chat)
                if chat.content == "old request" || chat.content == "old answer"
        )));
        assert!(agent.history.iter().any(|message| matches!(
            message,
            ConversationMessage::Chat(chat)
                if chat.role == "user" && chat.content.contains("new request")
        )));
        assert!(agent.history.windows(2).any(|pair| matches!(
            pair,
            [
                ConversationMessage::AssistantToolCalls { tool_calls, .. },
                ConversationMessage::ToolResults(results),
            ] if tool_calls[0].id == "error-path-call"
                && results[0].tool_call_id == "error-path-call"
        )));
        assert_eq!(
            capturing
                .events
                .lock()
                .iter()
                .filter(|event| matches!(event, ObserverEvent::HistoryTrimmed { .. }))
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn trim_history_runs_after_direct_vision_resolution_error() {
        let capturing = Arc::new(CapturingObserver::default());
        let observer: Arc<dyn Observer> = capturing.clone();
        let mut agent = trim_history_test_agent(2, observer);
        seed_old_trim_test_turn(&mut agent);

        let error = agent
            .turn("inspect [IMAGE:data:image/png;base64,iVBORw0KGgo=]")
            .await
            .expect_err("missing vision support should fail before provider dispatch");

        let capability_error = error
            .downcast_ref::<clawcrew_providers::ProviderCapabilityError>()
            .expect("vision refusal must retain its structured capability error");
        assert_eq!(capability_error.capability, "vision");
        assert_old_trim_test_turn_was_removed(&agent);
        assert_eq!(
            capturing
                .events
                .lock()
                .iter()
                .filter(|event| matches!(event, ObserverEvent::HistoryTrimmed { .. }))
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn trim_history_runs_after_streamed_vision_resolution_error() {
        let capturing = Arc::new(CapturingObserver::default());
        let observer: Arc<dyn Observer> = capturing.clone();
        let mut agent = trim_history_test_agent(2, observer);
        seed_old_trim_test_turn(&mut agent);
        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(8);

        let error = agent
            .turn_streamed(
                "inspect [IMAGE:data:image/png;base64,iVBORw0KGgo=]",
                event_tx,
                None,
            )
            .await
            .expect_err("missing vision support should fail before provider dispatch");

        let capability_error = error
            .downcast_ref::<clawcrew_providers::ProviderCapabilityError>()
            .expect("vision refusal must retain its structured capability error");
        assert_eq!(capability_error.capability, "vision");
        assert_old_trim_test_turn_was_removed(&agent);
        assert_eq!(drain_history_trim_events(&mut event_rx), 1);
    }

    #[tokio::test]
    async fn trim_history_runs_after_direct_system_prompt_rebuild_error() {
        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = trim_history_test_agent(2, observer);
        seed_old_trim_test_turn(&mut agent);
        agent.prompt_builder =
            SystemPromptBuilder::default().add_section(Box::new(FailingPromptSection));

        let error = agent
            .turn("new user")
            .await
            .expect_err("synthetic prompt rebuild should fail");

        assert!(
            error
                .to_string()
                .contains("synthetic prompt rebuild failure")
        );
        assert_old_trim_test_turn_was_removed(&agent);
    }

    #[tokio::test]
    async fn trim_history_runs_after_streamed_system_prompt_rebuild_error() {
        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = trim_history_test_agent(2, observer);
        seed_old_trim_test_turn(&mut agent);
        agent.prompt_builder =
            SystemPromptBuilder::default().add_section(Box::new(FailingPromptSection));
        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(8);

        let error = agent
            .turn_streamed("new user", event_tx, None)
            .await
            .expect_err("synthetic prompt rebuild should fail");

        assert!(
            error
                .to_string()
                .contains("synthetic prompt rebuild failure")
        );
        assert_old_trim_test_turn_was_removed(&agent);
        assert_eq!(drain_history_trim_events(&mut event_rx), 1);
    }

    #[tokio::test]
    async fn trim_history_runs_before_streamed_round_loop_exhaustion_error() {
        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = trim_history_test_agent(2, observer);
        agent.config.resolved.max_tool_iterations = 0;
        seed_old_trim_test_turn(&mut agent);
        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(8);

        let error = agent
            .turn_streamed("new user", event_tx, None)
            .await
            .expect_err("zero rounds should return the exhaustion error");

        assert!(
            error
                .to_string()
                .contains("exceeded maximum tool iterations (0)")
        );
        assert_old_trim_test_turn_was_removed(&agent);
        assert_eq!(drain_history_trim_events(&mut event_rx), 1);
    }

    #[test]
    fn trim_history_log_uses_canonical_attribution() {
        let _writer_guard = clawcrew_log::__private_test_writer_lock();
        let _hook_guard = clawcrew_log::__private_test_hook_lock();
        clawcrew_log::try_install_capture_subscriber();
        let mut log_rx = clawcrew_log::subscribe_or_install();
        while log_rx.try_recv().is_ok() {}

        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = trim_history_test_agent(2, observer);
        agent.agent_alias = "trim-test-agent".into();
        agent.channel_name = "trim-test-channel".into();
        agent.history = vec![
            ConversationMessage::Chat(ChatMessage::system("system")),
            ConversationMessage::Chat(ChatMessage::user("old user")),
            ConversationMessage::Chat(ChatMessage::assistant("old assistant")),
            ConversationMessage::Chat(ChatMessage::user("new user")),
            ConversationMessage::Chat(ChatMessage::assistant("new assistant")),
        ];

        let _ = agent.trim_history(Some("trim-test-turn"));

        let mut selected = None;
        let mut candidates = Vec::new();
        loop {
            match log_rx.try_recv() {
                Ok(value)
                    if value.get("message").and_then(serde_json::Value::as_str)
                        == Some("trim_history: dropped oldest whole turns") =>
                {
                    if value.get("trace_id").and_then(serde_json::Value::as_str)
                        == Some("trim-test-turn")
                    {
                        selected = Some(value.clone());
                    }
                    candidates.push(value);
                }
                Ok(_) | Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
                Err(tokio::sync::broadcast::error::TryRecvError::Closed) => break,
            }
        }
        let value = selected.unwrap_or_else(|| {
            panic!(
                "trim LogEvent with trace_id=trim-test-turn was not captured; candidates: {candidates:#?}"
            )
        });
        let event: clawcrew_log::LogEvent =
            serde_json::from_value(value).expect("captured trim event should deserialize");

        assert_eq!(event.clawcrew.get("agent_alias"), Some("trim-test-agent"));
        assert_eq!(
            event.clawcrew.get("channel_type"),
            Some("trim-test-channel")
        );
        assert_eq!(event.clawcrew.get("channel"), None);
        assert_eq!(event.trace_id.as_deref(), Some("trim-test-turn"));
        assert!(event.attributes.get("agent_alias").is_none());
        assert!(event.attributes.get("channel").is_none());
        assert!(event.attributes.get("turn_id").is_none());

        clawcrew_log::clear_broadcast_hook();
    }

    #[tokio::test]
    async fn trim_history_streamed_turn_forwards_single_hard_cap_event() {
        let capturing = Arc::new(CapturingObserver::default());
        let observer: Arc<dyn Observer> = capturing.clone();
        let mut agent = trim_history_test_agent(2, observer);
        agent.history = vec![
            ConversationMessage::Chat(ChatMessage::system("system")),
            ConversationMessage::Chat(ChatMessage::user("old user")),
            ConversationMessage::Chat(ChatMessage::assistant("old assistant")),
        ];
        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(16);

        agent
            .turn_streamed("new user", event_tx, None)
            .await
            .expect("streamed turn should succeed");

        let mut trim_events = Vec::new();
        while let Ok(event) = event_rx.try_recv() {
            if let TurnEvent::HistoryTrimmed {
                dropped_messages,
                kept_turns,
                reason,
                ..
            } = event
            {
                trim_events.push((dropped_messages, kept_turns, reason));
            }
        }
        assert_eq!(trim_events.len(), 1, "one streamed trim event is required");
        assert_eq!(trim_events[0].0, 2);
        assert_eq!(trim_events[0].1, 1);
        assert_eq!(
            trim_events[0].2,
            crate::i18n::get_required_cli_string("history-trim-reason-message-cap")
        );
        assert!(capturing.events.lock().iter().any(|event| matches!(
            event,
            ObserverEvent::HistoryTrimmed {
                turn_id: Some(_),
                ..
            }
        )));
    }

    #[tokio::test]
    async fn trim_history_cancel_before_output_retains_synthesized_newest_turn() {
        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = trim_history_test_agent(2, observer);
        agent.history = vec![
            ConversationMessage::Chat(ChatMessage::system("system")),
            ConversationMessage::Chat(ChatMessage::user("old user")),
            ConversationMessage::Chat(ChatMessage::assistant("old assistant")),
        ];
        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(16);
        let cancel_token = tokio_util::sync::CancellationToken::new();
        cancel_token.cancel();

        let error = agent
            .turn_streamed_with_steering_state("new user", event_tx, Some(cancel_token), None)
            .await
            .expect_err("pre-cancelled streamed turn should return cancellation");

        let breadcrumb = crate::i18n::get_required_cli_string("history-trim-breadcrumb");
        let interruption = crate::i18n::get_required_cli_string("turn-interrupted-by-user");
        assert!(crate::agent::loop_::is_tool_loop_cancelled(&error.error));
        assert_eq!(error.committed_response, interruption);
        assert_eq!(agent.history.len(), 4);
        assert!(matches!(
            agent.history.first(),
            Some(ConversationMessage::Chat(message))
                if message.role == "system"
        ));
        assert!(matches!(
            agent.history.get(1),
            Some(ConversationMessage::Chat(message))
                if message.role == "user" && message.content == breadcrumb
        ));
        assert!(matches!(
            agent.history.get(2),
            Some(ConversationMessage::Chat(message))
                if message.role == "user" && message.content.contains("new user")
        ));
        assert!(matches!(
            agent.history.last(),
            Some(ConversationMessage::Chat(message))
                if message.role == "assistant" && message.content == interruption
        ));
        assert!(!agent.history.iter().any(|message| matches!(
            message,
            ConversationMessage::Chat(chat)
                if chat.content == "old user" || chat.content == "old assistant"
        )));

        let mut trim_events = Vec::new();
        while let Ok(event) = event_rx.try_recv() {
            if let TurnEvent::HistoryTrimmed {
                dropped_messages,
                kept_turns,
                ..
            } = event
            {
                trim_events.push((dropped_messages, kept_turns));
            }
        }
        assert_eq!(trim_events, vec![(2, 1)]);
    }

    // ── Duplicate narration guard ────────────────────────────────────

    #[tokio::test]
    async fn narration_with_tool_calls_produces_no_consecutive_assistant_entries() {
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );

        let model_provider = Box::new(MockModelProvider {
            responses: Mutex::new(vec![clawcrew_providers::ChatResponse {
                text: Some("I will echo the message.".into()),
                tool_calls: vec![clawcrew_providers::ToolCall {
                    id: "tc1".into(),
                    name: "echo".into(),
                    arguments: "{}".into(),
                    extra_content: None,
                }],
                usage: None,
                reasoning_content: None,
            }]),
        });

        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = Agent::builder()
            .model_provider(model_provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .build()
            .expect("agent builder should succeed with valid config");

        agent.turn("hi").await.unwrap();

        let history = agent.history();
        for window in history.windows(2) {
            let prev_is_assistant_chat = matches!(
                &window[0],
                ConversationMessage::Chat(m) if m.role == "assistant"
            );
            let next_is_tool_calls =
                matches!(&window[1], ConversationMessage::AssistantToolCalls { .. });
            assert!(
                !(prev_is_assistant_chat && next_is_tool_calls),
                "history contains Chat(assistant) immediately before AssistantToolCalls — \
                 duplicate narration push was not removed"
            );
        }
    }

    /// Streaming mock that emits narration text + tool call on the first turn,
    /// then a plain text response on the second. Used to verify the streaming
    /// path has the same duplicate-narration guard as the blocking path.
    struct NarrationStreamModelProvider {
        call_count: Arc<Mutex<usize>>,
    }

    #[async_trait]
    impl ModelProvider for NarrationStreamModelProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<String> {
            Ok("ok".into())
        }

        async fn chat(
            &self,
            _request: ChatRequest<'_>,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<clawcrew_providers::ChatResponse> {
            Ok(clawcrew_providers::ChatResponse {
                text: Some("done".into()),
                tool_calls: vec![],
                usage: None,
                reasoning_content: None,
            })
        }

        fn supports_native_tools(&self) -> bool {
            true
        }

        fn stream_chat(
            &self,
            _request: ChatRequest<'_>,
            _model: &str,
            _temperature: Option<f64>,
            _options: clawcrew_providers::traits::StreamOptions,
        ) -> futures_util::stream::BoxStream<
            'static,
            clawcrew_providers::traits::StreamResult<clawcrew_providers::traits::StreamEvent>,
        > {
            use futures_util::stream::{self, StreamExt};
            let mut count = self.call_count.lock();
            *count += 1;
            if *count == 1 {
                stream::iter(vec![
                    Ok(clawcrew_providers::traits::StreamEvent::TextDelta(
                        clawcrew_providers::traits::StreamChunk {
                            delta: "I will echo the message.".into(),
                            is_final: false,
                            reasoning: None,
                            token_count: 0,
                        },
                    )),
                    Ok(clawcrew_providers::traits::StreamEvent::ToolCall(
                        clawcrew_providers::ToolCall {
                            id: "tc1".into(),
                            name: "echo".into(),
                            arguments: "{}".into(),
                            extra_content: None,
                        },
                    )),
                    Ok(clawcrew_providers::traits::StreamEvent::Final),
                ])
                .boxed()
            } else {
                stream::iter(vec![
                    Ok(clawcrew_providers::traits::StreamEvent::TextDelta(
                        clawcrew_providers::traits::StreamChunk {
                            delta: "done".into(),
                            is_final: false,
                            reasoning: None,
                            token_count: 0,
                        },
                    )),
                    Ok(clawcrew_providers::traits::StreamEvent::Final),
                ])
                .boxed()
            }
        }
    }
    impl ::clawcrew_api::attribution::Attributable for NarrationStreamModelProvider {
        fn role(&self) -> ::clawcrew_api::attribution::Role {
            ::clawcrew_api::attribution::Role::Provider(
                ::clawcrew_api::attribution::ProviderKind::Model(
                    ::clawcrew_api::attribution::ModelProviderKind::Custom,
                ),
            )
        }
        fn alias(&self) -> &str {
            "NarrationStreamModelProvider"
        }
    }

    #[tokio::test]
    async fn streaming_narration_with_tool_calls_produces_no_consecutive_assistant_entries() {
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );

        let model_provider = Box::new(NarrationStreamModelProvider {
            call_count: Arc::new(Mutex::new(0)),
        });

        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = Agent::builder()
            .model_provider(model_provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .build()
            .expect("agent builder should succeed with valid config");

        let (event_tx, _event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(64);
        agent.turn_streamed("hi", event_tx, None).await.unwrap();

        let history = agent.history();
        for window in history.windows(2) {
            let prev_is_assistant_chat = matches!(
                &window[0],
                ConversationMessage::Chat(m) if m.role == "assistant"
            );
            let next_is_tool_calls =
                matches!(&window[1], ConversationMessage::AssistantToolCalls { .. });
            assert!(
                !(prev_is_assistant_chat && next_is_tool_calls),
                "streaming path: history contains Chat(assistant) immediately before \
                 AssistantToolCalls — duplicate narration push was not removed"
            );
        }
    }

    #[tokio::test]
    async fn response_cache_key_uses_full_provider_visible_transcript() {
        let tmp = tempfile::tempdir().expect("temp response cache dir");
        let cache = Arc::new(
            clawcrew_memory::response_cache::ResponseCache::new(tmp.path(), 60, 100)
                .expect("response cache should initialize"),
        );

        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem_a: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );
        let mem_b: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );

        let seen_a = Arc::new(Mutex::new(Vec::new()));
        let seen_b = Arc::new(Mutex::new(Vec::new()));
        let provider_a = Box::new(TranscriptCaptureModelProvider {
            alias: "transcript-a".into(),
            responses: Mutex::new(vec![clawcrew_providers::ChatResponse {
                text: Some("from prior transcript".into()),
                tool_calls: vec![],
                usage: None,
                reasoning_content: None,
            }]),
            seen_messages: seen_a.clone(),
        });
        let provider_b = Box::new(TranscriptCaptureModelProvider {
            alias: "transcript-b".into(),
            responses: Mutex::new(vec![clawcrew_providers::ChatResponse {
                text: Some("from fresh transcript".into()),
                tool_calls: vec![],
                usage: None,
                reasoning_content: None,
            }]),
            seen_messages: seen_b.clone(),
        });

        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent_a = Agent::builder()
            .model_provider(provider_a)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![],
            ))
            .memory(mem_a)
            .observer(observer.clone())
            .response_cache(Some(cache.clone()))
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .model_name("test-model".into())
            .temperature(Some(0.0))
            .build()
            .expect("agent builder should succeed with valid config");
        agent_a.seed_history(&[
            ChatMessage::user("earlier turn"),
            ChatMessage::assistant("earlier answer"),
        ]);

        let mut agent_b = Agent::builder()
            .model_provider(provider_b)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![],
            ))
            .memory(mem_b)
            .observer(observer)
            .response_cache(Some(cache))
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .model_name("test-model".into())
            .temperature(Some(0.0))
            .build()
            .expect("agent builder should succeed with valid config");

        assert_eq!(
            agent_a.turn("same final prompt").await.unwrap(),
            "from prior transcript"
        );
        assert_eq!(
            agent_b.turn("same final prompt").await.unwrap(),
            "from fresh transcript"
        );
        assert_eq!(seen_a.lock().len(), 1);
        assert_eq!(
            seen_b.lock().len(),
            1,
            "fresh transcript must not reuse a cache entry written for a different prior transcript"
        );
    }

    #[tokio::test]
    async fn response_cache_hit_cannot_bypass_cancelling_before_llm_hook() {
        let tmp = tempfile::tempdir().expect("temp response cache dir");
        let cache = Arc::new(
            clawcrew_memory::response_cache::ResponseCache::new(tmp.path(), 60, 100)
                .expect("response cache should initialize"),
        );
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let memory = || -> Arc<dyn Memory> {
            Arc::from(
                clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                    .expect("memory creation should succeed"),
            )
        };
        let seed_seen = Arc::new(Mutex::new(Vec::new()));

        let mut seed_agent = Agent::builder()
            .model_provider(Box::new(TranscriptCaptureModelProvider {
                alias: "shared-provider-alias".into(),
                responses: Mutex::new(vec![clawcrew_providers::ChatResponse {
                    text: Some("cached answer".into()),
                    tool_calls: vec![],
                    usage: None,
                    reasoning_content: None,
                }]),
                seen_messages: seed_seen,
            }))
            .model_provider_name("provider-a".into())
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![],
            ))
            .memory(memory())
            .observer(Arc::from(crate::observability::NoopObserver {}))
            .response_cache(Some(cache.clone()))
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(tmp.path().to_path_buf())
            .model_name("test-model".into())
            .temperature(Some(0.0))
            .turn_datetime(fixed_response_cache_turn_datetime)
            .build()
            .expect("seed agent should build");
        assert_eq!(
            seed_agent.turn("same request").await.unwrap(),
            "cached answer"
        );

        let mut hooks = crate::hooks::HookRunner::new();
        hooks.register(Box::new(CancellingBeforeLlmHook));
        let guarded_seen = Arc::new(Mutex::new(Vec::new()));
        let capturing = Arc::new(CapturingObserver::default());
        let mut guarded_agent = Agent::builder()
            .model_provider(Box::new(TranscriptCaptureModelProvider {
                alias: "shared-provider-alias".into(),
                responses: Mutex::new(vec![clawcrew_providers::ChatResponse {
                    text: Some("must not be returned".into()),
                    tool_calls: vec![],
                    usage: None,
                    reasoning_content: None,
                }]),
                seen_messages: guarded_seen.clone(),
            }))
            .model_provider_name("provider-a".into())
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![],
            ))
            .memory(memory())
            .observer(capturing.clone())
            .response_cache(Some(cache))
            .hook_runner(Some(Arc::new(hooks)))
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(tmp.path().to_path_buf())
            .model_name("test-model".into())
            .temperature(Some(0.0))
            .turn_datetime(fixed_response_cache_turn_datetime)
            .build()
            .expect("guarded agent should build");

        let error = guarded_agent
            .turn("same request")
            .await
            .expect_err("the request hook must cancel before cache reuse or provider dispatch");
        assert!(error.to_string().contains("blocked by request policy"));
        assert!(
            guarded_seen.lock().is_empty(),
            "a cancelling request hook must prevent provider dispatch"
        );
        assert!(
            !capturing
                .events
                .lock()
                .iter()
                .any(|event| matches!(event, ObserverEvent::LlmRequest { .. })),
            "a cancelling request hook must prevent request announcement"
        );
    }

    #[tokio::test]
    async fn before_llm_hook_mutates_ephemeral_request_and_attributes_selected_model() {
        let tmp = tempfile::tempdir().expect("temp workspace");
        let seen_messages = Arc::new(Mutex::new(Vec::new()));
        let seen_models = Arc::new(Mutex::new(Vec::new()));
        let hook_inputs = Arc::new(Mutex::new(Vec::new()));
        struct RequestCaptureProvider {
            seen_messages: Arc<Mutex<Vec<Vec<ChatMessage>>>>,
            seen_models: Arc<Mutex<Vec<String>>>,
        }
        #[async_trait]
        impl ModelProvider for RequestCaptureProvider {
            async fn chat_with_system(
                &self,
                _system_prompt: Option<&str>,
                _message: &str,
                _model: &str,
                _temperature: Option<f64>,
            ) -> Result<String> {
                Ok("provider answer".into())
            }

            async fn chat(
                &self,
                request: ChatRequest<'_>,
                model: &str,
                _temperature: Option<f64>,
            ) -> Result<clawcrew_providers::ChatResponse> {
                self.seen_messages.lock().push(request.messages.to_vec());
                self.seen_models.lock().push(model.to_string());
                Ok(clawcrew_providers::ChatResponse {
                    text: Some("provider answer".into()),
                    tool_calls: vec![],
                    usage: Some(clawcrew_providers::traits::TokenUsage {
                        input_tokens: Some(1_000),
                        cached_input_tokens: None,
                        cache_creation_input_tokens: None,
                        output_tokens: Some(200),
                    }),
                    reasoning_content: None,
                })
            }
        }
        impl ::clawcrew_api::attribution::Attributable for RequestCaptureProvider {
            fn role(&self) -> ::clawcrew_api::attribution::Role {
                ::clawcrew_api::attribution::Role::Provider(
                    ::clawcrew_api::attribution::ProviderKind::Model(
                        ::clawcrew_api::attribution::ModelProviderKind::Custom,
                    ),
                )
            }
            fn alias(&self) -> &str {
                "RequestCaptureProvider"
            }
        }

        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let memory: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, tmp.path(), None)
                .expect("memory creation should succeed"),
        );
        let mut hooks = crate::hooks::HookRunner::new();
        hooks.register(Box::new(MutatingBeforeLlmHook {
            seen_inputs: hook_inputs.clone(),
        }));
        let capturing = Arc::new(CapturingObserver::default());
        let turn_usage = Arc::new(parking_lot::Mutex::new(
            crate::agent::cost::TurnUsage::default(),
        ));
        let cost_context = crate::agent::cost::ToolLoopCostTrackingContext {
            tracker: None,
            model_provider_pricing: Arc::new(std::collections::HashMap::from([(
                "provider-a".to_string(),
                std::collections::HashMap::from([
                    ("hook-selected-model.input".to_string(), 3.0),
                    ("hook-selected-model.output".to_string(), 15.0),
                ]),
            )])),
            turn_usage: turn_usage.clone(),
            agent_alias: None,
        };
        let mut agent = Agent::builder()
            .model_provider(Box::new(RequestCaptureProvider {
                seen_messages: seen_messages.clone(),
                seen_models: seen_models.clone(),
            }))
            .model_provider_name("provider-a".into())
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![],
            ))
            .memory(memory)
            .observer(capturing.clone())
            .hook_runner(Some(Arc::new(hooks)))
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(tmp.path().to_path_buf())
            .model_name("base-model".into())
            .temperature(Some(0.0))
            .turn_datetime(fixed_response_cache_turn_datetime)
            .build()
            .expect("agent should build");

        let answer = crate::agent::cost::TOOL_LOOP_COST_TRACKING_CONTEXT
            .scope(Some(cost_context), agent.turn("durable user request"))
            .await
            .unwrap();
        assert_eq!(answer, "provider answer");
        let requests = seen_messages.lock();
        let provider_user = requests[0]
            .iter()
            .rev()
            .find(|message| message.role == "user")
            .expect("provider request must contain a user message");
        assert_eq!(provider_user.content, "hook-only provider request");
        assert_eq!(seen_models.lock().as_slice(), ["hook-selected-model"]);
        let hook_inputs = hook_inputs.lock();
        let hook_user = hook_inputs[0]
            .0
            .iter()
            .rev()
            .find(|message| message.role == "user")
            .expect("void hook input must contain a user message");
        assert_eq!(hook_user.content, "hook-only provider request");
        assert_eq!(hook_inputs[0].1, "hook-selected-model");
        let observed_models: Vec<_> = capturing
            .events
            .lock()
            .iter()
            .filter_map(|event| match event {
                ObserverEvent::LlmRequest { model, .. }
                | ObserverEvent::LlmResponse { model, .. } => Some(model.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            observed_models,
            ["hook-selected-model", "hook-selected-model"],
            "request and response telemetry must use the dispatched model"
        );
        let usage = *turn_usage.lock();
        assert_eq!(usage.input_tokens, 1_000);
        assert_eq!(usage.output_tokens, 200);
        assert!(
            (usage.cost_usd - 0.006).abs() < f64::EPSILON,
            "cost tracking must use the hook-selected model's configured rates"
        );

        let durable_user = agent
            .history()
            .iter()
            .find_map(|message| match message {
                ConversationMessage::Chat(chat) if chat.role == "user" => Some(&chat.content),
                _ => None,
            })
            .expect("durable history must retain the user message");
        assert!(durable_user.contains("durable user request"));
        assert!(!durable_user.contains("hook-only provider request"));
    }

    #[tokio::test]
    async fn before_llm_hook_uses_selected_route_for_complete_tool_protocol() {
        let tmp = tempfile::tempdir().expect("temp workspace");
        let default_requests: CapturedToolProtocolRequests = Arc::new(Mutex::new(Vec::new()));
        let text_requests: CapturedToolProtocolRequests = Arc::new(Mutex::new(Vec::new()));
        let router = clawcrew_providers::router::RouterModelProvider::new(
            "hook-router",
            vec![
                (
                    "default".into(),
                    Box::new(HookProtocolCaptureProvider {
                        supports_native: true,
                        requests: Arc::clone(&default_requests),
                    }) as Box<dyn ModelProvider>,
                ),
                (
                    "text".into(),
                    Box::new(HookProtocolCaptureProvider {
                        supports_native: false,
                        requests: Arc::clone(&text_requests),
                    }) as Box<dyn ModelProvider>,
                ),
            ],
            vec![(
                "text".into(),
                clawcrew_providers::router::Route {
                    provider_name: "text".into(),
                    model: "text-model".into(),
                },
            )],
            "native-model".into(),
        );
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let memory: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, tmp.path(), None)
                .expect("memory creation should succeed"),
        );
        let mut hooks = crate::hooks::HookRunner::new();
        hooks.register(Box::new(SelectingBeforeLlmHook {
            model: "hint:text".into(),
            system_suffix: Some("\n\nHook-owned system mutation.".into()),
        }));
        let mut agent = Agent::builder()
            .model_provider(Box::new(router))
            .model_provider_name("hook-router".into())
            .model_name("native-model".into())
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(memory)
            .observer(Arc::from(crate::observability::NoopObserver {}))
            .hook_runner(Some(Arc::new(hooks)))
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(tmp.path().to_path_buf())
            .build()
            .expect("agent should build");

        assert_eq!(
            agent.turn("use the selected route").await.unwrap(),
            "routed response"
        );
        assert!(
            default_requests.lock().is_empty(),
            "the pre-hook default route must not receive the request"
        );
        let requests = text_requests.lock();
        assert_eq!(
            requests.len(),
            1,
            "the selected route must receive one request"
        );
        let (model, sent_native_tools, messages) = &requests[0];
        assert_eq!(model, "text-model");
        assert!(
            !sent_native_tools,
            "the text-only selected route must not receive native tool specs"
        );
        let system_prompt = messages
            .iter()
            .find(|message| message.role == "system")
            .expect("provider request must include a system prompt")
            .content
            .as_str();
        assert!(system_prompt.contains("## Tools"));
        assert!(system_prompt.contains("## Tool Use Protocol"));
        assert!(system_prompt.contains("<tool_call>"));
        assert!(system_prompt.contains("echo"));
        assert!(system_prompt.contains("Parameters"));
        assert!(
            system_prompt.contains("Hook-owned system mutation."),
            "the protocol refresh must preserve hook-owned system changes"
        );
    }

    #[tokio::test]
    async fn before_llm_hook_rejects_strict_mixed_selected_route_before_dispatch() {
        let tmp = tempfile::tempdir().expect("temp workspace");
        let default_requests: CapturedToolProtocolRequests = Arc::new(Mutex::new(Vec::new()));
        let native_requests: CapturedToolProtocolRequests = Arc::new(Mutex::new(Vec::new()));
        let text_requests: CapturedToolProtocolRequests = Arc::new(Mutex::new(Vec::new()));
        let mixed = clawcrew_providers::reliable::ReliableModelProvider::new(
            "mixed",
            vec![
                (
                    "native".into(),
                    Box::new(HookProtocolCaptureProvider {
                        supports_native: true,
                        requests: Arc::clone(&native_requests),
                    }) as Box<dyn ModelProvider>,
                ),
                (
                    "text".into(),
                    Box::new(HookProtocolCaptureProvider {
                        supports_native: false,
                        requests: Arc::clone(&text_requests),
                    }) as Box<dyn ModelProvider>,
                ),
            ],
            0,
            0,
        );
        let router = clawcrew_providers::router::RouterModelProvider::new(
            "hook-router",
            vec![
                (
                    "default".into(),
                    Box::new(HookProtocolCaptureProvider {
                        supports_native: true,
                        requests: Arc::clone(&default_requests),
                    }) as Box<dyn ModelProvider>,
                ),
                ("mixed".into(), Box::new(mixed) as Box<dyn ModelProvider>),
            ],
            vec![(
                "mixed".into(),
                clawcrew_providers::router::Route {
                    provider_name: "mixed".into(),
                    model: "mixed-model".into(),
                },
            )],
            "native-model".into(),
        );
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let memory: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, tmp.path(), None)
                .expect("memory creation should succeed"),
        );
        let mut hooks = crate::hooks::HookRunner::new();
        hooks.register(Box::new(SelectingBeforeLlmHook {
            model: "hint:mixed".into(),
            system_suffix: None,
        }));
        let config = clawcrew_config::schema::AliasedAgentConfig {
            resolved: clawcrew_config::schema::ResolvedRuntime {
                strict_tool_parsing: true,
                ..Default::default()
            },
            ..clawcrew_config::schema::AliasedAgentConfig::default()
        };
        let mut agent = Agent::builder()
            .model_provider(Box::new(router))
            .model_provider_name("hook-router".into())
            .model_name("native-model".into())
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(memory)
            .observer(Arc::from(crate::observability::NoopObserver {}))
            .hook_runner(Some(Arc::new(hooks)))
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .config(config)
            .workspace_dir(tmp.path().to_path_buf())
            .build()
            .expect("agent should build");

        let error = agent
            .turn("use the selected mixed route")
            .await
            .expect_err("strict mixed selected route must fail before dispatch");
        assert!(
            error
                .to_string()
                .contains("Strict tool parsing cannot run a fallback chain"),
            "unexpected error: {error}"
        );
        assert!(default_requests.lock().is_empty());
        assert!(native_requests.lock().is_empty());
        assert!(text_requests.lock().is_empty());
    }

    #[tokio::test]
    async fn before_llm_hook_selected_model_attributes_provider_failure() {
        let tmp = tempfile::tempdir().expect("temp workspace");
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let memory: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, tmp.path(), None)
                .expect("memory creation should succeed"),
        );
        let mut hooks = crate::hooks::HookRunner::new();
        hooks.register(Box::new(MutatingBeforeLlmHook {
            seen_inputs: Arc::new(Mutex::new(Vec::new())),
        }));
        let capturing = Arc::new(CapturingObserver::default());
        let mut agent = Agent::builder()
            .model_provider(Box::new(FailingModelProvider))
            .model_provider_name("provider-a".into())
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![],
            ))
            .memory(memory)
            .observer(capturing.clone())
            .hook_runner(Some(Arc::new(hooks)))
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(tmp.path().to_path_buf())
            .model_name("base-model".into())
            .temperature(Some(0.0))
            .turn_datetime(fixed_response_cache_turn_datetime)
            .build()
            .expect("agent should build");

        agent
            .turn("durable user request")
            .await
            .expect_err("provider failure must surface");

        let events = capturing.events.lock();
        let observed: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                ObserverEvent::LlmRequest { model, .. } => Some((model.as_str(), None)),
                ObserverEvent::LlmResponse { model, success, .. } => {
                    Some((model.as_str(), Some(*success)))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            observed,
            [
                ("hook-selected-model", None),
                ("hook-selected-model", Some(false)),
            ],
            "request and failure telemetry must use the dispatched model"
        );
    }

    #[tokio::test]
    async fn response_cache_separates_configured_provider_identity() {
        let tmp = tempfile::tempdir().expect("temp response cache dir");
        let cache = Arc::new(
            clawcrew_memory::response_cache::ResponseCache::new(tmp.path(), 60, 100)
                .expect("response cache should initialize"),
        );
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let memory = || -> Arc<dyn Memory> {
            Arc::from(
                clawcrew_memory::create_memory(&memory_cfg, tmp.path(), None)
                    .expect("memory creation should succeed"),
            )
        };
        let seen_a = Arc::new(Mutex::new(Vec::new()));
        let seen_b = Arc::new(Mutex::new(Vec::new()));
        let build =
            |provider_alias: &str,
             answer: &str,
             seen: Arc<Mutex<Vec<Vec<ChatMessage>>>>,
             cache: Arc<clawcrew_memory::response_cache::ResponseCache>| {
                Agent::builder()
                    .model_provider(Box::new(TranscriptCaptureModelProvider {
                        alias: provider_alias.into(),
                        responses: Mutex::new(vec![clawcrew_providers::ChatResponse {
                            text: Some(answer.into()),
                            tool_calls: vec![],
                            usage: None,
                            reasoning_content: None,
                        }]),
                        seen_messages: seen,
                    }))
                    .model_provider_name("provider-family".into())
                    .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                        vec![],
                    ))
                    .memory(memory())
                    .observer(Arc::from(crate::observability::NoopObserver {}))
                    .response_cache(Some(cache))
                    .tool_dispatcher(Box::new(NativeToolDispatcher))
                    .workspace_dir(tmp.path().to_path_buf())
                    .model_name("shared-model".into())
                    .temperature(Some(0.0))
                    .turn_datetime(fixed_response_cache_turn_datetime)
                    .build()
                    .expect("agent should build")
            };

        let mut agent_a = build("work", "answer-a", seen_a.clone(), cache.clone());
        let mut agent_b = build("personal", "answer-b", seen_b.clone(), cache);
        assert_eq!(agent_a.turn("same request").await.unwrap(), "answer-a");
        assert_eq!(agent_b.turn("same request").await.unwrap(), "answer-b");
        assert_eq!(seen_a.lock().len(), 1);
        assert_eq!(seen_b.lock().len(), 1);
    }

    fn context_recovery_cache_agent(
        workspace: &std::path::Path,
        cache: Arc<clawcrew_memory::response_cache::ResponseCache>,
        calls: Arc<AtomicUsize>,
        answer: &str,
        reject_full_context: bool,
    ) -> Agent {
        let memory: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(
                &clawcrew_config::schema::MemoryConfig {
                    backend: "none".into(),
                    ..clawcrew_config::schema::MemoryConfig::default()
                },
                workspace,
                None,
            )
            .expect("memory creation should succeed"),
        );
        Agent::builder()
            .model_provider(Box::new(
                clawcrew_providers::reliable::ReliableModelProvider::new(
                    "shared-reliable",
                    vec![(
                        "primary".into(),
                        Box::new(ContextWindowModelProvider {
                            calls,
                            answer: answer.into(),
                            reject_full_context,
                        }),
                    )],
                    1,
                    1,
                ),
            ))
            .model_provider_name("provider-family".into())
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![],
            ))
            .memory(memory)
            .observer(Arc::from(crate::observability::NoopObserver {}))
            .response_cache(Some(cache))
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(workspace.to_path_buf())
            .model_name("shared-model".into())
            .temperature(Some(0.0))
            .turn_datetime(fixed_response_cache_turn_datetime)
            .build()
            .expect("agent should build")
    }

    fn seed_context_recovery_history(agent: &mut Agent) {
        agent.seed_history(&[
            ChatMessage::user("older request"),
            ChatMessage::assistant("older answer"),
        ]);
    }

    #[tokio::test]
    async fn response_cache_does_not_store_non_streaming_context_recovery() {
        let tmp = tempfile::tempdir().expect("temp response cache dir");
        let cache = Arc::new(
            clawcrew_memory::response_cache::ResponseCache::new(tmp.path(), 60, 100)
                .expect("response cache should initialize"),
        );
        let recovering_calls = Arc::new(AtomicUsize::new(0));
        let full_context_calls = Arc::new(AtomicUsize::new(0));
        let mut recovering = context_recovery_cache_agent(
            tmp.path(),
            cache.clone(),
            recovering_calls.clone(),
            "truncated answer",
            true,
        );
        let mut full_context = context_recovery_cache_agent(
            tmp.path(),
            cache,
            full_context_calls.clone(),
            "full-context answer",
            false,
        );
        seed_context_recovery_history(&mut recovering);
        seed_context_recovery_history(&mut full_context);

        assert_eq!(
            recovering.turn("same request").await.unwrap(),
            "truncated answer"
        );
        assert_eq!(recovering_calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            full_context.turn("same request").await.unwrap(),
            "full-context answer",
            "the full-context turn must reach its provider instead of reusing the recovered response"
        );
        assert_eq!(full_context_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn response_cache_does_not_store_streamed_context_recovery() {
        let tmp = tempfile::tempdir().expect("temp response cache dir");
        let cache = Arc::new(
            clawcrew_memory::response_cache::ResponseCache::new(tmp.path(), 60, 100)
                .expect("response cache should initialize"),
        );
        let recovering_calls = Arc::new(AtomicUsize::new(0));
        let full_context_calls = Arc::new(AtomicUsize::new(0));
        let mut recovering = context_recovery_cache_agent(
            tmp.path(),
            cache.clone(),
            recovering_calls.clone(),
            "truncated stream",
            true,
        );
        let mut full_context = context_recovery_cache_agent(
            tmp.path(),
            cache,
            full_context_calls.clone(),
            "full-context stream",
            false,
        );
        seed_context_recovery_history(&mut recovering);
        seed_context_recovery_history(&mut full_context);
        let (event_tx_a, _event_rx_a) = tokio::sync::mpsc::channel(32);
        let (event_tx_b, _event_rx_b) = tokio::sync::mpsc::channel(32);

        assert_eq!(
            recovering
                .turn_streamed("same request", event_tx_a, None)
                .await
                .unwrap()
                .0,
            "truncated stream"
        );
        assert_eq!(recovering_calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            full_context
                .turn_streamed("same request", event_tx_b, None)
                .await
                .unwrap()
                .0,
            "full-context stream",
            "the streamed full-context turn must reach its provider instead of reusing the recovered response"
        );
        assert_eq!(full_context_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn response_cache_bypasses_non_streaming_provider_failover() {
        let tmp = tempfile::tempdir().expect("temp response cache dir");
        let cache = Arc::new(
            clawcrew_memory::response_cache::ResponseCache::new(tmp.path(), 60, 100)
                .expect("response cache should initialize"),
        );
        let primary_calls = Arc::new(AtomicUsize::new(0));
        let fallback_calls = Arc::new(AtomicUsize::new(0));
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let memory = || -> Arc<dyn Memory> {
            Arc::from(
                clawcrew_memory::create_memory(&memory_cfg, tmp.path(), None)
                    .expect("memory creation should succeed"),
            )
        };
        let build = |answer: &str| {
            Agent::builder()
                .model_provider(Box::new(
                    clawcrew_providers::reliable::ReliableModelProvider::new(
                        "shared-reliable",
                        vec![
                            (
                                "primary".into(),
                                Box::new(AlwaysFailModelProvider {
                                    calls: primary_calls.clone(),
                                }),
                            ),
                            (
                                "fallback".into(),
                                Box::new(CountingAnswerModelProvider {
                                    calls: fallback_calls.clone(),
                                    answer: answer.into(),
                                }),
                            ),
                        ],
                        0,
                        1,
                    ),
                ))
                .model_provider_name("provider-family".into())
                .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                    vec![],
                ))
                .memory(memory())
                .observer(Arc::from(crate::observability::NoopObserver {}))
                .response_cache(Some(cache.clone()))
                .tool_dispatcher(Box::new(NativeToolDispatcher))
                .workspace_dir(tmp.path().to_path_buf())
                .model_name("shared-model".into())
                .temperature(Some(0.0))
                .turn_datetime(fixed_response_cache_turn_datetime)
                .build()
                .expect("agent should build")
        };

        let mut first = build("fallback-a");
        let mut second = build("fallback-b");
        assert!(
            first
                .turn("same request")
                .await
                .unwrap()
                .contains("fallback-a")
        );
        assert!(
            second
                .turn("same request")
                .await
                .unwrap()
                .contains("fallback-b")
        );
        assert_eq!(primary_calls.load(Ordering::SeqCst), 2);
        assert_eq!(fallback_calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn response_cache_bypasses_streamed_provider_failover() {
        let tmp = tempfile::tempdir().expect("temp response cache dir");
        let cache = Arc::new(
            clawcrew_memory::response_cache::ResponseCache::new(tmp.path(), 60, 100)
                .expect("response cache should initialize"),
        );
        let primary_calls = Arc::new(AtomicUsize::new(0));
        let fallback_calls = Arc::new(AtomicUsize::new(0));
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let memory = || -> Arc<dyn Memory> {
            Arc::from(
                clawcrew_memory::create_memory(&memory_cfg, tmp.path(), None)
                    .expect("memory creation should succeed"),
            )
        };
        let build = |answer: &str| {
            Agent::builder()
                .model_provider(Box::new(
                    clawcrew_providers::reliable::ReliableModelProvider::new(
                        "shared-reliable",
                        vec![
                            (
                                "primary".into(),
                                Box::new(AlwaysFailModelProvider {
                                    calls: primary_calls.clone(),
                                }),
                            ),
                            (
                                "fallback".into(),
                                Box::new(CountingAnswerModelProvider {
                                    calls: fallback_calls.clone(),
                                    answer: answer.into(),
                                }),
                            ),
                        ],
                        0,
                        1,
                    ),
                ))
                .model_provider_name("provider-family".into())
                .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                    vec![],
                ))
                .memory(memory())
                .observer(Arc::from(crate::observability::NoopObserver {}))
                .response_cache(Some(cache.clone()))
                .tool_dispatcher(Box::new(NativeToolDispatcher))
                .workspace_dir(tmp.path().to_path_buf())
                .model_name("shared-model".into())
                .temperature(Some(0.0))
                .turn_datetime(fixed_response_cache_turn_datetime)
                .build()
                .expect("agent should build")
        };

        let mut first = build("fallback-a");
        let mut second = build("fallback-b");
        let (event_tx_a, _event_rx_a) = tokio::sync::mpsc::channel(32);
        let (event_tx_b, _event_rx_b) = tokio::sync::mpsc::channel(32);
        let first_response = first
            .turn_streamed("same request", event_tx_a, None)
            .await
            .unwrap()
            .0;
        let second_response = second
            .turn_streamed("same request", event_tx_b, None)
            .await
            .unwrap()
            .0;
        assert!(first_response.contains("fallback-a"));
        assert!(second_response.contains("fallback-b"));
        assert_eq!(primary_calls.load(Ordering::SeqCst), 2);
        assert_eq!(fallback_calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn response_cache_bypasses_non_streaming_safeguard_fallback() {
        let tmp = tempfile::tempdir().expect("temp response cache dir");
        let cache = Arc::new(
            clawcrew_memory::response_cache::ResponseCache::new(tmp.path(), 60, 100)
                .expect("response cache should initialize"),
        );
        let calls = Arc::new(AtomicUsize::new(0));
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let memory = || -> Arc<dyn Memory> {
            Arc::from(
                clawcrew_memory::create_memory(&memory_cfg, tmp.path(), None)
                    .expect("memory creation should succeed"),
            )
        };
        let build = |answer: &str| {
            Agent::builder()
                .model_provider(Box::new(CountingSafeguardModelProvider {
                    calls: calls.clone(),
                    answer: answer.into(),
                }))
                .model_provider_name("anthropic".into())
                .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                    vec![],
                ))
                .memory(memory())
                .observer(Arc::from(crate::observability::NoopObserver {}))
                .response_cache(Some(cache.clone()))
                .tool_dispatcher(Box::new(NativeToolDispatcher))
                .workspace_dir(tmp.path().to_path_buf())
                .model_name("claude-sonnet-4-6".into())
                .temperature(Some(0.0))
                .turn_datetime(fixed_response_cache_turn_datetime)
                .build()
                .expect("agent should build")
        };

        let mut first = build("first-answer");
        let mut second = build("second-answer");

        let first_resp = first.turn("hello").await.unwrap();
        assert!(first_resp.contains("first-answer"));
        assert!(
            first_resp.contains("server-fallback-model"),
            "non-streaming safeguard fallback turn must append fallback notice"
        );
        let history_text = match first.history.last() {
            Some(ConversationMessage::Chat(chat)) => chat.content.clone(),
            _ => String::new(),
        };
        assert!(
            !history_text.contains("server-fallback-model"),
            "canonical history must not contain appended safeguard notice: {history_text}"
        );

        let second_resp = second.turn("hello").await.unwrap();
        assert!(second_resp.contains("second-answer"));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "safeguard fallback turn must bypass cache and reach the provider"
        );
    }

    #[tokio::test]
    async fn response_cache_bypasses_streamed_safeguard_fallback() {
        let tmp = tempfile::tempdir().expect("temp response cache dir");
        let cache = Arc::new(
            clawcrew_memory::response_cache::ResponseCache::new(tmp.path(), 60, 100)
                .expect("response cache should initialize"),
        );
        let calls = Arc::new(AtomicUsize::new(0));
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let memory = || -> Arc<dyn Memory> {
            Arc::from(
                clawcrew_memory::create_memory(&memory_cfg, tmp.path(), None)
                    .expect("memory creation should succeed"),
            )
        };
        let build = |answer: &str| {
            Agent::builder()
                .model_provider(Box::new(CountingSafeguardModelProvider {
                    calls: calls.clone(),
                    answer: answer.into(),
                }))
                .model_provider_name("anthropic".into())
                .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                    vec![],
                ))
                .memory(memory())
                .observer(Arc::from(crate::observability::NoopObserver {}))
                .response_cache(Some(cache.clone()))
                .tool_dispatcher(Box::new(NativeToolDispatcher))
                .workspace_dir(tmp.path().to_path_buf())
                .model_name("claude-sonnet-4-6".into())
                .temperature(Some(0.0))
                .turn_datetime(fixed_response_cache_turn_datetime)
                .build()
                .expect("agent should build")
        };

        let mut first = build("first-stream");
        let mut second = build("second-stream");

        let (event_tx_a, _event_rx_a) = tokio::sync::mpsc::channel(32);
        let (event_tx_b, _event_rx_b) = tokio::sync::mpsc::channel(32);

        let (first_resp, _) = first
            .turn_streamed("hello", event_tx_a, None)
            .await
            .unwrap();
        assert!(first_resp.contains("first-stream"));
        assert!(
            first_resp.contains("server-fallback-model"),
            "streamed turn must append safeguard fallback notice"
        );

        let (second_resp, _) = second
            .turn_streamed("hello", event_tx_b, None)
            .await
            .unwrap();
        assert!(second_resp.contains("second-stream"));
        assert!(
            second_resp.contains("server-fallback-model"),
            "streamed turn must append safeguard fallback notice"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "streamed safeguard fallback turn must bypass cache and reach the provider"
        );
    }

    #[tokio::test]
    async fn response_cache_distinguishes_model_pin_identity() {
        let tmp = tempfile::tempdir().expect("temp response cache dir");
        let cache = Arc::new(
            clawcrew_memory::response_cache::ResponseCache::new(tmp.path(), 60, 100)
                .expect("response cache should initialize"),
        );
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let memory = || -> Arc<dyn Memory> {
            Arc::from(
                clawcrew_memory::create_memory(&memory_cfg, tmp.path(), None)
                    .expect("memory creation should succeed"),
            )
        };
        let seen_a = Arc::new(Mutex::new(Vec::new()));
        let seen_b = Arc::new(Mutex::new(Vec::new()));
        let seen_c = Arc::new(Mutex::new(Vec::new()));
        let seen_d = Arc::new(Mutex::new(Vec::new()));
        let build = |answer: &str, pinned_model: &str, seen: Arc<Mutex<Vec<Vec<ChatMessage>>>>| {
            let pinned = clawcrew_providers::model_pin::ModelPinnedProvider::builder("shared-pin")
                .pinned_model(pinned_model)
                .inner(Box::new(TranscriptCaptureModelProvider {
                    alias: "shared-child".into(),
                    responses: Mutex::new(vec![clawcrew_providers::ChatResponse {
                        text: Some(answer.into()),
                        tool_calls: vec![],
                        usage: None,
                        reasoning_content: None,
                    }]),
                    seen_messages: seen,
                }))
                .build();
            let reliable = clawcrew_providers::reliable::ReliableModelProvider::new(
                "shared-reliable",
                vec![("only".into(), Box::new(pinned))],
                0,
                1,
            );
            Agent::builder()
                .model_provider(Box::new(reliable))
                .model_provider_name("provider-family".into())
                .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                    vec![],
                ))
                .memory(memory())
                .observer(Arc::from(crate::observability::NoopObserver {}))
                .response_cache(Some(cache.clone()))
                .tool_dispatcher(Box::new(NativeToolDispatcher))
                .workspace_dir(tmp.path().to_path_buf())
                .model_name("requested-model".into())
                .temperature(Some(0.0))
                .turn_datetime(fixed_response_cache_turn_datetime)
                .build()
                .expect("agent should build")
        };

        let mut first = build("pin-a", "model-a", seen_a.clone());
        let mut second = build("pin-b", "model-b", seen_b.clone());
        assert_eq!(first.turn("same request").await.unwrap(), "pin-a");
        assert_eq!(second.turn("same request").await.unwrap(), "pin-b");
        assert_eq!(seen_a.lock().len(), 1);
        assert_eq!(seen_b.lock().len(), 1);

        let mut third = build("stable-pin-a", "requested-model", seen_c.clone());
        let mut fourth = build("stable-pin-b", "requested-model", seen_d.clone());
        assert_eq!(third.turn("stable request").await.unwrap(), "stable-pin-a");
        assert_eq!(fourth.turn("stable request").await.unwrap(), "stable-pin-a");
        assert_eq!(seen_c.lock().len(), 1);
        assert!(
            seen_d.lock().is_empty(),
            "identity-preserving pins must retain ordinary response-cache reuse"
        );
    }

    #[tokio::test]
    async fn response_cache_bypasses_router_hint_remaps() {
        let tmp = tempfile::tempdir().expect("temp response cache dir");
        let cache = Arc::new(
            clawcrew_memory::response_cache::ResponseCache::new(tmp.path(), 60, 100)
                .expect("response cache should initialize"),
        );
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let memory = || -> Arc<dyn Memory> {
            Arc::from(
                clawcrew_memory::create_memory(&memory_cfg, tmp.path(), None)
                    .expect("memory creation should succeed"),
            )
        };
        let seen_a = Arc::new(Mutex::new(Vec::new()));
        let seen_b = Arc::new(Mutex::new(Vec::new()));
        let seen_c = Arc::new(Mutex::new(Vec::new()));
        let seen_d = Arc::new(Mutex::new(Vec::new()));
        let build = |answer: &str, routed_model: &str, seen: Arc<Mutex<Vec<Vec<ChatMessage>>>>| {
            let router = clawcrew_providers::router::RouterModelProvider::new(
                "shared-router",
                vec![(
                    "provider.default".into(),
                    Box::new(TranscriptCaptureModelProvider {
                        alias: "shared-child".into(),
                        responses: Mutex::new(vec![clawcrew_providers::ChatResponse {
                            text: Some(answer.into()),
                            tool_calls: vec![],
                            usage: None,
                            reasoning_content: None,
                        }]),
                        seen_messages: seen,
                    }),
                )],
                vec![(
                    "fast".into(),
                    clawcrew_providers::router::Route {
                        provider_name: "provider.default".into(),
                        model: routed_model.into(),
                    },
                )],
                "default-model".into(),
            );
            Agent::builder()
                .model_provider(Box::new(router))
                .model_provider_name("router".into())
                .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                    vec![],
                ))
                .memory(memory())
                .observer(Arc::from(crate::observability::NoopObserver {}))
                .response_cache(Some(cache.clone()))
                .tool_dispatcher(Box::new(NativeToolDispatcher))
                .workspace_dir(tmp.path().to_path_buf())
                .model_name("hint:fast".into())
                .temperature(Some(0.0))
                .turn_datetime(fixed_response_cache_turn_datetime)
                .build()
                .expect("agent should build")
        };

        let mut first = build("route-a", "model-a", seen_a.clone());
        let mut second = build("route-b", "model-b", seen_b.clone());
        assert_eq!(first.turn("same request").await.unwrap(), "route-a");
        assert_eq!(second.turn("same request").await.unwrap(), "route-b");
        assert_eq!(seen_a.lock().len(), 1);
        assert_eq!(seen_b.lock().len(), 1);

        let mut third = build("route-c", "model-c", seen_c.clone());
        let mut fourth = build("route-d", "model-d", seen_d.clone());
        let (event_tx_c, _event_rx_c) = tokio::sync::mpsc::channel(32);
        let (event_tx_d, _event_rx_d) = tokio::sync::mpsc::channel(32);
        assert_eq!(
            third
                .turn_streamed("same request", event_tx_c, None)
                .await
                .unwrap()
                .0,
            "route-c"
        );
        assert_eq!(
            fourth
                .turn_streamed("same request", event_tx_d, None)
                .await
                .unwrap()
                .0,
            "route-d"
        );
        assert_eq!(seen_c.lock().len(), 1);
        assert_eq!(seen_d.lock().len(), 1);
    }

    #[tokio::test]
    async fn response_cache_bypasses_native_thinking_overrides() {
        let tmp = tempfile::tempdir().expect("temp response cache dir");
        let cache = Arc::new(
            clawcrew_memory::response_cache::ResponseCache::new(tmp.path(), 60, 100)
                .expect("response cache should initialize"),
        );
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let memory = || -> Arc<dyn Memory> {
            Arc::from(
                clawcrew_memory::create_memory(&memory_cfg, tmp.path(), None)
                    .expect("memory creation should succeed"),
            )
        };
        let seen_a = Arc::new(Mutex::new(Vec::new()));
        let seen_b = Arc::new(Mutex::new(Vec::new()));
        let build =
            |answer: &str,
             seen: Arc<Mutex<Vec<Vec<ChatMessage>>>>,
             cache: Arc<clawcrew_memory::response_cache::ResponseCache>| {
                Agent::builder()
                    .model_provider(Box::new(TranscriptCaptureModelProvider {
                        alias: "thinking-provider".into(),
                        responses: Mutex::new(vec![clawcrew_providers::ChatResponse {
                            text: Some(answer.into()),
                            tool_calls: vec![],
                            usage: None,
                            reasoning_content: None,
                        }]),
                        seen_messages: seen,
                    }))
                    .model_provider_name("provider-family".into())
                    .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                        vec![],
                    ))
                    .memory(memory())
                    .observer(Arc::from(crate::observability::NoopObserver {}))
                    .response_cache(Some(cache))
                    .tool_dispatcher(Box::new(NativeToolDispatcher))
                    .workspace_dir(tmp.path().to_path_buf())
                    .model_name("shared-model".into())
                    .temperature(Some(0.0))
                    .turn_datetime(fixed_response_cache_turn_datetime)
                    .build()
                    .expect("agent should build")
            };

        let mut agent_a = build("answer-a", seen_a.clone(), cache.clone());
        let mut agent_b = build("answer-b", seen_b.clone(), cache);
        let answer_a = clawcrew_api::NATIVE_THINKING_OVERRIDE
            .scope(
                Some(clawcrew_api::model_provider::NativeThinkingParams {
                    budget_tokens: 1_024,
                    display: None,
                }),
                agent_a.turn("same request"),
            )
            .await
            .unwrap();
        let answer_b = clawcrew_api::NATIVE_THINKING_OVERRIDE
            .scope(
                Some(clawcrew_api::model_provider::NativeThinkingParams {
                    budget_tokens: 2_048,
                    display: None,
                }),
                agent_b.turn("same request"),
            )
            .await
            .unwrap();

        assert_eq!(answer_a, "answer-a");
        assert_eq!(answer_b, "answer-b");
        assert_eq!(seen_a.lock().len(), 1);
        assert_eq!(seen_b.lock().len(), 1);
    }

    #[tokio::test]
    async fn response_cache_bypasses_requests_that_can_advertise_tools() {
        let tmp = tempfile::tempdir().expect("temp response cache dir");
        let cache = Arc::new(
            clawcrew_memory::response_cache::ResponseCache::new(tmp.path(), 60, 100)
                .expect("response cache should initialize"),
        );
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let memory = || -> Arc<dyn Memory> {
            Arc::from(
                clawcrew_memory::create_memory(&memory_cfg, tmp.path(), None)
                    .expect("memory creation should succeed"),
            )
        };
        let seen_a = Arc::new(Mutex::new(Vec::new()));
        let seen_b = Arc::new(Mutex::new(Vec::new()));
        let build =
            |answer: &str,
             seen: Arc<Mutex<Vec<Vec<ChatMessage>>>>,
             cache: Arc<clawcrew_memory::response_cache::ResponseCache>| {
                Agent::builder()
                    .model_provider(Box::new(TranscriptCaptureModelProvider {
                        alias: "tool-capable".into(),
                        responses: Mutex::new(vec![clawcrew_providers::ChatResponse {
                            text: Some(answer.into()),
                            tool_calls: vec![],
                            usage: None,
                            reasoning_content: None,
                        }]),
                        seen_messages: seen,
                    }))
                    .model_provider_name("provider-a".into())
                    .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                        vec![Box::new(MockTool)],
                    ))
                    .memory(memory())
                    .observer(Arc::from(crate::observability::NoopObserver {}))
                    .response_cache(Some(cache))
                    .tool_dispatcher(Box::new(NativeToolDispatcher))
                    .workspace_dir(tmp.path().to_path_buf())
                    .model_name("shared-model".into())
                    .temperature(Some(0.0))
                    .turn_datetime(fixed_response_cache_turn_datetime)
                    .build()
                    .expect("agent should build")
            };

        let mut agent_a = build("answer-a", seen_a.clone(), cache.clone());
        let mut agent_b = build("answer-b", seen_b.clone(), cache);
        assert_eq!(agent_a.turn("same request").await.unwrap(), "answer-a");
        assert_eq!(agent_b.turn("same request").await.unwrap(), "answer-b");
        assert_eq!(seen_a.lock().len(), 1);
        assert_eq!(seen_b.lock().len(), 1);
    }

    #[tokio::test]
    async fn response_cache_does_not_cross_serve_memory_conditioned_answers() {
        // A backend whose recall always returns one Core entry with the given
        // content, so injection yields a deterministic, agent-specific preamble.
        // name() != "none" marks it a real, injecting backend for the gate.
        struct FixtureRecallMemory {
            content: String,
        }
        #[async_trait]
        impl Memory for FixtureRecallMemory {
            fn name(&self) -> &str {
                "fixture"
            }
            async fn store(
                &self,
                _: &str,
                _: &str,
                _: MemoryCategory,
                _: Option<&str>,
            ) -> anyhow::Result<()> {
                Ok(())
            }
            async fn recall(
                &self,
                _: &str,
                _: usize,
                _: Option<&str>,
                _: Option<&str>,
                _: Option<&str>,
            ) -> anyhow::Result<Vec<clawcrew_memory::MemoryEntry>> {
                Ok(vec![clawcrew_memory::MemoryEntry {
                    id: "deploy".into(),
                    key: "deploy".into(),
                    content: self.content.clone(),
                    category: MemoryCategory::Core,
                    scope: Default::default(),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    session_id: None,
                    score: None,
                    namespace: "default".into(),
                    importance: None,
                    superseded_by: None,
                    kind: None,
                    pinned: false,
                    tenant_id: None,
                    agent_alias: None,
                    agent_id: None,
                }])
            }
            async fn get(&self, _: &str) -> anyhow::Result<Option<clawcrew_memory::MemoryEntry>> {
                Ok(None)
            }
            async fn list(
                &self,
                _: Option<&MemoryCategory>,
                _: Option<&str>,
            ) -> anyhow::Result<Vec<clawcrew_memory::MemoryEntry>> {
                Ok(vec![])
            }
            async fn forget(&self, _: &str) -> anyhow::Result<bool> {
                Ok(true)
            }
            async fn forget_for_agent(&self, _: &str, _: &str) -> anyhow::Result<bool> {
                Ok(true)
            }
            async fn count(&self) -> anyhow::Result<usize> {
                Ok(1)
            }
            async fn health_check(&self) -> bool {
                true
            }
            async fn store_with_agent(
                &self,
                _: &str,
                _: &str,
                _: MemoryCategory,
                _: Option<&str>,
                _: Option<&str>,
                _: Option<f64>,
                _: Option<&str>,
            ) -> anyhow::Result<()> {
                Ok(())
            }
            async fn recall_for_agents(
                &self,
                _: &[&str],
                query: &str,
                limit: usize,
                session_id: Option<&str>,
                since: Option<&str>,
                until: Option<&str>,
            ) -> anyhow::Result<Vec<clawcrew_memory::MemoryEntry>> {
                self.recall(query, limit, session_id, since, until).await
            }
        }
        impl ::clawcrew_api::attribution::Attributable for FixtureRecallMemory {
            fn role(&self) -> ::clawcrew_api::attribution::Role {
                ::clawcrew_api::attribution::Role::Memory(
                    ::clawcrew_api::attribution::MemoryKind::InMemory,
                )
            }
            fn alias(&self) -> &str {
                "FixtureRecallMemory"
            }
        }

        // Frozen clock so both turns share a byte-identical bare transcript (the
        // per-turn `[CURRENT DATE & TIME]` prefix is otherwise second-precision),
        // which is what makes the two pre-injection cache keys collide.
        let fixed = chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Local);
        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});

        let last_user = |seen: &Arc<Mutex<Vec<Vec<ChatMessage>>>>| -> String {
            seen.lock()
                .last()
                .expect("a model call was captured")
                .iter()
                .rev()
                .find(|m| m.role == "user")
                .expect("a user message")
                .content
                .clone()
        };

        let build =
            |mem: Arc<dyn Memory>,
             seen: Arc<Mutex<Vec<Vec<ChatMessage>>>>,
             cache: Arc<clawcrew_memory::response_cache::ResponseCache>| {
                let provider = Box::new(TranscriptCaptureModelProvider {
                    alias: "memory-regression".into(),
                    responses: Mutex::new(vec![clawcrew_providers::ChatResponse {
                        text: Some("answer".into()),
                        tool_calls: vec![],
                        usage: None,
                        reasoning_content: None,
                    }]),
                    seen_messages: seen,
                });
                Agent::builder()
                    .model_provider(provider)
                    .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                        vec![],
                    ))
                    .memory(mem)
                    .observer(observer.clone())
                    .response_cache(Some(cache))
                    .tool_dispatcher(Box::new(NativeToolDispatcher))
                    .workspace_dir(std::path::PathBuf::from("/tmp"))
                    .model_name("test-model".into())
                    .temperature(Some(0.0))
                    .turn_datetime(move || fixed)
                    .build()
                    .expect("agent builder should succeed")
            };

        const PROMPT: &str = "what is the deploy target";

        // Harm case: same prompt, DIFFERENT recalled memory, one shared cache.
        let harm_dir = tempfile::tempdir().expect("cache dir");
        let harm_cache = Arc::new(
            clawcrew_memory::response_cache::ResponseCache::new(harm_dir.path(), 60, 100)
                .expect("response cache"),
        );
        let seen_a = Arc::new(Mutex::new(Vec::new()));
        let seen_b = Arc::new(Mutex::new(Vec::new()));
        let mut agent_a = build(
            Arc::new(FixtureRecallMemory {
                content: "the deploy target is prod-3-alpha".into(),
            }),
            seen_a.clone(),
            harm_cache.clone(),
        );
        let mut agent_b = build(
            Arc::new(FixtureRecallMemory {
                content: "the deploy target is prod-9-beta".into(),
            }),
            seen_b.clone(),
            harm_cache.clone(),
        );
        agent_a.turn(PROMPT).await.expect("turn a");
        agent_b.turn(PROMPT).await.expect("turn b");

        assert_eq!(seen_a.lock().len(), 1, "agent A always runs the model");
        assert!(
            last_user(&seen_a).contains("prod-3-alpha"),
            "agent A's model call must see A's injected memory"
        );
        // Pre-fix, B's key equals A's (both pre-injection) so B is served A's
        // prod-3 answer and never runs against its own prod-9 memory.
        assert_eq!(
            seen_b.lock().len(),
            1,
            "agent B must run the model, not reuse A's cache entry keyed on the shared pre-injection transcript"
        );
        assert!(
            last_user(&seen_b).contains("prod-9-beta"),
            "agent B's model call must see B's OWN injected memory, not A's"
        );

        // Control: `none` backend injects nothing, so the two transcripts really
        // are identical and the shared cache DOES hit: the second agent is
        // served from cache and never reaches the model. This proves the harm
        // case is not passing merely because the cache never works.
        let ctrl_dir = tempfile::tempdir().expect("cache dir");
        let ctrl_cache = Arc::new(
            clawcrew_memory::response_cache::ResponseCache::new(ctrl_dir.path(), 60, 100)
                .expect("response cache"),
        );
        let none_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let none_mem = || -> Arc<dyn Memory> {
            Arc::from(
                clawcrew_memory::create_memory(&none_cfg, std::path::Path::new("/tmp"), None)
                    .expect("none memory"),
            )
        };
        let seen_c = Arc::new(Mutex::new(Vec::new()));
        let seen_d = Arc::new(Mutex::new(Vec::new()));
        let mut agent_c = build(none_mem(), seen_c.clone(), ctrl_cache.clone());
        let mut agent_d = build(none_mem(), seen_d.clone(), ctrl_cache.clone());
        agent_c.turn(PROMPT).await.expect("turn c");
        agent_d.turn(PROMPT).await.expect("turn d");
        assert_eq!(seen_c.lock().len(), 1, "agent C always runs the model");
        assert_eq!(
            seen_d.lock().len(),
            0,
            "control: with no injection the identical prompt is served from the shared response cache"
        );
    }

    #[test]
    fn response_cache_key_skips_multimodal_image_markers() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let cache = Arc::new(
            clawcrew_memory::response_cache::ResponseCache::new(tmp.path(), 60, 100)
                .expect("response cache init"),
        );

        let mut agent = Agent::builder()
            .model_provider(Box::new(MockModelProvider {
                responses: Mutex::new(vec![]),
            }))
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![],
            ))
            .memory(Arc::from(
                clawcrew_memory::create_memory(
                    &clawcrew_config::schema::MemoryConfig {
                        backend: "none".into(),
                        ..clawcrew_config::schema::MemoryConfig::default()
                    },
                    std::path::Path::new("/tmp"),
                    None,
                )
                .expect("memory"),
            ))
            .observer(Arc::from(crate::observability::NoopObserver {}))
            .response_cache(Some(cache))
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .model_name("test-model".into())
            .temperature(Some(0.0))
            .build()
            .expect("agent builder");

        // Plain text messages should produce a cache key.
        let plain_messages = vec![
            ChatMessage::system("system prompt"),
            ChatMessage::user("hello"),
        ];
        let key = agent.response_cache_key_for_messages(&plain_messages, "test-model");
        assert!(key.is_some(), "plain text prompt must produce a cache key");

        let structurally_distinct_a = vec![
            ChatMessage::system("s"),
            ChatMessage::user("x|role=4:user;content=1:b"),
        ];
        let structurally_distinct_b = vec![
            ChatMessage::system("s|role=4:user;content=25:x"),
            ChatMessage::user("b"),
        ];
        assert_ne!(
            agent.response_cache_key_for_messages(&structurally_distinct_a, "test-model"),
            agent.response_cache_key_for_messages(&structurally_distinct_b, "test-model"),
            "structurally distinct provider requests must not share a cache key"
        );

        let mut activated = crate::tools::ActivatedToolSet::new();
        activated.activate("echo".into(), Arc::new(MockTool));
        agent.activated_tools = Some(Arc::new(std::sync::Mutex::new(activated)));
        let key = agent.response_cache_key_for_messages(&plain_messages, "test-model");
        assert!(
            key.is_none(),
            "an activated deferred tool can advertise a schema and must bypass response caching"
        );
        agent.activated_tools = None;

        agent.hook_runner = Some(Arc::new(crate::hooks::HookRunner::new()));
        let key = agent.response_cache_key_for_messages(&plain_messages, "test-model");
        assert!(
            key.is_some(),
            "an empty hook runner cannot change the request and must preserve cache eligibility"
        );
        agent.hook_runner = None;

        // Messages containing `[IMAGE:]` must return None (skip cache).
        let multimodal_messages = vec![
            ChatMessage::system("system prompt"),
            ChatMessage::user("describe this image [IMAGE:/tmp/photo.png]"),
        ];
        let key = agent.response_cache_key_for_messages(&multimodal_messages, "test-model");
        assert!(
            key.is_none(),
            "multimodal prompt with [IMAGE:] marker must skip response cache"
        );
    }

    #[tokio::test]
    async fn turn_streamed_with_steering_commits_streamed_output_before_continuing() {
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );

        let seen_messages = Arc::new(Mutex::new(Vec::new()));
        let model_provider = Box::new(StreamingSteeringModelProvider {
            seen_messages: seen_messages.clone(),
            call_count: AtomicUsize::new(0),
            fail_on_call: None,
            fail_chat_on_call: None,
            fail_after_delta_on_call: None,
            delay_chat_on_call: None,
        });
        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = Agent::builder()
            .model_provider(model_provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .build()
            .expect("agent builder should succeed with valid config");

        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(64);
        let (steering_tx, mut steering_rx) = tokio::sync::mpsc::channel::<String>(4);
        let handle = clawcrew_spawn::spawn!(async move {
            agent
                .turn_streamed_with_steering_state("first", event_tx, None, Some(&mut steering_rx))
                .await
        });

        loop {
            match event_rx.recv().await.expect("turn event should arrive") {
                TurnEvent::Chunk { delta } if delta == "draft" => {
                    steering_tx
                        .send("second".into())
                        .await
                        .expect("steering message should enqueue");
                    break;
                }
                _ => {}
            }
        }

        let outcome = handle
            .await
            .expect("turn task should finish")
            .expect("steered turn should succeed");
        assert_eq!(outcome.response, "draftfinal");

        let new_chat_messages: Vec<_> = outcome
            .new_messages
            .iter()
            .filter_map(|msg| match msg {
                ConversationMessage::Chat(message) => {
                    Some((message.role.as_str(), message.content.as_str()))
                }
                _ => None,
            })
            .collect();
        assert!(
            new_chat_messages
                .iter()
                .any(|(role, content)| { *role == "assistant" && *content == "draft" }),
            "already streamed output must be committed before the steering continuation"
        );
        assert!(
            new_chat_messages
                .iter()
                .any(|(role, content)| { *role == "user" && content.contains("second") }),
            "accepted steering must be retained as its own user turn"
        );
        // The steering turn must reach history in the SAME canonical
        // labeled envelope as the initial streamed user message. A bare
        // `[timestamp] text` prefix is the log/API-payload shape this
        // change exists to remove, so assert the exact envelope rather
        // than only that the text survived.
        let committed_steering = new_chat_messages
            .iter()
            .find(|(role, content)| *role == "user" && content.contains("second"))
            .expect("accepted steering must be retained as its own user turn")
            .1;
        assert!(
            committed_steering.starts_with("[CURRENT DATE & TIME: "),
            "committed steering must carry the labeled envelope, got: {committed_steering}"
        );
        assert!(
            committed_steering.ends_with("]\n\nsecond"),
            "committed steering must end with the raw user text after the envelope, got: {committed_steering}"
        );

        let seen = seen_messages.lock();
        assert_eq!(seen.len(), 2);
        let second_call = &seen[1];
        assert!(
            second_call
                .iter()
                .any(|msg| msg.role == "assistant" && msg.content == "draft"),
            "second provider call must see the committed streamed assistant text"
        );
        let provider_steering = second_call
            .iter()
            .filter(|msg| msg.role == "user")
            .find(|msg| msg.content.contains("second"))
            .expect("second provider call must include the accepted steering user message");
        assert!(
            provider_steering
                .content
                .starts_with("[CURRENT DATE & TIME: "),
            "the provider must receive the steering turn in the labeled envelope, got: {}",
            provider_steering.content
        );
        assert!(
            provider_steering.content.ends_with("]\n\nsecond"),
            "the provider's steering turn must end with the raw user text, got: {}",
            provider_steering.content
        );
    }

    #[tokio::test]
    async fn turn_streamed_with_steering_error_returns_committed_partial_output() {
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );

        let model_provider = Box::new(StreamingSteeringModelProvider {
            seen_messages: Arc::new(Mutex::new(Vec::new())),
            call_count: AtomicUsize::new(0),
            fail_on_call: Some(2),
            fail_chat_on_call: Some(3),
            fail_after_delta_on_call: None,
            delay_chat_on_call: None,
        });
        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = Agent::builder()
            .model_provider(model_provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .build()
            .expect("agent builder should succeed with valid config");

        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(64);
        let (steering_tx, mut steering_rx) = tokio::sync::mpsc::channel::<String>(4);
        let handle = clawcrew_spawn::spawn!(async move {
            agent
                .turn_streamed_with_steering_state("first", event_tx, None, Some(&mut steering_rx))
                .await
        });

        loop {
            match event_rx.recv().await.expect("turn event should arrive") {
                TurnEvent::Chunk { delta } if delta == "draft" => {
                    steering_tx
                        .send("second".into())
                        .await
                        .expect("steering message should enqueue");
                    break;
                }
                _ => {}
            }
        }

        let err = handle
            .await
            .expect("turn task should finish")
            .expect_err("second provider call should fail");
        assert_eq!(err.committed_response, "draft");
        assert!(
            err.new_messages.iter().any(|msg| {
                matches!(msg, ConversationMessage::Chat(message) if message.role == "assistant" && message.content == "draft")
            }),
            "committed partial assistant output should be returned for persistence after continuation failure"
        );
        assert!(
            err.new_messages.iter().any(|msg| {
                matches!(msg, ConversationMessage::Chat(message) if message.role == "user" && message.content.contains("second"))
            }),
            "accepted steering user message should still be returned after continuation failure"
        );
    }

    #[tokio::test]
    async fn turn_streamed_error_before_visible_output_falls_back_to_chat() {
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );

        let seen_messages = Arc::new(Mutex::new(Vec::new()));
        let model_provider = Box::new(StreamingSteeringModelProvider {
            seen_messages: seen_messages.clone(),
            call_count: AtomicUsize::new(0),
            fail_on_call: Some(1),
            fail_chat_on_call: None,
            fail_after_delta_on_call: None,
            delay_chat_on_call: None,
        });
        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = Agent::builder()
            .model_provider(model_provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .build()
            .expect("agent builder should succeed with valid config");

        let (event_tx, _event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(64);
        let handle = clawcrew_spawn::spawn!(async move {
            agent
                .turn_streamed_with_steering_state("first", event_tx, None, None)
                .await
        });

        let outcome = handle
            .await
            .expect("turn task should finish")
            .expect("pre-output stream failure should fall back to non-streaming chat");
        assert_eq!(outcome.response, "final");
        assert!(
            outcome.new_messages.iter().any(|msg| {
                matches!(msg, ConversationMessage::Chat(message) if message.role == "assistant" && message.content == "final")
            }),
            "new messages should carry the fallback assistant answer"
        );
        assert!(
            !outcome.new_messages.iter().any(|msg| {
                matches!(msg, ConversationMessage::Chat(message) if message.role == "assistant" && message.content.contains(&crate::i18n::get_english_cli_string_with_args("turn-stream-interrupted", &[])))
            }),
            "successful fallback should not persist interrupted stream text"
        );

        let seen = seen_messages.lock();
        assert_eq!(seen.len(), 2);
        assert!(
            !seen[1]
                .iter()
                .any(|msg| { msg.role == "assistant" && msg.content.contains("draft") }),
            "fallback chat must not receive the abandoned stream attempt as prior assistant text"
        );
    }

    #[tokio::test]
    async fn turn_streamed_error_after_delta_preserves_visible_partial() {
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );

        let model_provider = Box::new(StreamingSteeringModelProvider {
            seen_messages: Arc::new(Mutex::new(Vec::new())),
            call_count: AtomicUsize::new(0),
            fail_on_call: None,
            fail_chat_on_call: None,
            fail_after_delta_on_call: Some(1),
            delay_chat_on_call: None,
        });
        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = Agent::builder()
            .model_provider(model_provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .build()
            .expect("agent builder should succeed with valid config");

        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(64);
        let handle = clawcrew_spawn::spawn!(async move {
            agent
                .turn_streamed_with_steering_state("first", event_tx, None, None)
                .await
        });

        assert!(
            matches!(
                event_rx.recv().await,
                Some(TurnEvent::Chunk { delta }) if delta == "draft"
            ),
            "the client should see the streamed text before the provider error"
        );

        let err = handle
            .await
            .expect("turn task should finish")
            .expect_err("post-output stream failure should return an error with partial output");
        assert!(
            err.error
                .to_string()
                .contains("synthetic provider failure after delta"),
            "unexpected error: {}",
            err.error
        );
        assert!(
            err.committed_response
                .contains(&crate::i18n::get_english_cli_string_with_args(
                    "turn-stream-interrupted",
                    &[]
                )),
            "persisted partial text should mark that the visible stream was interrupted"
        );
        assert!(
            err.new_messages.iter().any(|msg| {
                matches!(msg, ConversationMessage::Chat(message) if message.role == "assistant" && message.content.contains("draft"))
            }),
            "new messages should carry the visible assistant partial for gateway persistence"
        );
    }

    #[tokio::test]
    async fn turn_streamed_error_before_visible_output_fallback_can_be_cancelled() {
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );

        let model_provider = Box::new(StreamingSteeringModelProvider {
            seen_messages: Arc::new(Mutex::new(Vec::new())),
            call_count: AtomicUsize::new(0),
            fail_on_call: Some(1),
            fail_chat_on_call: None,
            fail_after_delta_on_call: None,
            delay_chat_on_call: Some(2),
        });
        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = Agent::builder()
            .model_provider(model_provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .build()
            .expect("agent builder should succeed with valid config");

        let (event_tx, _event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(64);
        let cancel_token = tokio_util::sync::CancellationToken::new();
        let cancel_for_task = cancel_token.clone();
        let handle = clawcrew_spawn::spawn!(async move {
            agent
                .turn_streamed_with_steering_state("first", event_tx, Some(cancel_for_task), None)
                .await
        });

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        cancel_token.cancel();

        let err = handle
            .await
            .expect("turn task should finish")
            .expect_err("cancelled fallback should return cancellation");
        assert!(
            crate::agent::loop_::is_tool_loop_cancelled(&err.error),
            "unexpected error: {}",
            err.error
        );
        assert_eq!(
            err.committed_response,
            crate::i18n::get_english_cli_string_with_args("turn-interrupted-by-user", &[])
        );
        assert!(
            err.new_messages.iter().any(|msg| {
                matches!(msg, ConversationMessage::Chat(message) if message.role == "assistant" && message.content == crate::i18n::get_english_cli_string_with_args("turn-interrupted-by-user", &[]))
            }),
            "pre-output fallback cancellation should include an interruption marker"
        );
    }

    #[tokio::test]
    async fn turn_streamed_cancel_before_output_returns_interruption_message() {
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );

        let model_provider = Box::new(StreamingSteeringModelProvider {
            seen_messages: Arc::new(Mutex::new(Vec::new())),
            call_count: AtomicUsize::new(0),
            fail_on_call: None,
            fail_chat_on_call: None,
            fail_after_delta_on_call: None,
            delay_chat_on_call: None,
        });
        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = Agent::builder()
            .model_provider(model_provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .build()
            .expect("agent builder should succeed with valid config");

        let (event_tx, _event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(64);
        let cancel_token = tokio_util::sync::CancellationToken::new();
        cancel_token.cancel();

        let err = agent
            .turn_streamed_with_steering_state("first", event_tx, Some(cancel_token), None)
            .await
            .expect_err("pre-cancelled turn should return cancellation");

        assert!(
            crate::agent::loop_::is_tool_loop_cancelled(&err.error),
            "unexpected error: {}",
            err.error
        );
        assert_eq!(
            err.committed_response,
            crate::i18n::get_english_cli_string_with_args("turn-interrupted-by-user", &[])
        );
        assert!(
            err.new_messages.iter().any(|msg| {
                matches!(msg, ConversationMessage::Chat(message) if message.role == "assistant" && message.content == crate::i18n::get_english_cli_string_with_args("turn-interrupted-by-user", &[]))
            }),
            "cancelled turn should include an assistant interruption marker for persistence"
        );
    }

    #[tokio::test]
    async fn turn_streamed_stream_error_after_delta_emits_llm_response_failure() {
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );

        let model_provider = Box::new(StreamingSteeringModelProvider {
            seen_messages: Arc::new(Mutex::new(Vec::new())),
            call_count: AtomicUsize::new(0),
            fail_on_call: None,
            fail_chat_on_call: None,
            fail_after_delta_on_call: Some(1),
            delay_chat_on_call: None,
        });
        let capturing = Arc::new(CapturingObserver::default());
        let observer: Arc<dyn Observer> = capturing.clone();
        let mut agent = Agent::builder()
            .model_provider(model_provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .build()
            .expect("agent builder should succeed with valid config");

        let (event_tx, _event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(64);
        let err = agent
            .turn_streamed_with_steering_state("test", event_tx, None, None)
            .await
            .expect_err("provider stream failure should be returned");

        assert!(
            err.committed_response.contains("draft")
                && err
                    .committed_response
                    .contains(&crate::i18n::get_english_cli_string_with_args(
                        "turn-stream-interrupted",
                        &[]
                    )),
            "unexpected committed_response: {}",
            err.committed_response
        );

        let events = capturing.events.lock();
        let request = events
            .iter()
            .find(|e| matches!(e, ObserverEvent::LlmRequest { .. }))
            .expect("LlmRequest should have been recorded");
        let response = events
            .iter()
            .find(|e| matches!(e, ObserverEvent::LlmResponse { .. }))
            .expect("LlmResponse should have been recorded");

        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, ObserverEvent::LlmRequest { .. }))
                .count(),
            1,
            "exactly one LlmRequest expected"
        );
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, ObserverEvent::LlmResponse { .. }))
                .count(),
            1,
            "exactly one LlmResponse expected"
        );

        let (
            ObserverEvent::LlmRequest {
                model_provider: req_provider,
                model: req_model,
                ..
            },
            ObserverEvent::LlmResponse {
                model_provider: resp_provider,
                model: resp_model,
                success,
                error_message,
                ..
            },
        ) = (request, response)
        else {
            panic!("matched event variants should be LlmRequest and LlmResponse");
        };

        assert!(!success, "LlmResponse on stream error must be a failure");
        assert!(
            error_message.as_deref().is_some_and(|m| !m.is_empty()),
            "failure LlmResponse must carry a non-empty error_message"
        );
        assert_eq!(req_provider, resp_provider, "provider should match");
        assert_eq!(req_model, resp_model, "model should match");
    }

    #[tokio::test]
    async fn turn_streamed_cancel_during_stream_emits_llm_response_failure() {
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );

        let model_provider = Box::new(StreamingSteeringModelProvider {
            seen_messages: Arc::new(Mutex::new(Vec::new())),
            call_count: AtomicUsize::new(0),
            fail_on_call: None,
            fail_chat_on_call: None,
            fail_after_delta_on_call: None,
            delay_chat_on_call: None,
        });
        let capturing = Arc::new(CapturingObserver::default());
        let observer: Arc<dyn Observer> = capturing.clone();
        let mut agent = Agent::builder()
            .model_provider(model_provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .build()
            .expect("agent builder should succeed with valid config");

        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(64);
        let cancel_token = tokio_util::sync::CancellationToken::new();
        let cancel_for_task = cancel_token.clone();

        let canceller = clawcrew_spawn::spawn!(async move {
            while let Some(event) = event_rx.recv().await {
                if matches!(event, TurnEvent::Chunk { ref delta } if delta == "draft") {
                    cancel_for_task.cancel();
                    break;
                }
            }
            while event_rx.recv().await.is_some() {}
        });

        let err = agent
            .turn_streamed_with_steering_state("test", event_tx, Some(cancel_token), None)
            .await
            .expect_err("cancelled turn should return cancellation");

        canceller.await.expect("canceller task should finish");

        assert!(
            crate::agent::loop_::is_tool_loop_cancelled(&err.error),
            "cancelled turn should carry the cancellation error: {}",
            err.error
        );

        let events = capturing.events.lock();
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, ObserverEvent::LlmRequest { .. }))
                .count(),
            1,
            "exactly one LlmRequest expected"
        );
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, ObserverEvent::LlmResponse { .. }))
                .count(),
            1,
            "exactly one LlmResponse expected"
        );

        let request = events
            .iter()
            .find(|e| matches!(e, ObserverEvent::LlmRequest { .. }))
            .expect("LlmRequest should have been recorded");
        let response = events
            .iter()
            .find(|e| matches!(e, ObserverEvent::LlmResponse { .. }))
            .expect("LlmResponse should have been recorded");

        let (
            ObserverEvent::LlmRequest {
                model_provider: req_provider,
                model: req_model,
                ..
            },
            ObserverEvent::LlmResponse {
                model_provider: resp_provider,
                model: resp_model,
                success,
                error_message,
                ..
            },
        ) = (request, response)
        else {
            panic!("matched event variants should be LlmRequest and LlmResponse");
        };

        assert!(!success, "cancellation LlmResponse must be a failure");
        assert_eq!(
            error_message.as_deref(),
            Some("request cancelled by user"),
            "cancellation LlmResponse must carry the fixed cancel message"
        );
        assert_eq!(req_provider, resp_provider, "provider should match");
        assert_eq!(req_model, resp_model, "model should match");
    }

    // ── Skill tool registration & excluded_tools filtering ──────────

    /// A mock tool whose name is configurable (unlike `MockTool` which is
    /// always "echo").
    struct NamedMockTool {
        tool_name: String,
    }

    impl NamedMockTool {
        fn new(name: &str) -> Self {
            Self {
                tool_name: name.to_string(),
            }
        }
    }

    #[async_trait]
    impl Tool for NamedMockTool {
        fn name(&self) -> &str {
            &self.tool_name
        }

        fn description(&self) -> &str {
            "mock"
        }

        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }

        async fn execute(&self, _args: serde_json::Value) -> Result<crate::tools::ToolResult> {
            Ok(crate::tools::ToolResult {
                success: true,
                output: "ok".into(),
                error: None,
            })
        }
    }

    fn make_skill(name: &str, tool_names: &[&str]) -> crate::skills::Skill {
        crate::skills::Skill {
            name: name.to_string(),
            description: format!("{name} skill"),
            description_localizations: Default::default(),
            version: "0.1.0".to_string(),
            author: None,
            tags: vec![],
            tools: tool_names
                .iter()
                .map(|t| crate::skills::SkillTool {
                    name: t.to_string(),
                    description: format!("{t} tool"),
                    kind: "shell".to_string(),
                    command: format!("echo {t}"),
                    args: std::collections::HashMap::new(),
                    target: None,
                    locked_args: std::collections::HashMap::new(),
                    timeout_secs: None,
                })
                .collect(),
            prompts: vec![],
            slash_options: Vec::new(),
            always: false,
            location: None,
        }
    }

    #[test]
    fn register_skill_tools_adds_skill_tools_to_registry() {
        let security = Arc::new(crate::security::SecurityPolicy::default());
        let mut tools: Vec<Box<dyn Tool>> = vec![Box::new(NamedMockTool::new("builtin_a"))];

        let skills = vec![make_skill("deploy", &["run", "status"])];
        tools::register_skill_tools(&mut tools, &skills, security);

        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert_eq!(names, &["builtin_a", "deploy__run", "deploy__status"]);
    }

    #[test]
    fn register_skill_tools_skips_shadowed_builtins() {
        let security = Arc::new(crate::security::SecurityPolicy::default());
        // Pre-populate with a tool whose name matches what the skill would produce.
        let mut tools: Vec<Box<dyn Tool>> = vec![Box::new(NamedMockTool::new("my_skill__run"))];

        let skills = vec![make_skill("my_skill", &["run"])];
        tools::register_skill_tools(&mut tools, &skills, security);

        // Should still be just 1 tool — the duplicate was skipped.
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name(), "my_skill__run");
    }

    #[test]
    fn register_skill_tools_honors_excluded_tools() {
        // excluded_tools always subtracts — including skill-defined tools (previously
        // skill tools bypassed the policy entirely; theclass, missed for skills).
        let security = Arc::new(crate::security::SecurityPolicy {
            excluded_tools: Some(vec!["deploy__status".to_string()]),
            ..crate::security::SecurityPolicy::default()
        });
        let mut tools: Vec<Box<dyn Tool>> = vec![Box::new(NamedMockTool::new("builtin_a"))];

        let skills = vec![make_skill("deploy", &["run", "status"])];
        tools::register_skill_tools(&mut tools, &skills, security);

        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(
            names.contains(&"deploy__run"),
            "non-excluded skill tool must register, got {names:?}"
        );
        assert!(
            !names.contains(&"deploy__status"),
            "excluded_tools must subtract the skill tool deploy__status, got {names:?}"
        );
    }

    #[test]
    fn register_skill_tools_allowlist_does_not_hide_skills() {
        // The allowlist gates built-ins, NOT skill tools: skills are granted explicitly via
        // skill config, and builtin-kind skill tools are scoped-elevation wrappers meant to
        // stay callable when the raw tool is off the allowlist. A restrictive allowed_tools
        // that omits the skill tool must NOT remove it (only excluded_tools does).
        let security = Arc::new(crate::security::SecurityPolicy {
            allowed_tools: Some(vec!["shell".to_string()]),
            ..crate::security::SecurityPolicy::default()
        });
        let mut tools: Vec<Box<dyn Tool>> = Vec::new();

        let skills = vec![make_skill("deploy", &["run"])];
        tools::register_skill_tools(&mut tools, &skills, security);

        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(
            names.contains(&"deploy__run"),
            "allowlist must not hide an explicitly-granted skill tool, got {names:?}"
        );
    }

    #[test]
    fn register_skill_tools_deny_all_allowlist_denies_skill_tools() {
        // Deny-all (the runtime `Some(vec![])` produced by
        // `deny_all_tools = true`) applies to skill-defined tools as well as
        // built-ins and MCP. A nonempty allowlist still leaves skill tools
        // visible (see register_skill_tools_allowlist_does_not_hide_skills).
        let security = Arc::new(crate::security::SecurityPolicy {
            allowed_tools: Some(vec![]),
            ..crate::security::SecurityPolicy::default()
        });
        let mut tools: Vec<Box<dyn Tool>> = Vec::new();

        let skills = vec![
            make_skill("deploy", &["run"]),
            crate::skills::Skill {
                name: "weather".to_string(),
                description: "weather skill".to_string(),
                description_localizations: Default::default(),
                version: "0.1.0".to_string(),
                author: None,
                tags: vec![],
                tools: vec![crate::skills::SkillTool {
                    name: "get_weather".to_string(),
                    description: "http weather".to_string(),
                    kind: "http".to_string(),
                    command: String::new(),
                    args: std::collections::HashMap::new(),
                    target: Some("https://example.invalid/weather".to_string()),
                    locked_args: std::collections::HashMap::new(),
                    timeout_secs: None,
                }],
                prompts: vec![],
                slash_options: Vec::new(),
                always: false,
                location: None,
            },
        ];
        tools::register_skill_tools(&mut tools, &skills, security);

        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(
            !names.contains(&"deploy__run"),
            "deny-all must hide shell skill tools, got {names:?}"
        );
        assert!(
            !names.iter().any(|n| n.contains("weather")),
            "deny-all must hide HTTP skill tools, got {names:?}"
        );
    }

    #[test]
    fn from_config_policy_filter_blocks_raw_target_but_keeps_scoped_wrapper() {
        use crate::skills::{Skill, SkillTool};

        let shell: Arc<dyn Tool> = Arc::new(NamedMockTool::new("shell"));
        let file_read: Arc<dyn Tool> = Arc::new(NamedMockTool::new("file_read"));
        // The resolution registry retains the raw tool so the wrapper can
        // delegate to it even after the policy filter removes it below.
        let resolution: Vec<Arc<dyn Tool>> = vec![Arc::clone(&shell), Arc::clone(&file_read)];

        let mut tools: Vec<Box<dyn Tool>> = vec![
            Box::new(crate::tools::ArcToolRef(Arc::clone(&shell))),
            Box::new(crate::tools::ArcToolRef(Arc::clone(&file_read))),
        ];

        // Allowlist the agent to `file_read` only — the gate from_config now
        // applies to built-ins before skills register. (Pre-fix, from_config
        // honored only the denylist, so raw `shell` leaked through.)
        let policy = crate::security::SecurityPolicy {
            allowed_tools: Some(vec!["file_read".to_string()]),
            workspace_dir: std::env::temp_dir(),
            ..crate::security::SecurityPolicy::default()
        };
        crate::agent::loop_::apply_policy_tool_filter(&mut tools, Some(&policy), None);
        assert!(
            !tools.iter().any(|t| t.name() == "shell"),
            "raw shell must be removed by the allowlist on the from_config path"
        );
        assert!(
            tools.iter().any(|t| t.name() == "file_read"),
            "allowlisted file_read must survive the filter"
        );

        let skill = Skill {
            name: "ops".to_string(),
            description: "d".to_string(),
            description_localizations: Default::default(),
            version: "1".to_string(),
            author: None,
            tags: vec![],
            tools: vec![SkillTool {
                name: "use_shell".to_string(),
                description: "scoped shell".to_string(),
                kind: "builtin".to_string(),
                command: String::new(),
                args: std::collections::HashMap::new(),
                target: Some("shell".to_string()),
                locked_args: std::collections::HashMap::new(),
                timeout_secs: None,
            }],
            prompts: vec![],
            slash_options: Vec::new(),
            always: false,
            location: None,
        };
        tools::register_skill_tools_with_context(
            &mut tools,
            &[skill],
            Arc::new(crate::security::SecurityPolicy::default()),
            &resolution,
        );

        assert!(
            !tools.iter().any(|t| t.name() == "shell"),
            "raw shell must STILL be unavailable after skill registration"
        );
        assert!(
            tools.iter().any(|t| t.name() == "ops__use_shell"),
            "the scoped elevation wrapper must remain the only callable path to shell"
        );
    }

    #[test]
    fn excluded_tools_filters_matching_tools() {
        let mut tools: Vec<Box<dyn Tool>> = vec![
            Box::new(NamedMockTool::new("shell")),
            Box::new(NamedMockTool::new("file_write")),
            Box::new(NamedMockTool::new("web_search")),
        ];

        let excluded = ["shell".to_string(), "file_write".to_string()];
        tools.retain(|t| !excluded.iter().any(|ex| ex == t.name()));

        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert_eq!(names, &["web_search"]);
    }

    #[test]
    fn excluded_tools_preserves_non_excluded() {
        let mut tools: Vec<Box<dyn Tool>> = vec![
            Box::new(NamedMockTool::new("shell")),
            Box::new(NamedMockTool::new("file_read")),
            Box::new(NamedMockTool::new("web_fetch")),
        ];

        // Exclude only "shell" — the other two should survive.
        let excluded = ["shell".to_string()];
        tools.retain(|t| !excluded.iter().any(|ex| ex == t.name()));

        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert_eq!(names, &["file_read", "web_fetch"]);
    }

    #[test]
    fn empty_excluded_tools_preserves_all() {
        let mut tools: Vec<Box<dyn Tool>> = vec![
            Box::new(NamedMockTool::new("shell")),
            Box::new(NamedMockTool::new("file_read")),
        ];

        let excluded: Vec<String> = vec![];
        if !excluded.is_empty() {
            tools.retain(|t| !excluded.iter().any(|ex| ex == t.name()));
        }

        assert_eq!(tools.len(), 2);
    }

    #[tokio::test]
    async fn turn_streamed_returns_new_messages_at_history_limit() {
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );

        // Use a small limit so that pre-filling to the limit forces a trim on
        // the very first new turn.
        let agent_config = clawcrew_config::schema::AliasedAgentConfig {
            resolved: clawcrew_config::schema::ResolvedRuntime::default(),
            ..clawcrew_config::schema::AliasedAgentConfig::default()
        };

        // Simple streaming provider that returns plain text (no tool calls).
        let provider = Box::new(NarrationStreamModelProvider {
            call_count: Arc::new(Mutex::new(0)),
        });

        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = Agent::builder()
            .model_provider(provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .config(agent_config)
            .structured_max_history_messages(4)
            .build()
            .expect("agent builder should succeed with valid config");

        // Pre-fill the history to exactly max_history_messages non-system
        // messages so that adding a new user+assistant pair triggers trim.
        // (system message is added by turn_streamed on first call, so we
        // push user+assistant pairs to simulate a history-at-limit state.)
        agent
            .history
            .push(ConversationMessage::Chat(ChatMessage::system("sys")));
        for i in 0..2 {
            agent
                .history
                .push(ConversationMessage::Chat(ChatMessage::user(format!(
                    "old {i}"
                ))));
            agent
                .history
                .push(ConversationMessage::Chat(ChatMessage::assistant(format!(
                    "old reply {i}"
                ))));
        }
        // History is now: [system, user0, assistant0, user1, assistant1] = 5
        // entries. The structured message limit of 4 means trim fires after
        // adding the new turn.

        let (event_tx, _rx) = tokio::sync::mpsc::channel::<TurnEvent>(8);
        let (_, new_msgs) = agent
            .turn_streamed("new question", event_tx, None)
            .await
            .expect("turn_streamed should succeed");

        // The returned Vec must contain the new user message.
        let has_user = new_msgs
            .iter()
            .any(|m| matches!(m, ConversationMessage::Chat(c) if c.role == "user"));
        assert!(
            has_user,
            "new_msgs must include the user message even after trim; got: {new_msgs:?}"
        );

        // The returned Vec must contain the new assistant reply.
        let has_assistant = new_msgs
            .iter()
            .any(|m| matches!(m, ConversationMessage::Chat(c) if c.role == "assistant"));
        assert!(
            has_assistant,
            "new_msgs must include the assistant reply even after trim; got: {new_msgs:?}"
        );
    }

    #[test]
    fn excluded_tools_then_skill_registration_end_to_end() {
        let security = Arc::new(crate::security::SecurityPolicy::default());
        let mut tools: Vec<Box<dyn Tool>> = vec![
            Box::new(NamedMockTool::new("shell")),
            Box::new(NamedMockTool::new("file_read")),
            Box::new(NamedMockTool::new("web_fetch")),
        ];

        // Step 1: filter excluded tools (mirrors from_config logic)
        let excluded = ["shell".to_string()];
        tools.retain(|t| !excluded.iter().any(|ex| ex == t.name()));

        // Step 2: register skill tools (mirrors from_config logic)
        let skills = vec![make_skill("ops", &["deploy", "rollback"])];
        tools::register_skill_tools(&mut tools, &skills, security);

        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert_eq!(
            names,
            &["file_read", "web_fetch", "ops__deploy", "ops__rollback"]
        );
    }

    fn observer_event_turn_id(event: &ObserverEvent) -> Option<&str> {
        match event {
            ObserverEvent::AgentStart { turn_id, .. }
            | ObserverEvent::LlmRequest { turn_id, .. }
            | ObserverEvent::LlmResponse { turn_id, .. }
            | ObserverEvent::AgentEnd { turn_id, .. }
            | ObserverEvent::ToolCall { turn_id, .. }
            | ObserverEvent::ToolCallStart { turn_id, .. }
            | ObserverEvent::MemoryRecall { turn_id, .. }
            | ObserverEvent::MemoryStore { turn_id, .. }
            | ObserverEvent::RagRetrieve { turn_id, .. } => turn_id.as_deref(),
            _ => None,
        }
    }

    fn assert_all_events_share_turn_id(
        events: &[ObserverEvent],
        expected_alias: Option<&str>,
        expected_channel: Option<&str>,
    ) {
        let mut turn_ids: Vec<String> = Vec::new();
        for event in events {
            let (variant, channel, agent_alias, turn_id) = match event {
                ObserverEvent::AgentStart {
                    channel,
                    agent_alias,
                    turn_id,
                    ..
                } => ("AgentStart", channel, agent_alias, turn_id),
                ObserverEvent::AgentEnd {
                    channel,
                    agent_alias,
                    turn_id,
                    ..
                } => ("AgentEnd", channel, agent_alias, turn_id),
                ObserverEvent::LlmRequest {
                    channel,
                    agent_alias,
                    turn_id,
                    ..
                } => ("LlmRequest", channel, agent_alias, turn_id),
                ObserverEvent::LlmResponse {
                    channel,
                    agent_alias,
                    turn_id,
                    ..
                } => ("LlmResponse", channel, agent_alias, turn_id),
                ObserverEvent::ToolCallStart {
                    channel,
                    agent_alias,
                    turn_id,
                    ..
                } => ("ToolCallStart", channel, agent_alias, turn_id),
                ObserverEvent::ToolCall {
                    channel,
                    agent_alias,
                    turn_id,
                    ..
                } => ("ToolCall", channel, agent_alias, turn_id),
                ObserverEvent::MemoryRecall {
                    channel,
                    agent_alias,
                    turn_id,
                    ..
                } => ("MemoryRecall", channel, agent_alias, turn_id),
                ObserverEvent::MemoryStore {
                    channel,
                    agent_alias,
                    turn_id,
                    ..
                } => ("MemoryStore", channel, agent_alias, turn_id),
                ObserverEvent::RagRetrieve {
                    channel,
                    agent_alias,
                    turn_id,
                    ..
                } => ("RagRetrieve", channel, agent_alias, turn_id),
                _ => continue,
            };
            assert!(
                channel.is_some(),
                "{variant} observer event must carry channel, got None: {event:?}"
            );
            assert!(
                agent_alias.is_some(),
                "{variant} observer event must carry agent_alias, got None: {event:?}"
            );
            assert!(
                turn_id.is_some(),
                "{variant} observer event must carry turn_id, got None: {event:?}"
            );
            turn_ids.push(turn_id.clone().expect("checked Some above"));
        }

        assert!(!turn_ids.is_empty(), "expected turn events with turn_id");
        let first = &turn_ids[0];
        assert!(
            turn_ids.iter().all(|id| id == first),
            "all turn_ids should be consistent"
        );

        if let Some(alias) = expected_alias {
            for e in events {
                let agent_alias = match e {
                    ObserverEvent::AgentStart { agent_alias, .. }
                    | ObserverEvent::AgentEnd { agent_alias, .. }
                    | ObserverEvent::LlmRequest { agent_alias, .. }
                    | ObserverEvent::LlmResponse { agent_alias, .. }
                    | ObserverEvent::ToolCallStart { agent_alias, .. }
                    | ObserverEvent::ToolCall { agent_alias, .. }
                    | ObserverEvent::MemoryRecall { agent_alias, .. }
                    | ObserverEvent::MemoryStore { agent_alias, .. }
                    | ObserverEvent::RagRetrieve { agent_alias, .. } => agent_alias,
                    _ => continue,
                };
                assert_eq!(
                    agent_alias.as_deref(),
                    Some(alias),
                    "agent_alias should be consistent"
                );
            }
        }

        if let Some(channel) = expected_channel {
            for e in events {
                let ch = match e {
                    ObserverEvent::AgentStart { channel: ch, .. }
                    | ObserverEvent::LlmRequest { channel: ch, .. }
                    | ObserverEvent::LlmResponse { channel: ch, .. }
                    | ObserverEvent::ToolCallStart { channel: ch, .. }
                    | ObserverEvent::ToolCall { channel: ch, .. }
                    | ObserverEvent::AgentEnd { channel: ch, .. }
                    | ObserverEvent::MemoryRecall { channel: ch, .. }
                    | ObserverEvent::MemoryStore { channel: ch, .. }
                    | ObserverEvent::RagRetrieve { channel: ch, .. } => ch,
                    _ => continue,
                };
                assert_eq!(ch.as_deref(), Some(channel), "channel should be consistent");
            }
        }
    }

    fn assert_single_agent_lifecycle(events: &[ObserverEvent]) -> (usize, usize) {
        let starts: Vec<_> = events
            .iter()
            .enumerate()
            .filter(|(_, event)| matches!(event, ObserverEvent::AgentStart { .. }))
            .collect();
        let ends: Vec<_> = events
            .iter()
            .enumerate()
            .filter(|(_, event)| matches!(event, ObserverEvent::AgentEnd { .. }))
            .collect();

        assert_eq!(starts.len(), 1, "expected exactly one AgentStart");
        assert_eq!(ends.len(), 1, "expected exactly one AgentEnd");
        assert!(starts[0].0 < ends[0].0, "AgentEnd must follow AgentStart");
        assert_eq!(
            observer_event_turn_id(starts[0].1),
            observer_event_turn_id(ends[0].1),
            "AgentEnd turn_id must match AgentStart turn_id"
        );

        (starts[0].0, ends[0].0)
    }

    fn agent_end_tokens(
        event: &ObserverEvent,
    ) -> Option<clawcrew_api::observability_traits::TurnTokenUsage> {
        match event {
            ObserverEvent::AgentEnd { tokens_used, .. } => tokens_used.clone(),
            _ => None,
        }
    }

    #[tokio::test]
    async fn turn_cache_hit_emits_agent_end_with_none_tokens() {
        let tmp = tempfile::tempdir().expect("temp response cache dir");
        let cache = Arc::new(
            clawcrew_memory::response_cache::ResponseCache::new(tmp.path(), 60, 100)
                .expect("response cache should initialize"),
        );
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem_a: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );
        let mem_b: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );

        let ws_dir = tmp.path().to_path_buf();
        let mut agent_a = Agent::builder()
            .model_provider(Box::new(MockModelProvider {
                responses: Mutex::new(vec![clawcrew_providers::ChatResponse {
                    text: Some("cached answer".into()),
                    tool_calls: vec![],
                    usage: Some(clawcrew_providers::traits::TokenUsage {
                        input_tokens: Some(10),
                        cached_input_tokens: None,
                        cache_creation_input_tokens: None,
                        output_tokens: Some(5),
                    }),
                    reasoning_content: None,
                }]),
            }))
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![],
            ))
            .memory(mem_a)
            .observer(Arc::from(crate::observability::NoopObserver {}) as Arc<dyn Observer>)
            .response_cache(Some(cache.clone()))
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(ws_dir.clone())
            .model_name("test-model".into())
            .temperature(Some(0.0))
            .prompt_builder(SystemPromptBuilder::default())
            .turn_datetime(fixed_response_cache_turn_datetime)
            .build()
            .expect("agent builder should succeed with valid config");

        assert_eq!(agent_a.turn("seed").await.unwrap(), "cached answer");

        let capturing = Arc::new(CapturingObserver::default());
        let observer: Arc<dyn Observer> = capturing.clone();
        let mut agent_b = Agent::builder()
            .model_provider(Box::new(MockModelProvider {
                responses: Mutex::new(vec![clawcrew_providers::ChatResponse {
                    text: Some("uncached answer".into()),
                    tool_calls: vec![],
                    usage: None,
                    reasoning_content: None,
                }]),
            }))
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![],
            ))
            .memory(mem_b)
            .observer(observer)
            .response_cache(Some(cache))
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(ws_dir)
            .model_name("test-model".into())
            .temperature(Some(0.0))
            .prompt_builder(SystemPromptBuilder::default())
            .turn_datetime(fixed_response_cache_turn_datetime)
            .build()
            .expect("agent builder should succeed with valid config");

        assert_eq!(agent_b.turn("seed").await.unwrap(), "cached answer");

        let events = capturing.events.lock();
        let (_, end_idx) = assert_single_agent_lifecycle(&events);
        assert!(agent_end_tokens(&events[end_idx]).is_none());
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, ObserverEvent::LlmRequest { .. })),
            "cache hit should not call the LLM"
        );
    }

    #[tokio::test]
    async fn turn_streamed_cancel_during_tool_execution_emits_agent_end_with_tokens() {
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );
        let capturing = Arc::new(CapturingObserver::default());
        let observer: Arc<dyn Observer> = capturing.clone();
        let mut agent = Agent::builder()
            .model_provider(Box::new(MockModelProvider {
                responses: Mutex::new(vec![clawcrew_providers::ChatResponse {
                    text: Some("I will echo.".into()),
                    tool_calls: vec![clawcrew_providers::ToolCall {
                        id: "tc1".into(),
                        name: "echo".into(),
                        arguments: "{}".into(),
                        extra_content: None,
                    }],
                    usage: Some(clawcrew_providers::traits::TokenUsage {
                        input_tokens: Some(10),
                        cached_input_tokens: None,
                        cache_creation_input_tokens: None,
                        output_tokens: Some(5),
                    }),
                    reasoning_content: None,
                }]),
            }))
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(SlowTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .build()
            .expect("agent builder should succeed with valid config");

        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(64);
        let cancel_token = tokio_util::sync::CancellationToken::new();
        let cancel_for_task = cancel_token.clone();
        let handle = clawcrew_spawn::spawn!(async move {
            agent
                .turn_streamed_with_steering_state(
                    "use echo",
                    event_tx,
                    Some(cancel_for_task),
                    None,
                )
                .await
        });

        while let Some(event) = event_rx.recv().await {
            if matches!(event, TurnEvent::Usage { .. }) {
                cancel_token.cancel();
                break;
            }
        }

        handle
            .await
            .expect("turn task should finish")
            .expect_err("turn should be cancelled before tool execution completes");

        let events = capturing.events.lock();
        let (_, end_idx) = assert_single_agent_lifecycle(&events);
        let tokens = agent_end_tokens(&events[end_idx]).expect("AgentEnd should include tokens");
        assert_eq!(tokens.input_tokens, 10);
        assert_eq!(tokens.output_tokens, 5);
        let llm_response_idx = events
            .iter()
            .position(|event| matches!(event, ObserverEvent::LlmResponse { success: true, .. }))
            .expect("successful LlmResponse should be recorded");
        assert!(
            llm_response_idx < end_idx,
            "AgentEnd must follow LlmResponse"
        );
    }

    #[tokio::test]
    async fn turn_reuses_outer_cost_tracking_context() {
        use crate::agent::cost::{
            TOOL_LOOP_COST_TRACKING_CONTEXT, TOOL_LOOP_TURN_USAGE, ToolLoopCostTrackingContext,
            TurnUsage,
        };
        use crate::cost::CostTracker;
        use std::collections::HashMap;

        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );
        let workspace = tempfile::TempDir::new().expect("temp dir");
        let tracker = Arc::new(
            CostTracker::new(
                clawcrew_config::schema::CostConfig {
                    enabled: true,
                    track_per_agent: true,
                    ..clawcrew_config::schema::CostConfig::default()
                },
                workspace.path(),
            )
            .expect("cost tracker should initialize"),
        );
        let pricing = Arc::new(HashMap::from([(
            "mock-provider".to_string(),
            HashMap::from([
                ("test-model.input".to_string(), 3.0),
                ("test-model.output".to_string(), 15.0),
            ]),
        )]));
        let cost_context = ToolLoopCostTrackingContext::new(Arc::clone(&tracker), pricing)
            .with_agent_alias("agent-turn");
        let turn_usage = Arc::new(parking_lot::Mutex::new(TurnUsage::default()));

        let mut agent = Agent::builder()
            .model_provider(Box::new(MockModelProvider {
                responses: Mutex::new(vec![clawcrew_providers::ChatResponse {
                    text: Some("turn cost".into()),
                    tool_calls: vec![],
                    usage: Some(clawcrew_providers::traits::TokenUsage {
                        input_tokens: Some(1_000),
                        cached_input_tokens: None,
                        cache_creation_input_tokens: None,
                        output_tokens: Some(200),
                    }),
                    reasoning_content: None,
                }]),
            }))
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(Arc::from(crate::observability::NoopObserver {}) as Arc<dyn Observer>)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .model_name("test-model".into())
            .model_provider_name("mock-provider".into())
            .agent_alias("agent-turn".into())
            .build()
            .expect("agent builder should succeed with valid config");

        let response = TOOL_LOOP_TURN_USAGE
            .scope(
                Some(Arc::clone(&turn_usage)),
                TOOL_LOOP_COST_TRACKING_CONTEXT.scope(Some(cost_context), agent.turn("hello")),
            )
            .await
            .expect("turn should succeed");

        assert_eq!(response, "turn cost");

        let recorded = *turn_usage.lock();
        assert_eq!(recorded.input_tokens, 1_000);
        assert_eq!(recorded.output_tokens, 200);
        assert!(
            recorded.cost_usd > 0.0,
            "outer turn usage should accumulate non-zero cost from scoped pricing"
        );

        let summary = tracker.get_summary().expect("cost summary");
        assert_eq!(summary.request_count, 1);
        assert_eq!(summary.total_tokens, 1_200);
        assert!(
            summary.session_cost_usd > 0.0,
            "scoped tracker should persist turn usage"
        );
        let agent_summary = tracker
            .get_summary_for_agent("agent-turn")
            .expect("agent-scoped summary");
        assert_eq!(agent_summary.request_count, 1);
        assert!(
            agent_summary.session_cost_usd > 0.0,
            "agent alias should flow through persisted turn usage"
        );
    }

    #[tokio::test]
    async fn turn_streamed_reuses_outer_cost_tracking_context() {
        use crate::agent::cost::{
            TOOL_LOOP_COST_TRACKING_CONTEXT, TOOL_LOOP_TURN_USAGE, ToolLoopCostTrackingContext,
            TurnUsage,
        };
        use crate::cost::CostTracker;
        use std::collections::HashMap;

        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );
        let workspace = tempfile::TempDir::new().expect("temp dir");
        let tracker = Arc::new(
            CostTracker::new(
                clawcrew_config::schema::CostConfig {
                    enabled: true,
                    track_per_agent: true,
                    ..clawcrew_config::schema::CostConfig::default()
                },
                workspace.path(),
            )
            .expect("cost tracker should initialize"),
        );
        let pricing = Arc::new(HashMap::from([(
            "mock-provider".to_string(),
            HashMap::from([
                ("test-model.input".to_string(), 3.0),
                ("test-model.output".to_string(), 15.0),
            ]),
        )]));
        let cost_context = ToolLoopCostTrackingContext::new(Arc::clone(&tracker), pricing)
            .with_agent_alias("streamed-agent");
        let turn_usage = Arc::new(parking_lot::Mutex::new(TurnUsage::default()));

        let mut agent = Agent::builder()
            .model_provider(Box::new(MockModelProvider {
                responses: Mutex::new(vec![clawcrew_providers::ChatResponse {
                    text: Some("streamed cost".into()),
                    tool_calls: vec![],
                    usage: Some(clawcrew_providers::traits::TokenUsage {
                        input_tokens: Some(1_000),
                        cached_input_tokens: None,
                        cache_creation_input_tokens: None,
                        output_tokens: Some(200),
                    }),
                    reasoning_content: None,
                }]),
            }))
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(Arc::from(crate::observability::NoopObserver {}) as Arc<dyn Observer>)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .model_name("test-model".into())
            .model_provider_name("mock-provider".into())
            .agent_alias("streamed-agent".into())
            .build()
            .expect("agent builder should succeed with valid config");

        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(64);
        let outcome = TOOL_LOOP_TURN_USAGE
            .scope(
                Some(Arc::clone(&turn_usage)),
                TOOL_LOOP_COST_TRACKING_CONTEXT.scope(
                    Some(cost_context),
                    agent.turn_streamed_with_steering_state("hello", event_tx, None, None),
                ),
            )
            .await
            .expect("streamed turn should succeed");

        assert_eq!(outcome.response, "streamed cost");
        while event_rx.recv().await.is_some() {}

        let recorded = *turn_usage.lock();
        assert_eq!(recorded.input_tokens, 1_000);
        assert_eq!(recorded.output_tokens, 200);
        assert!(
            recorded.cost_usd > 0.0,
            "outer turn usage should accumulate non-zero cost from scoped pricing"
        );

        let summary = tracker.get_summary().expect("cost summary");
        assert_eq!(summary.request_count, 1);
        assert_eq!(summary.total_tokens, 1_200);
        assert!(
            summary.session_cost_usd > 0.0,
            "scoped tracker should persist streamed-turn usage"
        );
        let agent_summary = tracker
            .get_summary_for_agent("streamed-agent")
            .expect("agent-scoped summary");
        assert_eq!(agent_summary.request_count, 1);
        assert!(
            agent_summary.session_cost_usd > 0.0,
            "agent alias should flow through persisted streamed-turn usage"
        );
    }

    #[tokio::test]
    async fn turn_llm_error_emits_agent_end() {
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );
        let capturing = Arc::new(CapturingObserver::default());
        let observer: Arc<dyn Observer> = capturing.clone();
        let mut agent = Agent::builder()
            .model_provider(Box::new(FailingModelProvider))
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .model_name("test-model".into())
            .temperature(Some(0.0))
            .build()
            .expect("agent builder should succeed with valid config");

        let result = agent.turn("hello").await;
        assert!(
            result.is_err(),
            "turn should fail when provider is unavailable"
        );

        let events = capturing.events.lock();
        let (_, end_idx) = assert_single_agent_lifecycle(&events);
        assert!(
            agent_end_tokens(&events[end_idx]).is_none(),
            "AgentEnd should have tokens_used: None on LLM error"
        );
    }

    #[tokio::test]
    async fn turn_events_share_consistent_turn_id() {
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );

        let model_provider = Box::new(MockModelProvider {
            responses: Mutex::new(vec![clawcrew_providers::ChatResponse {
                text: Some("done".into()),
                tool_calls: vec![],
                usage: None,
                reasoning_content: None,
            }]),
        });
        let capturing = Arc::new(CapturingObserver::default());
        let observer: Arc<dyn Observer> = capturing.clone();
        let mut agent = Agent::builder()
            .model_provider(model_provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .agent_alias("test-agent".into())
            .auto_save(true)
            .build()
            .expect("agent builder should succeed with valid config");

        let _ = agent.turn("test").await.expect("turn should succeed");

        let events = capturing.events.lock();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ObserverEvent::MemoryStore { .. })),
            "auto_save(true) must cause Agent::turn to emit a MemoryStore event \
             so its (channel, agent_alias, turn_id) triple is actually asserted below"
        );
        assert_all_events_share_turn_id(&events, Some("test-agent"), Some("agent"));
    }

    #[tokio::test]
    async fn streamed_turn_events_share_consistent_turn_id() {
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );

        let model_provider = Box::new(MockModelProvider {
            responses: Mutex::new(vec![clawcrew_providers::ChatResponse {
                text: Some("done".into()),
                tool_calls: vec![],
                usage: None,
                reasoning_content: None,
            }]),
        });
        let capturing = Arc::new(CapturingObserver::default());
        let observer: Arc<dyn Observer> = capturing.clone();
        let mut agent = Agent::builder()
            .model_provider(model_provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .agent_alias("test-agent".into())
            .auto_save(true)
            .build()
            .expect("agent builder should succeed with valid config");

        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(64);
        let _ = agent
            .turn_streamed_with_steering_state("test", event_tx, None, None)
            .await
            .expect("streamed turn should succeed");
        while event_rx.recv().await.is_some() {}

        let events = capturing.events.lock();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ObserverEvent::MemoryStore { .. })),
            "auto_save(true) must cause the streamed turn to emit a MemoryStore event"
        );
        assert_all_events_share_turn_id(&events, Some("test-agent"), Some("agent"));
    }

    // B4: a streamed turn whose final (and only) provider call returns
    // `usage: None` must still publish a terminal `TurnEvent::Usage` carrying the
    // served route's budget/window, so the client meter reflects the final route
    // instead of staying blank or stuck on an earlier snapshot.
    #[tokio::test]
    async fn streamed_turn_publishes_terminal_context_snapshot_without_usage() {
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );
        // Single response with no token usage.
        let model_provider = Box::new(MockModelProvider {
            responses: Mutex::new(vec![clawcrew_providers::ChatResponse {
                text: Some("done".into()),
                tool_calls: vec![],
                usage: None,
                reasoning_content: None,
            }]),
        });
        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = Agent::builder()
            .model_provider(model_provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .agent_alias("test-agent".into())
            .build()
            .expect("agent builder should succeed with valid config");

        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(64);
        let _ = agent
            .turn_streamed_with_steering_state("hello", event_tx, None, None)
            .await
            .expect("streamed turn should succeed");

        let mut terminal_usage = None;
        while let Ok(ev) = event_rx.try_recv() {
            if let TurnEvent::Usage {
                input_tokens,
                output_tokens,
                context_token_budget,
                model_context_window,
                ..
            } = ev
            {
                // The terminal snapshot carries route limits but no token counts.
                if input_tokens.is_none() && output_tokens.is_none() {
                    terminal_usage = Some((context_token_budget, model_context_window));
                }
            }
        }
        let (budget, window) = terminal_usage
            .expect("a terminal Usage snapshot must fire even without provider usage");
        // Default builder has no configured capacity -> 32k compatibility
        // fallback budget, and the window is omitted (not configured truth).
        assert_eq!(
            budget,
            Some(clawcrew_config::schema::LEGACY_DEFAULT_CONTEXT_BUDGET as u64)
        );
        assert_eq!(
            window, None,
            "compatibility-fallback capacity is omitted from the wire snapshot"
        );
    }

    // A provider that returns `Some(usage)` with `None` token counts (kilocli,
    // gemini-cli) still emits a per-call frame carrying the route limits. The
    // terminal snapshot must NOT also fire, or the client would get two frames
    // for one call. The gate keys on whether a per-call frame was emitted, not
    // on whether token counts were present.
    #[tokio::test]
    async fn no_terminal_snapshot_when_call_emits_usage_frame_without_token_counts() {
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );
        // Single response carrying usage WITHOUT token counts (kilocli-style).
        let model_provider = Box::new(MockModelProvider {
            responses: Mutex::new(vec![clawcrew_providers::ChatResponse {
                text: Some("done".into()),
                tool_calls: vec![],
                usage: Some(clawcrew_providers::traits::TokenUsage::default()),
                reasoning_content: None,
            }]),
        });
        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = Agent::builder()
            .model_provider(model_provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .agent_alias("test-agent".into())
            .build()
            .expect("agent builder should succeed with valid config");

        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(64);
        let _ = agent
            .turn_streamed_with_steering_state("hello", event_tx, None, None)
            .await
            .expect("streamed turn should succeed");

        let usage_frames = std::iter::from_fn(|| event_rx.try_recv().ok())
            .filter(|ev| matches!(ev, TurnEvent::Usage { .. }))
            .count();
        assert_eq!(
            usage_frames, 1,
            "a usage frame with no token counts must not also trigger a terminal snapshot"
        );
    }

    // A multi-iteration turn whose earlier call reports usage but whose final
    // call returns no usage must still publish a terminal snapshot for the final
    // served route. The gate keys on the FINAL call's usage, not the turn's
    // cumulative usage, so the earlier usage-bearing frame does not suppress the
    // authoritative final-route window.
    #[tokio::test]
    async fn terminal_snapshot_fires_when_only_final_call_lacks_usage() {
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );
        // Call 1: a tool request WITH usage (drives iteration 2 and emits a
        // usage-bearing per-call frame). Call 2: the final response WITHOUT usage.
        let model_provider = Box::new(MockModelProvider {
            responses: Mutex::new(vec![
                clawcrew_providers::ChatResponse {
                    text: Some("calling tool".into()),
                    tool_calls: vec![clawcrew_providers::ToolCall {
                        id: "tc1".into(),
                        name: "echo".into(),
                        arguments: "{}".into(),
                        extra_content: None,
                    }],
                    usage: Some(clawcrew_providers::traits::TokenUsage {
                        input_tokens: Some(100),
                        cached_input_tokens: None,
                        cache_creation_input_tokens: None,
                        output_tokens: Some(20),
                    }),
                    reasoning_content: None,
                },
                clawcrew_providers::ChatResponse {
                    text: Some("final answer".into()),
                    tool_calls: vec![],
                    usage: None,
                    reasoning_content: None,
                },
            ]),
        });
        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = Agent::builder()
            .model_provider(model_provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .agent_alias("test-agent".into())
            .build()
            .expect("agent builder should succeed with valid config");
        // A resolver so the served route carries a configured window distinct
        // from the 32k compatibility fallback; the terminal frame must carry it.
        agent.context_limits_resolver = Some(Arc::new(|_provider, _model| {
            clawcrew_config::schema::ResolvedContextLimits {
                model_context_window: 200_000,
                context_token_budget: 180_000,
                model_context_window_source:
                    clawcrew_config::schema::ModelContextWindowSource::Configured,
            }
        }));

        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(64);
        let _ = agent
            .turn_streamed_with_steering_state("hello", event_tx, None, None)
            .await
            .expect("streamed turn should succeed");

        let mut usage_frames = 0usize;
        let mut terminal_snapshot = None;
        while let Ok(ev) = event_rx.try_recv() {
            if let TurnEvent::Usage {
                input_tokens,
                output_tokens,
                context_token_budget,
                model_context_window,
                ..
            } = ev
            {
                usage_frames += 1;
                // The terminal snapshot carries route limits but no token counts.
                if input_tokens.is_none() && output_tokens.is_none() {
                    terminal_snapshot = Some((context_token_budget, model_context_window));
                }
            }
        }
        // One usage-bearing frame from call 1, plus the terminal snapshot.
        assert_eq!(
            usage_frames, 2,
            "expected the call-1 usage frame and a terminal snapshot for the usage-less final call"
        );
        let (budget, window) = terminal_snapshot.expect(
            "the final usage-less call must publish a terminal snapshot despite the earlier \
             usage-bearing call",
        );
        assert_eq!(budget, Some(180_000));
        assert_eq!(
            window,
            Some(200_000),
            "the terminal snapshot must carry the final served route's configured window"
        );
    }

    // End-to-end route-switch boundary: a text call WITH usage, a tool that
    // injects an image forcing a switch to a DIFFERENT vision route whose final
    // call reports NO usage, driven through the real streamed producer path
    // (`turn_streamed_with_steering_state` -> `run_tool_call_loop` ->
    // `resolve_vision_provider`). Proves the terminal `TurnEvent::Usage` replaces
    // the 200k text route with the final 8k vision route's provider/model and
    // budget/window, rather than retaining the earlier usage-bearing route.
    #[tokio::test]
    async fn terminal_snapshot_follows_text_to_vision_switch_through_producer() {
        use axum::{Json, Router, routing::post};
        use std::sync::atomic::{AtomicUsize, Ordering};

        // Mock vision endpoint: a small plain-text answer with NO usage.
        async fn vision_reply(Json(_body): Json<serde_json::Value>) -> Json<serde_json::Value> {
            Json(serde_json::json!({
                "choices": [{"message": {"content": "vision saw the image"}}]
            }))
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind vision provider");
        let addr = listener.local_addr().expect("vision provider address");
        let app = Router::new().route("/v1/chat/completions", post(vision_reply));
        let _server = clawcrew_spawn::spawn!(async move {
            axum::serve(listener, app).await.expect("vision serves");
        });

        let temp = tempfile::tempdir().expect("tempdir");
        let image_path = temp.path().join("shot.png");
        std::fs::write(
            &image_path,
            [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'],
        )
        .expect("write png");

        // 200k text route (with usage) and an 8k vision route (no usage) served
        // by the mock endpoint.
        let config: clawcrew_config::schema::Config = toml::from_str(&format!(
            r#"
schema_version = 3
[providers.models.custom.text]
model = "text-model"
context_window = 200000
[providers.models.custom.vision]
uri = "http://{addr}/v1"
model = "vision-model"
context_window = 8000
[agents.coder]
enabled = true
model_provider = "custom.text"
[multimodal]
vision_model_provider = "custom.vision"
"#
        ))
        .expect("config parses");

        // Text primary: iteration 0 emits a native tool call WITH usage; the tool
        // injects an image, so iteration 1 routes to the vision endpoint (which
        // returns no usage) and this provider is not called again.
        struct TextPrimary {
            calls: Arc<AtomicUsize>,
        }
        #[async_trait]
        impl ModelProvider for TextPrimary {
            fn capabilities(&self) -> clawcrew_api::model_provider::ProviderCapabilities {
                clawcrew_api::model_provider::ProviderCapabilities {
                    native_tool_calling: true,
                    ..Default::default()
                }
            }
            async fn chat_with_system(
                &self,
                _s: Option<&str>,
                _m: &str,
                _model: &str,
                _t: Option<f64>,
            ) -> Result<String> {
                Ok(String::new())
            }
            async fn chat(
                &self,
                _request: ChatRequest<'_>,
                _model: &str,
                _t: Option<f64>,
            ) -> Result<clawcrew_providers::ChatResponse> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Ok(clawcrew_providers::ChatResponse {
                    text: None,
                    tool_calls: vec![clawcrew_providers::ToolCall {
                        id: "c1".into(),
                        name: "attach_image".into(),
                        arguments: "{}".into(),
                        extra_content: None,
                    }],
                    usage: Some(clawcrew_providers::traits::TokenUsage {
                        input_tokens: Some(100),
                        cached_input_tokens: None,
                        cache_creation_input_tokens: None,
                        output_tokens: Some(20),
                    }),
                    reasoning_content: None,
                })
            }
        }
        impl clawcrew_api::attribution::Attributable for TextPrimary {
            fn role(&self) -> clawcrew_api::attribution::Role {
                clawcrew_api::attribution::Role::Provider(
                    clawcrew_api::attribution::ProviderKind::Model(
                        clawcrew_api::attribution::ModelProviderKind::Custom,
                    ),
                )
            }
            fn alias(&self) -> &str {
                "text-primary"
            }
        }

        struct AttachImage {
            path: String,
        }
        #[async_trait]
        impl Tool for AttachImage {
            fn name(&self) -> &str {
                "attach_image"
            }
            fn description(&self) -> &str {
                "attaches an image"
            }
            fn parameters_schema(&self) -> serde_json::Value {
                serde_json::json!({"type": "object", "properties": {}})
            }
            async fn execute(&self, _args: serde_json::Value) -> Result<crate::tools::ToolResult> {
                Ok(crate::tools::ToolResult {
                    success: true,
                    output: format!("here it is [IMAGE:{}]", self.path).into(),
                    error: None,
                })
            }
        }
        impl clawcrew_api::attribution::Attributable for AttachImage {
            fn role(&self) -> clawcrew_api::attribution::Role {
                clawcrew_api::attribution::Role::Tool(clawcrew_api::attribution::ToolKind::Plugin)
            }
            fn alias(&self) -> &str {
                "attach_image"
            }
        }

        let calls = Arc::new(AtomicUsize::new(0));
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, temp.path(), None)
                .expect("memory creation should succeed"),
        );
        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = Agent::builder()
            .model_provider(Box::new(TextPrimary {
                calls: Arc::clone(&calls),
            }))
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(AttachImage {
                    path: image_path.display().to_string(),
                })],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(temp.path().to_path_buf())
            .agent_alias("coder".into())
            // The first call must resolve against the configured text route, not
            // the builder's `<unconfigured>` default, or its per-call usage frame
            // reports no window (the source stays Fallback, not Configured).
            .model_provider_name("custom.text".into())
            .model_name("text-model".into())
            .multimodal_config(config.multimodal.clone())
            .provider_switch_config(ProviderSwitchConfig {
                config: Some(Arc::new(config.clone())),
                live: None,
            })
            // Auto-approve so the image-injecting tool runs and the vision route
            // engages.
            .approval_manager(Some(Arc::new(
                crate::approval::ApprovalManager::for_non_interactive(
                    &clawcrew_config::schema::RiskProfileConfig {
                        auto_approve: vec!["*".to_string()],
                        ..Default::default()
                    },
                ),
            )))
            .build()
            .expect("agent builder should succeed with valid config");
        // Route-aware limits: the text route resolves to 200k, the vision route
        // to 8k. The loop re-resolves per call, so the served (final) route is
        // the 8k vision one.
        let limit_config = Arc::new(config);
        agent.context_limits_resolver = Some(Arc::new(move |provider_ref, model| {
            limit_config.resolved_context_limits_for_route("coder", provider_ref, model)
        }));

        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(64);
        let outcome = agent
            .turn_streamed_with_steering_state("please attach the image", event_tx, None, None)
            .await
            .expect("streamed turn should succeed");

        assert!(
            calls.load(Ordering::SeqCst) >= 1,
            "the text primary must have served the first call"
        );

        // Collect usage frames IN ORDER so the transition itself is asserted,
        // not just the final state: the first frame must carry the 200k text
        // route's window with real usage, and only the later usage-less frame
        // may carry the 8k vision window.
        let mut usage_frames = Vec::new();
        while let Ok(ev) = event_rx.try_recv() {
            if let TurnEvent::Usage {
                input_tokens,
                output_tokens,
                context_token_budget,
                model_context_window,
                ..
            } = ev
            {
                usage_frames.push((
                    input_tokens,
                    output_tokens,
                    context_token_budget,
                    model_context_window,
                ));
            }
        }
        assert!(
            usage_frames.len() >= 2,
            "expected a text-route usage frame followed by the vision terminal snapshot, got {:?}",
            usage_frames
        );

        let (first_input, _, first_budget, first_window) = usage_frames[0];
        assert!(
            first_input.is_some(),
            "the first frame is the text call that DID report usage, got {:?}",
            usage_frames[0]
        );
        assert_eq!(
            first_window,
            Some(200_000),
            "the first usage frame must carry the 200k text route it was served on"
        );
        // No runtime profile in this config, so the budget is the legacy 32k
        // default: under the text route's 200k capacity it survives unclamped,
        // which is what distinguishes it from the vision frame below.
        assert_eq!(
            first_budget,
            Some(32_000),
            "the legacy default budget fits under the text route's 200k capacity unclamped"
        );

        let terminal_index = usage_frames
            .iter()
            .position(|(input, output, _, _)| input.is_none() && output.is_none())
            .expect(
                "the usage-less vision call must publish a terminal snapshot even though the \
                 earlier text call reported usage",
            );
        assert!(
            terminal_index > 0,
            "the terminal snapshot must come AFTER the text route's usage frame, not before it"
        );
        let (_, _, budget, window) = usage_frames[terminal_index];
        // The terminal snapshot must reflect the FINAL served (vision) route:
        // an 8k window, not the 200k text route it started on.
        assert_eq!(
            window,
            Some(8_000),
            "terminal snapshot must carry the final vision route's 8k window, not the text route"
        );
        assert_eq!(
            budget,
            Some(8_000),
            "the 8k vision capacity clamps the effective budget the terminal snapshot reports"
        );

        // Route IDENTITY on the outcome, not just the numbers: a window that
        // happened to match would otherwise pass without the vision route
        // actually having served the final call.
        assert_eq!(
            outcome.provider_name, "custom.vision",
            "the outcome's final provider must be the vision route the switch landed on"
        );
        assert_eq!(
            outcome.model, "vision-model",
            "the outcome's final model must be the vision route's model"
        );
        let final_limits = outcome
            .final_context_limits
            .expect("a served call must publish final limits on the outcome");
        assert_eq!(
            final_limits.model_context_window, 8_000,
            "the outcome's limits must agree with the terminal snapshot's window"
        );
        assert_eq!(
            final_limits.context_token_budget, 8_000,
            "the outcome's limits must agree with the terminal snapshot's budget"
        );
    }

    /// A reliable fallback that serves a DIFFERENT alias must report ITS
    /// capacity, not the requested route's.
    ///
    /// Regression for the mixed-generation split inside one `ServedRoute`:
    /// limits were resolved pre-dispatch from the requested route, while
    /// `provider_name`/`model` were updated post-dispatch from `accepted_route`.
    /// A 200k primary failing over to an 8k backup therefore reported the
    /// backup's identity against the primary's 200k window, and the loop's own
    /// trim/recovery arithmetic kept using the wide budget for a model that
    /// only holds 8k.
    #[tokio::test]
    async fn served_limits_follow_the_accepted_reliable_fallback_route() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use clawcrew_providers::reliable::ReliableModelProvider;

        // 200k primary that always fails, 8k backup that answers. Both are
        // configured aliases, so each resolves to a real `context_window`.
        //
        // The backup alias deliberately pins no `model`: reliable serves a
        // fallback entry with the REQUESTED model name, and
        // `resolved_model_context_window_for_route` only honors an alias's
        // window when the alias either pins no model or pins the selected one.
        // Leaving it unpinned is what a fallback alias looks like in practice,
        // and it keeps this test asserting the thing under repair — capacity
        // keyed on the ACCEPTED alias — rather than model-pin mechanics.
        let config: clawcrew_config::schema::Config = toml::from_str(
            r#"
schema_version = 3
[providers.models.custom.primary]
model = "primary-model"
context_window = 200000
[providers.models.custom.backup]
context_window = 8000
[agents.coder]
enabled = true
model_provider = "custom.primary"
"#,
        )
        .expect("config parses");

        struct AlwaysFails;
        #[async_trait]
        impl ModelProvider for AlwaysFails {
            async fn chat_with_system(
                &self,
                _s: Option<&str>,
                _m: &str,
                _model: &str,
                _t: Option<f64>,
            ) -> Result<String> {
                anyhow::bail!("primary is down")
            }
            async fn chat(
                &self,
                _request: ChatRequest<'_>,
                _model: &str,
                _t: Option<f64>,
            ) -> Result<clawcrew_providers::ChatResponse> {
                anyhow::bail!("primary is down")
            }
            // Deliberately does NOT support streaming so that reliable's
            // `stream_chat` skips entry 0 and routes to the backup entry.
            // That makes `entry_index != 0` true for the backup, which is
            // what marks it as a fallback and produces an `accepted_route`
            // carrying "custom.backup" into the call-provider accounting.
            fn supports_streaming(&self) -> bool {
                false
            }
        }
        impl clawcrew_api::attribution::Attributable for AlwaysFails {
            fn role(&self) -> clawcrew_api::attribution::Role {
                clawcrew_api::attribution::Role::Provider(
                    clawcrew_api::attribution::ProviderKind::Model(
                        clawcrew_api::attribution::ModelProviderKind::Custom,
                    ),
                )
            }
            fn alias(&self) -> &str {
                "primary"
            }
        }

        // Answers WITHOUT usage via streaming, so the terminal snapshot is the
        // only frame carrying limits — exactly the path the stale pair corrupted.
        // Streaming is required so that `scope_provider_fallback` establishes the
        // reliable accounting task-local; the non-streaming path does not scope it
        // and `accepted_route` would be None, which is a separate, pre-existing gap.
        struct BackupAnswers {
            calls: Arc<AtomicUsize>,
        }
        #[async_trait]
        impl ModelProvider for BackupAnswers {
            async fn chat_with_system(
                &self,
                _s: Option<&str>,
                _m: &str,
                _model: &str,
                _t: Option<f64>,
            ) -> Result<String> {
                Ok(String::new())
            }
            async fn chat(
                &self,
                _request: ChatRequest<'_>,
                _model: &str,
                _t: Option<f64>,
            ) -> Result<clawcrew_providers::ChatResponse> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Ok(clawcrew_providers::ChatResponse {
                    text: Some("backup answered".to_string()),
                    tool_calls: Vec::new(),
                    usage: None,
                    reasoning_content: None,
                })
            }
            fn supports_streaming(&self) -> bool {
                true
            }
            fn stream_chat(
                &self,
                _request: clawcrew_providers::ChatRequest<'_>,
                _model: &str,
                _t: Option<f64>,
                _options: clawcrew_providers::traits::StreamOptions,
            ) -> futures_util::stream::BoxStream<
                'static,
                clawcrew_providers::traits::StreamResult<clawcrew_api::model_provider::StreamEvent>,
            > {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Box::pin(futures_util::stream::iter(vec![
                    Ok(clawcrew_api::model_provider::StreamEvent::TextDelta(
                        clawcrew_api::model_provider::StreamChunk {
                            delta: "backup answered".to_string(),
                            reasoning: None,
                            is_final: false,
                            token_count: 0,
                        },
                    )),
                    Ok(clawcrew_api::model_provider::StreamEvent::Final),
                ]))
            }
        }
        impl clawcrew_api::attribution::Attributable for BackupAnswers {
            fn role(&self) -> clawcrew_api::attribution::Role {
                clawcrew_api::attribution::Role::Provider(
                    clawcrew_api::attribution::ProviderKind::Model(
                        clawcrew_api::attribution::ModelProviderKind::Custom,
                    ),
                )
            }
            fn alias(&self) -> &str {
                "backup"
            }
        }

        let backup_calls = Arc::new(AtomicUsize::new(0));
        // `new` keys each entry's candidate identity on its display name, so the
        // accepted route reports `custom.backup` — the alias the config resolver
        // needs in order to find the 8k window.
        let reliable = ReliableModelProvider::new(
            "custom.primary",
            vec![
                (
                    "custom.primary".to_string(),
                    Box::new(AlwaysFails) as Box<dyn ModelProvider>,
                ),
                (
                    "custom.backup".to_string(),
                    Box::new(BackupAnswers {
                        calls: Arc::clone(&backup_calls),
                    }) as Box<dyn ModelProvider>,
                ),
            ],
            0,
            1,
        );

        let temp = tempfile::tempdir().expect("tempdir");
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, temp.path(), None)
                .expect("memory creation should succeed"),
        );
        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = Agent::builder()
            .model_provider(Box::new(reliable))
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                Vec::new(),
            ))
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .memory(mem)
            .observer(observer)
            .workspace_dir(temp.path().to_path_buf())
            .agent_alias("coder".into())
            .model_provider_name("custom.primary".into())
            .model_name("primary-model".into())
            .provider_switch_config(ProviderSwitchConfig {
                config: Some(Arc::new(config.clone())),
                live: None,
            })
            .build()
            .expect("agent builder should succeed with valid config");
        let limit_config = Arc::new(config);
        agent.context_limits_resolver = Some(Arc::new(move |provider_ref, model| {
            limit_config.resolved_context_limits_for_route("coder", provider_ref, model)
        }));

        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(64);
        let outcome = agent
            .turn_streamed_with_steering_state("hello", event_tx, None, None)
            .await
            .expect("the backup must carry the turn");

        assert_eq!(
            backup_calls.load(Ordering::SeqCst),
            1,
            "the backup entry must have served the call after the primary failed"
        );

        // Identity: the accepted fallback, not the requested primary.
        assert_eq!(
            outcome.provider_name, "custom.backup",
            "the outcome must attribute the route that actually answered"
        );

        // The limits must be re-resolved for THAT identity. Before the fix these
        // stayed the primary's 200k pair while the identity above already said
        // `custom.backup` — one `ServedRoute` holding two generations.
        let final_limits = outcome
            .final_context_limits
            .expect("a served call must publish final limits");
        assert_eq!(
            final_limits.model_context_window, 8_000,
            "capacity must come from the accepted fallback's alias, not the requested primary's"
        );
        assert_eq!(
            final_limits.context_token_budget, 8_000,
            "the fallback's 8k capacity must clamp the effective trim budget"
        );

        // The wire frame consumers actually read must agree with the outcome.
        let mut terminal = None;
        while let Ok(ev) = event_rx.try_recv() {
            if let TurnEvent::Usage {
                context_token_budget,
                model_context_window,
                ..
            } = ev
            {
                terminal = Some((context_token_budget, model_context_window));
            }
        }
        assert_eq!(
            terminal,
            Some((Some(8_000), Some(8_000))),
            "the usage-less fallback must publish a terminal snapshot carrying ITS window"
        );
    }

    /// A same-alias `fallback_models` failover must re-key the context limits
    /// to the model that actually served.
    ///
    /// `push_pinned_entries` builds the primary and every `fallback_models`
    /// entry under ONE provider alias (one shared `cooldown_key`), differing
    /// only in the pinned model. `AcceptedRoute.provider_ref` is that shared
    /// key, so an alias-only comparison reports "route unchanged" even though a
    /// different model answered. The limits, the terminal snapshot, and the
    /// recovery arithmetic would then all keep the primary model's capacity.
    ///
    /// Observable difference: `context_window` describes only the model
    /// configured on the alias, so the fallback model resolves to the explicit
    /// compatibility fallback rather than borrowing the primary's 200k. Before
    /// the fix the terminal frame reported the primary's 200,000-token window
    /// while naming `small-model`.
    #[tokio::test]
    async fn served_limits_follow_a_same_alias_pinned_model_fallback() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        // One alias. `model` pins the primary and carries the only configured
        // capacity; `fallback_models` adds a second pinned entry under the very
        // same alias.
        let config: clawcrew_config::schema::Config = toml::from_str(
            r#"
schema_version = 3
[providers.models.custom.only]
model = "large-model"
context_window = 200000
fallback_models = ["small-model"]
[agents.coder]
enabled = true
model_provider = "custom.only"
"#,
        )
        .expect("config parses");

        // Fails for the primary pinned model, answers for the fallback one.
        // Streaming, because that is the path that records an accepted route.
        struct PinnedByModel {
            small_calls: Arc<AtomicUsize>,
        }
        #[async_trait]
        impl ModelProvider for PinnedByModel {
            async fn chat_with_system(
                &self,
                _s: Option<&str>,
                _m: &str,
                _model: &str,
                _t: Option<f64>,
            ) -> Result<String> {
                Ok(String::new())
            }
            async fn chat(
                &self,
                _request: ChatRequest<'_>,
                model: &str,
                _t: Option<f64>,
            ) -> Result<clawcrew_providers::ChatResponse> {
                if model == "small-model" {
                    self.small_calls.fetch_add(1, Ordering::SeqCst);
                    return Ok(clawcrew_providers::ChatResponse {
                        text: Some("fallback model answered".to_string()),
                        tool_calls: Vec::new(),
                        usage: None,
                        reasoning_content: None,
                    });
                }
                anyhow::bail!("large-model is down")
            }
            fn supports_streaming(&self) -> bool {
                true
            }
            fn stream_chat(
                &self,
                _request: clawcrew_providers::ChatRequest<'_>,
                model: &str,
                _t: Option<f64>,
                _options: clawcrew_providers::traits::StreamOptions,
            ) -> futures_util::stream::BoxStream<
                'static,
                clawcrew_providers::traits::StreamResult<clawcrew_api::model_provider::StreamEvent>,
            > {
                if model == "small-model" {
                    self.small_calls.fetch_add(1, Ordering::SeqCst);
                    return Box::pin(futures_util::stream::iter(vec![
                        Ok(clawcrew_api::model_provider::StreamEvent::TextDelta(
                            clawcrew_api::model_provider::StreamChunk {
                                delta: "fallback model answered".to_string(),
                                reasoning: None,
                                is_final: false,
                                token_count: 0,
                            },
                        )),
                        Ok(clawcrew_api::model_provider::StreamEvent::Final),
                    ]));
                }
                Box::pin(futures_util::stream::iter(vec![Err(
                    clawcrew_api::model_provider::StreamError::ModelProvider(
                        "large-model is down".to_string(),
                    ),
                )]))
            }
        }
        impl clawcrew_api::attribution::Attributable for PinnedByModel {
            fn role(&self) -> clawcrew_api::attribution::Role {
                clawcrew_api::attribution::Role::Provider(
                    clawcrew_api::attribution::ProviderKind::Model(
                        clawcrew_api::attribution::ModelProviderKind::Custom,
                    ),
                )
            }
            fn alias(&self) -> &str {
                "only"
            }
        }

        let small_calls = Arc::new(AtomicUsize::new(0));
        let inner: Arc<dyn ModelProvider> = Arc::new(PinnedByModel {
            small_calls: Arc::clone(&small_calls),
        });
        // Mirror `push_pinned_entries`: both entries share ONE cooldown key
        // (the alias) and differ only in the pinned model.
        let reliable = clawcrew_providers::reliable::ReliableModelProvider::new_pinned_for_test(
            "custom.only",
            vec![
                ("custom.only", "only", "large-model", Arc::clone(&inner)),
                ("custom.only", "only", "small-model", Arc::clone(&inner)),
            ],
            0,
            1,
        );

        let temp = tempfile::tempdir().expect("tempdir");
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, temp.path(), None)
                .expect("memory creation should succeed"),
        );
        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut agent = Agent::builder()
            .model_provider(Box::new(reliable))
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                Vec::new(),
            ))
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .memory(mem)
            .observer(observer)
            .workspace_dir(temp.path().to_path_buf())
            .agent_alias("coder".into())
            .model_provider_name("custom.only".into())
            .model_name("large-model".into())
            .provider_switch_config(ProviderSwitchConfig {
                config: Some(Arc::new(config.clone())),
                live: None,
            })
            .build()
            .expect("agent builder should succeed with valid config");
        let limit_config = Arc::new(config);
        agent.context_limits_resolver = Some(Arc::new(move |provider_ref, model| {
            limit_config.resolved_context_limits_for_route("coder", provider_ref, model)
        }));

        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(64);
        let outcome = agent
            .turn_streamed_with_steering_state("hello", event_tx, None, None)
            .await
            .expect("the pinned fallback model must carry the turn");

        assert_eq!(
            small_calls.load(Ordering::SeqCst),
            1,
            "the small-model entry must have served the call"
        );

        // The alias is unchanged by construction — that is the whole point.
        assert_eq!(
            outcome.provider_name, "custom.only",
            "harness invariant: both pinned entries share one alias, so an \
             alias-only comparison cannot detect this failover"
        );
        assert_eq!(
            outcome.model, "small-model",
            "the outcome must attribute the pinned model that actually answered"
        );

        // `context_window` describes only `large-model`, so the served
        // `small-model` route resolves to the explicit compatibility fallback.
        // Before the fix this was the primary's 200_000.
        let final_limits = outcome
            .final_context_limits
            .expect("a served call must publish final limits");
        assert_ne!(
            final_limits.model_context_window, 200_000,
            "the primary's capacity must not be reported for a different served model"
        );
        assert_eq!(
            final_limits.model_context_window_source,
            clawcrew_config::schema::ModelContextWindowSource::CompatibilityFallback,
            "an unconfigured served model must be explicit compatibility fallback, \
             never borrowed metadata from the alias's configured model"
        );
        assert_eq!(
            final_limits.configured_model_context_window(),
            None,
            "unknown capacity must be omitted from the wire rather than presented \
             as model truth"
        );

        // The terminal frame consumers read must agree with the outcome.
        let mut terminal = None;
        while let Ok(ev) = event_rx.try_recv() {
            if let TurnEvent::Usage {
                context_token_budget,
                model_context_window,
                ..
            } = ev
            {
                terminal = Some((context_token_budget, model_context_window));
            }
        }
        let (budget, window) = terminal.expect("the usage-less fallback must publish a snapshot");
        assert_eq!(
            window, None,
            "the terminal frame must omit capacity for the served model rather \
             than carrying the primary's window"
        );
        assert_eq!(
            budget,
            Some(final_limits.context_token_budget as u64),
            "the terminal budget must agree with the re-keyed served limits"
        );
    }

    fn build_test_agent(
        initial_provider_name: &str,
        initial_model_name: &str,
        switch_config: Option<ProviderSwitchConfig>,
    ) -> Agent {
        let provider = Box::new(MockModelProvider {
            responses: Mutex::new(vec![]),
        });
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation"),
        );
        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        let mut builder = Agent::builder()
            .model_provider(provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(MockTool)],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .model_provider_name(initial_provider_name.to_string())
            .model_name(initial_model_name.to_string());
        if let Some(cfg) = switch_config {
            builder = builder.provider_switch_config(cfg);
        }
        builder.build().expect("agent builder")
    }

    /// Build a config whose `ollama.large` route carries `window` capacity, so a
    /// generation swap is observable in both dispatch (the route's model) and the
    /// resolved limits.
    fn generation_config(window: usize, model: &str) -> clawcrew_config::schema::Config {
        let mut cfg = clawcrew_config::schema::Config::default();
        cfg.providers.models.custom.insert(
            "large".to_string(),
            clawcrew_config::schema::CustomModelProviderConfig {
                base: clawcrew_config::schema::ModelProviderConfig {
                    // The custom slot has no family-default endpoint, so a
                    // provider rebuild fails without a uri.
                    uri: Some("http://127.0.0.1:1/v1".to_string()),
                    model: Some(model.to_string()),
                    context_window: Some(window),
                    ..clawcrew_config::schema::ModelProviderConfig::default()
                },
            },
        );
        cfg
    }

    fn direct_live_generation_config(
        data_dir: &Path,
        provider_alias: &str,
        model: &str,
        window: usize,
        history_cap: usize,
    ) -> clawcrew_config::schema::Config {
        let provider_ref = format!("custom.{provider_alias}");
        let mut cfg = clawcrew_config::schema::Config {
            data_dir: data_dir.to_path_buf(),
            memory: clawcrew_config::schema::MemoryConfig {
                backend: "none".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        cfg.providers.models.custom.insert(
            provider_alias.to_string(),
            clawcrew_config::schema::CustomModelProviderConfig {
                base: clawcrew_config::schema::ModelProviderConfig {
                    uri: Some("http://127.0.0.1:1/v1".to_string()),
                    model: Some(model.to_string()),
                    context_window: Some(window),
                    ..Default::default()
                },
            },
        );
        cfg.risk_profiles.insert(
            "default".to_string(),
            clawcrew_config::schema::RiskProfileConfig::default(),
        );
        cfg.runtime_profiles.insert(
            "default".to_string(),
            clawcrew_config::schema::RuntimeProfileConfig {
                context_compact_ratio: Some(0.5),
                max_history_messages: Some(history_cap),
                ..Default::default()
            },
        );
        cfg.agents.insert(
            "direct".to_string(),
            clawcrew_config::schema::AliasedAgentConfig {
                model_provider: provider_ref.clone().into(),
                risk_profile: "default".into(),
                runtime_profile: "default".into(),
                ..Default::default()
            },
        );
        cfg.model_routes = vec![clawcrew_config::schema::ModelRouteConfig {
            hint: "fast".to_string(),
            model_provider: provider_ref,
            model: model.to_string(),
            api_key: None,
        }];
        cfg
    }

    #[tokio::test]
    async fn direct_live_agents_pin_one_route_generation_until_reconnect() {
        let temp = tempfile::tempdir().expect("tempdir");
        let live = Arc::new(parking_lot::RwLock::new(direct_live_generation_config(
            temp.path(),
            "old",
            "old-model",
            200_000,
            12,
        )));
        let mut retained = Agent::from_pinned_live_config_with_session_cwd_and_mcp_backchannel(
            Arc::clone(&live),
            "direct",
            Some(temp.path()),
            false,
            true,
            false,
            None,
            None,
            None,
        )
        .await
        .expect("direct Agent construction");

        *live.write() = direct_live_generation_config(temp.path(), "new", "new-model", 8_000, 3);
        retained.sync_config_generation();

        let (_, retained_provider, retained_model) = retained.attribution_fields();
        let retained_route = retained.resolved_route_for_test("hint:fast");
        let retained_limits = retained.context_limits();
        assert_eq!(
            (retained_provider.as_str(), retained_model.as_str()),
            ("custom.old", "old-model")
        );
        assert_eq!(
            (
                retained_route.provider_name.as_str(),
                retained_route.model.as_str()
            ),
            ("custom.old", "old-model")
        );
        assert_eq!(
            (
                retained_limits.model_context_window,
                retained_limits.context_token_budget
            ),
            (200_000, 100_000)
        );
        assert_eq!(
            retained
                .structured_history_cap_resolver
                .as_ref()
                .expect("direct Agent history resolver")(),
            3,
            "independently live history policy must adopt the reload while route state stays pinned"
        );
        assert_eq!(
            retained
                .provider_switch_config
                .as_ref()
                .and_then(|switch| switch.config.as_ref())
                .and_then(|cfg| cfg.providers.models.custom.get("old"))
                .and_then(|provider| provider.base.model.as_deref()),
            Some("old-model"),
            "the provider-rebuild snapshot must stay on the retained Agent's generation"
        );

        let rebuilt = Agent::from_pinned_live_config_with_session_cwd_and_mcp_backchannel(
            Arc::clone(&live),
            "direct",
            Some(temp.path()),
            false,
            true,
            false,
            None,
            None,
            None,
        )
        .await
        .expect("replacement direct Agent construction");
        let (_, rebuilt_provider, rebuilt_model) = rebuilt.attribution_fields();
        let rebuilt_route = rebuilt.resolved_route_for_test("hint:fast");
        let rebuilt_limits = rebuilt.context_limits();
        assert_eq!(
            (rebuilt_provider.as_str(), rebuilt_model.as_str()),
            ("custom.new", "new-model")
        );
        assert_eq!(
            (
                rebuilt_route.provider_name.as_str(),
                rebuilt_route.model.as_str()
            ),
            ("custom.new", "new-model")
        );
        assert_eq!(
            (
                rebuilt_limits.model_context_window,
                rebuilt_limits.context_token_budget
            ),
            (8_000, 4_000)
        );
    }

    /// A mid-session `config/set` must be observed by provider rebuilding and by
    /// limit resolution as ONE generation. The regression this pins: limits read
    /// the live shared config while `try_apply_model_switch` rebuilt from the
    /// construction-time snapshot, so a `config/set` followed by an explicit
    /// model switch dispatched on the old profiles while reporting the new
    /// capacity.
    #[test]
    fn config_set_then_model_switch_dispatches_and_reports_one_generation() {
        let live = Arc::new(parking_lot::RwLock::new(generation_config(
            200_000, "large-v1",
        )));
        let generation: ConfigGeneration =
            Arc::new(parking_lot::RwLock::new(Arc::new(live.read().clone())));

        let mut agent = build_test_agent(
            "custom.large",
            "large-v1",
            Some(ProviderSwitchConfig {
                config: Some(Arc::clone(&generation.read())),
                live: Some(Arc::clone(&live)),
            }),
        );
        agent.config_generation = Some(Arc::clone(&generation));
        agent.context_limits_resolver = Some(Arc::new(Agent::context_generation_limits_resolver(
            Arc::clone(&generation),
            String::new(),
        )));

        assert_eq!(
            agent
                .context_limits_for_route("custom.large", "large-v1")
                .model_context_window,
            200_000,
            "baseline limits come from the construction generation"
        );

        // A `config/set` lands on the live shared config mid-session.
        *live.write() = generation_config(8_000, "large-v2");

        // Within the turn already in flight the generation is still the old one:
        // a reload must not be observed by half a turn.
        assert_eq!(
            agent
                .context_limits_for_route("custom.large", "large-v1")
                .model_context_window,
            200_000,
            "an in-flight turn must not observe a mid-turn reload"
        );

        // The next turn boundary republishes it.
        agent.sync_config_generation();

        let switched = agent.try_apply_model_switch(
            "large-v1",
            "custom.large".to_string(),
            "large-v2".to_string(),
        );
        assert_eq!(
            switched.as_deref(),
            Some("large-v2"),
            "the switch must rebuild from the refreshed generation, which knows large-v2"
        );

        let limits = agent.context_limits_for_route("custom.large", "large-v2");
        assert_eq!(
            limits.model_context_window, 8_000,
            "limits must report the SAME generation the provider was rebuilt from"
        );
        assert_eq!(
            limits.context_token_budget, 8_000,
            "the legacy 32k default clamps to the refreshed 8k capacity"
        );
        assert_eq!(
            agent
                .provider_switch_config
                .as_ref()
                .and_then(|cfg| cfg.config.as_ref())
                .and_then(|cfg| cfg.providers.models.custom.get("large"))
                .and_then(|p| p.base.model.as_deref()),
            Some("large-v2"),
            "the provider-rebuild snapshot must be the refreshed generation, not the \
             construction-time clone"
        );
    }

    /// `apply_model_provider` publishes the generation it built the provider box
    /// from. Both derived handles must move together, or a later switch rebuilds
    /// from a generation the caller already replaced.
    #[test]
    fn set_config_generation_moves_switch_snapshot_and_limits_together() {
        let generation: ConfigGeneration = Arc::new(parking_lot::RwLock::new(Arc::new(
            generation_config(200_000, "large-v1"),
        )));
        let mut agent = build_test_agent(
            "custom.large",
            "large-v1",
            Some(ProviderSwitchConfig {
                config: Some(Arc::clone(&generation.read())),
                live: None,
            }),
        );
        agent.config_generation = Some(Arc::clone(&generation));
        agent.context_limits_resolver = Some(Arc::new(Agent::context_generation_limits_resolver(
            Arc::clone(&generation),
            String::new(),
        )));

        agent.set_config_generation(Arc::new(generation_config(8_000, "large-v2")));

        assert_eq!(
            agent
                .context_limits_for_route("custom.large", "large-v2")
                .model_context_window,
            8_000,
            "limit resolution must follow the published generation"
        );
        assert_eq!(
            agent
                .provider_switch_config
                .as_ref()
                .and_then(|cfg| cfg.config.as_ref())
                .and_then(|cfg| cfg.providers.models.custom.get("large"))
                .and_then(|p| p.base.context_window),
            Some(8_000),
            "the provider-rebuild snapshot must follow the same published generation"
        );
    }

    #[test]
    fn strip_trailing_interruption_marker_preserves_folded_partial_response() {
        let mut agent = build_test_agent("openai", "gpt-4o-mini", None);
        let marker = crate::i18n::get_required_cli_string("turn-interrupted-by-user");
        agent.history = vec![
            ConversationMessage::Chat(ChatMessage::user("prompt")),
            ConversationMessage::Chat(ChatMessage::assistant(format!("partial text\n\n{marker}"))),
        ];

        assert!(agent.strip_trailing_interruption_marker());
        assert!(matches!(
            agent.history.last(),
            Some(ConversationMessage::Chat(message))
                if message.role == "assistant" && message.content == "partial text"
        ));

        agent
            .history
            .push(ConversationMessage::Chat(ChatMessage::assistant(
                "ordinary assistant text",
            )));
        assert!(!agent.strip_trailing_interruption_marker());
        assert!(matches!(
            agent.history.last(),
            Some(ConversationMessage::Chat(message))
                if message.content == "ordinary assistant text"
        ));
    }

    #[test]
    fn try_apply_model_switch_noop_when_identical_to_current() {
        let mut agent = build_test_agent("openai", "gpt-4o-mini", None);
        let result = agent.try_apply_model_switch(
            "gpt-4o-mini",
            "openai".to_string(),
            "gpt-4o-mini".to_string(),
        );
        assert_eq!(result, None, "same-provider/same-model is a no-op");
    }

    #[test]
    fn try_apply_model_switch_preserves_agent_without_switch_config() {
        // Agent has NO provider_switch_config — cannot rebuild provider.
        let mut agent = build_test_agent("openai", "gpt-4o-mini", None);
        let result = agent.try_apply_model_switch(
            "gpt-4o-mini",
            "anthropic".to_string(),
            "claude-haiku".to_string(),
        );

        // Returns None (failed switch) and leaves the agent unchanged.
        assert_eq!(result, None);
        assert_eq!(
            agent.model_provider_name, "openai",
            "provider_name must NOT change when provider rebuild is not possible"
        );
        assert_eq!(
            agent.model_name, "gpt-4o-mini",
            "model_name must NOT change when provider rebuild is not possible"
        );
    }

    #[test]
    fn try_apply_model_switch_succeeds_with_switch_config() {
        let switch_cfg = ProviderSwitchConfig {
            config: Some(std::sync::Arc::new(
                clawcrew_config::schema::Config::default(),
            )),
            live: None,
        };

        let mut agent = build_test_agent("openai", "gpt-4o-mini", Some(switch_cfg));
        let result =
            agent.try_apply_model_switch("gpt-4o-mini", "ollama".to_string(), "llama3".to_string());

        assert_eq!(
            result.as_deref(),
            Some("llama3"),
            "successful switch must return the new effective model"
        );
        assert_eq!(
            agent.model_provider_name, "ollama",
            "provider_name must reflect the switched provider after success"
        );
        assert_eq!(
            agent.model_name, "llama3",
            "model_name must reflect the switched model after success"
        );
    }

    /// Regression: model_provider_context_window_opt follows the in-turn
    /// provider switch, not the static agent alias.
    #[test]
    fn model_context_window_follows_in_turn_model_switch() {
        let mut cfg = Config::default();
        let provider_a = cfg
            .providers
            .models
            .ensure("openai", "provider-a")
            .expect("ensure provider A");
        provider_a.context_window = Some(128_000);
        provider_a.model = Some("gpt-4o-mini".into());
        let provider_b = cfg
            .providers
            .models
            .ensure("ollama", "provider-b")
            .expect("ensure provider B");
        provider_b.context_window = Some(1_000_000);
        provider_b.model = Some("llama3".into());

        let cfg_arc = std::sync::Arc::new(cfg);
        let switch_cfg = ProviderSwitchConfig {
            config: Some(cfg_arc.clone()),
            live: None,
        };

        let mut agent = build_test_agent("openai.provider-a", "gpt-4o-mini", Some(switch_cfg));

        // Before switch: resolve with provider A's ref
        let (_, live_provider_before, live_model_before) = agent.attribution_fields();
        assert_eq!(live_provider_before, "openai.provider-a");
        let window_before = cfg_arc
            .model_provider_context_window_opt(&live_provider_before, &live_model_before)
            .map(|v| v as u64);
        assert_eq!(window_before, Some(128_000));

        // Apply in-turn switch
        let result = agent.try_apply_model_switch(
            "gpt-4o-mini",
            "ollama.provider-b".to_string(),
            "llama3".to_string(),
        );
        assert_eq!(
            result.as_deref(),
            Some("llama3"),
            "switch must return the new effective model (proves switch ran, not short-circuited)"
        );

        // After switch: B's ref and window
        let (_, live_provider_after, live_model_after) = agent.attribution_fields();
        assert_eq!(live_provider_after, "ollama.provider-b");
        let window_after = cfg_arc
            .model_provider_context_window_opt(&live_provider_after, &live_model_after)
            .map(|v| v as u64);
        assert_eq!(window_after, Some(1_000_000));
    }

    #[test]
    fn try_apply_model_switch_succeeds_on_provider_only_change() {
        let switch_cfg = ProviderSwitchConfig {
            config: Some(std::sync::Arc::new(
                clawcrew_config::schema::Config::default(),
            )),
            live: None,
        };

        let mut agent = build_test_agent("openai", "shared-name", Some(switch_cfg));
        let result = agent.try_apply_model_switch(
            "shared-name",
            "ollama".to_string(),
            "shared-name".to_string(),
        );

        assert_eq!(
            result.as_deref(),
            Some("shared-name"),
            "provider-only switch must also be treated as a successful switch"
        );
        assert_eq!(
            agent.model_provider_name, "ollama",
            "provider_name must update on a provider-only switch"
        );
        assert_eq!(agent.model_name, "shared-name");
    }

    #[test]
    fn model_switch_re_resolves_context_limits_for_new_route() {
        let switch_cfg = ProviderSwitchConfig {
            config: Some(std::sync::Arc::new(
                clawcrew_config::schema::Config::default(),
            )),
            live: None,
        };
        let mut agent = build_test_agent("ollama.large", "large", Some(switch_cfg));
        agent.context_limits_resolver = Some(Arc::new(|provider_ref, model| {
            match (provider_ref, model) {
                ("ollama.large", "large") => clawcrew_config::schema::ResolvedContextLimits {
                    model_context_window: 200_000,
                    context_token_budget: 180_000,
                    model_context_window_source:
                        clawcrew_config::schema::ModelContextWindowSource::Configured,
                },
                ("ollama.small", "small") => clawcrew_config::schema::ResolvedContextLimits {
                    model_context_window: 8_000,
                    context_token_budget: 7_200,
                    model_context_window_source:
                        clawcrew_config::schema::ModelContextWindowSource::Configured,
                },
                _ => clawcrew_config::schema::ResolvedContextLimits {
                    model_context_window: 32_000,
                    context_token_budget: 32_000,
                    model_context_window_source:
                        clawcrew_config::schema::ModelContextWindowSource::CompatibilityFallback,
                },
            }
        }));

        assert_eq!(agent.context_limits().context_token_budget, 180_000);
        let switched =
            agent.try_apply_model_switch("large", "ollama.small".to_string(), "small".to_string());

        assert_eq!(switched.as_deref(), Some("small"));
        let selected = agent.model_route_resolver.resolve("small");
        assert_eq!(selected.provider_name, "ollama.small");
        assert_eq!(selected.model, "small");
        assert_eq!(
            agent.context_limits(),
            clawcrew_config::schema::ResolvedContextLimits {
                model_context_window: 8_000,
                context_token_budget: 7_200,
                model_context_window_source:
                    clawcrew_config::schema::ModelContextWindowSource::Configured,
            },
            "provider/model and their limits must change as one session state transition"
        );
    }

    #[test]
    fn try_apply_model_switch_prefers_route_api_key() {
        let route = clawcrew_config::schema::ModelRouteConfig {
            model_provider: "ollama".to_string(),
            model: "tinyllama".to_string(),
            hint: "fast".to_string(),
            api_key: Some("route-specific-key".to_string()),
        };

        let route_config = clawcrew_config::schema::Config {
            model_routes: vec![route],
            ..clawcrew_config::schema::Config::default()
        };
        let switch_cfg = ProviderSwitchConfig {
            config: Some(std::sync::Arc::new(route_config)),
            live: None,
        };

        let mut agent = build_test_agent("openai", "gpt-4o-mini", Some(switch_cfg));
        let result = agent.try_apply_model_switch(
            "gpt-4o-mini",
            "ollama".to_string(),
            "tinyllama".to_string(),
        );

        assert_eq!(
            result.as_deref(),
            Some("tinyllama"),
            "switch must succeed when a model_routes entry matches the target"
        );
        assert_eq!(agent.model_provider_name, "ollama");
    }

    /// Streamed mock whose first call emits a tool call (queuing a model
    /// switch via `ModelSwitchTriggerTool`) and whose later calls emit final
    /// text. `call_count` lets the test prove the original provider is used
    /// for exactly the first call — the next call goes to the switched one.
    struct StreamSwitchTriggerProvider {
        call_count: Arc<Mutex<usize>>,
    }

    #[async_trait]
    impl ModelProvider for StreamSwitchTriggerProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<String> {
            Ok("ok".into())
        }

        async fn chat(
            &self,
            _request: ChatRequest<'_>,
            _model: &str,
            _temperature: Option<f64>,
        ) -> Result<clawcrew_providers::ChatResponse> {
            // The unified loop drives the streaming wrapper through `chat`
            // (stream events are synthesized post-hoc), so the tool call that
            // queues the switch is emitted here on the first call.
            let mut count = self.call_count.lock();
            *count += 1;
            if *count == 1 {
                Ok(clawcrew_providers::ChatResponse {
                    text: Some(String::new()),
                    tool_calls: vec![clawcrew_providers::ToolCall {
                        id: "00000000-0000-0000-0000-000000000002".into(),
                        name: "model_switch_trigger".into(),
                        arguments: "{}".into(),
                        extra_content: None,
                    }],
                    usage: None,
                    reasoning_content: None,
                })
            } else {
                // Should not be reached: after the switch, the next call goes
                // to the switched provider, not this one.
                Ok(clawcrew_providers::ChatResponse {
                    text: Some("original-provider-should-not-be-reused".into()),
                    tool_calls: vec![],
                    usage: None,
                    reasoning_content: None,
                })
            }
        }

        fn supports_native_tools(&self) -> bool {
            true
        }

        fn stream_chat(
            &self,
            _request: ChatRequest<'_>,
            _model: &str,
            _temperature: Option<f64>,
            _options: clawcrew_providers::traits::StreamOptions,
        ) -> futures_util::stream::BoxStream<
            'static,
            clawcrew_providers::traits::StreamResult<clawcrew_providers::traits::StreamEvent>,
        > {
            use futures_util::stream::{self, StreamExt};
            let mut count = self.call_count.lock();
            *count += 1;
            if *count == 1 {
                // First call: ask to run the tool that queues a model switch.
                let tc = clawcrew_providers::traits::StreamEvent::ToolCall(
                    clawcrew_providers::ToolCall {
                        id: "00000000-0000-0000-0000-000000000002".into(),
                        name: "model_switch_trigger".into(),
                        arguments: "{}".into(),
                        extra_content: None,
                    },
                );
                stream::iter(vec![
                    Ok(tc),
                    Ok(clawcrew_providers::traits::StreamEvent::Final),
                ])
                .boxed()
            } else {
                // Should not be reached: after the switch, the next call goes
                // to the switched provider, not this one.
                let chunk = clawcrew_providers::traits::StreamEvent::TextDelta(
                    clawcrew_providers::traits::StreamChunk {
                        delta: "original-provider-should-not-be-reused".into(),
                        is_final: false,
                        reasoning: None,
                        token_count: 0,
                    },
                );
                stream::iter(vec![
                    Ok(chunk),
                    Ok(clawcrew_providers::traits::StreamEvent::Final),
                ])
                .boxed()
            }
        }
    }

    impl ::clawcrew_api::attribution::Attributable for StreamSwitchTriggerProvider {
        fn role(&self) -> ::clawcrew_api::attribution::Role {
            ::clawcrew_api::attribution::Role::Provider(
                ::clawcrew_api::attribution::ProviderKind::Model(
                    ::clawcrew_api::attribution::ModelProviderKind::Custom,
                ),
            )
        }
        fn alias(&self) -> &str {
            "StreamSwitchTriggerProvider"
        }
    }

    /// Test tool that queues a pending `model_switch` when executed, standing
    /// in for the real `model_switch` tool during a streamed turn.
    struct ModelSwitchTriggerTool {
        target_provider: String,
        target_model: String,
    }

    #[async_trait]
    impl Tool for ModelSwitchTriggerTool {
        fn name(&self) -> &str {
            "model_switch_trigger"
        }
        fn description(&self) -> &str {
            "test tool: queues a pending model switch"
        }
        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }
        async fn execute(&self, _args: serde_json::Value) -> Result<crate::tools::ToolResult> {
            let state = crate::agent::turn::current_model_switch_state()?;
            *state.lock().unwrap() =
                Some((self.target_provider.clone(), self.target_model.clone()));
            Ok(crate::tools::ToolResult {
                success: true,
                output: "model switch queued".into(),
                error: None,
            })
        }
    }

    #[test]
    fn turn_streamed_applies_pending_model_switch_for_next_call() {
        let initial_calls = Arc::new(Mutex::new(0usize));
        let provider = Box::new(StreamSwitchTriggerProvider {
            call_count: Arc::clone(&initial_calls),
        });

        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation"),
        );
        let capturing = Arc::new(CapturingObserver::default());
        let observer: Arc<dyn Observer> = capturing.clone();

        let switch_cfg = ProviderSwitchConfig {
            config: Some(std::sync::Arc::new(clawcrew_config::schema::Config {
                reliability: clawcrew_config::schema::ReliabilityConfig {
                    provider_retries: 0,
                    provider_backoff_ms: 0,
                    ..clawcrew_config::schema::ReliabilityConfig::default()
                },
                ..clawcrew_config::schema::Config::default()
            })),
            live: None,
        };
        let agent_config = clawcrew_config::schema::AliasedAgentConfig {
            resolved: clawcrew_config::schema::ResolvedRuntime {
                strict_tool_parsing: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let skills = vec![crate::skills::Skill {
            name: "deploy".into(),
            description: "Release safely".into(),
            description_localizations: Default::default(),
            version: "1.0.0".into(),
            author: None,
            tags: vec![],
            tools: vec![],
            prompts: vec!["Run smoke tests before deploy.".into()],
            slash_options: Vec::new(),
            always: false,
            location: None,
        }];

        let mut agent = Agent::builder()
            .model_provider(provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![Box::new(ModelSwitchTriggerTool {
                    target_provider: "ollama".to_string(),
                    target_model: "llama3".to_string(),
                })],
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .config(agent_config)
            .skills(skills)
            .skills_prompt_mode(clawcrew_config::schema::SkillsPromptInjectionMode::Compact)
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .model_provider_name("openai".to_string())
            .model_name("gpt-4o-mini".to_string())
            .provider_switch_config(switch_cfg)
            .build()
            .expect("agent builder");

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        rt.block_on(async {
            let (event_tx, _event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(64);
            // The turn ultimately errors because the switched provider has no
            // live server; the timeout only guards against an unexpected hang.
            let _ = tokio::time::timeout(
                std::time::Duration::from_secs(15),
                agent.turn_streamed("please switch the model", event_tx, None),
            )
            .await;
        });

        // `turn_streamed` itself must have consumed the pending switch and
        // committed the rebuilt provider/model via `ProviderSwitchConfig`.
        assert_eq!(
            agent.model_provider_name, "ollama",
            "turn_streamed must commit the switched provider after the tool result"
        );
        assert_eq!(
            agent.model_name, "llama3",
            "turn_streamed must commit the switched model after the tool result"
        );
        let prompt = match agent.history.first() {
            Some(ConversationMessage::Chat(message)) if message.role == "system" => {
                &message.content
            }
            _ => panic!("history must retain the rebuilt system prompt"),
        };
        assert!(
            prompt.contains("Model: llama3"),
            "turn_streamed must rebuild the system prompt against the switched model"
        );

        // The original provider is used for exactly the first call; the next
        // call in the same turn goes to the switched provider instead.
        assert_eq!(
            *initial_calls.lock(),
            1,
            "the original provider must serve only the first call — the next \
             call must use the switched provider, not the original"
        );

        // The next provider call in the same streamed turn targets the
        // switched provider/model: the `LlmRequest` event is recorded at the
        // top of the post-switch iteration, immediately before that call.
        let events = capturing.events.lock();
        let switched_request = events.iter().any(|e| {
            matches!(
                e,
                ObserverEvent::LlmRequest { model_provider, model, .. }
                    if model_provider == "ollama" && model == "llama3"
            )
        });
        assert!(
            switched_request,
            "turn_streamed must issue the next provider call against the switched \
             provider/model (ollama/llama3); captured events: {events:?}"
        );
        drop(events);
    }

    fn turn_datetime_agent(
        model_provider: Box<dyn ModelProvider>,
        fixed: chrono::DateTime<chrono::Local>,
    ) -> Agent {
        let memory_cfg = clawcrew_config::schema::MemoryConfig {
            backend: "none".into(),
            ..clawcrew_config::schema::MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> = Arc::from(
            clawcrew_memory::create_memory(&memory_cfg, std::path::Path::new("/tmp"), None)
                .expect("memory creation should succeed with valid config"),
        );
        let observer: Arc<dyn Observer> = Arc::from(crate::observability::NoopObserver {});
        Agent::builder()
            .model_provider(model_provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                Vec::new(),
            ))
            .memory(mem)
            .observer(observer)
            .tool_dispatcher(Box::new(NativeToolDispatcher))
            .workspace_dir(std::path::PathBuf::from("/tmp"))
            .turn_datetime(move || fixed)
            .build()
            .expect("agent builder should succeed with valid config")
    }

    fn stored_user_message(agent: &Agent) -> String {
        agent
            .history()
            .iter()
            .find_map(|m| match m {
                ConversationMessage::Chat(chat) if chat.role == "user" => {
                    Some(chat.content.clone())
                }
                _ => None,
            })
            .expect("a user message must be stored in history")
    }

    #[tokio::test]
    async fn streamed_history_uses_labeled_shape_not_bare_timestamp() {
        let fixed = chrono::Local
            .with_ymd_and_hms(2026, 3, 14, 9, 30, 0)
            .single()
            .expect("fixed local test timestamp");
        let model_provider = Box::new(MockModelProvider {
            responses: Mutex::new(Vec::new()),
        });
        let mut agent = turn_datetime_agent(model_provider, fixed);

        let mut new_msgs = Vec::new();
        agent
            .append_streamed_user_message_to_history("hello there", &mut new_msgs, "test-turn")
            .await;

        let stored = stored_user_message(&agent);
        assert!(
            stored.starts_with("[CURRENT DATE & TIME: 2026-03-14 09:30:00"),
            "streamed history must use the labeled shape, got: {stored}"
        );
        assert!(
            stored.contains("hello there"),
            "stored message must retain the original text: {stored}"
        );
        assert!(
            !stored.starts_with("[2026-03-14 09:30:00"),
            "streamed history must not fall back to the bare `[{{ts}}] {{msg}}` shape: {stored}"
        );
    }

    #[tokio::test]
    async fn streamed_and_non_streamed_enrichment_match_for_same_clock_and_message() {
        let fixed = chrono::Local
            .with_ymd_and_hms(2026, 3, 14, 9, 30, 0)
            .single()
            .expect("fixed local test timestamp");

        let mut streamed_agent = turn_datetime_agent(
            Box::new(MockModelProvider {
                responses: Mutex::new(Vec::new()),
            }),
            fixed,
        );
        let mut new_msgs = Vec::new();
        streamed_agent
            .append_streamed_user_message_to_history("same message", &mut new_msgs, "turn-a")
            .await;
        let streamed_content = stored_user_message(&streamed_agent);

        let mut non_streamed_agent = turn_datetime_agent(
            Box::new(MockModelProvider {
                responses: Mutex::new(Vec::new()),
            }),
            fixed,
        );
        non_streamed_agent
            .turn("same message")
            .await
            .expect("turn should succeed");
        let non_streamed_content = stored_user_message(&non_streamed_agent);

        assert_eq!(
            streamed_content, non_streamed_content,
            "streamed and non-streamed enrichment must be byte-identical for the same clock and message"
        );
    }
}

#[cfg(test)]
mod approval_route_tests {
    use super::*;
    use parking_lot::RwLock;
    use std::collections::HashMap;
    use clawcrew_api::channel::{ChannelApprovalRequest, ChannelApprovalResponse};
    use clawcrew_config::autonomy::{ApprovalRoute, OnNoApprover};

    enum StubBehavior {
        Answer(ChannelApprovalResponse),
        NoDecision,
        Slow,
    }

    struct StubChannel {
        name: String,
        behavior: StubBehavior,
    }

    impl clawcrew_api::attribution::Attributable for StubChannel {
        fn role(&self) -> clawcrew_api::attribution::Role {
            clawcrew_api::attribution::Role::Channel(clawcrew_api::attribution::ChannelKind::Cli)
        }
        fn alias(&self) -> &str {
            &self.name
        }
    }

    #[async_trait::async_trait]
    impl clawcrew_api::channel::Channel for StubChannel {
        fn name(&self) -> &str {
            &self.name
        }
        async fn send(&self, _m: &clawcrew_api::channel::SendMessage) -> anyhow::Result<()> {
            Ok(())
        }
        async fn listen(
            &self,
            _tx: tokio::sync::mpsc::Sender<clawcrew_api::channel::ChannelMessage>,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        async fn request_approval(
            &self,
            _recipient: &str,
            _request: &ChannelApprovalRequest,
        ) -> anyhow::Result<Option<ChannelApprovalResponse>> {
            match &self.behavior {
                StubBehavior::Answer(resp) => Ok(Some(resp.clone())),
                StubBehavior::NoDecision => Ok(None),
                StubBehavior::Slow => {
                    // Far exceeds the route timeout; with a paused clock the
                    // timeout fires at +timeout_secs virtual time, instantly.
                    tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
                    Ok(Some(ChannelApprovalResponse::Approve))
                }
            }
        }
    }

    fn registry(channels: Vec<StubChannel>) -> tools::PerToolChannelHandle {
        let mut map: HashMap<String, Arc<dyn clawcrew_api::channel::Channel>> = HashMap::new();
        for c in channels {
            map.insert(c.name.clone(), Arc::new(c));
        }
        Arc::new(RwLock::new(map))
    }

    fn req() -> ChannelApprovalRequest {
        ChannelApprovalRequest {
            tool_name: "shell".into(),
            arguments_summary: "rm -rf /".into(),
            raw_arguments: None,
            position: None,
        }
    }

    fn route(approver: &str, policy: OnNoApprover) -> ApprovalRoute {
        ApprovalRoute {
            approver_channel: approver.into(),
            on_no_approver: policy,
            timeout_secs: 1,
        }
    }

    #[tokio::test]
    async fn approver_answer_is_used_and_attributed() {
        let h = registry(vec![StubChannel {
            name: "ops".into(),
            behavior: StubBehavior::Answer(ChannelApprovalResponse::Approve),
        }]);
        match resolve_routed_approval(&h, &route("ops", OnNoApprover::Deny), "r", &req()).await {
            RoutedApproval::Decided {
                response,
                decider,
                source,
            } => {
                assert_eq!(response, ChannelApprovalResponse::Approve);
                assert_eq!(
                    decider.as_deref(),
                    Some("ops"),
                    "decider names the approver"
                );
                assert_eq!(
                    source,
                    clawcrew_api::channel::ApprovalSource::Operator,
                    "an approver's answer is an operator decision"
                );
            }
            RoutedApproval::Fallthrough => panic!("expected a routed decision"),
        }
    }

    #[tokio::test]
    async fn unregistered_approver_fails_closed_by_default() {
        let h = registry(vec![]);
        match resolve_routed_approval(&h, &route("ops", OnNoApprover::Deny), "r", &req()).await {
            RoutedApproval::Decided {
                response,
                decider,
                source,
            } => {
                assert_eq!(response, ChannelApprovalResponse::Deny, "fail-closed deny");
                assert!(decider.is_none(), "synthetic deny has no decider");
                // The regression this guards: a fail-closed deny is Some(Deny)
                // with no decider, so anything inferring "a user decided" from
                // the presence of a response reports a denial nobody made.
                assert_eq!(
                    source,
                    clawcrew_api::channel::ApprovalSource::Unavailable,
                    "an unregistered approver is a runtime denial, not a user's"
                );
                assert!(source.is_runtime_fail_closed());
            }
            RoutedApproval::Fallthrough => panic!("default policy must NOT fall through"),
        }
    }

    #[tokio::test]
    async fn unregistered_approver_inherits_when_opted_in() {
        let h = registry(vec![]);
        let out = resolve_routed_approval(
            &h,
            &route("ops", OnNoApprover::InheritOriginator),
            "r",
            &req(),
        )
        .await;
        assert!(
            matches!(out, RoutedApproval::Fallthrough),
            "InheritOriginator must fall through to the originating fan-out"
        );
    }

    #[tokio::test]
    async fn no_decision_fails_closed() {
        let h = registry(vec![StubChannel {
            name: "ops".into(),
            behavior: StubBehavior::NoDecision,
        }]);
        let out = resolve_routed_approval(&h, &route("ops", OnNoApprover::Deny), "r", &req()).await;
        assert!(
            matches!(
                out,
                RoutedApproval::Decided {
                    response: ChannelApprovalResponse::Deny,
                    source: clawcrew_api::channel::ApprovalSource::Unreachable,
                    ..
                }
            ),
            "an approver that returns no decision is a runtime denial: {out:?}"
        );
    }

    // The route timeout (1s) fires and cancels the stub's long sleep, so this
    // resolves in ~1s of real time without needing tokio's `test-util` clock.
    #[tokio::test]
    async fn slow_approver_times_out_and_fails_closed() {
        let h = registry(vec![StubChannel {
            name: "ops".into(),
            behavior: StubBehavior::Slow,
        }]);
        let out = resolve_routed_approval(&h, &route("ops", OnNoApprover::Deny), "r", &req()).await;
        // A timeout is the case most easily mistaken for a user's "no": the
        // route returns Some(Deny) exactly as an operator denial would.
        assert!(
            matches!(
                out,
                RoutedApproval::Decided {
                    response: ChannelApprovalResponse::Deny,
                    source: clawcrew_api::channel::ApprovalSource::TimedOut,
                    ..
                }
            ),
            "a timed-out approver is a runtime denial, not a user's: {out:?}"
        );
    }

    use clawcrew_api::channel::Channel as _;

    #[tokio::test]
    async fn routed_channel_returns_and_attributes_approver_decision() {
        let h = registry(vec![StubChannel {
            name: "ops".into(),
            behavior: StubBehavior::Answer(ChannelApprovalResponse::Approve),
        }]);
        let bridge = RoutedApprovalChannel::new(h, route("ops", OnNoApprover::Deny));
        let out = bridge
            .request_approval_attributed("r", &req())
            .await
            .unwrap()
            .expect("the approver decided");
        assert_eq!(out.response, ChannelApprovalResponse::Approve);
        assert_eq!(
            out.decided_by.as_deref(),
            Some("ops"),
            "the gate attributes the approval to the deciding channel"
        );
    }

    #[tokio::test]
    async fn routed_channel_fails_closed_when_approver_unregistered() {
        let bridge = RoutedApprovalChannel::new(registry(vec![]), route("ops", OnNoApprover::Deny));
        let out = bridge
            .request_approval_attributed("r", &req())
            .await
            .unwrap()
            .expect("the fail-closed deny is a decision");
        assert_eq!(
            out.response,
            ChannelApprovalResponse::Deny,
            "unreachable approver denies, not auto-approves"
        );
        assert!(
            out.decided_by.is_none(),
            "a bridge-synthesized fail-closed deny has no deciding channel"
        );
    }

    #[tokio::test]
    async fn routed_channel_inherit_returns_none_on_channelless_path() {
        let bridge = RoutedApprovalChannel::new(
            registry(vec![]),
            route("ops", OnNoApprover::InheritOriginator),
        );
        let out = bridge.request_approval("r", &req()).await.unwrap();
        assert_eq!(
            out, None,
            "no originator to inherit; gate applies the non-interactive auto-deny"
        );
    }
