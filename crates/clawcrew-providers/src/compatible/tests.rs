    use super::*;

    /// Empty / whitespace arguments must collapse to `"{}"` so OpenAI-style
    /// providers never see an invalid `tool_calls[].function.arguments`.
    #[test]
    fn sanitize_tool_arguments_empty_or_whitespace_becomes_empty_object() {
        assert_eq!(sanitize_tool_arguments("f", ""), "{}");
        assert_eq!(sanitize_tool_arguments("f", "   \n\t  "), "{}");
    }

    /// The `/models` success path buffers the whole body before parsing, so a
    /// misbehaving or compromised router could otherwise make the client hold
    /// an unbounded response — most exposed on the credential-free
    /// `PUBLIC_MODEL_LISTING` path. `read_body_capped` must refuse an oversized
    /// success body two ways: a declared `Content-Length` over the cap fails
    /// fast, and a chunked body with NO `Content-Length` fails as it grows.
    #[tokio::test]
    async fn read_body_capped_bounds_oversized_success_bodies() {
        use axum::Router;
        use axum::body::{Body, Bytes};
        use axum::routing::get;
        use tokio::net::TcpListener;

        const CAP: u64 = 1024;
        let under = vec![b'x'; 512];

        let under_route = under.clone();
        let app = Router::new()
            .route(
                "/under",
                get(move || {
                    let body = under_route.clone();
                    async move { body }
                }),
            )
            .route(
                // Honest Content-Length over the cap.
                "/declared_over",
                get(|| async { vec![b'x'; (CAP as usize) + 4096] }),
            )
            .route(
                // Eight 512-byte chunks (4096 > CAP) streamed with no
                // Content-Length, so only the running accumulator can catch it.
                "/streamed_over",
                get(|| async {
                    let chunks =
                        (0..8).map(|_| Ok::<_, std::io::Error>(Bytes::from(vec![b'x'; 512])));
                    Body::from_stream(futures_util::stream::iter(chunks))
                }),
            );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = ::clawcrew_spawn::spawn!(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = reqwest::Client::new();
        let fetch = |path: &str| {
            let url = format!("http://{addr}{path}");
            let client = client.clone();
            async move { client.get(url).send().await.expect("fixture request") }
        };

        // Under the cap: accepted, bytes preserved exactly.
        assert_eq!(
            read_body_capped(fetch("/under").await, CAP)
                .await
                .expect("a body under the cap is accepted"),
            under
        );
        // Declared oversize: refused before buffering.
        assert!(
            read_body_capped(fetch("/declared_over").await, CAP)
                .await
                .is_err(),
            "a declared-oversize body must be refused"
        );
        // Streamed oversize, no Content-Length: refused as it grows.
        assert!(
            read_body_capped(fetch("/streamed_over").await, CAP)
                .await
                .is_err(),
            "a streamed body past the cap must be refused"
        );

        server.abort();
    }

    /// Well-formed JSON object returns untouched — only object-shaped arguments
    /// satisfy the strict-provider function-arguments contract.
    #[test]
    fn sanitize_tool_arguments_valid_json_is_passthrough() {
        let args = r#"{"path":"/tmp/x","recursive":true}"#;
        assert_eq!(sanitize_tool_arguments("file_read", args), args);
    }

    /// Non-object JSON values (null, array, string, number, boolean) are
    /// rejected to `"{}"` because strict providers require a JSON object for
    /// tool-call arguments.
    #[test]
    fn sanitize_tool_arguments_non_object_becomes_empty_object() {
        assert_eq!(sanitize_tool_arguments("f", "null"), "{}");
        assert_eq!(sanitize_tool_arguments("f", "[]"), "{}");
        assert_eq!(sanitize_tool_arguments("f", "42"), "{}");
        assert_eq!(sanitize_tool_arguments("f", "\"hello\""), "{}");
        assert_eq!(sanitize_tool_arguments("f", "true"), "{}");
    }

    /// Malformed arguments are dropped to `"{}"` so strict upstreams (Cohere,
    /// OpenInference, Nvidia via OpenRouter) no longer reject the whole request
    /// with HTTP 400 just because the model emitted junk arguments.
    #[test]
    fn sanitize_tool_arguments_invalid_json_becomes_empty_object() {
        // Unterminated string
        assert_eq!(sanitize_tool_arguments("f", r#"{"path":"/tmp"#), "{}");
        // Trailing junk
        assert_eq!(sanitize_tool_arguments("f", r#"{"x":1}garbage"#), "{}");
        // Truncated (the observed failure case from the field)
        assert_eq!(sanitize_tool_arguments("f", ""), "{}");
    }

    #[test]
    fn streaming_api_error_sanitizes_and_bounds_upstream_body() {
        let secret = "sk-test-streaming-secret";
        let body = format!(r#"{{"error":"{secret} {}"}}"#, "x".repeat(4_000));
        let error = streaming_api_error(reqwest::StatusCode::UNAUTHORIZED, &body).to_string();

        assert!(error.starts_with("ModelProvider error: 401 Unauthorized:"));
        assert!(error.contains("[REDACTED]"));
        assert!(!error.contains(secret));
        assert!(error.chars().count() <= 550);
    }

    #[test]
    fn streaming_api_error_extracts_message_from_stringified_error_envelope() {
        let message =
            "anthropic error: Message: fetch failed Cause: AggregateError Name: TypeError";
        let body = serde_json::json!({
            "status": "failure",
            "message": message,
            "error": serde_json::json!({
                "message": message,
                "type": "APIError",
                "code": "500",
            })
            .to_string(),
            "error_origin_level": "api_error",
            "provider": "anthropic",
        })
        .to_string();

        let error =
            streaming_api_error(reqwest::StatusCode::INTERNAL_SERVER_ERROR, &body).to_string();

        assert_eq!(
            error,
            format!("ModelProvider error: 500 Internal Server Error: {message}")
        );
    }

    fn make_model_provider(
        name: &str,
        url: &str,
        key: Option<&str>,
    ) -> OpenAiCompatibleModelProvider {
        OpenAiCompatibleModelProvider::builder("test")
            .display_name(name)
            .base_url(url)
            .credential(key)
            .auth_style(AuthStyle::Bearer)
            .build()
    }

    async fn mock_non_streaming_response(
        response_body: serde_json::Value,
    ) -> (
        OpenAiCompatibleModelProvider,
        std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
        tokio::task::JoinHandle<()>,
    ) {
        use axum::{Json, Router, routing::post};
        use tokio::net::TcpListener;

        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured_for_route = std::sync::Arc::clone(&captured);
        let app = Router::new().route(
            "/chat/completions",
            post(move |Json(request): Json<serde_json::Value>| {
                let captured = std::sync::Arc::clone(&captured_for_route);
                let response_body = response_body.clone();
                async move {
                    captured.lock().unwrap().push(request);
                    Json(response_body)
                }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = ::clawcrew_spawn::spawn!(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let provider = make_model_provider("custom", &format!("http://{addr}"), Some("test-key"));

        (provider, captured, server)
    }

    fn make_cache_passthrough_model_provider(
        name: &str,
        url: &str,
    ) -> OpenAiCompatibleModelProvider {
        OpenAiCompatibleModelProvider::builder("test")
            .display_name(name)
            .base_url(url)
            .auth_style(AuthStyle::Bearer)
            .with_cache_passthrough()
            .build()
    }

    /// Capability pin: the compat provider reports prompt caching exactly
    /// when the flag is set, so routing and UI gating can trust capability
    /// reporting instead of probing the wire.
    #[test]
    fn cache_passthrough_capability_reports_flag_state() {
        let off = make_model_provider("custom", "http://127.0.0.1:1", None);
        assert!(!off.capabilities().prompt_caching);
        let on = make_cache_passthrough_model_provider("custom", "http://127.0.0.1:1");
        assert!(on.capabilities().prompt_caching);
    }

    /// Byte-identity pin for the default path: the flag-off request body is
    /// fully specified, so any future injection work that leaks into the
    /// default path fails this test instead of silently changing the wire.
    #[tokio::test]
    async fn cache_passthrough_flag_off_request_body_is_pinned() {
        let (provider, captured, server) = mock_non_streaming_response(serde_json::json!({
            "choices": [{"message": {"content": "ok"}}]
        }))
        .await;
        let result = provider
            .chat_with_system(Some("be brief"), "hello", "test-model", None)
            .await;
        server.abort();
        let result = result.unwrap_or_else(|error| panic!("flag-off request failed: {error}"));
        assert_eq!(result, "ok");
        let requests = captured.lock().unwrap();
        assert_eq!(
            requests[0],
            serde_json::json!({
                "model": "test-model",
                "messages": [
                    {"role": "system", "content": "be brief"},
                    {"role": "user", "content": "hello"},
                ],
                "stream": false,
            }),
            "flag-off request body must stay byte-identical to the pre-feature wire"
        );
    }

    /// Capture mock for the TTL path: passthrough on or off, provider
    /// built with the requested `cache_ttl` when set. Same wire as
    /// [`Self::mock_streaming_cache_capture`] so tests pin both paths
    /// against one shape.
    async fn mock_cache_capture_with_ttl(
        cache_passthrough: bool,
        cache_ttl: Option<CacheTtl>,
    ) -> (
        OpenAiCompatibleModelProvider,
        std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
        tokio::task::JoinHandle<()>,
    ) {
        use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing::post};
        use tokio::net::TcpListener;

        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured_for_route = std::sync::Arc::clone(&captured);
        let app = Router::new().route(
            "/chat/completions",
            post(move |Json(body): Json<serde_json::Value>| {
                let captured = std::sync::Arc::clone(&captured_for_route);
                async move {
                    let streaming =
                        body.get("stream").and_then(serde_json::Value::as_bool) == Some(true);
                    captured.lock().unwrap().push(body);
                    if streaming {
                        return (
                            StatusCode::OK,
                            [("content-type", "text/event-stream")],
                            concat!(
                                "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n",
                                "data: [DONE]\n\n"
                            ),
                        )
                            .into_response();
                    }
                    Json(serde_json::json!({
                        "choices": [{"message": {"content": "ok"}}]
                    }))
                    .into_response()
                }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = ::clawcrew_spawn::spawn!(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let mut builder = OpenAiCompatibleModelProvider::builder("test")
            .display_name("custom")
            .base_url(&format!("http://{addr}"))
            .auth_style(AuthStyle::Bearer);
        if cache_passthrough {
            builder = builder.with_cache_passthrough();
        }
        if let Some(cache_ttl) = cache_ttl {
            builder = builder.with_cache_ttl(cache_ttl);
        }
        let provider = builder.build();

        (provider, captured, server)
    }

    /// D2 + D5: behind the flag, the 1h lifetime lands on every breakpoint
    /// the compat provider places (system prompt; rolling last message),
    /// and only breakpoint-carrying messages convert to block form. With
    /// the default lifetime the body contains no `ttl` key at all.
    #[tokio::test]
    async fn cache_ttl_one_hour_marks_every_compat_breakpoint() {
        let (provider, captured, server) =
            mock_cache_capture_with_ttl(true, Some(CacheTtl::OneHour)).await;
        let messages = vec![
            ChatMessage::system("be brief"),
            ChatMessage::user("first question"),
            ChatMessage::assistant("first answer"),
            ChatMessage::user("second question"),
        ];
        let result = provider
            .chat(
                ProviderChatRequest {
                    messages: &messages,
                    tools: None,
                    thinking: None,
                },
                "test-model",
                None,
            )
            .await;
        server.abort();
        result.unwrap_or_else(|error| panic!("1h request failed: {error}"));

        {
            let requests = captured.lock().unwrap();
            let body = &requests[0];
            let msgs = body["messages"].as_array().expect("messages array");

            let system_block = msgs[0]["content"][0]["cache_control"]
                .as_object()
                .expect("system breakpoint in block form");
            assert_eq!(system_block["type"], "ephemeral");
            assert_eq!(
                system_block["ttl"], "1h",
                "system marker carries the 1h lifetime"
            );

            let rolling_block = msgs[3]["content"][0]["cache_control"]
                .as_object()
                .expect("rolling breakpoint in block form");
            assert_eq!(rolling_block["type"], "ephemeral");
            assert_eq!(
                rolling_block["ttl"], "1h",
                "rolling marker carries the 1h lifetime"
            );

            assert_eq!(
                msgs[2]["content"], "first answer",
                "non-carrier messages must keep plain string serialization"
            );
        }

        // Default-lifetime control run: same placement, no ttl anywhere.
        let (provider, captured, server) = mock_cache_capture_with_ttl(true, None).await;
        let messages = vec![
            ChatMessage::system("be brief"),
            ChatMessage::user("first question"),
            ChatMessage::assistant("first answer"),
            ChatMessage::user("second question"),
        ];
        let result = provider
            .chat(
                ProviderChatRequest {
                    messages: &messages,
                    tools: None,
                    thinking: None,
                },
                "test-model",
                None,
            )
            .await;
        server.abort();
        result.unwrap_or_else(|error| panic!("default request failed: {error}"));
        let requests = captured.lock().unwrap();
        let body = &requests[0];
        assert!(
            !body.to_string().contains("\"ttl\""),
            "default config must keep the compat wire free of ttl keys: {body}"
        );
        let msgs = body["messages"].as_array().expect("messages array");
        assert_eq!(msgs[0]["content"][0]["cache_control"]["type"], "ephemeral");
        assert_eq!(msgs[3]["content"][0]["cache_control"]["type"], "ephemeral");
    }

    /// D3: `cache_ttl` without `cache_passthrough` is inert — the structured
    /// `chat` path hits the `!cache_passthrough` early return in
    /// `apply_cache_breakpoints`, so the body is byte-identical to the
    /// flag-off structured wire and a staged value waiting for a passthrough
    /// flip changes nothing on the wire. Removing that early return fails
    /// this test (breakpoints would appear in the body).
    #[tokio::test]
    async fn cache_ttl_one_hour_without_passthrough_is_inert() {
        let (provider, captured, server) =
            mock_cache_capture_with_ttl(false, Some(CacheTtl::OneHour)).await;
        let messages = vec![
            ChatMessage::system("be brief"),
            ChatMessage::user("first question"),
            ChatMessage::assistant("first answer"),
            ChatMessage::user("second question"),
        ];
        let result = provider
            .chat(
                ProviderChatRequest {
                    messages: &messages,
                    tools: None,
                    thinking: None,
                },
                "test-model",
                None,
            )
            .await;
        server.abort();
        result.unwrap_or_else(|error| panic!("inert request failed: {error}"));

        let requests = captured.lock().unwrap();
        assert_eq!(
            requests[0],
            serde_json::json!({
                "model": "test-model",
                "messages": [
                    {"role": "system", "content": "be brief"},
                    {"role": "user", "content": "first question"},
                    {"role": "assistant", "content": "first answer"},
                    {"role": "user", "content": "second question"},
                ],
                "stream": false,
            }),
            "cache_ttl without passthrough must leave the structured body identical to flag-off"
        );
    }

    /// Streaming capture mock: records every request body, answers with a
    /// minimal SSE stream. Returns the provider built with or without the
    /// cache flag so tests can pin both wire paths.
    async fn mock_streaming_cache_capture(
        cache_passthrough: bool,
    ) -> (
        OpenAiCompatibleModelProvider,
        std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
        tokio::task::JoinHandle<()>,
    ) {
        use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing::post};
        use tokio::net::TcpListener;

        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured_for_route = std::sync::Arc::clone(&captured);
        let app = Router::new().route(
            "/chat/completions",
            post(move |Json(body): Json<serde_json::Value>| {
                let captured = std::sync::Arc::clone(&captured_for_route);
                async move {
                    let streaming =
                        body.get("stream").and_then(serde_json::Value::as_bool) == Some(true);
                    captured.lock().unwrap().push(body);
                    if streaming {
                        return (
                            StatusCode::OK,
                            [("content-type", "text/event-stream")],
                            concat!(
                                "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n",
                                "data: [DONE]\n\n"
                            ),
                        )
                            .into_response();
                    }
                    Json(serde_json::json!({
                        "choices": [{"message": {"content": "ok"}}]
                    }))
                    .into_response()
                }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = ::clawcrew_spawn::spawn!(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let mut builder = OpenAiCompatibleModelProvider::builder("test")
            .display_name("custom")
            .base_url(&format!("http://{addr}"))
            .auth_style(AuthStyle::Bearer);
        if cache_passthrough {
            builder = builder.with_cache_passthrough();
        }
        let provider = builder.build();

        (provider, captured, server)
    }

    /// D3 wire shape: behind the flag, the system prompt converts from
    /// string content to the single-text-block form carrying
    /// `cache_control`, while every other message keeps its existing
    /// serialization untouched.
    #[tokio::test]
    async fn cache_passthrough_system_breakpoint_serializes_block_form() {
        let (provider, captured, server) = mock_streaming_cache_capture(true).await;
        let messages = vec![ChatMessage::system("be brief"), ChatMessage::user("hello")];
        let result = provider
            .chat(
                ProviderChatRequest {
                    messages: &messages,
                    tools: None,
                    thinking: None,
                },
                "test-model",
                None,
            )
            .await;
        server.abort();
        result.unwrap_or_else(|error| panic!("flag-on request failed: {error}"));
        let requests = captured.lock().unwrap();
        assert_eq!(
            requests[0],
            serde_json::json!({
                "model": "test-model",
                "messages": [
                    {"role": "system", "content": [
                        {"type": "text", "text": "be brief",
                         "cache_control": {"type": "ephemeral"}},
                    ]},
                    {"role": "user", "content": "hello"},
                ],
                "stream": false,
            }),
            "system prompt must serialize as a cache_control text block; other messages untouched"
        );
        assert_eq!(
            requests[0].to_string().matches("cache_control").count(),
            1,
            "single exchange must carry exactly one breakpoint"
        );
    }

    /// Rolling breakpoint: once the conversation has more than one
    /// non-system message (the native provider's gate), the last message
    /// gains the second breakpoint and converts to block form; messages in
    /// between serialize exactly as before.
    #[tokio::test]
    async fn cache_passthrough_rolling_breakpoint_after_first_exchange() {
        let (provider, captured, server) = mock_streaming_cache_capture(true).await;
        let messages = vec![
            ChatMessage::system("you are brief"),
            ChatMessage::user("hi"),
            ChatMessage::assistant("hello"),
            ChatMessage::user("bye"),
        ];
        let result = provider
            .chat(
                ProviderChatRequest {
                    messages: &messages,
                    tools: None,
                    thinking: None,
                },
                "test-model",
                None,
            )
            .await;
        server.abort();
        result.unwrap_or_else(|error| panic!("flag-on history request failed: {error}"));
        let requests = captured.lock().unwrap();
        assert_eq!(
            requests[0]["messages"],
            serde_json::json!([
                {"role": "system", "content": [
                    {"type": "text", "text": "you are brief",
                     "cache_control": {"type": "ephemeral"}},
                ]},
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": "hello"},
                {"role": "user", "content": [
                    {"type": "text", "text": "bye",
                     "cache_control": {"type": "ephemeral"}},
                ]},
            ]),
            "system and last message carry the breakpoints; middle messages untouched"
        );
        assert_eq!(
            requests[0].to_string().matches("cache_control").count(),
            2,
            "rolling breakpoint must never exceed two per request"
        );
    }

    /// Writes a minimal real PNG (a 1x1 transparent pixel) into a fresh
    /// tempdir and returns the dir plus the file path. The dir must outlive
    /// the request so the multimodal prepare pass can inline the file, and
    /// tests build the image marker at runtime from the path so this
    /// source never carries a literal marker.
    fn write_minimal_png() -> (tempfile::TempDir, std::path::PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let png: [u8; 67] = [
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        let image_path = temp.path().join("pixel.png");
        std::fs::write(&image_path, png).unwrap();
        (temp, image_path)
    }

    /// Wire form of [`write_minimal_png`]'s file once the multimodal
    /// prepare pass inlines it: MIME detected from the PNG signature,
    /// standard padded base64.
    const MINIMAL_PNG_DATA_URI: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAACklEQVR4nGMAAQAABQABDQottAAAAABJRU5ErkJggg==";

    /// Rolling breakpoint on an image-ending turn: the last user message
    /// serializes as [text, image] parts, and the breakpoint must land on
    /// that text part instead of vanishing because the final part is an
    /// image. The image part stays unmarked; the following turn's rolling
    /// breakpoint covers it.
    #[tokio::test]
    async fn cache_passthrough_rolling_breakpoint_lands_on_text_before_trailing_image() {
        let (provider, captured, server) = mock_streaming_cache_capture(true).await;
        let (_temp, image_path) = write_minimal_png();
        let messages = vec![
            ChatMessage::system("you are brief"),
            ChatMessage::user("hi"),
            ChatMessage::assistant("hello"),
            ChatMessage::user(format!(
                "look at this [{}:{}]",
                "IMAGE",
                image_path.display()
            )),
        ];
        let result = provider
            .chat(
                ProviderChatRequest {
                    messages: &messages,
                    tools: None,
                    thinking: None,
                },
                "test-model",
                None,
            )
            .await;
        server.abort();
        result.unwrap_or_else(|error| panic!("flag-on image-turn request failed: {error}"));
        let requests = captured.lock().unwrap();
        assert_eq!(
            requests[0]["messages"],
            serde_json::json!([
                {"role": "system", "content": [
                    {"type": "text", "text": "you are brief",
                     "cache_control": {"type": "ephemeral"}},
                ]},
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": "hello"},
                {"role": "user", "content": [
                    {"type": "text", "text": "look at this",
                     "cache_control": {"type": "ephemeral"}},
                    {"type": "image_url", "image_url": {"url": MINIMAL_PNG_DATA_URI}},
                ]},
            ]),
            "system and the text part ahead of the trailing image carry the breakpoints; middle messages untouched"
        );
        assert_eq!(
            requests[0].to_string().matches("cache_control").count(),
            2,
            "an image-ending turn must still carry exactly two breakpoints"
        );
    }

    /// Streaming twin of the image-turn pin: the streaming path must place
    /// the rolling breakpoint on the same text part when the final user
    /// message ends with an image.
    #[tokio::test]
    async fn cache_passthrough_streaming_rolling_breakpoint_lands_on_text_before_trailing_image() {
        use futures_util::StreamExt as _;

        let (provider, captured, server) = mock_streaming_cache_capture(true).await;
        let (_temp, image_path) = write_minimal_png();
        let messages = vec![
            ChatMessage::system("you are brief"),
            ChatMessage::user("hi"),
            ChatMessage::assistant("hello"),
            ChatMessage::user(format!(
                "look at this [{}:{}]",
                "IMAGE",
                image_path.display()
            )),
        ];
        let events = provider
            .stream_chat(
                ProviderChatRequest {
                    messages: &messages,
                    tools: None,
                    thinking: None,
                },
                "test-model",
                None,
                StreamOptions {
                    enabled: true,
                    count_tokens: false,
                },
            )
            .collect::<Vec<_>>()
            .await;
        server.abort();
        assert!(
            events.iter().all(Result::is_ok),
            "streaming image-turn request must succeed: {events:?}"
        );
        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0]["messages"],
            serde_json::json!([
                {"role": "system", "content": [
                    {"type": "text", "text": "you are brief",
                     "cache_control": {"type": "ephemeral"}},
                ]},
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": "hello"},
                {"role": "user", "content": [
                    {"type": "text", "text": "look at this",
                     "cache_control": {"type": "ephemeral"}},
                    {"type": "image_url", "image_url": {"url": MINIMAL_PNG_DATA_URI}},
                ]},
            ]),
            "streaming path must inject the image-turn breakpoints identically"
        );
        assert_eq!(
            requests[0].to_string().matches("cache_control").count(),
            2,
            "streaming image-ending turn must carry exactly two breakpoints"
        );
    }

    /// Image-only turn: a message whose content is just the image carries
    /// no text part at all, so it cannot host the rolling breakpoint. The
    /// breakpoint rolls back onto the nearest earlier non-system message
    /// with text, the image part stays unmarked, and the two-breakpoint
    /// ceiling holds.
    #[tokio::test]
    async fn cache_passthrough_rolling_breakpoint_falls_back_when_last_message_is_image_only() {
        let (provider, captured, server) = mock_streaming_cache_capture(true).await;
        let (_temp, image_path) = write_minimal_png();
        let messages = vec![
            ChatMessage::system("you are brief"),
            ChatMessage::user("hi"),
            ChatMessage::assistant("hello"),
            ChatMessage::user(format!("[{}:{}]", "IMAGE", image_path.display())),
        ];
        let result = provider
            .chat(
                ProviderChatRequest {
                    messages: &messages,
                    tools: None,
                    thinking: None,
                },
                "test-model",
                None,
            )
            .await;
        server.abort();
        result.unwrap_or_else(|error| panic!("flag-on image-only request failed: {error}"));
        let requests = captured.lock().unwrap();
        assert_eq!(
            requests[0]["messages"],
            serde_json::json!([
                {"role": "system", "content": [
                    {"type": "text", "text": "you are brief",
                     "cache_control": {"type": "ephemeral"}},
                ]},
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": [
                    {"type": "text", "text": "hello",
                     "cache_control": {"type": "ephemeral"}},
                ]},
                {"role": "user", "content": [
                    {"type": "image_url", "image_url": {"url": MINIMAL_PNG_DATA_URI}},
                ]},
            ]),
            "image-only turn rolls the breakpoint onto the nearest earlier non-system message; the image part stays unmarked"
        );
        assert_eq!(
            requests[0].to_string().matches("cache_control").count(),
            2,
            "image-only fallback must still carry exactly two breakpoints"
        );
    }

    /// Unit pin for the fallback walk on hand-built messages, so the
    /// invariant holds regardless of how a history was constructed: an
    /// image-only last message rolls the rolling breakpoint back onto the
    /// nearest earlier non-system message with text, a trailing system
    /// message never absorbs the slot or gains a second mark, and the
    /// two-breakpoint ceiling holds.
    #[test]
    fn cache_passthrough_breakpoint_walk_handles_hand_built_image_only_messages() {
        let provider = make_cache_passthrough_model_provider("custom", "http://127.0.0.1:1");
        let mut messages = vec![
            Message {
                role: "system".to_string(),
                content: MessageContent::Text("you are brief".to_string()),
            },
            Message {
                role: "user".to_string(),
                content: MessageContent::Text("hi".to_string()),
            },
            Message {
                role: "assistant".to_string(),
                content: MessageContent::Text("hello".to_string()),
            },
            Message {
                role: "user".to_string(),
                content: MessageContent::Parts(vec![MessagePart::ImageUrl {
                    image_url: ImageUrlPart {
                        url: "data:image/png;base64,abcd".to_string(),
                    },
                }]),
            },
            Message {
                role: "system".to_string(),
                content: MessageContent::Text("extra".to_string()),
            },
        ];
        provider.apply_cache_breakpoints(&mut messages, None);
        let value = serde_json::to_value(&messages).unwrap();
        assert_eq!(
            value,
            serde_json::json!([
                {"role": "system", "content": [
                    {"type": "text", "text": "you are brief",
                     "cache_control": {"type": "ephemeral"}},
                ]},
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": [
                    {"type": "text", "text": "hello",
                     "cache_control": {"type": "ephemeral"}},
                ]},
                {"role": "user", "content": [
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,abcd"}},
                ]},
                {"role": "system", "content": "extra"},
            ]),
            "image-only turn rolls the breakpoint onto the nearest earlier non-system message; system messages never take the rolling slot"
        );
        assert_eq!(
            value.to_string().matches("cache_control").count(),
            2,
            "hand-built fallback must keep the two-breakpoint ceiling"
        );
    }

    /// Flag-off twin for the multimodal shape: with the flag off, the
    /// image-ending history serializes byte-identically to the pre-feature
    /// wire, with plain string content where the flag-on path would mark,
    /// the same unmarked image parts, and no cache markers anywhere in the
    /// body.
    #[tokio::test]
    async fn cache_passthrough_flag_off_image_turn_body_unmarked() {
        let (provider, captured, server) = mock_streaming_cache_capture(false).await;
        let (_temp, image_path) = write_minimal_png();
        let messages = vec![
            ChatMessage::system("you are brief"),
            ChatMessage::user("hi"),
            ChatMessage::assistant("hello"),
            ChatMessage::user(format!(
                "look at this [{}:{}]",
                "IMAGE",
                image_path.display()
            )),
        ];
        let result = provider
            .chat(
                ProviderChatRequest {
                    messages: &messages,
                    tools: None,
                    thinking: None,
                },
                "test-model",
                None,
            )
            .await;
        server.abort();
        result.unwrap_or_else(|error| panic!("flag-off image-turn request failed: {error}"));
        let requests = captured.lock().unwrap();
        assert_eq!(
            requests[0]["messages"],
            serde_json::json!([
                {"role": "system", "content": "you are brief"},
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": "hello"},
                {"role": "user", "content": [
                    {"type": "text", "text": "look at this"},
                    {"type": "image_url", "image_url": {"url": MINIMAL_PNG_DATA_URI}},
                ]},
            ]),
            "flag-off multimodal body must stay byte-identical: plain strings and unmarked image parts"
        );
        assert!(
            !requests[0].to_string().contains("cache_control"),
            "flag-off image-ending turn must carry no cache markers"
        );
    }

    /// The rolling breakpoint follows the native provider's gate: a
    /// conversation with only one non-system message gets the system
    /// breakpoint alone.
    #[tokio::test]
    async fn cache_passthrough_single_exchange_skips_rolling_breakpoint() {
        let (provider, captured, server) = mock_streaming_cache_capture(true).await;
        let messages = vec![
            ChatMessage::system("you are brief"),
            ChatMessage::user("hi"),
        ];
        let result = provider
            .chat(
                ProviderChatRequest {
                    messages: &messages,
                    tools: None,
                    thinking: None,
                },
                "test-model",
                None,
            )
            .await;
        server.abort();
        result.unwrap_or_else(|error| panic!("flag-on history request failed: {error}"));
        let requests = captured.lock().unwrap();
        assert_eq!(
            requests[0].to_string().matches("cache_control").count(),
            1,
            "one non-system message means no rolling breakpoint"
        );
        assert_eq!(
            requests[0]["messages"][1]["content"],
            serde_json::json!("hi"),
            "first user message must stay a plain string"
        );
    }

    /// Streaming twin of the system-breakpoint pin: the injection must be
    /// identical on the streaming path (a previous feature shipped the
    /// non-streaming path and silently missed streaming).
    #[tokio::test]
    async fn cache_passthrough_streaming_system_breakpoint_matches_non_streaming() {
        use futures_util::StreamExt as _;

        let (provider, captured, server) = mock_streaming_cache_capture(true).await;
        let events = provider
            .stream_chat(
                ProviderChatRequest {
                    messages: &[ChatMessage::system("be brief"), ChatMessage::user("hello")],
                    tools: None,
                    thinking: None,
                },
                "test-model",
                None,
                StreamOptions {
                    enabled: true,
                    count_tokens: false,
                },
            )
            .collect::<Vec<_>>()
            .await;
        server.abort();
        assert!(
            events.iter().all(Result::is_ok),
            "streaming request must succeed: {events:?}"
        );
        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0]["messages"][0],
            serde_json::json!({
                "role": "system",
                "content": [
                    {"type": "text", "text": "be brief",
                     "cache_control": {"type": "ephemeral"}},
                ],
            }),
            "streaming path must inject the system breakpoint identically"
        );
        assert_eq!(
            requests[0]["messages"][1]["content"],
            serde_json::json!("hello"),
            "streaming user message must stay a plain string"
        );
    }

    /// Flag-off streaming twin: the default path stays byte-identical on
    /// the streaming wire too.
    #[tokio::test]
    async fn cache_passthrough_flag_off_streaming_body_unmarked() {
        use futures_util::StreamExt as _;

        let (provider, captured, server) = mock_streaming_cache_capture(false).await;
        let events = provider
            .stream_chat(
                ProviderChatRequest {
                    messages: &[ChatMessage::system("be brief"), ChatMessage::user("hello")],
                    tools: None,
                    thinking: None,
                },
                "test-model",
                None,
                StreamOptions {
                    enabled: true,
                    count_tokens: false,
                },
            )
            .collect::<Vec<_>>()
            .await;
        server.abort();
        assert!(events.iter().all(Result::is_ok));
        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0]["messages"][0]["content"],
            serde_json::json!("be brief"),
            "flag-off system message must stay a plain string"
        );
        assert!(
            !requests[0].to_string().contains("cache_control"),
            "flag-off streaming body must carry no cache markers"
        );
    }

    /// Native-tools path: the same two-breakpoint placement applies to the
    /// native-tools request builder, so tool-bearing agent loops get the
    /// same caching shape as plain conversations.
    #[tokio::test]
    async fn cache_passthrough_tools_path_rolls_breakpoint_onto_last_message() {
        let (provider, captured, server) = mock_streaming_cache_capture(true).await;
        let messages = vec![
            ChatMessage::system("you are brief"),
            ChatMessage::user("hi"),
            ChatMessage::assistant("hello"),
            ChatMessage::user("bye"),
        ];
        let tools = vec![serde_json::json!({
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "Get weather",
                "parameters": {"type": "object", "properties": {}}
            }
        })];
        let result = provider
            .chat_with_tools(&messages, &tools, "test-model", None)
            .await;
        server.abort();
        result.unwrap_or_else(|error| panic!("flag-on tools request failed: {error}"));
        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].to_string().matches("cache_control").count(),
            2,
            "tools path must carry at most the two standard breakpoints"
        );
        assert_eq!(
            requests[0]["messages"][0]["content"][0]["cache_control"],
            serde_json::json!({"type": "ephemeral"}),
            "system breakpoint present on the tools path"
        );
        assert_eq!(
            requests[0]["messages"].as_array().unwrap().last().unwrap()["content"][0]["cache_control"],
            serde_json::json!({"type": "ephemeral"}),
            "rolling breakpoint lands on the last message of the tools path"
        );
    }

    fn non_streaming_response_cases() -> Vec<(&'static str, serde_json::Value, Option<&'static str>)>
    {
        vec![
            (
                "direct",
                serde_json::json!({
                    "choices": [{"message": {"content": "direct"}}]
                }),
                Some("direct"),
            ),
            (
                "wrapped",
                serde_json::json!({
                    "data": {
                        "choices": [{"message": {"content": "wrapped"}}]
                    },
                    "success": true
                }),
                Some("wrapped"),
            ),
            (
                "direct_precedence",
                serde_json::json!({
                    "choices": [{"message": {"content": "top-level"}}],
                    "data": {
                        "choices": [{"message": {"content": "nested"}}]
                    }
                }),
                Some("top-level"),
            ),
            (
                "malformed",
                serde_json::json!({
                    "data": {
                        "choices": "invalid",
                        "api_key": "sk-test-secret-value"
                    }
                }),
                None,
            ),
        ]
    }

    fn assert_sanitized_envelope_error(error: &anyhow::Error, case: &str) {
        let message = error.to_string();
        assert!(
            message.contains("custom API returned an unexpected chat-completions payload"),
            "{case}: unexpected error: {message}"
        );
        assert!(
            message.contains("[REDACTED]"),
            "{case}: sanitized body missing redaction: {message}"
        );
        assert!(
            !message.contains("sk-test-secret-value"),
            "{case}: secret leaked in error: {message}"
        );
    }

    #[test]
    fn convert_tool_specs_serializes_openai_wire_shape() {
        let p = make_model_provider("vllm", "http://localhost:8000/v1", None);
        // Clean schema (shared as-is) and dirty schema (rewritten by the
        // OpenAI strategy's strategy-independent passes).
        let tools = vec![
            clawcrew_api::tool::ToolSpec::new(
                "get_weather",
                "Fetch the weather",
                serde_json::json!({
                    "type": "object",
                    "properties": { "city": { "type": "string" } }
                }),
            ),
            clawcrew_api::tool::ToolSpec::new(
                "set_mode",
                "Set the mode",
                serde_json::json!({
                    "type": "object",
                    "properties": { "mode": { "const": "fast" } }
                }),
            ),
        ];

        let converted = p.convert_tool_specs(Some(&tools)).expect("Some(tools) in");
        let raw = serde_json::to_string(&converted).unwrap();

        // Raw string, not `serde_json::Value` equality: `Value` object
        // equality ignores key order, so it cannot pin the declared
        // key-order delta (typed structs serialize `type`/`function`, and
        // `name`/`description`/`parameters` within it, in field-declaration
        // order; the `parameters` schema itself is a plain `Value` with no
        // `preserve_order` feature enabled, so its own keys always come out
        // alphabetical regardless of insertion order, e.g. `properties`
        // before `type`). `const` is also rewritten to a single-value
        // `enum` by the cleaner, exactly as the pre-typed-struct pipeline
        // did.
        assert_eq!(
            raw,
            concat!(
                r#"[{"type":"function","function":{"name":"get_weather","description":"Fetch the weather","parameters":{"properties":{"city":{"type":"string"}},"type":"object"}}},"#,
                r#"{"type":"function","function":{"name":"set_mode","description":"Set the mode","parameters":{"properties":{"mode":{"enum":["fast"]}},"type":"object"}}}]"#
            ),
            "typed tool specs must serialize to the same byte-for-byte wire \
             shape (including key order) as the previous json!-built payload"
        );
    }

    #[test]
    fn convert_tool_specs_shares_clean_schema_and_memoizes_dirty_schema() {
        let p = make_model_provider("vllm", "http://localhost:8000/v1", None);
        let tools = vec![
            clawcrew_api::tool::ToolSpec::new(
                "clean_tool",
                "already clean",
                serde_json::json!({
                    "type": "object",
                    "properties": { "path": { "type": "string" } }
                }),
            ),
            clawcrew_api::tool::ToolSpec::new(
                "dirty_tool",
                "needs cleaning",
                serde_json::json!({ "type": "string", "const": "x" }),
            ),
        ];

        let first = p.convert_tool_specs(Some(&tools)).unwrap();
        let second = p.convert_tool_specs(Some(&tools)).unwrap();

        assert!(
            std::sync::Arc::ptr_eq(&first[0].function.parameters, &tools[0].parameters),
            "clean schemas must be shared straight from the registry Arc"
        );
        assert!(
            std::sync::Arc::ptr_eq(
                &first[1].function.parameters,
                &second[1].function.parameters
            ),
            "dirty schemas must be cleaned once and memoized, not re-copied per request"
        );
    }

    #[test]
    fn streaming_native_tool_request_serializes_tools_and_guards_tool_choice() {
        let p = make_model_provider("vllm", "http://localhost:8000/v1", None);
        let messages = vec![ChatMessage::user("hello")];
        let tools = vec![clawcrew_api::tool::ToolSpec::new(
            "get_weather",
            "Fetch the weather",
            serde_json::json!({ "type": "object", "properties": {} }),
        )];
        let converted = p.convert_tool_specs_for_model(Some(&tools), "test-model");

        let value = serde_json::to_value(p.build_streaming_native_tool_request(
            "test-model",
            &messages,
            converted,
            Some(0.5),
            true,
            false,
            false,
        ))
        .unwrap();

        assert_eq!(value["stream"], serde_json::json!(true));
        assert_eq!(
            value["stream_options"]["include_usage"],
            serde_json::json!(true)
        );
        assert_eq!(value["tool_choice"], serde_json::json!("auto"));
        assert_eq!(
            value["tools"],
            serde_json::json!([{
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Fetch the weather",
                    "parameters": { "type": "object", "properties": {} }
                }
            }]),
            "streaming payload must carry the typed tools in OpenAI wire shape"
        );

        // Converted-empty tools must omit tool_choice (vLLM 0.19+ rejects
        // tool_choice without a tools field).
        let empty = serde_json::to_value(p.build_streaming_native_tool_request(
            "test-model",
            &messages,
            Some(vec![]),
            None,
            true,
            false,
            false,
        ))
        .unwrap();
        assert!(
            empty.get("tool_choice").is_none(),
            "empty converted tools must not set tool_choice; got: {empty}"
        );
    }

    #[test]
    fn provider_clones_share_one_schema_memo() {
        // stream_chat clones the provider per call and relies on the
        // Arc<SchemaCleanCache> field so the clone shares the instance memo;
        // a rebuild-per-call refactor would silently reintroduce per-request
        // cold-cache cleaning on the streaming path with identical wire
        // bytes, so pin the sharing directly.
        let p = make_model_provider("vllm", "http://localhost:8000/v1", None);
        let tools = vec![clawcrew_api::tool::ToolSpec::new(
            "dirty_tool",
            "needs cleaning",
            serde_json::json!({ "type": "string", "const": "x" }),
        )];

        let original = p.convert_tool_specs(Some(&tools)).unwrap();
        let via_clone = p.clone().convert_tool_specs(Some(&tools)).unwrap();

        assert!(
            std::sync::Arc::ptr_eq(
                &original[0].function.parameters,
                &via_clone[0].function.parameters
            ),
            "provider clones must serve dirty schemas from the same memo"
        );
    }

    #[test]
    fn creates_with_key() {
        let p = make_model_provider(
            "venice",
            "https://api.venice.ai",
            Some("venice-test-credential"),
        );
        assert_eq!(p.name, "venice");
        assert_eq!(p.base_url, "https://api.venice.ai");
        assert_eq!(p.credential.as_deref(), Some("venice-test-credential"));
    }

    #[test]
    fn creates_without_key() {
        let p = make_model_provider("test", "https://example.com", None);
        assert!(p.credential.is_none());
    }

    // Regression: vLLM 0.19+ and spec-compliant validators reject
    // `tool_choice` when `tools` is absent or empty (HTTP 400:
    // "When using `tool_choice`, `tools` must be set."). The request builders
    // must omit `tool_choice` whenever the converted tool list is empty.
    #[test]
    fn build_native_tool_chat_request_omits_tool_choice_when_no_tools() {
        let p = make_model_provider("vllm", "http://localhost:8000/v1", None);
        let messages = vec![ChatMessage::user("hello")];

        // Assert on the structured value rather than substring-matching the
        // serialized string: a JSON-shape or escaping change could otherwise
        // flip these assertions silently. Inspect the `tool_choice` key
        // directly.

        // None tools → no tool_choice key.
        let req =
            p.build_native_tool_chat_request(&messages, None, "test-model", None, false, false);
        let value = serde_json::to_value(&req).unwrap();
        assert!(
            value.get("tool_choice").is_none(),
            "tool_choice must be omitted when tools is None; got: {value}"
        );

        // Empty tools vec → still no tool_choice key.
        let req_empty = p.build_native_tool_chat_request(
            &messages,
            Some(vec![]),
            "test-model",
            None,
            false,
            false,
        );
        let value_empty = serde_json::to_value(&req_empty).unwrap();
        assert!(
            value_empty.get("tool_choice").is_none(),
            "tool_choice must be omitted when tools is empty; got: {value_empty}"
        );
    }

    #[test]
    fn build_native_tool_chat_request_sets_tool_choice_when_tools_present() {
        let p = make_model_provider("vllm", "http://localhost:8000/v1", None);
        let messages = vec![ChatMessage::user("hello")];
        let tools = vec![NativeToolSpec {
            kind: "function".to_string(),
            extra: serde_json::Map::new(),
            function: NativeToolFunctionSpec {
                extra: serde_json::Map::new(),
                name: "get_weather".to_string(),
                description: String::new(),
                parameters: std::sync::Arc::new(serde_json::json!({})),
            },
        }];
        let req = p.build_native_tool_chat_request(
            &messages,
            Some(tools),
            "test-model",
            None,
            false,
            false,
        );
        let value = serde_json::to_value(&req).unwrap();
        assert_eq!(
            value.get("tool_choice").and_then(serde_json::Value::as_str),
            Some("auto"),
            "tool_choice must be 'auto' when tools are present; got: {value}"
        );
    }

    // A compatible endpoint may support tools and reasoning together. Preserve
    // the operator-selected effort on the first request; the bounded HTTP
    // fallback handles only endpoints that explicitly reject the combination.
    #[test]
    fn build_native_tool_chat_request_keeps_reasoning_effort_when_tools_present() {
        let p = OpenAiCompatibleModelProvider::builder("test")
            .display_name("test")
            .base_url("https://example.com")
            .credential(None)
            .auth_style(AuthStyle::Bearer)
            .reasoning_effort(Some("high".to_string()))
            .build();
        let messages = vec![ChatMessage::user("hello")];
        let tools = vec![NativeToolSpec {
            kind: "function".to_string(),
            extra: serde_json::Map::new(),
            function: NativeToolFunctionSpec {
                extra: serde_json::Map::new(),
                name: "get_weather".to_string(),
                description: String::new(),
                parameters: std::sync::Arc::new(serde_json::json!({})),
            },
        }];

        let req =
            p.build_native_tool_chat_request(&messages, Some(tools), "gpt-5", None, false, false);
        let value = serde_json::to_value(&req).unwrap();
        assert_eq!(
            value
                .get("reasoning_effort")
                .and_then(serde_json::Value::as_str),
            Some("high"),
            "capable endpoints must receive reasoning_effort with tools on the first request; got: {value}"
        );
    }

    // Regression guard: the no-tools path must keep sending reasoning_effort
    // for models that qualify, so the tool-bearing fix above doesn't
    // regress the common case.
    #[test]
    fn build_native_tool_chat_request_keeps_reasoning_effort_when_no_tools() {
        let p = OpenAiCompatibleModelProvider::builder("test")
            .display_name("test")
            .base_url("https://example.com")
            .credential(None)
            .auth_style(AuthStyle::Bearer)
            .reasoning_effort(Some("high".to_string()))
            .build();
        let messages = vec![ChatMessage::user("hello")];

        let req = p.build_native_tool_chat_request(&messages, None, "gpt-5", None, false, false);
        let value = serde_json::to_value(&req).unwrap();
        assert_eq!(
            value
                .get("reasoning_effort")
                .and_then(serde_json::Value::as_str),
            Some("high"),
            "reasoning_effort must be present when no tools are sent; got: {value}"
        );
    }

    /// Spawn a mock `/chat/completions` endpoint that rejects any request
    /// carrying tools unless `reasoning_effort` is explicitly `"none"`.
    ///
    /// This mirrors endpoints that refuse function tools combined with any
    /// effective reasoning effort: because an omitted `reasoning_effort`
    /// defaults to a non-`none` effort server-side, an absent field is
    /// rejected just like `"high"`. Returns the bound address, the recorded
    /// request bodies, and the server task handle.
    ///
    /// `stream` selects an SSE success body instead of a JSON one.
    fn spawn_reasoning_rejecting_endpoint(
        stream: bool,
    ) -> impl std::future::Future<
        Output = (
            std::net::SocketAddr,
            std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
            ::tokio::task::JoinHandle<()>,
        ),
    > {
        use axum::Json;
        use axum::Router;
        use axum::http::StatusCode;
        use axum::response::IntoResponse;
        use axum::routing::post;
        use std::sync::{Arc, Mutex};
        use tokio::net::TcpListener;

        async move {
            let bodies = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
            let bodies_for_route = Arc::clone(&bodies);
            let app = Router::new().route(
                "/chat/completions",
                post(move |Json(body): Json<serde_json::Value>| {
                    let bodies = Arc::clone(&bodies_for_route);
                    async move {
                        let rejects = body.get("tools").is_some()
                            && body
                                .get("reasoning_effort")
                                .and_then(serde_json::Value::as_str)
                                != Some("none");
                        bodies.lock().unwrap().push(body);
                        if rejects {
                            return (
                                StatusCode::BAD_REQUEST,
                                Json(serde_json::json!({
                                    "error": {
                                        "message": "Function tools with reasoning effort are not supported",
                                        "param": "reasoning_effort"
                                    }
                                })),
                            )
                                .into_response();
                        }
                        if stream {
                            return (
                                StatusCode::OK,
                                [("content-type", "text/event-stream")],
                                concat!(
                                    "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n",
                                    "data: [DONE]\n\n"
                                ),
                            )
                                .into_response();
                        }
                        Json(serde_json::json!({
                            "choices": [{"message": {"content": "ok"}}]
                        }))
                        .into_response()
                    }
                }),
            );
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let server = ::clawcrew_spawn::spawn!(async move {
                axum::serve(listener, app).await.unwrap();
            });
            (addr, bodies, server)
        }
    }

    #[tokio::test]
    async fn capable_endpoint_keeps_reasoning_effort_with_tools_without_retry() {
        use axum::Json;
        use axum::Router;
        use axum::routing::post;
        use std::sync::{Arc, Mutex};
        use tokio::net::TcpListener;

        let bodies = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
        let bodies_for_route = Arc::clone(&bodies);
        let app = Router::new().route(
            "/chat/completions",
            post(move |Json(body): Json<serde_json::Value>| {
                let bodies = Arc::clone(&bodies_for_route);
                async move {
                    bodies.lock().unwrap().push(body);
                    Json(serde_json::json!({
                        "choices": [{"message": {"content": "ok"}}]
                    }))
                }
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = ::clawcrew_spawn::spawn!(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let provider = OpenAiCompatibleModelProvider::builder("test")
            .display_name("test")
            .base_url(&format!("http://{addr}"))
            .credential(None)
            .auth_style(AuthStyle::Bearer)
            .reasoning_effort(Some("high".to_string()))
            .build();
        let messages = vec![ChatMessage::user("hello")];
        let tools = vec![clawcrew_api::tool::ToolSpec::new(
            "get_weather",
            "Get weather",
            serde_json::json!({"type": "object", "properties": {}}),
        )];

        let response = provider
            .chat(
                crate::traits::ChatRequest {
                    messages: &messages,
                    tools: Some(&tools),
                    thinking: None,
                },
                "gpt-5",
                None,
            )
            .await
            .unwrap();
        assert_eq!(response.text.as_deref(), Some("ok"));

        let bodies = bodies.lock().unwrap();
        assert_eq!(bodies.len(), 1, "capable endpoint must not be retried");
        assert_eq!(
            bodies[0]
                .get("reasoning_effort")
                .and_then(serde_json::Value::as_str),
            Some("high")
        );
        assert!(bodies[0].get("tools").is_some());
        server.abort();
    }

    #[tokio::test]
    async fn rejecting_endpoint_retries_once_with_reasoning_disabled() {
        let (addr, bodies, server) = spawn_reasoning_rejecting_endpoint(false).await;

        let provider = OpenAiCompatibleModelProvider::builder("test")
            .display_name("test")
            .base_url(&format!("http://{addr}"))
            .credential(None)
            .auth_style(AuthStyle::Bearer)
            .reasoning_effort(Some("high".to_string()))
            .extra_body(serde_json::json!({"reasoning_effort": "xhigh"}))
            .build();
        let messages = vec![ChatMessage::user("hello")];
        let tools = vec![clawcrew_api::tool::ToolSpec::new(
            "get_weather",
            "Get weather",
            serde_json::json!({"type": "object", "properties": {}}),
        )];

        let response = provider
            .chat(
                crate::traits::ChatRequest {
                    messages: &messages,
                    tools: Some(&tools),
                    thinking: None,
                },
                "gpt-5",
                None,
            )
            .await
            .unwrap();
        assert_eq!(response.text.as_deref(), Some("ok"));

        let bodies = bodies.lock().unwrap();
        assert_eq!(bodies.len(), 2, "fallback must be bounded to one retry");
        assert_eq!(
            bodies[0]
                .get("reasoning_effort")
                .and_then(serde_json::Value::as_str),
            Some("xhigh"),
            "provider extra body must be reflected in the canonical wire payload"
        );
        assert_eq!(
            bodies[1]
                .get("reasoning_effort")
                .and_then(serde_json::Value::as_str),
            Some("none"),
            "retry must explicitly disable reasoning"
        );
        assert!(bodies.iter().all(|body| body.get("tools").is_some()));
        server.abort();
    }

    #[tokio::test]
    async fn streaming_rejection_retries_once_with_reasoning_disabled() {
        use futures_util::StreamExt as _;

        let (addr, bodies, server) = spawn_reasoning_rejecting_endpoint(true).await;

        let provider = OpenAiCompatibleModelProvider::builder("test")
            .display_name("test")
            .base_url(&format!("http://{addr}"))
            .credential(None)
            .auth_style(AuthStyle::Bearer)
            .reasoning_effort(Some("high".to_string()))
            .build();
        let messages = vec![ChatMessage::user("hello")];
        let tools = vec![clawcrew_api::tool::ToolSpec::new(
            "get_weather",
            "Get weather",
            serde_json::json!({"type": "object", "properties": {}}),
        )];

        let events = provider
            .stream_chat(
                crate::traits::ChatRequest {
                    messages: &messages,
                    tools: Some(&tools),
                    thinking: None,
                },
                "gpt-5",
                None,
                StreamOptions {
                    enabled: true,
                    count_tokens: false,
                },
            )
            .collect::<Vec<_>>()
            .await;
        assert!(events.iter().all(Result::is_ok));
        assert!(events.iter().any(|event| {
            matches!(
                event,
                Ok(StreamEvent::TextDelta(StreamChunk { delta, .. })) if delta == "ok"
            )
        }));

        let bodies = bodies.lock().unwrap();
        assert_eq!(bodies.len(), 2, "stream fallback must retry exactly once");
        assert_eq!(
            bodies[0]
                .get("reasoning_effort")
                .and_then(serde_json::Value::as_str),
            Some("high")
        );
        assert_eq!(
            bodies[1]
                .get("reasoning_effort")
                .and_then(serde_json::Value::as_str),
            Some("none")
        );
        assert!(bodies.iter().all(|body| body.get("tools").is_some()));
        server.abort();
    }

    #[tokio::test]
    async fn chat_with_tools_retries_once_with_reasoning_disabled() {
        let (addr, bodies, server) = spawn_reasoning_rejecting_endpoint(false).await;

        let provider = OpenAiCompatibleModelProvider::builder("test")
            .display_name("test")
            .base_url(&format!("http://{addr}"))
            .credential(None)
            .auth_style(AuthStyle::Bearer)
            .reasoning_effort(Some("high".to_string()))
            .build();
        let messages = vec![ChatMessage::user("hello")];
        let tools = vec![serde_json::json!({
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "Get weather",
                "parameters": {"type": "object", "properties": {}}
            }
        })];

        let response = provider
            .chat_with_tools(&messages, &tools, "gpt-5", None)
            .await
            .unwrap();
        assert_eq!(response.text.as_deref(), Some("ok"));

        let bodies = bodies.lock().unwrap();
        assert_eq!(bodies.len(), 2, "fallback must be bounded to one retry");
        assert_eq!(
            bodies[0]
                .get("reasoning_effort")
                .and_then(serde_json::Value::as_str),
            Some("high")
        );
        assert_eq!(
            bodies[1]
                .get("reasoning_effort")
                .and_then(serde_json::Value::as_str),
            Some("none")
        );
        assert!(bodies.iter().all(|body| body.get("tools").is_some()));
        server.abort();
    }

    #[tokio::test]
    async fn compatible_retries_once_inserting_reasoning_effort_none_when_unset() {
        // Regression: a model that receives no reasoning_effort at all (the
        // common case, since reasoning_effort_for_model returns None outside
        // GPT-5/o-series/codex) must still recover. An omitted field defaults
        // to a non-"none" effort server-side, so the retry has to *insert*
        // the explicit "none" rather than only overwrite an existing value.
        let (addr, bodies, server) = spawn_reasoning_rejecting_endpoint(false).await;

        let provider = OpenAiCompatibleModelProvider::builder("test")
            .display_name("test")
            .base_url(&format!("http://{addr}"))
            .credential(None)
            .auth_style(AuthStyle::Bearer)
            .reasoning_effort(None)
            .build();
        let messages = vec![ChatMessage::user("hello")];
        let tools = vec![clawcrew_api::tool::ToolSpec::new(
            "get_weather",
            "Get weather",
            serde_json::json!({"type": "object", "properties": {}}),
        )];

        let response = provider
            .chat(
                crate::traits::ChatRequest {
                    messages: &messages,
                    tools: Some(&tools),
                    thinking: None,
                },
                "gpt-5",
                None,
            )
            .await
            .unwrap();
        assert_eq!(response.text.as_deref(), Some("ok"));

        let bodies = bodies.lock().unwrap();
        assert_eq!(bodies.len(), 2, "fallback must be bounded to one retry");
        assert!(
            bodies[0].get("reasoning_effort").is_none(),
            "first request must omit reasoning_effort when none is configured; got: {}",
            bodies[0]
        );
        assert_eq!(
            bodies[1]
                .get("reasoning_effort")
                .and_then(serde_json::Value::as_str),
            Some("none"),
            "retry must insert an explicit reasoning_effort of none"
        );
        assert!(bodies.iter().all(|body| body.get("tools").is_some()));
        server.abort();
    }

    #[tokio::test]
    async fn compatible_streaming_retries_once_inserting_reasoning_effort_none_when_unset() {
        use futures_util::StreamExt as _;

        let (addr, bodies, server) = spawn_reasoning_rejecting_endpoint(true).await;

        let provider = OpenAiCompatibleModelProvider::builder("test")
            .display_name("test")
            .base_url(&format!("http://{addr}"))
            .credential(None)
            .auth_style(AuthStyle::Bearer)
            .reasoning_effort(None)
            .build();
        let messages = vec![ChatMessage::user("hello")];
        let tools = vec![clawcrew_api::tool::ToolSpec::new(
            "get_weather",
            "Get weather",
            serde_json::json!({"type": "object", "properties": {}}),
        )];

        let events = provider
            .stream_chat(
                crate::traits::ChatRequest {
                    messages: &messages,
                    tools: Some(&tools),
                    thinking: None,
                },
                "gpt-5",
                None,
                StreamOptions {
                    enabled: true,
                    count_tokens: false,
                },
            )
            .collect::<Vec<_>>()
            .await;
        assert!(events.iter().all(Result::is_ok));

        let bodies = bodies.lock().unwrap();
        assert_eq!(bodies.len(), 2, "stream fallback must retry exactly once");
        assert!(bodies[0].get("reasoning_effort").is_none());
        assert_eq!(
            bodies[1]
                .get("reasoning_effort")
                .and_then(serde_json::Value::as_str),
            Some("none")
        );
        server.abort();
    }

    #[tokio::test]
    async fn chat_with_tools_retries_once_inserting_reasoning_effort_none_when_unset() {
        let (addr, bodies, server) = spawn_reasoning_rejecting_endpoint(false).await;

        let provider = OpenAiCompatibleModelProvider::builder("test")
            .display_name("test")
            .base_url(&format!("http://{addr}"))
            .credential(None)
            .auth_style(AuthStyle::Bearer)
            .reasoning_effort(None)
            .build();
        let messages = vec![ChatMessage::user("hello")];
        let tools = vec![serde_json::json!({
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "Get weather",
                "parameters": {"type": "object", "properties": {}}
            }
        })];

        let response = provider
            .chat_with_tools(&messages, &tools, "gpt-5", None)
            .await
            .unwrap();
        assert_eq!(response.text.as_deref(), Some("ok"));

        let bodies = bodies.lock().unwrap();
        assert_eq!(bodies.len(), 2, "fallback must be bounded to one retry");
        assert!(bodies[0].get("reasoning_effort").is_none());
        assert_eq!(
            bodies[1]
                .get("reasoning_effort")
                .and_then(serde_json::Value::as_str),
            Some("none")
        );
        server.abort();
    }

    #[tokio::test]
    async fn bad_reasoning_value_naming_a_tool_model_does_not_retry() {
        // Interaction of both fixes: the endpoint reports a plain bad-value
        // error whose text happens to name a model containing "tool". The
        // classifier must not treat that as a tools conflict, so the
        // configured effort is preserved and the error propagates unchanged
        // after exactly one request.
        use axum::Json;
        use axum::Router;
        use axum::http::StatusCode;
        use axum::response::IntoResponse;
        use axum::routing::post;
        use std::sync::{Arc, Mutex};
        use tokio::net::TcpListener;

        let bodies = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
        let bodies_for_route = Arc::clone(&bodies);
        let app = Router::new().route(
            "/chat/completions",
            post(move |Json(body): Json<serde_json::Value>| {
                let bodies = Arc::clone(&bodies_for_route);
                async move {
                    bodies.lock().unwrap().push(body);
                    (
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({
                            "error": {
                                "message": "reasoning_effort value 'high' is unsupported for tool-model",
                                "param": "reasoning_effort"
                            }
                        })),
                    )
                        .into_response()
                }
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = ::clawcrew_spawn::spawn!(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let provider = OpenAiCompatibleModelProvider::builder("test")
            .display_name("test")
            .base_url(&format!("http://{addr}"))
            .credential(None)
            .auth_style(AuthStyle::Bearer)
            .reasoning_effort(Some("high".to_string()))
            .build();
        let messages = vec![ChatMessage::user("hello")];
        let tools = vec![clawcrew_api::tool::ToolSpec::new(
            "get_weather",
            "Get weather",
            serde_json::json!({"type": "object", "properties": {}}),
        )];

        let result = provider
            .chat(
                crate::traits::ChatRequest {
                    messages: &messages,
                    tools: Some(&tools),
                    thinking: None,
                },
                "gpt-5",
                None,
            )
            .await;
        assert!(result.is_err(), "bad-value errors must propagate unchanged");

        let bodies = bodies.lock().unwrap();
        assert_eq!(
            bodies.len(),
            1,
            "a bad-value error must not trigger the tools/reasoning fallback"
        );
        assert_eq!(
            bodies[0]
                .get("reasoning_effort")
                .and_then(serde_json::Value::as_str),
            Some("high"),
            "the operator-configured effort must not be downgraded"
        );
        server.abort();
    }

    #[test]
    fn ensure_reasoning_effort_none_normalizes_non_canonical_spellings() {
        // A case-sensitive endpoint can reject "NONE" supplied through
        // provider_extra, so a non-canonical spelling must be rewritten to the
        // exact lowercase value rather than counted as already repaired.
        for spelling in ["NONE", "None", "nOnE"] {
            let mut payload = serde_json::json!({ "reasoning_effort": spelling });
            assert!(
                ensure_reasoning_effort_none(&mut payload),
                "{spelling} must be normalized to the canonical value"
            );
            assert_eq!(
                payload.get("reasoning_effort"),
                Some(&serde_json::Value::String("none".to_string()))
            );
            // The rewritten payload is the fixed point, so the retry stays
            // bounded to a single additional request.
            assert!(!ensure_reasoning_effort_none(&mut payload));
        }

        let mut canonical = serde_json::json!({ "reasoning_effort": "none" });
        assert!(
            !ensure_reasoning_effort_none(&mut canonical),
            "exact lowercase none is the fixed point"
        );
    }

    #[tokio::test]
    async fn compatible_retry_normalizes_uppercase_reasoning_effort_on_the_wire() {
        // Wire-payload regression: an operator-supplied "NONE" reaches a
        // case-sensitive endpoint, is rejected, and the retry must carry the
        // canonical lowercase spelling.
        let (addr, bodies, server) = spawn_reasoning_rejecting_endpoint(false).await;

        let provider = OpenAiCompatibleModelProvider::builder("test")
            .display_name("test")
            .base_url(&format!("http://{addr}"))
            .credential(None)
            .auth_style(AuthStyle::Bearer)
            .reasoning_effort(Some("NONE".to_string()))
            .build();
        let messages = vec![ChatMessage::user("hello")];
        let tools = vec![clawcrew_api::tool::ToolSpec::new(
            "get_weather",
            "Get weather",
            serde_json::json!({"type": "object", "properties": {}}),
        )];

        let response = provider
            .chat(
                crate::traits::ChatRequest {
                    messages: &messages,
                    tools: Some(&tools),
                    thinking: None,
                },
                "gpt-5",
                None,
            )
            .await
            .unwrap();
        assert_eq!(response.text.as_deref(), Some("ok"));

        let bodies = bodies.lock().unwrap();
        assert_eq!(bodies.len(), 2, "fallback must be bounded to one retry");
        assert_eq!(
            bodies[0]
                .get("reasoning_effort")
                .and_then(serde_json::Value::as_str),
            Some("NONE"),
            "the first request preserves the operator-supplied spelling"
        );
        assert_eq!(
            bodies[1]
                .get("reasoning_effort")
                .and_then(serde_json::Value::as_str),
            Some("none"),
            "the retry must normalize to the canonical lowercase none"
        );
        assert!(bodies.iter().all(|body| body.get("tools").is_some()));
        server.abort();
    }

    #[test]
    fn strips_trailing_slash() {
        let p = make_model_provider("test", "https://example.com/", None);
        assert_eq!(p.base_url, "https://example.com");
    }

    #[test]
    fn with_tls_ca_cert_path_missing_file_leaves_pem_none() {
        // Regression: a non-existent cert path must not panic or propagate an
        // error — the provider falls back to system roots and logs a warning.
        let p = OpenAiCompatibleModelProvider::builder("test")
            .display_name("test")
            .base_url("https://example.com")
            .credential(None)
            .auth_style(AuthStyle::Bearer)
            .tls_ca_cert_path("/nonexistent/path/to/ca.pem")
            .build();
        assert!(
            p.tls_ca_cert_pem.is_none(),
            "missing cert file must leave tls_ca_cert_pem as None (fall back to system roots)"
        );
    }

    #[test]
    fn with_tls_ca_cert_path_invalid_pem_stores_bytes_and_http_client_still_builds() {
        // The path-read step stores raw bytes; PEM parsing happens in http_client().
        // Writing invalid PEM to a temp file: read succeeds (bytes stored), then
        // http_client() logs a WARN and falls back to system roots — no panic, no error.
        let path = format!("/tmp/clawcrew-test-invalid-pem-{}.pem", std::process::id());
        std::fs::write(&path, b"not-a-valid-pem").unwrap();
        let p = OpenAiCompatibleModelProvider::builder("test")
            .display_name("test")
            .base_url("https://example.com")
            .credential(None)
            .auth_style(AuthStyle::Bearer)
            .tls_ca_cert_path(&path)
            .build();
        std::fs::remove_file(&path).ok();
        assert!(
            p.tls_ca_cert_pem.is_some(),
            "readable file (even with bad PEM) must populate tls_ca_cert_pem bytes"
        );
        // http_client() must build cleanly even when PEM parse fails internally.
        // The method returns Client directly (panics on builder error), so if we
        // reach here without panic the fallback-to-system-roots path is working.
        let _client = p.http_client();
    }

    #[test]
    fn with_tls_ca_cert_path_invalid_pem_streaming_http_client_still_builds() {
        // Streaming requests use a separate client builder, so the TLS override
        // must degrade the same way there: warn, use system roots, and keep going.
        let path = format!(
            "/tmp/clawcrew-test-invalid-pem-streaming-{}.pem",
            std::process::id()
        );
        std::fs::write(&path, b"not-a-valid-pem").unwrap();
        let p = OpenAiCompatibleModelProvider::builder("test")
            .display_name("test")
            .base_url("https://example.com")
            .credential(None)
            .auth_style(AuthStyle::Bearer)
            .tls_ca_cert_path(&path)
            .build();
        std::fs::remove_file(&path).ok();
        assert!(
            p.tls_ca_cert_pem.is_some(),
            "readable file (even with bad PEM) must populate tls_ca_cert_pem bytes"
        );
        let _client = p.streaming_http_client();
    }

    #[tokio::test]
    async fn chat_without_key_attempts_request() {
        let p = make_model_provider("Local", "http://127.0.0.1:1", None);
        let result = p
            .chat_with_system(None, "hello", "default", Some(0.7))
            .await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            !err_msg.contains("API key not set"),
            "should not get credential error, got: {err_msg}"
        );
    }

    fn sse_response(body: &'static str) -> reqwest::Response {
        reqwest::Response::from(
            axum::http::Response::builder()
                .status(reqwest::StatusCode::OK)
                .body(reqwest::Body::from(body))
                .expect("test response should build"),
        )
    }

    async fn collect_stream_events(body: &'static str) -> Vec<StreamResult<StreamEvent>> {
        let mut stream = sse_bytes_to_events(sse_response(body), false);
        let mut events = Vec::new();
        while let Ok(Some(ev)) =
            tokio::time::timeout(std::time::Duration::from_secs(2), stream.next()).await
        {
            events.push(ev);
        }
        events
    }

    async fn open_sse_response(
        body: &'static str,
    ) -> (reqwest::Response, tokio::task::JoinHandle<()>) {
        use axum::{Router, response::IntoResponse, routing::get};

        let app = Router::new().route(
            "/stream",
            get(move || async move {
                let first = futures_util::stream::once(async move {
                    Ok::<_, std::convert::Infallible>(axum::body::Bytes::from_static(
                        body.as_bytes(),
                    ))
                });
                let open = futures_util::stream::pending::<
                    Result<axum::body::Bytes, std::convert::Infallible>,
                >();
                axum::body::Body::from_stream(first.chain(open)).into_response()
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind SSE test server");
        let addr = listener.local_addr().expect("SSE test server address");
        let server = ::clawcrew_spawn::spawn!(async move {
            axum::serve(listener, app).await.expect("serve SSE test");
        });
        let response = reqwest::Client::new()
            .get(format!("http://{addr}/stream"))
            .send()
            .await
            .expect("request SSE test stream");
        (response, server)
    }

    #[tokio::test]
    async fn done_sentinel_finishes_chunk_stream_without_eof() {
        let (response, server) = open_sse_response(
            "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\ndata: [DONE]\n\n",
        )
        .await;
        let mut stream = sse_bytes_to_chunks(
            response,
            false,
            crate::StreamIdleBound::Fixed(crate::STREAM_IDLE_TIMEOUT),
        );

        let first = tokio::time::timeout(std::time::Duration::from_secs(2), stream.next())
            .await
            .expect("text delta must arrive before the connection closes")
            .expect("chunk stream must yield text")
            .expect("text chunk must be valid");
        assert_eq!(first.delta, "hi");
        let final_chunk = tokio::time::timeout(std::time::Duration::from_secs(2), stream.next())
            .await
            .expect("[DONE] must finish the stream without EOF")
            .expect("chunk stream must yield Final")
            .expect("Final chunk must be valid");
        assert!(final_chunk.is_final);
        server.abort();
    }

    #[tokio::test]
    async fn done_sentinel_finishes_event_stream_without_eof() {
        let (response, server) = open_sse_response(
            "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\ndata: [DONE]\n\n",
        )
        .await;
        let mut stream = sse_bytes_to_events(response, false);
        let mut saw_final = false;

        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while let Some(event) = stream.next().await {
                if matches!(event, Ok(StreamEvent::Final)) {
                    saw_final = true;
                    break;
                }
            }
        })
        .await
        .expect("[DONE] must finish the event stream without EOF");

        server.abort();
        assert!(saw_final, "terminal sentinel must emit Final");
    }

    #[tokio::test]
    async fn eof_after_done_sentinel_emits_final() {
        let events = collect_stream_events(
            "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\ndata: [DONE]\n\n",
        )
        .await;
        assert!(
            matches!(events.last(), Some(Ok(StreamEvent::Final))),
            "got: {events:?}"
        );
    }

    #[tokio::test]
    async fn eof_after_finish_reason_without_done_emits_final() {
        let events = collect_stream_events(
            "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"},\"finish_reason\":\"stop\"}]}\n\n",
        )
        .await;
        assert!(
            matches!(events.last(), Some(Ok(StreamEvent::Final))),
            "got: {events:?}"
        );
    }

    #[tokio::test]
    async fn eof_before_completion_signal_surfaces_error_not_final() {
        let events =
            collect_stream_events("data: {\"choices\":[{\"delta\":{\"content\":\"par\"}}]}\n\n")
                .await;
        assert!(
            !events.iter().any(|e| matches!(e, Ok(StreamEvent::Final))),
            "truncated stream must not emit Final, got: {events:?}"
        );
        assert!(
            matches!(
                events.last(),
                Some(Err(StreamError::Http(msg))) if msg.contains("truncated")
            ),
            "expected truncation error, got: {events:?}"
        );
    }

    #[test]
    fn native_chat_request_with_tools_includes_stream_options() {
        // Regression: tool-enabled streaming requests must opt the response
        // into a final `usage` SSE event, otherwise OpenAI-compatible providers
        // never report token counts on the `/ws/chat` path (the gateway's
        // primary path uses native tools). See Audacity88'sreview.
        let req: NativeChatRequest = NativeChatRequest {
            model: "gpt-4o".to_string(),
            messages: vec![NativeMessage {
                role: "user".to_string(),
                content: Some(MessageContent::Text("hello".to_string())),
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
                reasoning: None,
                name: None,
            }],
            temperature: Some(0.7),
            stream: Some(true),
            stream_options: Some(StreamOptionsBody {
                include_usage: true,
            }),
            reasoning_effort: None,
            tool_stream: None,
            tools: Some(vec![NativeToolSpec {
                kind: "function".to_string(),
                extra: serde_json::Map::new(),
                function: NativeToolFunctionSpec {
                    extra: serde_json::Map::new(),
                    name: "echo".to_string(),
                    description: String::new(),
                    parameters: std::sync::Arc::new(serde_json::json!({})),
                },
            }]),
            tool_choice: Some("auto".to_string()),
            max_tokens: None,
            extra_body: None,
        };
        let value: serde_json::Value = serde_json::to_value(&req).unwrap();
        assert_eq!(
            value
                .get("stream_options")
                .and_then(|v| v.get("include_usage"))
                .and_then(serde_json::Value::as_bool),
            Some(true),
            "tool-enabled streaming request must serialize stream_options.include_usage=true; \
             without it OpenAI-compatible providers omit the final usage event"
        );
    }

    #[test]
    fn native_chat_request_omits_stream_options_when_none() {
        // Non-streaming path (e.g. classic `chat()` call) does not need
        // `stream_options.include_usage` because the final response carries
        // `usage` directly. The field must be skipped in serialization.
        let req: NativeChatRequest = NativeChatRequest {
            model: "gpt-4o".to_string(),
            messages: vec![],
            temperature: Some(0.7),
            stream: Some(false),
            stream_options: None,
            reasoning_effort: None,
            tool_stream: None,
            tools: None,
            tool_choice: None,
            max_tokens: None,
            extra_body: None,
        };
        let value: serde_json::Value = serde_json::to_value(&req).unwrap();
        assert!(
            value.get("stream_options").is_none(),
            "non-streaming NativeChatRequest must not emit a stream_options key"
        );
    }

    #[test]
    fn extra_body_flattens_into_request_top_level() {
        let req: NativeChatRequest = NativeChatRequest {
            model: "qwen".to_string(),
            messages: vec![],
            temperature: None,
            stream: None,
            stream_options: None,
            reasoning_effort: None,
            tool_stream: None,
            tools: None,
            tool_choice: None,
            max_tokens: None,
            extra_body: Some(serde_json::json!({"thinking": "off"})),
        };
        let value: serde_json::Value = serde_json::to_value(&req).unwrap();
        assert_eq!(
            value.get("thinking").and_then(serde_json::Value::as_str),
            Some("off"),
            "extra_body fields must serialize at the top level, not nested"
        );
        assert!(
            value.get("extra_body").is_none(),
            "extra_body key itself must not appear in serialized JSON"
        );
    }

    #[test]
    fn api_chat_request_flattens_extra_body_into_top_level() {
        // Regression: the no-tools request struct (`chat_with_system`,
        // `chat_with_history`, no-tools streaming) must also carry the
        // config-driven `extra_body`, not just the native-tools path.
        let req = ApiChatRequest {
            model: "qwen".to_string(),
            messages: vec![],
            temperature: None,
            stream: None,
            stream_options: None,
            reasoning_effort: None,
            tool_stream: None,
            tools: None,
            tool_choice: None,
            max_tokens: None,
            extra_body: Some(serde_json::json!({
                "top_p": 0.95,
                "chat_template_kwargs": {"thinking": true, "reasoning_effort": "max"},
            })),
        };
        let value: serde_json::Value = serde_json::to_value(&req).unwrap();
        assert_eq!(
            value.get("top_p").and_then(serde_json::Value::as_f64),
            Some(0.95),
            "provider_extra keys must serialize at the top level of a no-tools request"
        );
        assert_eq!(
            value.pointer("/chat_template_kwargs/reasoning_effort"),
            Some(&serde_json::json!("max")),
            "chat_template_kwargs must be nested under its own top-level key in a no-tools request"
        );
        assert!(
            value.get("extra_body").is_none(),
            "extra_body key itself must not appear in serialized JSON"
        );
    }

    #[test]
    fn normalize_model_ids_trims_filters_and_sorts() {
        let body = serde_json::from_value(serde_json::json!({
            "data": [
                {"id": " zeta-model "},
                {"id": ""},
                {"id": "alpha-model"}
            ]
        }))
        .unwrap();

        assert_eq!(normalize_model_ids(body), vec!["alpha-model", "zeta-model"]);
    }

    #[test]
    fn request_serializes_correctly() {
        let req = ApiChatRequest {
            model: "llama-3.3-70b".to_string(),
            messages: vec![
                Message {
                    role: "system".to_string(),
                    content: MessageContent::Text("You are ClawCrew".to_string()),
                },
                Message {
                    role: "user".to_string(),
                    content: MessageContent::Text("hello".to_string()),
                },
            ],
            temperature: Some(0.4),
            stream: Some(false),
            stream_options: None,
            reasoning_effort: None,
            tool_stream: None,
            tools: None,
            tool_choice: None,
            max_tokens: None,
            extra_body: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("llama-3.3-70b"));
        assert!(json.contains("system"));
        assert!(json.contains("user"));
        // tools/tool_choice should be omitted when None
        assert!(!json.contains("tools"));
        assert!(!json.contains("tool_choice"));
    }

    #[test]
    fn response_deserializes() {
        let json = r#"{"choices":[{"message":{"content":"Hello from Venice!"}}]}"#;
        let resp: ApiChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(
            resp.choices[0].message.content,
            Some("Hello from Venice!".to_string())
        );
    }

    #[test]
    fn response_deserializes_content_as_openai_text_parts_array() {
        let json =
            r#"{"choices":[{"message":{"content":[{"type":"text","text":"Hello array"}]}}]}"#;
        let resp: ApiChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Hello array")
        );
    }

    #[test]
    fn response_deserializes_multiple_text_parts_with_newlines() {
        let json = r#"{"choices":[{"message":{"content":[{"type":"text","text":"Hello"},{"type":"image_url","image_url":{"url":"https://example.com/image.png"}},{"type":"text","text":"array"}]}}]}"#;
        let resp: ApiChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Hello\narray")
        );
    }

    #[test]
    fn response_rejects_unsupported_top_level_content_shape() {
        let json = r#"{"choices":[{"message":{"content":{"type":"text","text":"Hello object"}}}]}"#;
        serde_json::from_str::<ApiChatResponse>(json)
            .expect_err("object-shaped assistant content must remain an invalid payload");
    }

    #[test]
    fn response_empty_choices() {
        let json = r#"{"choices":[]}"#;
        let resp: ApiChatResponse = serde_json::from_str(json).unwrap();
        assert!(resp.choices.is_empty());
    }

    #[test]
    fn parse_chat_response_body_reports_sanitized_snippet() {
        let body = r#"{"choices":"invalid","api_key":"sk-test-secret-value"}"#;
        let err = parse_chat_response_body("custom", body).expect_err("payload should fail");
        let msg = err.to_string();

        assert!(msg.contains("custom API returned an unexpected chat-completions payload"));
        assert!(msg.contains("body="));
        assert!(msg.contains("[REDACTED]"));
        assert!(!msg.contains("sk-test-secret-value"));
    }

    #[tokio::test]
    async fn ordinary_non_streaming_path_uses_shared_envelope_parser() {
        for (case, response_body, expected_text) in non_streaming_response_cases() {
            let (provider, captured, server) = mock_non_streaming_response(response_body).await;
            let result = provider
                .chat_with_system(None, "hello", "test-model", None)
                .await;
            server.abort();

            let requests = captured.lock().unwrap();
            assert_eq!(requests.len(), 1, "{case}: expected one request");
            assert!(
                requests[0].get("tools").is_none(),
                "{case}: ordinary request unexpectedly contained tools: {}",
                requests[0]
            );

            match expected_text {
                Some(expected) => assert_eq!(
                    result.unwrap_or_else(|error| panic!("{case}: request failed: {error}")),
                    expected,
                    "{case}: response text mismatch"
                ),
                None => assert_sanitized_envelope_error(
                    &result.expect_err("malformed response must fail"),
                    case,
                ),
            }
        }
    }

    #[tokio::test]
    async fn native_tool_non_streaming_path_uses_shared_envelope_parser() {
        for (case, response_body, expected_text) in non_streaming_response_cases() {
            let (provider, captured, server) = mock_non_streaming_response(response_body).await;
            let messages = vec![ChatMessage::user("hello")];
            let tools = vec![clawcrew_api::tool::ToolSpec::new(
                "echo",
                "Echo a value",
                serde_json::json!({
                    "type": "object",
                    "properties": {"value": {"type": "string"}}
                }),
            )];
            let result = provider
                .chat(
                    ProviderChatRequest {
                        messages: &messages,
                        tools: Some(&tools),
                        thinking: None,
                    },
                    "test-model",
                    None,
                )
                .await;
            server.abort();

            let requests = captured.lock().unwrap();
            assert_eq!(requests.len(), 1, "{case}: expected one request");
            assert_eq!(
                requests[0]["tools"][0]["function"]["name"], "echo",
                "{case}: request did not exercise the native-tool path: {}",
                requests[0]
            );

            match expected_text {
                Some(expected) => assert_eq!(
                    result
                        .unwrap_or_else(|error| panic!("{case}: request failed: {error}"))
                        .text
                        .as_deref(),
                    Some(expected),
                    "{case}: response text mismatch"
                ),
                None => assert_sanitized_envelope_error(
                    &result.expect_err("malformed response must fail"),
                    case,
                ),
            }
        }
    }

    #[test]
    fn x_api_key_auth_style() {
        let p = OpenAiCompatibleModelProvider::builder("test")
            .display_name("moonshot")
            .base_url("https://api.moonshot.cn")
            .credential(Some("ms-key"))
            .auth_style(AuthStyle::XApiKey)
            .build();
        assert!(matches!(p.auth_header, AuthStyle::XApiKey));
    }

    #[test]
    fn custom_auth_style() {
        let p = OpenAiCompatibleModelProvider::builder("test")
            .display_name("custom")
            .base_url("https://api.example.com")
            .credential(Some("key"))
            .auth_style(AuthStyle::Custom("X-Custom-Key".into()))
            .build();
        assert!(matches!(p.auth_header, AuthStyle::Custom(_)));
    }

    #[test]
    fn zhipu_jwt_produces_valid_three_part_token() {
        let result = zhipu_jwt_bearer("testid.testsecret").unwrap();
        assert!(result.starts_with("Bearer "));
        let jwt = result.strip_prefix("Bearer ").unwrap();
        let parts: Vec<&str> = jwt.split('.').collect();
        assert_eq!(parts.len(), 3, "JWT must have 3 dot-separated parts: {jwt}");
    }

    #[test]
    fn zhipu_jwt_header_is_correct() {
        use base64::engine::{Engine, general_purpose::URL_SAFE_NO_PAD};
        let result = zhipu_jwt_bearer("myid.mysecret").unwrap();
        let jwt = result.strip_prefix("Bearer ").unwrap();
        let header_b64 = jwt.split('.').next().unwrap();
        let header_bytes = URL_SAFE_NO_PAD.decode(header_b64).unwrap();
        let header: serde_json::Value = serde_json::from_slice(&header_bytes).unwrap();
        assert_eq!(header["alg"], "HS256");
        assert_eq!(header["typ"], "JWT");
        assert_eq!(header["sign_type"], "SIGN");
    }

    #[test]
    fn zhipu_jwt_payload_contains_api_key_and_timestamps() {
        use base64::engine::{Engine, general_purpose::URL_SAFE_NO_PAD};
        let result = zhipu_jwt_bearer("myapiid.mysecretkey").unwrap();
        let jwt = result.strip_prefix("Bearer ").unwrap();
        let payload_b64 = jwt.split('.').nth(1).unwrap();
        let payload_bytes = URL_SAFE_NO_PAD.decode(payload_b64).unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&payload_bytes).unwrap();
        assert_eq!(payload["api_key"], "myapiid");
        assert!(payload["exp"].is_number());
        assert!(payload["timestamp"].is_number());
        // exp should be ~210s after timestamp
        let ts = payload["timestamp"].as_u64().unwrap();
        let exp = payload["exp"].as_u64().unwrap();
        assert_eq!(exp - ts, 210_000);
    }

    #[test]
    fn zhipu_jwt_signature_is_verifiable() {
        let secret = "testsecret123";
        let credential = format!("testid.{secret}");
        let result = zhipu_jwt_bearer(&credential).unwrap();
        let jwt = result.strip_prefix("Bearer ").unwrap();
        let parts: Vec<&str> = jwt.split('.').collect();
        let signing_input = format!("{}.{}", parts[0], parts[1]);

        // Verify HMAC-SHA256 signature
        let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, secret.as_bytes());
        use base64::engine::{Engine, general_purpose::URL_SAFE_NO_PAD};
        let sig_bytes = URL_SAFE_NO_PAD.decode(parts[2]).unwrap();
        ring::hmac::verify(&key, signing_input.as_bytes(), &sig_bytes)
            .expect("signature must verify");
    }

    #[test]
    fn zhipu_jwt_rejects_invalid_key_format() {
        assert!(zhipu_jwt_bearer("no-dot-here").is_err());
        assert!(zhipu_jwt_bearer("").is_err());
        assert!(zhipu_jwt_bearer(".secret").is_err());
        assert!(zhipu_jwt_bearer("id.").is_err());
    }

    fn opencode_provider(base_url: &str) -> OpenAiCompatibleModelProvider {
        OpenAiCompatibleModelProvider::builder("opencode")
            .display_name("OpenCode Zen")
            .base_url(base_url)
            .credential(Some("test-key"))
            .auth_style(AuthStyle::Bearer)
            .build()
    }

    /// Request as the chat paths build it, addressed to the real endpoint,
    /// without sending anything.
    fn built_opencode_request(provider: &OpenAiCompatibleModelProvider) -> reqwest::Request {
        provider
            .apply_opencode_session_header(
                reqwest::Client::new().post(provider.chat_completions_url()),
            )
            .build()
            .expect("request must build")
    }

    /// Header value as it would go on the wire, without sending anything.
    fn built_session_header(provider: &OpenAiCompatibleModelProvider) -> Option<String> {
        built_opencode_request(provider)
            .headers()
            .get(OPENCODE_SESSION_HEADER)
            .map(|value| value.to_str().expect("header must be ASCII").to_string())
    }

    #[test]
    fn opencode_session_header_follows_the_built_request_destination() {
        // Header selection must agree with the parser that addresses the
        // request, not with a textual reading of the configured URI.
        for (base_url, api_path, expected_host) in [
            // `\` ends the authority; `@opencode.ai/v1` is only path.
            (
                "https://relay.example\\@opencode.ai/v1",
                None,
                "relay.example",
            ),
            // A percent-encoded host decodes to the relay.
            ("https://%6fpencode.ai/v1", None, "opencode.ai"),
            // `api_path` is appended to the base, so the base alone need not
            // name the destination; only the finished endpoint does.
            (
                "https:",
                Some("//opencode.ai/zen/v1/chat/completions"),
                "opencode.ai",
            ),
            (
                "https:",
                Some("//relay.example/v1/chat/completions"),
                "relay.example",
            ),
        ] {
            let provider = OpenAiCompatibleModelProvider::builder("opencode")
                .display_name("OpenCode Zen")
                .base_url(base_url)
                .api_path(api_path.map(str::to_string))
                .credential(Some("test-key"))
                .auth_style(AuthStyle::Bearer)
                .build();
            let request = built_opencode_request(&provider);
            let host = request.url().host_str().expect("request must have a host");
            assert_eq!(host, expected_host, "{base_url} + {api_path:?}");
            assert_eq!(
                request.headers().contains_key(OPENCODE_SESSION_HEADER),
                host == "opencode.ai",
                "{base_url} + {api_path:?}: header selection must match the request host {host}"
            );
        }
    }

    #[test]
    fn opencode_requests_carry_the_session_header() {
        for base_url in [
            "https://opencode.ai/zen/v1",
            "https://opencode.ai/zen/go/v1",
        ] {
            let header = built_session_header(&opencode_provider(base_url))
                .unwrap_or_else(|| panic!("{base_url} must carry the affinity header"));
            assert_eq!(header.len(), 32, "expected a 128-bit hex token");
            assert!(header.chars().all(|c| c.is_ascii_hexdigit()));
        }
    }

    #[test]
    fn non_opencode_requests_do_not_carry_the_session_header() {
        assert!(
            built_session_header(&opencode_provider("https://api.openai.com/v1")).is_none(),
            "the header must not leak to unrelated providers"
        );
    }

    #[test]
    fn operator_pinned_session_header_is_not_overridden() {
        // `extra_headers` become client default headers, so emitting our own
        // value too would put the header on the wire twice.
        let mut headers = std::collections::HashMap::new();
        headers.insert(
            "X-Opencode-Session".to_string(),
            "pinned-by-operator".to_string(),
        );
        let provider = OpenAiCompatibleModelProvider::builder("opencode")
            .display_name("OpenCode Zen")
            .base_url("https://opencode.ai/zen/v1")
            .credential(Some("test-key"))
            .auth_style(AuthStyle::Bearer)
            .extra_headers(headers)
            .build();

        assert!(
            provider.opencode_session_value().is_none(),
            "an operator-pinned header must win over the derived value"
        );
    }

    #[test]
    fn malformed_pinned_session_header_falls_back_to_the_derived_token() {
        // The client builder skips a header value it cannot encode, so treating
        // it as a pin would leave the request with no affinity header at all.
        let headers = std::collections::HashMap::from([(
            "x-opencode-session".to_string(),
            "bad\nvalue".to_string(),
        )]);
        let provider = OpenAiCompatibleModelProvider::builder("opencode")
            .display_name("OpenCode Zen")
            .base_url("https://opencode.ai/zen/v1")
            .credential(Some("test-key"))
            .auth_style(AuthStyle::Bearer)
            .extra_headers(headers)
            .build();

        assert!(
            built_session_header(&provider).is_some(),
            "an invalid pinned value must not suppress the derived token"
        );
    }

    #[test]
    fn opencode_clients_carry_the_cross_host_redirect_policy() {
        // reqwest strips only credential headers on a cross-host redirect, so
        // every client an OpenCode provider builds must stop there instead.
        // reqwest's `Debug` names the redirect policy only when it is not the
        // default.
        let has_policy = |client: Client| format!("{client:?}").contains("redirect_policy");

        let opencode = opencode_provider("https://opencode.ai/zen/v1");
        assert!(has_policy(opencode.http_client()));
        assert!(has_policy(opencode.streaming_http_client()));

        let other = opencode_provider("https://api.openai.com/v1");
        assert!(!has_policy(other.http_client()));
        assert!(!has_policy(other.streaming_http_client()));
    }

    #[test]
    fn zhipu_jwt_auth_style_applies_correctly() {
        let p = OpenAiCompatibleModelProvider::builder("test")
            .display_name("Z.AI")
            .base_url("https://api.z.ai/api/coding/paas/v4")
            .credential(Some("testid.testsecret"))
            .auth_style(AuthStyle::ZhipuJwt)
            .build();
        assert!(matches!(p.auth_header, AuthStyle::ZhipuJwt));
    }

    /// Every request that reached the test server, as `(method, path)`.
    type WireLog = std::sync::Arc<std::sync::Mutex<Vec<(String, String)>>>;

    /// A server that records anything that arrives, on any method and path,
    /// and answers a well-formed chat completion. Recording everything is the
    /// point: a test asserting the log is empty then fails if a request
    /// escapes by a route the test did not anticipate, instead of passing
    /// because the request 404'd.
    async fn recording_server() -> (String, WireLog, tokio::task::JoinHandle<()>) {
        use axum::Router;
        use tokio::net::TcpListener;

        let log: WireLog = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log_for_route = std::sync::Arc::clone(&log);
        let app =
            Router::new().fallback(move |method: axum::http::Method, uri: axum::http::Uri| {
                let log = std::sync::Arc::clone(&log_for_route);
                async move {
                    log.lock()
                        .expect("wire log poisoned")
                        .push((method.to_string(), uri.path().to_string()));
                    axum::Json(serde_json::json!({
                        "choices": [{"message": {"content": "ok"}}],
                        "data": []
                    }))
                }
            });

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = ::clawcrew_spawn::spawn!(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{addr}"), log, server)
    }

    /// A `ZhipuJwt` credential that is not `id.secret` cannot be minted into a
    /// per-request token, and this provider fails closed rather than falling
    /// back to sending the stored value as a plain bearer token.
    ///
    /// Driven through every entry point on this provider that carries a
    /// credential, because the fallback lived in the helper they all share.
    /// Two properties per entry point: the operator gets a local error naming
    /// the provider, and — the security property — nothing reaches the server,
    /// so the stored value cannot have left the client in any form.
    ///
    /// The local error is also the behaviour change. These paths previously
    /// sent `Authorization: Bearer <stored value>` and surfaced whatever
    /// rejection the provider chose to return, which reads as a credential
    /// problem at the far end rather than a malformed credential here.
    #[tokio::test]
    async fn malformed_zhipu_credential_fails_closed_on_every_credential_bearing_path() {
        const MALFORMED: &str = "no-dot-separator-here";

        let (base_url, wire, server) = recording_server().await;
        let provider = OpenAiCompatibleModelProvider::builder("zai")
            .display_name("Z.AI")
            .base_url(&base_url)
            .credential(Some(MALFORMED))
            .auth_style(AuthStyle::ZhipuJwt)
            .build();

        let history = [ChatMessage::user("hi")];
        let stream_options = StreamOptions {
            enabled: true,
            count_tokens: false,
        };

        let mut refusals: Vec<(&str, String)> = Vec::new();
        let mut push = |entry_point: &'static str, err: String| refusals.push((entry_point, err));

        push(
            "list_models",
            provider.list_models().await.unwrap_err().to_string(),
        );
        push(
            "list_models_with_pricing",
            provider
                .list_models_with_pricing()
                .await
                .unwrap_err()
                .to_string(),
        );
        push(
            "chat_with_system",
            provider
                .chat_with_system(None, "hi", "m", None)
                .await
                .unwrap_err()
                .to_string(),
        );
        push(
            "chat_with_history",
            provider
                .chat_with_history(&history, "m", None)
                .await
                .unwrap_err()
                .to_string(),
        );
        push(
            "chat_with_tools",
            provider
                .chat_with_tools(&history, &[], "m", None)
                .await
                .unwrap_err()
                .to_string(),
        );
        push(
            "chat",
            provider
                .chat(
                    ProviderChatRequest {
                        messages: &history,
                        tools: None,
                        thinking: None,
                    },
                    "m",
                    None,
                )
                .await
                .unwrap_err()
                .to_string(),
        );
        push("warmup", provider.warmup().await.unwrap_err().to_string());

        // The streaming paths build their request inside a spawned task, so the
        // refusal arrives as the stream's first item rather than as a return
        // value. It must still be the first item — not an empty stream that
        // looks like a successful, contentless response.
        let mut events = provider.stream_chat(
            ProviderChatRequest {
                messages: &history,
                tools: None,
                thinking: None,
            },
            "m",
            None,
            stream_options,
        );
        push(
            "stream_chat",
            match events.next().await {
                Some(Err(err)) => err.to_string(),
                other => panic!("stream_chat must yield a refusal first, got {other:?}"),
            },
        );
        drop(events);

        let mut chunks = provider.stream_chat_with_system(None, "hi", "m", None, stream_options);
        push(
            "stream_chat_with_system",
            match chunks.next().await {
                Some(Err(err)) => err.to_string(),
                other => {
                    panic!("stream_chat_with_system must yield a refusal first, got {other:?}")
                }
            },
        );
        drop(chunks);

        let mut chunks = provider.stream_chat_with_history(&history, "m", None, stream_options);
        push(
            "stream_chat_with_history",
            match chunks.next().await {
                Some(Err(err)) => err.to_string(),
                other => {
                    panic!("stream_chat_with_history must yield a refusal first, got {other:?}")
                }
            },
        );
        drop(chunks);

        for (entry_point, err) in &refusals {
            assert!(
                err.contains("Z.AI"),
                "{entry_point}: the error must name the provider: {err}"
            );
            assert!(
                err.contains("no request was sent"),
                "{entry_point}: the error must say the request was refused locally: {err}"
            );
            assert!(
                !err.contains(MALFORMED),
                "{entry_point}: the error must not quote the stored credential: {err}"
            );
        }

        let seen = wire.lock().expect("wire log poisoned").clone();
        assert!(
            seen.is_empty(),
            "no request may leave the client for a credential that cannot be minted: {seen:?}"
        );
        server.abort();
    }

    /// The counterpart: a well-formed `id.secret` still reaches the provider,
    /// so failing closed did not disable Zhipu-auth families outright.
    #[tokio::test]
    async fn a_well_formed_zhipu_credential_still_reaches_the_provider() {
        let (base_url, wire, server) = recording_server().await;
        let provider = OpenAiCompatibleModelProvider::builder("zai")
            .display_name("Z.AI")
            .base_url(&base_url)
            .credential(Some("keyid.longlivedsecret"))
            .auth_style(AuthStyle::ZhipuJwt)
            .build();

        provider
            .chat_with_system(None, "hi", "m", None)
            .await
            .expect("a well-formed id.secret must still be usable");

        let seen = wire.lock().expect("wire log poisoned").clone();
        assert_eq!(
            seen.len(),
            1,
            "the well-formed case must still issue its request: {seen:?}"
        );
        server.abort();
    }

    /// A credential that cannot be minted refuses the request rather than
    /// building one. Sending it unauthenticated would leave the operator
    /// reading whatever 401 the provider returns instead of the local
    /// credential fault that actually happened.
    #[test]
    fn an_unmintable_zhipu_credential_refuses_the_request_instead_of_building_one() {
        const STORED: &str = "raw-secret-without-required-separator";

        let error = apply_auth_to_request(
            reqwest::Client::new().get("https://example.com"),
            &AuthStyle::ZhipuJwt,
            Some(STORED),
            "GLM",
        )
        .expect_err("an unmintable credential must not produce a request builder");

        let message = error.to_string();
        assert!(
            !message.contains(STORED),
            "the refusal must not quote the credential: {message}"
        );
        assert!(
            message.contains("GLM"),
            "the refusal must name the provider it belongs to: {message}"
        );
    }

    /// The three auth styles that pass a credential through untransformed keep
    /// doing so — fail-closed is scoped to the style that mints.
    #[test]
    fn untransformed_auth_styles_still_build_their_request() {
        for style in [
            AuthStyle::Bearer,
            AuthStyle::XApiKey,
            AuthStyle::Custom("X-Token".to_string()),
        ] {
            let request = apply_auth_to_request(
                reqwest::Client::new().get("https://example.com"),
                &style,
                Some("plain-key"),
                "test",
            )
            .expect("a credential needing no transformation always builds")
            .build()
            .expect("request should build");

            assert!(
                request
                    .headers()
                    .iter()
                    .any(|(_, v)| v.to_str().is_ok_and(|v| v.contains("plain-key"))),
                "expected the credential on the wire for {style:?}"
            );
        }
    }

    #[tokio::test]
    async fn all_compatible_providers_attempt_request_without_key() {
        let model_providers = vec![
            make_model_provider("Venice", "http://127.0.0.1:1", None),
            make_model_provider("Moonshot", "http://127.0.0.1:1", None),
            make_model_provider("GLM", "http://127.0.0.1:1", None),
            make_model_provider("MiniMax", "http://127.0.0.1:1", None),
            make_model_provider("Groq", "http://127.0.0.1:1", None),
            make_model_provider("Mistral", "http://127.0.0.1:1", None),
            make_model_provider("xAI", "http://127.0.0.1:1", None),
            make_model_provider("Astrai", "http://127.0.0.1:1", None),
        ];

        for p in model_providers {
            let result = p.chat_with_system(None, "test", "model", Some(0.7)).await;
            assert!(result.is_err(), "{} should fail (unreachable host)", p.name);
            let err_msg = result.unwrap_err().to_string();
            assert!(
                !err_msg.contains("API key not set"),
                "{} should get transport error, not credential error, got: {err_msg}",
                p.name
            );
        }
    }

    #[test]
    fn tool_call_function_name_falls_back_to_top_level_name() {
        let call: ToolCall = serde_json::from_value(serde_json::json!({
            "name": "memory_recall",
            "arguments": "{\"query\":\"latest roadmap\"}"
        }))
        .unwrap();

        assert_eq!(call.function_name().as_deref(), Some("memory_recall"));
    }

    #[test]
    fn tool_call_function_arguments_falls_back_to_parameters_object() {
        let call: ToolCall = serde_json::from_value(serde_json::json!({
            "name": "shell",
            "parameters": {"command": "pwd"}
        }))
        .unwrap();

        assert_eq!(
            call.function_arguments().as_deref(),
            Some("{\"command\":\"pwd\"}")
        );
    }

    #[test]
    fn tool_call_function_arguments_prefers_nested_function_field() {
        let call: ToolCall = serde_json::from_value(serde_json::json!({
            "name": "ignored_name",
            "arguments": "{\"query\":\"ignored\"}",
            "function": {
                "name": "memory_recall",
                "arguments": "{\"query\":\"preferred\"}"
            }
        }))
        .unwrap();

        assert_eq!(call.function_name().as_deref(), Some("memory_recall"));
        assert_eq!(
            call.function_arguments().as_deref(),
            Some("{\"query\":\"preferred\"}")
        );
    }

    // ----------------------------------------------------------
    // Custom endpoint path tests
    // ----------------------------------------------------------

    #[test]
    fn chat_completions_url_standard_openai() {
        // Standard OpenAI-compatible model_providers get /chat/completions appended
        let p = make_model_provider("openai", "https://api.openai.com/v1", None);
        assert_eq!(
            p.chat_completions_url(),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn chat_completions_url_trailing_slash() {
        // Trailing slash is stripped, then /chat/completions appended
        let p = make_model_provider("test", "https://api.example.com/v1/", None);
        assert_eq!(
            p.chat_completions_url(),
            "https://api.example.com/v1/chat/completions"
        );
    }

    #[test]
    fn chat_completions_url_volcengine_ark() {
        // VolcEngine ARK uses custom path - should use as-is
        let p = make_model_provider(
            "volcengine",
            "https://ark.cn-beijing.volces.com/api/coding/v3/chat/completions",
            None,
        );
        assert_eq!(
            p.chat_completions_url(),
            "https://ark.cn-beijing.volces.com/api/coding/v3/chat/completions"
        );
    }

    #[test]
    fn chat_completions_url_custom_full_endpoint() {
        // Custom model_provider with full endpoint path
        let p = make_model_provider(
            "custom",
            "https://my-api.example.com/v2/llm/chat/completions",
            None,
        );
        assert_eq!(
            p.chat_completions_url(),
            "https://my-api.example.com/v2/llm/chat/completions"
        );
    }

    #[test]
    fn chat_completions_url_requires_exact_suffix_match() {
        let p = make_model_provider(
            "custom",
            "https://my-api.example.com/v2/llm/chat/completions-proxy",
            None,
        );
        assert_eq!(
            p.chat_completions_url(),
            "https://my-api.example.com/v2/llm/chat/completions-proxy/chat/completions"
        );
    }

    #[test]
    fn chat_completions_url_without_v1() {
        // ModelProvider configured without /v1 in base URL
        let p = make_model_provider("test", "https://api.example.com", None);
        assert_eq!(
            p.chat_completions_url(),
            "https://api.example.com/chat/completions"
        );
    }

    #[test]
    fn chat_completions_url_base_with_v1() {
        // ModelProvider configured with /v1 in base URL
        let p = make_model_provider("test", "https://api.example.com/v1", None);
        assert_eq!(
            p.chat_completions_url(),
            "https://api.example.com/v1/chat/completions"
        );
    }

    // ----------------------------------------------------------
    // ModelProvider-specific endpoint tests
    // ----------------------------------------------------------

    #[test]
    fn chat_completions_url_zai() {
        // Z.AI uses /api/paas/v4 base path
        let p = make_model_provider("zai", "https://api.z.ai/api/paas/v4", None);
        assert_eq!(
            p.chat_completions_url(),
            "https://api.z.ai/api/paas/v4/chat/completions"
        );
    }

    #[test]
    fn chat_completions_url_minimax() {
        // MiniMax OpenAI-compatible endpoint requires /v1 base path.
        let p = make_model_provider("minimax", "https://api.minimaxi.com/v1", None);
        assert_eq!(
            p.chat_completions_url(),
            "https://api.minimaxi.com/v1/chat/completions"
        );
    }

    #[test]
    fn chat_completions_url_glm() {
        // GLM (BigModel) uses /api/paas/v4 base path
        let p = make_model_provider("glm", "https://open.bigmodel.cn/api/paas/v4", None);
        assert_eq!(
            p.chat_completions_url(),
            "https://open.bigmodel.cn/api/paas/v4/chat/completions"
        );
    }

    #[test]
    fn chat_completions_url_opencode() {
        // OpenCode Zen uses /zen/v1 base path
        let p = make_model_provider("opencode", "https://opencode.ai/zen/v1", None);
        assert_eq!(
            p.chat_completions_url(),
            "https://opencode.ai/zen/v1/chat/completions"
        );
    }

    #[test]
    fn chat_completions_url_opencode_go() {
        // OpenCode Go uses /zen/go/v1 base path
        let p = make_model_provider("opencode-go", "https://opencode.ai/zen/go/v1", None);
        assert_eq!(
            p.chat_completions_url(),
            "https://opencode.ai/zen/go/v1/chat/completions"
        );
    }

    #[test]
    fn parse_native_response_preserves_tool_call_id() {
        let provider = make_model_provider("test", "https://example.com", None);
        let message = ResponseMessage {
            content: None,
            tool_calls: Some(vec![ToolCall {
                id: Some("call_123".to_string()),
                kind: Some("function".to_string()),
                function: Some(Function {
                    name: Some("shell".to_string()),
                    arguments: Some(r#"{"command":"pwd"}"#.to_string()),
                }),
                name: None,
                arguments: None,
                parameters: None,
                extra_content: None,
            }]),
            reasoning_content: None,
        };

        let parsed = provider.parse_native_response(message);
        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].id, "call_123");
        assert_eq!(parsed.tool_calls[0].name, "shell");
    }

    #[test]
    fn parse_native_response_rejects_non_object_tool_arguments() {
        let provider = make_model_provider("test", "https://example.com", None);
        let message = ResponseMessage {
            content: None,
            tool_calls: Some(vec![ToolCall {
                id: Some("call_123".to_string()),
                kind: Some("function".to_string()),
                function: Some(Function {
                    name: Some("shell".to_string()),
                    arguments: Some(r#"["not", "an", "object"]"#.to_string()),
                }),
                name: None,
                arguments: None,
                parameters: None,
                extra_content: None,
            }]),
            reasoning_content: None,
        };

        let parsed = provider.parse_native_response(message);

        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].arguments, "{}");
    }

    #[tokio::test]
    async fn streaming_entry_points_sanitize_upstream_error_bodies() {
        use axum::{Router, http::StatusCode, response::IntoResponse, routing::post};
        use tokio::net::TcpListener;

        let secret = "sk-streaming-boundary-secret";
        let body = format!(r#"{{"error":"{secret} {}"}}"#, "x".repeat(4_000));
        let app = Router::new().route(
            "/chat/completions",
            post(move || {
                let body = body.clone();
                async move { (StatusCode::UNAUTHORIZED, body).into_response() }
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = ::clawcrew_spawn::spawn!(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let provider = make_model_provider("test", &format!("http://{addr}"), Some("key"));
        let options = StreamOptions {
            enabled: true,
            count_tokens: false,
        };

        let mut event_stream = provider.stream_chat(
            ProviderChatRequest {
                messages: &[ChatMessage::user("hi")],
                tools: None,
                thinking: None,
            },
            "test-model",
            None,
            options,
        );
        assert_sanitized_streaming_error(event_stream.next().await.unwrap().unwrap_err(), secret);

        let mut system_stream =
            provider.stream_chat_with_system(None, "hi", "test-model", None, options);
        assert_sanitized_streaming_error(system_stream.next().await.unwrap().unwrap_err(), secret);

        let mut history_stream = provider.stream_chat_with_history(
            &[ChatMessage::user("hi")],
            "test-model",
            None,
            options,
        );
        assert_sanitized_streaming_error(history_stream.next().await.unwrap().unwrap_err(), secret);

        server.abort();
    }

    fn assert_sanitized_streaming_error(error: StreamError, secret: &str) {
        let StreamError::ModelProvider(message) = error else {
            panic!("expected model-provider error, got {error}");
        };
        assert!(message.contains("401 Unauthorized"));
        assert!(message.contains("[REDACTED]"));
        assert!(!message.contains(secret));
        assert!(message.chars().count() <= 525);
    }

    #[test]
    fn parse_native_response_mistral_normalizes_invalid_tool_call_id() {
        let provider = make_model_provider("Mistral", "https://api.mistral.ai/v1", None);
        let message = ResponseMessage {
            content: None,
            tool_calls: Some(vec![ToolCall {
                id: Some("xvL0p9bZ41j2X0O3Q1y9vL0p9bZ41j2X".to_string()),
                kind: Some("function".to_string()),
                function: Some(Function {
                    name: Some("shell".to_string()),
                    arguments: Some(r#"{"command":"pwd"}"#.to_string()),
                }),
                name: None,
                arguments: None,
                parameters: None,
                extra_content: None,
            }]),
            reasoning_content: None,
        };

        let parsed = provider.parse_native_response(message);
        assert_eq!(parsed.tool_calls.len(), 1);
        let id = &parsed.tool_calls[0].id;
        assert_eq!(id.len(), 9);
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn parse_native_response_mistral_generates_valid_id_when_missing() {
        let provider = make_model_provider("Mistral", "https://api.mistral.ai/v1", None);
        let message = ResponseMessage {
            content: None,
            tool_calls: Some(vec![ToolCall {
                id: None,
                kind: Some("function".to_string()),
                function: Some(Function {
                    name: Some("shell".to_string()),
                    arguments: Some(r#"{"command":"pwd"}"#.to_string()),
                }),
                name: None,
                arguments: None,
                parameters: None,
                extra_content: None,
            }]),
            reasoning_content: None,
        };

        let parsed = provider.parse_native_response(message);
        assert_eq!(parsed.tool_calls.len(), 1);
        let id = &parsed.tool_calls[0].id;
        assert_eq!(id.len(), 9);
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn parse_native_response_custom_mistral_endpoint_normalizes_tool_call_id() {
        let provider = make_model_provider("Custom", "https://api.mistral.ai/v1", None);
        let message = ResponseMessage {
            content: None,
            tool_calls: Some(vec![ToolCall {
                id: Some("xvL0p9bZ41j2X0O3Q1y9vL0p9bZ41j2X".to_string()),
                kind: Some("function".to_string()),
                function: Some(Function {
                    name: Some("shell".to_string()),
                    arguments: Some(r#"{"command":"pwd"}"#.to_string()),
                }),
                name: None,
                arguments: None,
                parameters: None,
                extra_content: None,
            }]),
            reasoning_content: None,
        };

        let parsed = provider.parse_native_response(message);
        assert_eq!(parsed.tool_calls.len(), 1);
        let id = &parsed.tool_calls[0].id;
        assert_eq!(id.len(), 9);
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn parse_native_response_mistral_avoids_id_collision_after_normalization() {
        let provider = make_model_provider("Mistral", "https://api.mistral.ai/v1", None);
        let message = ResponseMessage {
            content: None,
            tool_calls: Some(vec![
                ToolCall {
                    id: Some("ABCDEFGHI123".to_string()),
                    kind: Some("function".to_string()),
                    function: Some(Function {
                        name: Some("shell".to_string()),
                        arguments: Some(r#"{"command":"pwd"}"#.to_string()),
                    }),
                    name: None,
                    arguments: None,
                    parameters: None,
                    extra_content: None,
                },
                ToolCall {
                    id: Some("ABCDEFGHIxyz".to_string()),
                    kind: Some("function".to_string()),
                    function: Some(Function {
                        name: Some("echo".to_string()),
                        arguments: Some(r#"{"text":"ok"}"#.to_string()),
                    }),
                    name: None,
                    arguments: None,
                    parameters: None,
                    extra_content: None,
                },
            ]),
            reasoning_content: None,
        };

        let parsed = provider.parse_native_response(message);
        assert_eq!(parsed.tool_calls.len(), 2);
        let id0 = &parsed.tool_calls[0].id;
        let id1 = &parsed.tool_calls[1].id;
        assert_eq!(id0.len(), 9);
        assert_eq!(id1.len(), 9);
        assert!(id0.chars().all(|c| c.is_ascii_alphanumeric()));
        assert!(id1.chars().all(|c| c.is_ascii_alphanumeric()));
        assert_ne!(id0, id1);
    }

    #[test]
    fn convert_messages_for_native_maps_tool_result_payload() {
        let input = vec![ChatMessage::tool(
            r#"{"tool_call_id":"call_abc","content":"done"}"#,
        )];

        let provider = make_model_provider("test", "https://example.com", None);
        let converted = provider.convert_messages_for_native(&input, true);
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].role, "tool");
        assert_eq!(converted[0].tool_call_id.as_deref(), Some("call_abc"));
        assert!(matches!(
            converted[0].content.as_ref(),
            Some(MessageContent::Text(value)) if value == "done"
        ));
    }

    #[tokio::test]
    async fn operator_max_images_bounds_the_outbound_request() {
        // Behaviour boundary: the number of images that survive into the
        // messages this provider is about to send upstream. Asserting the
        // config field alone would still pass if the expansion pass kept
        // using library defaults.
        let temp = tempfile::tempdir().unwrap();
        // Minimal PNG signature bytes are enough for MIME detection.
        let png = [0x89u8, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
        let first = temp.path().join("first.png");
        let second = temp.path().join("second.png");
        std::fs::write(&first, png).unwrap();
        std::fs::write(&second, png).unwrap();

        // One image per message: `trim_old_images` evicts whole messages, so
        // co-locating both in a single message would exercise that eviction
        // granularity rather than whether the operator's cap is honoured.
        let messages = vec![
            ChatMessage::user(format!("first [IMAGE:{}]", first.display())),
            ChatMessage::user(format!("second [IMAGE:{}]", second.display())),
        ];

        let build = |multimodal| {
            OpenAiCompatibleModelProvider::builder("test")
                .display_name("test")
                .base_url("https://example.com")
                .credential(None)
                .auth_style(AuthStyle::Bearer)
                .multimodal(multimodal)
                .build()
        };

        let permissive = build(clawcrew_config::schema::MultimodalConfig::default());
        let prepared = permissive
            .normalize_messages_for_upstream(&messages)
            .await
            .expect("default policy prepares both images");
        assert_eq!(
            crate::multimodal::count_image_markers(&prepared),
            2,
            "default policy admits both images"
        );

        let capped = build(clawcrew_config::schema::MultimodalConfig {
            max_images: 1,
            ..Default::default()
        });
        let prepared = capped
            .normalize_messages_for_upstream(&messages)
            .await
            .expect("capped policy still prepares the message");
        assert_eq!(
            crate::multimodal::count_image_markers(&prepared),
            1,
            "an operator capping max_images must bound the outbound request"
        );
    }

    #[test]
    fn builder_without_multimodal_falls_back_to_library_defaults() {
        let provider = OpenAiCompatibleModelProvider::builder("test")
            .display_name("test")
            .base_url("https://example.com")
            .credential(None)
            .auth_style(AuthStyle::Bearer)
            .build();

        let defaults = clawcrew_config::schema::MultimodalConfig::default();
        assert_eq!(provider.multimodal.max_images, defaults.max_images);
    }

    #[test]
    fn convert_messages_for_native_promotes_tool_result_image_markers() {
        // A tool result carrying an inline base64 image marker (e.g. a snapshot
        // tool) must serialize as structured `image_url` parts, not one large
        // text blob — vision backends count base64 bytes as text tokens and
        // reject the request as over-context otherwise
        let input = vec![ChatMessage::tool(
            r#"{"tool_call_id":"call_img","content":"snapshot captured\n\n[IMAGE:data:image/jpeg;base64,/9j/4AAQ]"}"#,
        )];

        let provider = make_model_provider("test", "https://example.com", None);
        assert_eq!(
            provider.tool_result_image_policy,
            ToolResultImagePolicy::ImageUrl
        );
        let converted = provider.convert_messages_for_native(&input, true);
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].role, "tool");
        assert_eq!(converted[0].tool_call_id.as_deref(), Some("call_img"));

        let value = serde_json::to_value(
            converted[0]
                .content
                .as_ref()
                .expect("tool message should carry content"),
        )
        .unwrap();
        let parts = value
            .as_array()
            .expect("tool image content should serialize as a parts array");
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["type"], "text");
        assert_eq!(parts[0]["text"], "snapshot captured");
        assert_eq!(parts[1]["type"], "image_url");
        assert_eq!(
            parts[1]["image_url"]["url"],
            "data:image/jpeg;base64,/9j/4AAQ"
        );
    }

    #[test]
    fn convert_messages_for_native_omits_tool_result_image_payloads() {
        let input = vec![ChatMessage::tool(
            serde_json::json!({
                "tool_call_id": "call_img",
                "content": "before [IMAGE:data:image/jpeg;base64,/9j/4AAQ] middle [IMAGE:https://example.com/secret.png] after"
            })
            .to_string(),
        )];

        let provider = OpenAiCompatibleModelProvider::builder("test")
            .display_name("test")
            .base_url("https://example.com")
            .credential(None)
            .auth_style(AuthStyle::Bearer)
            .tool_result_image_policy(ToolResultImagePolicy::Omit)
            .build();
        let converted = provider.convert_messages_for_native(&input, false);
        let content = serde_json::to_value(
            converted[0]
                .content
                .as_ref()
                .expect("tool message should carry content"),
        )
        .unwrap();
        let content = content.as_str().expect("omitted tool content is text");

        assert_eq!(
            content,
            "before  middle  after\n\n[tool-result image omitted by provider policy]"
        );
        assert_eq!(
            content
                .matches("[tool-result image omitted by provider policy]")
                .count(),
            1
        );
        assert!(!content.contains("data:image"));
        assert!(!content.contains("https://example.com/secret.png"));
        assert!(!content.contains("/9j/4AAQ"));
        assert_eq!(converted[0].tool_call_id.as_deref(), Some("call_img"));
    }

    #[test]
    fn convert_messages_for_native_sanitizes_malformed_tool_result_json() {
        let input = vec![ChatMessage::tool(
            "malformed result [IMAGE:/tmp/secret.png]",
        )];
        let provider = OpenAiCompatibleModelProvider::builder("test")
            .display_name("test")
            .base_url("https://example.com")
            .credential(None)
            .auth_style(AuthStyle::Bearer)
            .tool_result_image_policy(ToolResultImagePolicy::Omit)
            .build();

        let converted = provider.convert_messages_for_native(&input, true);
        let content = serde_json::to_value(
            converted[0]
                .content
                .as_ref()
                .expect("malformed tool message should carry content"),
        )
        .unwrap();
        let content = content.as_str().expect("sanitized fallback should be text");

        assert_eq!(converted[0].role, "tool");
        assert_eq!(
            content,
            "malformed result \n\n[tool-result image omitted by provider policy]"
        );
        assert_eq!(converted[0].tool_call_id, None);
        assert_eq!(converted[0].name, None);
        assert!(!content.contains("[IMAGE:"));
        assert!(!content.contains("/tmp/secret.png"));
    }

    #[test]
    fn convert_messages_for_native_sanitizes_non_string_tool_result_content() {
        let input = vec![ChatMessage::tool(
            serde_json::json!({
                "tool_call_id": "call_obj",
                "name": "read",
                "content": {"payload": "[IMAGE:/tmp/secret.png]"}
            })
            .to_string(),
        )];
        let provider = OpenAiCompatibleModelProvider::builder("test")
            .display_name("test")
            .base_url("https://example.com")
            .credential(None)
            .auth_style(AuthStyle::Bearer)
            .tool_result_image_policy(ToolResultImagePolicy::Omit)
            .build();

        let converted = provider.convert_messages_for_native(&input, true);
        let content = serde_json::to_value(
            converted[0]
                .content
                .as_ref()
                .expect("non-string tool message should carry content"),
        )
        .unwrap();
        let content = content.as_str().expect("sanitized fallback should be text");

        assert_eq!(converted[0].tool_call_id.as_deref(), Some("call_obj"));
        assert_eq!(converted[0].name.as_deref(), Some("read"));
        assert!(content.contains("\"payload\":\""));
        assert!(content.contains(TOOL_RESULT_IMAGE_OMITTED_NOTICE));
        assert_eq!(content.matches(TOOL_RESULT_IMAGE_OMITTED_NOTICE).count(), 1);
        assert!(!content.contains("[IMAGE:"));
        assert!(!content.contains("/tmp/secret.png"));
    }

    #[test]
    fn convert_messages_for_native_sanitizes_unterminated_tool_result_marker() {
        let input = vec![ChatMessage::tool(
            serde_json::json!({
                "tool_call_id": "call_unterminated",
                "content": "prefix [IMAGE:/tmp/secret.png"
            })
            .to_string(),
        )];
        let provider = OpenAiCompatibleModelProvider::builder("test")
            .display_name("test")
            .base_url("https://example.com")
            .credential(None)
            .auth_style(AuthStyle::Bearer)
            .tool_result_image_policy(ToolResultImagePolicy::Omit)
            .build();

        let converted = provider.convert_messages_for_native(&input, true);
        let content = serde_json::to_value(
            converted[0]
                .content
                .as_ref()
                .expect("unterminated tool message should carry content"),
        )
        .unwrap();
        let content = content
            .as_str()
            .expect("sanitized tool content should be text");

        assert_eq!(
            content,
            "prefix \n\n[tool-result image omitted by provider policy]"
        );
        assert_eq!(
            converted[0].tool_call_id.as_deref(),
            Some("call_unterminated")
        );
        assert!(!content.contains("[IMAGE:"));
        assert!(!content.contains("/tmp/secret.png"));
    }

    #[tokio::test]
    async fn chat_with_history_no_tools_sanitizes_tool_result_request_content() {
        let (mut provider, captured, server) = mock_non_streaming_response(serde_json::json!({
            "choices": [{"message": {"content": "ok"}}]
        }))
        .await;
        provider.tool_result_image_policy = ToolResultImagePolicy::Omit;

        let messages = vec![ChatMessage::tool(
            "history [IMAGE:data:image/png;base64,SECRET] tail",
        )];
        let response = provider
            .chat_with_history(&messages, "test-model", None)
            .await
            .expect("chat history should succeed");
        assert_eq!(response, "ok");

        let request = captured
            .lock()
            .expect("capture lock poisoned")
            .pop()
            .expect("server should capture request");
        let content = request["messages"][0]["content"]
            .as_str()
            .expect("tool content should serialize as a string");
        assert_eq!(
            content,
            "history  tail\n\n[tool-result image omitted by provider policy]"
        );
        assert!(!content.contains("[IMAGE:"));
        assert!(!content.contains("data:image"));
        assert!(!content.contains("SECRET"));
        server.abort();
    }

    #[tokio::test]
    async fn chat_with_history_no_tools_sanitizes_escaped_tool_result_marker() {
        let (mut provider, captured, server) = mock_non_streaming_response(serde_json::json!({
            "choices": [{"message": {"content": "ok"}}]
        }))
        .await;
        provider.tool_result_image_policy = ToolResultImagePolicy::Omit;

        let messages = vec![ChatMessage::tool(
            r#"{"tool_call_id":"call_escaped","name":"inspect","content":"before \u005bIMAGE:data:image/png;base64,SECRET] after"}"#,
        )];
        provider
            .chat_with_history(&messages, "test-model", None)
            .await
            .expect("chat history should succeed");

        let request = captured
            .lock()
            .expect("capture lock poisoned")
            .pop()
            .expect("server should capture request");
        let envelope: serde_json::Value = serde_json::from_str(
            request["messages"][0]["content"]
                .as_str()
                .expect("tool envelope should serialize as a string"),
        )
        .expect("tool envelope remains valid JSON");

        assert_eq!(envelope["tool_call_id"], "call_escaped");
        assert_eq!(envelope["name"], "inspect");
        assert_eq!(
            envelope["content"],
            "before  after\n\n[tool-result image omitted by provider policy]"
        );
        let serialized = envelope.to_string();
        assert!(!serialized.contains("[IMAGE:"));
        assert!(!serialized.contains("data:image"));
        assert!(!serialized.contains("SECRET"));
        server.abort();
    }

    #[test]
    fn convert_messages_for_native_omits_older_tool_results_across_rounds() {
        let messages = vec![
            ChatMessage::assistant(
                serde_json::json!({
                    "content": "",
                    "tool_calls": [{
                        "id": "call_old",
                        "name": "first",
                        "arguments": "{}"
                    }]
                })
                .to_string(),
            ),
            ChatMessage::tool(
                serde_json::json!({
                    "tool_call_id": "call_old",
                    "content": "old result [IMAGE:/tmp/old.png]"
                })
                .to_string(),
            ),
            ChatMessage::assistant(
                serde_json::json!({
                    "content": "",
                    "tool_calls": [{
                        "id": "call_new",
                        "name": "second",
                        "arguments": "{}"
                    }]
                })
                .to_string(),
            ),
            ChatMessage::tool(
                serde_json::json!({
                    "tool_call_id": "call_new",
                    "content": "new result [IMAGE:data:image/png;base64,NEW]"
                })
                .to_string(),
            ),
        ];

        let provider = OpenAiCompatibleModelProvider::builder("test")
            .display_name("test")
            .base_url("https://example.com")
            .credential(None)
            .auth_style(AuthStyle::Bearer)
            .tool_result_image_policy(ToolResultImagePolicy::Omit)
            .build();
        let converted = provider.convert_messages_for_native(&messages, true);

        assert_eq!(converted.len(), 4);
        for (index, expected_id) in [(1, "call_old"), (3, "call_new")] {
            assert_eq!(converted[index].role, "tool");
            assert_eq!(converted[index].tool_call_id.as_deref(), Some(expected_id));
            assert_eq!(
                converted[index].name.as_deref(),
                Some(if index == 1 { "first" } else { "second" })
            );
            let content = serde_json::to_value(
                converted[index]
                    .content
                    .as_ref()
                    .expect("historical tool result should carry content"),
            )
            .unwrap();
            let content = content.as_str().expect("omitted tool content is text");
            assert!(content.ends_with("[tool-result image omitted by provider policy]"));
            assert!(!content.contains("[IMAGE:"));
            assert!(!content.contains("data:image"));
            assert!(!content.contains("/tmp/old.png"));
            assert!(!content.contains("base64,NEW"));
        }
    }

    #[test]
    fn convert_messages_for_native_keeps_direct_user_images_under_omit_policy() {
        let input = vec![ChatMessage::user(
            "describe this [IMAGE:data:image/png;base64,USER]",
        )];
        let provider = OpenAiCompatibleModelProvider::builder("test")
            .display_name("test")
            .base_url("https://example.com")
            .credential(None)
            .auth_style(AuthStyle::Bearer)
            .tool_result_image_policy(ToolResultImagePolicy::Omit)
            .build();

        let converted = provider.convert_messages_for_native(&input, true);
        let content = serde_json::to_value(
            converted[0]
                .content
                .as_ref()
                .expect("user message should carry content"),
        )
        .unwrap();
        let parts = content
            .as_array()
            .expect("direct user image should remain structured");
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["type"], "text");
        assert_eq!(parts[1]["type"], "image_url");
        assert_eq!(parts[1]["image_url"]["url"], "data:image/png;base64,USER");
    }

    #[test]
    fn convert_messages_for_native_tool_result_resolves_name_from_tool_name_map() {
        let history_json = serde_json::json!({
            "content": "",
            "tool_calls": [{
                "id": "call_abc",
                "name": "shell",
                "arguments": "{\"cmd\":\"pwd\"}"
            }]
        });
        let messages = vec![
            ChatMessage::assistant(history_json.to_string()),
            ChatMessage::tool(
                serde_json::json!({
                    "tool_call_id": "call_abc",
                    "content": "done"
                })
                .to_string(),
            ),
        ];

        let provider = make_model_provider("test", "https://example.com", None);
        let native = provider.convert_messages_for_native(&messages, true);
        assert_eq!(native.len(), 2);
        assert_eq!(native[0].role, "assistant");
        let tool_msg = &native[1];
        assert_eq!(tool_msg.role, "tool");
        assert_eq!(
            tool_msg.name.as_deref(),
            Some("shell"),
            "tool name should resolve from paired assistant tool-call"
        );
    }

    #[test]
    fn convert_messages_for_native_keeps_tool_result_image_markers_as_text_when_disabled() {
        // Models that don't accept structured image parts (the same gate that
        // keeps user image markers as text) must keep tool-result markers
        // verbatim — preserving prior behavior and thesafety posture.
        let input = vec![ChatMessage::tool(
            r#"{"tool_call_id":"call_img","content":"snapshot captured\n\n[IMAGE:data:image/jpeg;base64,/9j/4AAQ]"}"#,
        )];

        let provider = make_model_provider("test", "https://example.com", None);
        let converted = provider.convert_messages_for_native(&input, false);
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].role, "tool");
        assert!(matches!(
            converted[0].content.as_ref(),
            Some(MessageContent::Text(value))
                if value == "snapshot captured\n\n[IMAGE:data:image/jpeg;base64,/9j/4AAQ]"
        ));
    }

    #[test]
    fn convert_messages_for_native_tool_result_falls_back_to_content_name() {
        // When there is no paired assistant tool-call, the tool message's
        // own "name" field should be used as a fallback.
        let messages = vec![ChatMessage::tool(
            serde_json::json!({
                "tool_call_id": "call_xyz",
                "name": "read",
                "content": "file contents"
            })
            .to_string(),
        )];

        let provider = make_model_provider("test", "https://example.com", None);
        let native = provider.convert_messages_for_native(&messages, true);
        assert_eq!(native.len(), 1);
        assert_eq!(native[0].role, "tool");
        assert_eq!(
            native[0].name.as_deref(),
            Some("read"),
            "tool name should fall back to the content name field"
        );
    }

    #[test]
    fn native_message_name_serialized_only_when_present() {
        // Role "tool" messages must include `name` when set; non-tool
        // messages and tool messages without a name must omit the key.
        let tool_with_name = NativeMessage {
            role: "tool".to_string(),
            content: Some(MessageContent::Text("result".to_string())),
            tool_call_id: Some("call_1".to_string()),
            tool_calls: None,
            reasoning_content: None,
            reasoning: None,
            name: Some("shell".to_string()),
        };
        let json = serde_json::to_string(&tool_with_name).unwrap();
        assert!(
            json.contains("\"name\":\"shell\""),
            "name should be present when Some for tool messages"
        );

        let tool_without_name = NativeMessage {
            role: "tool".to_string(),
            content: Some(MessageContent::Text("result".to_string())),
            tool_call_id: Some("call_2".to_string()),
            tool_calls: None,
            reasoning_content: None,
            reasoning: None,
            name: None,
        };
        let json = serde_json::to_string(&tool_without_name).unwrap();
        assert!(
            !json.contains("\"name\""),
            "name should be omitted when None"
        );

        let assistant_msg = NativeMessage {
            role: "assistant".to_string(),
            content: Some(MessageContent::Text("hello".to_string())),
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
            reasoning: None,
            name: None,
        };
        let json = serde_json::to_string(&assistant_msg).unwrap();
        assert!(
            !json.contains("\"name\""),
            "name should be omitted for non-tool messages"
        );
    }

    #[test]
    fn native_chat_request_mistral_serializes_matching_valid_tool_call_ids() {
        let provider = make_model_provider("Mistral", "https://api.mistral.ai/v1", None);
        let invalid_id = "chatcmpl-tool-abc";
        let history_json = serde_json::json!({
            "content": "",
            "tool_calls": [{
                "id": invalid_id,
                "name": "shell",
                "arguments": "{\"cmd\":\"pwd\"}"
            }]
        });
        let messages = vec![
            ChatMessage::assistant(history_json.to_string()),
            ChatMessage::tool(
                serde_json::json!({
                    "tool_call_id": invalid_id,
                    "content": "done"
                })
                .to_string(),
            ),
        ];

        let req = NativeChatRequest {
            model: "mistral-large-latest".to_string(),
            messages: provider.convert_messages_for_native(&messages, true),
            temperature: Some(0.7),
            stream: Some(false),
            stream_options: None,
            reasoning_effort: None,
            tool_stream: None,
            tools: Some(vec![NativeToolSpec {
                kind: "function".to_string(),
                extra: serde_json::Map::new(),
                function: NativeToolFunctionSpec {
                    extra: serde_json::Map::new(),
                    name: "shell".to_string(),
                    description: "Run a shell command".to_string(),
                    parameters: std::sync::Arc::new(serde_json::json!({"type": "object"})),
                },
            }]),
            tool_choice: Some("auto".to_string()),
            max_tokens: None,
            extra_body: None,
        };

        let value = serde_json::to_value(&req).unwrap();
        let assistant_id = value["messages"][0]["tool_calls"][0]["id"]
            .as_str()
            .expect("assistant tool call id should serialize");
        let tool_id = value["messages"][1]["tool_call_id"]
            .as_str()
            .expect("tool result id should serialize");

        assert_ne!(assistant_id, invalid_id);
        assert!(is_valid_mistral_tool_call_id(assistant_id));
        assert_eq!(assistant_id, tool_id);
    }

    #[test]
    fn convert_messages_for_native_keeps_user_image_markers_as_text_when_disabled() {
        let input = vec![ChatMessage::user(
            "System primer [IMAGE:data:image/png;base64,abcd] user turn",
        )];

        let provider = make_model_provider("test", "https://example.com", None);
        let converted = provider.convert_messages_for_native(&input, false);
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].role, "user");
        assert!(matches!(
            converted[0].content.as_ref(),
            Some(MessageContent::Text(value))
                if value == "System primer [IMAGE:data:image/png;base64,abcd] user turn"
        ));
    }

    #[test]
    fn flatten_system_messages_merges_into_first_user() {
        let input = vec![
            ChatMessage::system("core policy"),
            ChatMessage::assistant("ack"),
            ChatMessage::system("delivery rules"),
            ChatMessage::user("hello"),
            ChatMessage::assistant("post-user"),
        ];

        let (output, system_merged) =
            OpenAiCompatibleModelProvider::flatten_system_messages(&input, true);
        assert!(system_merged, "merged into the existing user message");
        assert_eq!(output.len(), 3);
        assert_eq!(output[0].role, "assistant");
        assert_eq!(output[0].content, "ack");
        assert_eq!(output[1].role, "user");
        assert_eq!(output[1].content, "core policy\n\ndelivery rules\n\nhello");
        assert_eq!(output[2].role, "assistant");
        assert_eq!(output[2].content, "post-user");
        assert!(output.iter().all(|m| m.role != "system"));
    }

    #[test]
    fn flatten_system_messages_inserts_user_when_missing() {
        let input = vec![
            ChatMessage::system("core policy"),
            ChatMessage::assistant("ack"),
        ];

        let (output, system_merged) =
            OpenAiCompatibleModelProvider::flatten_system_messages(&input, true);
        assert!(system_merged, "synthetic user inserted as the carrier");
        assert_eq!(output.len(), 2);
        assert_eq!(output[0].role, "user");
        assert_eq!(output[0].content, "core policy");
        assert_eq!(output[1].role, "assistant");
        assert_eq!(output[1].content, "ack");
    }

    #[test]
    fn effective_content_preserves_literal_think_tags() {
        // The deleted `strip_think_tags()` helper searched for the exact
        // substring `<think>` / `</think>` and stripped those blocks
        // unconditionally. This regression pins that literal `<think>` tags
        // now round-trip byte-for-byte, including legitimate uses where the
        // model legitimately discusses the tag (HTML sample, code quoting,
        // meta-discussion).
        let json = r#"{"choices":[{"message":{"content":"Here is the HTML: <think>internal note</think>"}}]}"#;
        let resp: ApiChatResponse = serde_json::from_str(json).unwrap();
        let msg = &resp.choices[0].message;
        assert_eq!(
            msg.effective_content(),
            "Here is the HTML: <think>internal note</think>"
        );
    }

    #[test]
    fn native_tool_schema_unsupported_detection_is_precise() {
        assert!(
            OpenAiCompatibleModelProvider::is_native_tool_schema_unsupported(
                reqwest::StatusCode::BAD_REQUEST,
                "unknown parameter: tools"
            )
        );
        assert!(
            !OpenAiCompatibleModelProvider::is_native_tool_schema_unsupported(
                reqwest::StatusCode::UNAUTHORIZED,
                "unknown parameter: tools"
            )
        );
    }

    #[test]
    fn native_tool_schema_unsupported_detects_groq_tool_validation_error() {
        assert!(
            OpenAiCompatibleModelProvider::is_native_tool_schema_unsupported(
                reqwest::StatusCode::BAD_REQUEST,
                r#"Groq API error (400 Bad Request): {"error":{"message":"tool call validation failed: attempted to call tool 'memory_recall={\"limit\":5}' which was not in request"}}"#
            )
        );
    }

    #[test]
    fn prompt_guided_tool_fallback_injects_system_instruction() {
        let input = vec![ChatMessage::user("check status")];
        let tools = vec![clawcrew_api::tool::ToolSpec::new(
            "shell_exec",
            "Execute shell command",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" }
                },
                "required": ["command"]
            }),
        )];

        let output = OpenAiCompatibleModelProvider::with_prompt_guided_tool_instructions(
            &input,
            Some(&tools),
        );
        assert!(!output.is_empty());
        assert_eq!(output[0].role, "system");
        assert!(output[0].content.contains("Available Tools"));
        assert!(output[0].content.contains("shell_exec"));
    }

    #[test]
    fn reasoning_effort_only_applies_to_openai_and_selected_codex_models() {
        let model_provider = OpenAiCompatibleModelProvider::builder("test")
            .display_name("test")
            .base_url("https://example.com")
            .credential(None)
            .auth_style(AuthStyle::Bearer)
            .reasoning_effort(Some("high".to_string()))
            .build();

        assert_eq!(
            model_provider.reasoning_effort_for_model("o1-preview"),
            Some("high".to_string())
        );
        assert_eq!(
            model_provider.reasoning_effort_for_model("openai/o3-mini"),
            Some("high".to_string())
        );
        assert_eq!(
            model_provider.reasoning_effort_for_model("o4-mini"),
            Some("high".to_string())
        );
        assert_eq!(
            model_provider.reasoning_effort_for_model("gpt-5"),
            Some("high".to_string())
        );
        assert_eq!(
            model_provider.reasoning_effort_for_model("gpt-5.3-codex"),
            Some("high".to_string())
        );
        assert_eq!(
            model_provider.reasoning_effort_for_model("openai/gpt-5"),
            Some("high".to_string())
        );
        assert_eq!(
            model_provider.reasoning_effort_for_model("gpt-5-chat-latest"),
            None,
            "gpt-5*-chat-latest are non-reasoning chat-router models and must not receive reasoning_effort",
        );
        assert_eq!(
            model_provider.reasoning_effort_for_model("gpt-5.1-chat-latest"),
            None,
            "gpt-5*-chat-latest are non-reasoning chat-router models and must not receive reasoning_effort",
        );
        assert_eq!(
            model_provider.reasoning_effort_for_model("gpt-4-codex"),
            Some("high".to_string())
        );
        assert_eq!(
            model_provider.reasoning_effort_for_model("llama-3-codex"),
            None,
            "generic codex-like model names must not receive OpenAI-only reasoning_effort",
        );
        assert_eq!(
            model_provider.reasoning_effort_for_model("llama-3.3-70b"),
            None
        );
    }

    #[tokio::test]
    async fn warmup_uses_models_endpoint_with_existing_auth() {
        use axum::{
            Router,
            body::Body,
            http::{Request, StatusCode},
        };
        use tokio::net::TcpListener;

        for (base_path, status, auth_style, credential, header) in [
            (
                "",
                200,
                AuthStyle::Bearer,
                Some("test-key"),
                Some(("authorization", "Bearer test-key")),
            ),
            (
                "/v1",
                401,
                AuthStyle::Bearer,
                Some("test-key"),
                Some(("authorization", "Bearer test-key")),
            ),
            (
                "/v1/",
                404,
                AuthStyle::XApiKey,
                Some("test-key"),
                Some(("x-api-key", "test-key")),
            ),
            (
                "/custom/api",
                405,
                AuthStyle::Custom("x-provider-key".into()),
                Some("test-key"),
                Some(("x-provider-key", "test-key")),
            ),
            ("/v1", 500, AuthStyle::Bearer, None, None),
        ] {
            let (tx, mut rx) = tokio::sync::mpsc::channel(1);
            let app = Router::new().fallback(move |request: Request<Body>| {
                let tx = tx.clone();
                async move {
                    tx.send((
                        request.method().clone(),
                        request.uri().path().to_owned(),
                        request.headers().clone(),
                    ))
                    .await
                    .unwrap();
                    (StatusCode::from_u16(status).unwrap(), "probe body")
                }
            });
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let server = ::clawcrew_spawn::spawn!(async move {
                axum::serve(listener, app).await.unwrap();
            });
            let provider = OpenAiCompatibleModelProvider::builder("test")
                .display_name("test")
                .base_url(&format!("http://{addr}{base_path}"))
                .credential(credential)
                .auth_style(auth_style)
                .extra_headers(std::collections::HashMap::from([(
                    "x-probe-test".into(),
                    "preserved".into(),
                )]))
                .api_path((status == 500).then(|| "/inference".to_string()))
                .build();
            let result = provider.warmup().await;
            server.abort();
            assert!(
                result.is_ok(),
                "HTTP {status} must not fail warmup: {result:?}"
            );
            let (method, path, headers) = rx.recv().await.unwrap();
            assert_eq!(method, "GET");
            assert_eq!(headers.get("x-probe-test").unwrap(), "preserved");
            assert_eq!(path, format!("{}/models", base_path.trim_end_matches('/')));
            if let Some((name, value)) = header {
                assert_eq!(headers.get(name).unwrap(), value);
            } else {
                assert!(!headers.contains_key("authorization"));
            }
        }
    }

    #[tokio::test]
    async fn warmup_drains_response_with_configured_timeout() {
        use axum::{
            Router,
            body::{Body, Bytes},
        };
        let app = Router::new().fallback(|| async {
            Body::from_stream(futures_util::stream::once(async {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                Ok::<_, std::io::Error>(Bytes::from_static(b"catalog"))
            }))
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = ::clawcrew_spawn::spawn!(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let provider = OpenAiCompatibleModelProvider::builder("test")
            .display_name("test")
            .base_url(&format!("http://{addr}"))
            .auth_style(AuthStyle::Bearer)
            .timeout_secs(1)
            .build();
        let result = provider.warmup().await;
        server.abort();
        assert!(
            result
                .unwrap_err()
                .downcast_ref::<reqwest::Error>()
                .unwrap()
                .is_timeout()
        );
    }

    #[tokio::test]
    async fn warmup_without_key_attempts_connection() {
        let model_provider = make_model_provider("test", "http://127.0.0.1:1", None);
        let result = model_provider.warmup().await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            !err_msg.contains("API key not set"),
            "should not get credential error, got: {err_msg}"
        );
    }

    // ══════════════════════════════════════════════════════════
    // Native tool calling tests
    // ══════════════════════════════════════════════════════════

    #[test]
    fn capabilities_reports_native_tool_calling() {
        let p = make_model_provider("test", "https://example.com", None);
        let caps = <OpenAiCompatibleModelProvider as ModelProvider>::capabilities(&p);
        assert!(caps.native_tool_calling);
        assert!(!caps.vision);
    }

    #[test]
    fn capabilities_reports_vision_for_qwen_compatible_provider() {
        let p = OpenAiCompatibleModelProvider::builder("test")
            .display_name("Qwen")
            .base_url("https://dashscope.aliyuncs.com/compatible-mode/v1")
            .credential(Some("k"))
            .auth_style(AuthStyle::Bearer)
            .vision(true)
            .build();
        let caps = <OpenAiCompatibleModelProvider as ModelProvider>::capabilities(&p);
        assert!(caps.native_tool_calling);
        assert!(caps.vision);
    }

    #[test]
    fn minimax_provider_supports_native_tool_calling_with_system_merge() {
        let p = OpenAiCompatibleModelProvider::builder("test")
            .display_name("MiniMax")
            .base_url("https://api.minimax.chat/v1")
            .credential(Some("k"))
            .auth_style(AuthStyle::Bearer)
            .merge_system_into_user_preserving_native()
            .build();
        let caps = <OpenAiCompatibleModelProvider as ModelProvider>::capabilities(&p);
        assert!(
            caps.native_tool_calling,
            "MiniMax should preserve native tool calling when system messages are merged"
        );
        assert!(!caps.vision);
    }

    #[test]
    fn strip_native_tool_messages_removes_tool_and_tool_calls() {
        let messages = vec![
            ChatMessage::system("sys"),
            ChatMessage::user("search for cats"),
            ChatMessage::assistant(
                r#"{"content":"I'll search","tool_calls":[{"id":"chatcmpl-tool-abc","name":"web_search","arguments":"{}"}]}"#,
            ),
            ChatMessage::tool(
                r#"{"tool_call_id":"chatcmpl-tool-abc","content":"Found 10 results"}"#,
            ),
            ChatMessage::assistant("Here are the results about cats"),
            ChatMessage::user("thanks"),
        ];
        let p = OpenAiCompatibleModelProvider::builder("test")
            .display_name("MiniMax")
            .base_url("https://api.minimax.chat/v1")
            .credential(Some("k"))
            .auth_style(AuthStyle::Bearer)
            .merge_system_into_user()
            .build();
        let stripped = p.strip_native_tool_messages(&messages);
        // tool message dropped; the pre-tool narration and the reply that
        // follows the tool result are now coalesced into a single assistant
        // message so the output never contains consecutive assistants.
        assert_eq!(stripped.len(), 4);
        assert_eq!(stripped[0].role, "system");
        assert_eq!(stripped[1].role, "user");
        assert_eq!(stripped[1].content, "search for cats");
        assert_eq!(stripped[2].role, "assistant");
        assert!(
            stripped[2].content.starts_with("I'll search"),
            "coalesced assistant must preserve the pre-tool narration; got {:?}",
            stripped[2].content
        );
        assert!(
            stripped[2]
                .content
                .contains("Here are the results about cats"),
            "coalesced assistant must preserve the post-tool reply; got {:?}",
            stripped[2].content
        );
        assert!(
            !stripped[2].content.contains("tool_calls"),
            "tool_calls structure must be stripped"
        );
        assert_eq!(stripped[3].role, "user");
    }

    #[test]
    fn strip_native_tool_messages_drops_empty_assistant_tool_calls() {
        let messages = vec![
            ChatMessage::system("sys"),
            ChatMessage::user("do it"),
            ChatMessage::assistant(
                r#"{"content":"","tool_calls":[{"id":"tc1","name":"shell","arguments":"{}"}]}"#,
            ),
            ChatMessage::tool(r#"{"tool_call_id":"tc1","content":"ok"}"#),
            ChatMessage::assistant("Done"),
        ];
        let p = OpenAiCompatibleModelProvider::builder("test")
            .display_name("MiniMax")
            .base_url("https://api.minimax.chat/v1")
            .credential(Some("k"))
            .auth_style(AuthStyle::Bearer)
            .merge_system_into_user()
            .build();
        let stripped = p.strip_native_tool_messages(&messages);
        // assistant with empty content + tool_calls → dropped; tool → dropped
        assert_eq!(stripped.len(), 3);
        assert_eq!(stripped[0].role, "system");
        assert_eq!(stripped[1].role, "user");
        assert_eq!(stripped[2].role, "assistant");
        assert_eq!(stripped[2].content, "Done");
    }

    #[test]
    fn strip_native_tool_messages_preserves_regular_messages() {
        let messages = vec![
            ChatMessage::system("sys"),
            ChatMessage::user("hello"),
            ChatMessage::assistant("hi there"),
            ChatMessage::user("bye"),
        ];
        let p = OpenAiCompatibleModelProvider::builder("test")
            .display_name("MiniMax")
            .base_url("https://api.minimax.chat/v1")
            .credential(Some("k"))
            .auth_style(AuthStyle::Bearer)
            .merge_system_into_user()
            .build();
        let stripped = p.strip_native_tool_messages(&messages);
        assert_eq!(stripped.len(), 4);
        for (orig, result) in messages.iter().zip(stripped.iter()) {
            assert_eq!(orig.role, result.role);
            assert_eq!(orig.content, result.content);
        }
    }

    #[test]
    fn strip_native_tool_messages_passthrough_when_native_tool_calling_enabled() {
        let messages = vec![
            ChatMessage::system("sys"),
            ChatMessage::user("search for cats"),
            ChatMessage::assistant(
                r#"{"content":"I'll search","tool_calls":[{"id":"chatcmpl-tool-abc","name":"web_search","arguments":"{}"}]}"#,
            ),
            ChatMessage::tool(
                r#"{"tool_call_id":"chatcmpl-tool-abc","content":"Found 10 results"}"#,
            ),
            ChatMessage::assistant("Here are the results about cats"),
        ];
        let p = OpenAiCompatibleModelProvider::builder("test")
            .display_name("NativeToolProvider")
            .base_url("https://api.example.com/v1")
            .credential(Some("k"))
            .auth_style(AuthStyle::Bearer)
            .build();
        assert!(
            <OpenAiCompatibleModelProvider as ModelProvider>::capabilities(&p).native_tool_calling,
            "model_provider must have native_tool_calling enabled for this test"
        );
        let result = p.strip_native_tool_messages(&messages);
        assert_eq!(result.len(), messages.len());
        for (orig, out) in messages.iter().zip(result.iter()) {
            assert_eq!(orig.role, out.role);
            assert_eq!(orig.content, out.content);
        }
    }

    #[test]
    fn user_agent_constructor_keeps_native_tool_calling_enabled() {
        let p = OpenAiCompatibleModelProvider::builder("test")
            .display_name("TestProvider")
            .base_url("https://example.com")
            .credential(Some("k"))
            .auth_style(AuthStyle::Bearer)
            .user_agent("clawcrew-test/1.0")
            .build();
        let caps = <OpenAiCompatibleModelProvider as ModelProvider>::capabilities(&p);
        assert!(caps.native_tool_calling);
        assert!(!caps.vision);
        assert_eq!(p.user_agent.as_deref(), Some("clawcrew-test/1.0"));
    }

    #[test]
    fn user_agent_and_vision_constructor_preserves_capability_flags() {
        let p = OpenAiCompatibleModelProvider::builder("test")
            .display_name("VisionModelProvider")
            .base_url("https://example.com")
            .credential(Some("k"))
            .auth_style(AuthStyle::Bearer)
            .user_agent("clawcrew-test/vision")
            .vision(true)
            .build();
        let caps = <OpenAiCompatibleModelProvider as ModelProvider>::capabilities(&p);
        assert!(caps.native_tool_calling);
        assert!(caps.vision);
        assert_eq!(p.user_agent.as_deref(), Some("clawcrew-test/vision"));
    }

    #[test]
    fn to_message_content_converts_image_markers_to_openai_parts() {
        let content = "Describe this\n\n[IMAGE:data:image/png;base64,abcd]";
        let value = serde_json::to_value(OpenAiCompatibleModelProvider::to_message_content(
            "user", content, true,
        ))
        .unwrap();
        let parts = value
            .as_array()
            .expect("multimodal content should be an array");
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["type"], "text");
        assert_eq!(parts[0]["text"], "Describe this");
        assert_eq!(parts[1]["type"], "image_url");
        assert_eq!(parts[1]["image_url"]["url"], "data:image/png;base64,abcd");
    }

    #[test]
    fn to_message_content_keeps_markers_as_text_when_user_image_parts_disabled() {
        let content = "Policy [IMAGE:data:image/png;base64,abcd]";
        let value = serde_json::to_value(OpenAiCompatibleModelProvider::to_message_content(
            "user", content, false,
        ))
        .unwrap();
        assert_eq!(value, serde_json::json!(content));
    }

    #[test]
    fn to_message_content_keeps_plain_text_for_non_user_roles() {
        let value = serde_json::to_value(OpenAiCompatibleModelProvider::to_message_content(
            "system",
            "You are a helpful assistant.",
            true,
        ))
        .unwrap();
        assert_eq!(value, serde_json::json!("You are a helpful assistant."));
    }

    #[tokio::test]
    async fn normalize_messages_for_upstream_rewrites_local_image_path_to_data_uri() {
        // bare local paths inside `[IMAGE:...]` markers
        // must be base64-encoded at the provider boundary so strict upstreams
        // (vLLM 0.20+) never see `image_url.url = "/home/.../photo.png"`.
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let path = tmp.path().join("pixel.png");
        // 1x1 transparent PNG.
        let png: [u8; 67] = [
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        std::fs::write(&path, png).expect("write pixel.png");
        let path_str = path.to_string_lossy().into_owned();

        let msg = ChatMessage {
            role: "user".into(),
            content: format!("Caption please [IMAGE:{}]", path_str),
        };

        let mut provider = make_model_provider("test", "https://example.com", None);
        provider.tool_result_image_policy = ToolResultImagePolicy::Omit;
        let normalized = provider
            .normalize_messages_for_upstream(std::slice::from_ref(&msg))
            .await
            .expect("normalize ok");

        assert_eq!(normalized.len(), 1);
        let content = &normalized[0].content;
        assert!(
            content.contains("[IMAGE:data:image/png;base64,"),
            "expected base64 data URI in normalized content, got: {content}"
        );
        assert!(
            !content.contains(&path_str),
            "raw local path must not leak to upstream, got: {content}"
        );
    }

    #[tokio::test]
    async fn normalize_messages_for_upstream_omit_preserves_tool_envelope() {
        let provider = OpenAiCompatibleModelProvider::builder("test")
            .display_name("test")
            .base_url("https://example.com")
            .credential(None)
            .auth_style(AuthStyle::Bearer)
            .tool_result_image_policy(ToolResultImagePolicy::Omit)
            .build();
        let message = ChatMessage::tool(
            r#"{"tool_call_id":"call_image","name":"inspect","content":"before \u005bIMAGE:/tmp/secret.png] after"}"#,
        );

        let normalized = provider
            .normalize_messages_for_upstream(std::slice::from_ref(&message))
            .await
            .expect("normalize ok");
        let envelope: serde_json::Value =
            serde_json::from_str(&normalized[0].content).expect("tool envelope remains valid JSON");

        assert_eq!(envelope["tool_call_id"], "call_image");
        assert_eq!(envelope["name"], "inspect");
        assert_eq!(
            envelope["content"],
            "before  after\n\n[tool-result image omitted by provider policy]"
        );
        assert!(!normalized[0].content.contains("[IMAGE:"));
        assert!(!normalized[0].content.contains("/tmp/secret.png"));
    }

    #[test]
    fn request_serializes_with_tools() {
        let tools = vec![serde_json::json!({
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "Get weather for a location",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "location": {"type": "string"}
                    }
                }
            }
        })];

        let req = ApiChatRequest {
            model: "test-model".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::Text("What is the weather?".to_string()),
            }],
            temperature: Some(0.7),
            stream: Some(false),
            stream_options: None,
            reasoning_effort: None,
            tool_stream: None,
            tools: Some(tools),
            tool_choice: Some("auto".to_string()),
            max_tokens: None,
            extra_body: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"tools\""));
        assert!(json.contains("get_weather"));
        assert!(json.contains("\"tool_choice\":\"auto\""));
    }

    #[test]
    fn zai_tool_requests_enable_tool_stream() {
        let model_provider = make_model_provider("zai", "https://api.z.ai/api/paas/v4", None);
        let req = ApiChatRequest {
            model: "glm-5".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::Text("List /tmp".to_string()),
            }],
            temperature: Some(0.7),
            stream: Some(false),
            stream_options: None,
            reasoning_effort: None,
            tool_stream: model_provider.tool_stream_for_tools(true),
            tools: Some(vec![serde_json::json!({
                "type": "function",
                "function": {
                    "name": "shell",
                    "description": "Run a shell command",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "command": {"type": "string"}
                        }
                    }
                }
            })]),
            tool_choice: Some("auto".to_string()),
            max_tokens: None,
            extra_body: None,
        };

        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"tool_stream\":true"));
    }

    #[test]
    fn non_zai_tool_requests_omit_tool_stream() {
        let model_provider = make_model_provider("test", "https://api.example.com/v1", None);
        let req = ApiChatRequest {
            model: "test-model".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::Text("List /tmp".to_string()),
            }],
            temperature: Some(0.7),
            stream: Some(false),
            stream_options: None,
            reasoning_effort: None,
            tool_stream: model_provider.tool_stream_for_tools(true),
            tools: Some(vec![serde_json::json!({
                "type": "function",
                "function": {
                    "name": "shell",
                    "description": "Run a shell command",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "command": {"type": "string"}
                        }
                    }
                }
            })]),
            tool_choice: Some("auto".to_string()),
            max_tokens: None,
            extra_body: None,
        };

        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("\"tool_stream\""));
    }

    #[test]
    fn non_zai_provider_omits_tool_stream_regardless_of_streaming() {
        let model_provider = make_model_provider("custom", "https://proxy.example.com/v1", None);
        // tool_stream_for_tools should return None for non-Z.AI model_providers
        assert_eq!(model_provider.tool_stream_for_tools(true), None);
        assert_eq!(model_provider.tool_stream_for_tools(false), None);
    }

    #[test]
    fn z_ai_host_enables_tool_stream_for_custom_profiles() {
        let model_provider =
            make_model_provider("custom", "https://api.z.ai/api/coding/paas/v4", None);
        assert_eq!(model_provider.tool_stream_for_tools(true), Some(true));
    }

    #[test]
    fn response_with_tool_calls_deserializes() {
        let json = r#"{
            "choices": [{
                "message": {
                    "content": null,
                    "tool_calls": [{
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"location\":\"London\"}"
                        }
                    }]
                }
            }]
        }"#;

        let resp: ApiChatResponse = serde_json::from_str(json).unwrap();
        let msg = &resp.choices[0].message;
        assert!(msg.content.is_none());
        let tool_calls = msg.tool_calls.as_ref().unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(
            tool_calls[0].function.as_ref().unwrap().name.as_deref(),
            Some("get_weather")
        );
        assert_eq!(
            tool_calls[0]
                .function
                .as_ref()
                .unwrap()
                .arguments
                .as_deref(),
            Some("{\"location\":\"London\"}")
        );
    }

    #[test]
    fn response_with_multiple_tool_calls() {
        let json = r#"{
            "choices": [{
                "message": {
                    "content": "I'll check both.",
                    "tool_calls": [
                        {
                            "type": "function",
                            "function": {
                                "name": "get_weather",
                                "arguments": "{\"location\":\"London\"}"
                            }
                        },
                        {
                            "type": "function",
                            "function": {
                                "name": "get_time",
                                "arguments": "{\"timezone\":\"UTC\"}"
                            }
                        }
                    ]
                }
            }]
        }"#;

        let resp: ApiChatResponse = serde_json::from_str(json).unwrap();
        let msg = &resp.choices[0].message;
        assert_eq!(msg.content.as_deref(), Some("I'll check both."));
        let tool_calls = msg.tool_calls.as_ref().unwrap();
        assert_eq!(tool_calls.len(), 2);
        assert_eq!(
            tool_calls[0].function.as_ref().unwrap().name.as_deref(),
            Some("get_weather")
        );
        assert_eq!(
            tool_calls[1].function.as_ref().unwrap().name.as_deref(),
            Some("get_time")
        );
    }

    #[tokio::test]
    async fn chat_with_tools_without_key_attempts_request() {
        let p = make_model_provider("TestProvider", "http://127.0.0.1:1", None);
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: "hello".to_string(),
        }];
        let tools = vec![serde_json::json!({
            "type": "function",
            "function": {
                "name": "test_tool",
                "description": "A test tool",
                "parameters": {}
            }
        })];

        let result = p
            .chat_with_tools(&messages, &tools, "model", Some(0.7))
            .await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            !err_msg.contains("API key not set"),
            "should not get credential error, got: {err_msg}"
        );
    }

    #[test]
    fn chat_with_tools_request_preserves_reasoning_content_in_history() {
        let p = make_model_provider("DeepSeek", "https://api.deepseek.example/v1", None);
        let history_json = serde_json::json!({
            "content": "I will inspect the workspace.",
            "tool_calls": [{
                "id": "call_1",
                "name": "shell",
                "arguments": "{\"cmd\":\"ls\"}"
            }],
            "reasoning_content": "Need to inspect the current files before answering."
        });
        let messages = vec![
            ChatMessage::assistant(history_json.to_string()),
            ChatMessage::tool(r#"{"tool_call_id":"call_1","content":"src\nCargo.toml"}"#),
            ChatMessage::user("continue"),
        ];
        let tools = vec![NativeToolSpec {
            kind: "function".to_string(),
            extra: serde_json::Map::new(),
            function: NativeToolFunctionSpec {
                extra: serde_json::Map::new(),
                name: "shell".to_string(),
                description: "Run a shell command".to_string(),
                parameters: std::sync::Arc::new(serde_json::json!({})),
            },
        }];

        let request = p.build_native_tool_chat_request(
            &messages,
            Some(tools),
            "deepseek-v4-flash",
            Some(0.7),
            true,
            false,
        );
        let value = serde_json::to_value(&request).unwrap();
        let first_message = &value["messages"][0];

        assert_eq!(first_message["role"], "assistant");
        assert_eq!(
            first_message["reasoning_content"],
            "Need to inspect the current files before answering."
        );
        assert!(
            first_message["tool_calls"].is_array(),
            "assistant tool-call history must stay native in chat_with_tools requests"
        );
        assert_eq!(value["tools"][0]["function"]["name"], "shell");
        assert_eq!(value["tool_choice"], "auto");
    }

    #[test]
    fn response_with_no_tool_calls_has_empty_vec() {
        let json = r#"{"choices":[{"message":{"content":"Just text, no tools."}}]}"#;
        let resp: ApiChatResponse = serde_json::from_str(json).unwrap();
        let msg = &resp.choices[0].message;
        assert_eq!(msg.content.as_deref(), Some("Just text, no tools."));
        assert!(msg.tool_calls.is_none());
    }

    #[test]
    fn flatten_system_messages_merges_into_first_user_and_removes_system_roles() {
        let messages = vec![
            ChatMessage::system("System A"),
            ChatMessage::assistant("Earlier assistant turn"),
            ChatMessage::system("System B"),
            ChatMessage::user("User turn"),
            ChatMessage::tool(r#"{"ok":true}"#),
        ];

        let (flattened, system_merged) =
            OpenAiCompatibleModelProvider::flatten_system_messages(&messages, true);
        assert!(system_merged);
        assert_eq!(flattened.len(), 3);
        assert_eq!(flattened[0].role, "assistant");
        assert_eq!(
            flattened[1].content,
            "System A\n\nSystem B\n\nUser turn".to_string()
        );
        assert_eq!(flattened[1].role, "user");
        assert_eq!(flattened[2].role, "tool");
        assert!(!flattened.iter().any(|m| m.role == "system"));
    }

    #[test]
    fn flatten_system_messages_keeps_system_only_at_start_without_user_merge() {
        let messages = vec![
            ChatMessage::system("System A"),
            ChatMessage::user("User turn"),
            ChatMessage::assistant("Assistant turn"),
            ChatMessage::system("System B"),
            ChatMessage::user("Follow-up"),
        ];

        let (flattened, system_merged) =
            OpenAiCompatibleModelProvider::flatten_system_messages(&messages, false);
        assert!(!system_merged);
        assert_eq!(
            flattened
                .iter()
                .map(|message| message.role.as_str())
                .collect::<Vec<_>>(),
            vec!["system", "user", "assistant", "user"]
        );
        assert_eq!(
            flattened
                .iter()
                .filter(|message| message.role == "system")
                .count(),
            1
        );
        assert!(flattened[0].content.contains("System A"));
        assert!(flattened[0].content.contains("System B"));
    }

    #[test]
    fn flatten_system_messages_drops_empty_system_messages() {
        let messages = vec![
            ChatMessage::system(""),
            ChatMessage::user("User turn"),
            ChatMessage::system(""),
        ];

        let (flattened, system_merged) =
            OpenAiCompatibleModelProvider::flatten_system_messages(&messages, false);
        assert!(!system_merged, "empty system content merged nothing");

        assert_eq!(flattened.len(), 1);
        assert_eq!(flattened[0].role, "user");
        assert_eq!(flattened[0].content, "User turn");
    }

    #[test]
    fn flatten_system_messages_inserts_synthetic_user_when_no_user_exists() {
        let messages = vec![
            ChatMessage::assistant("Assistant only"),
            ChatMessage::system("Synthetic system"),
        ];

        let (flattened, system_merged) =
            OpenAiCompatibleModelProvider::flatten_system_messages(&messages, true);
        assert!(system_merged);
        assert_eq!(flattened.len(), 2);
        assert_eq!(flattened[0].role, "user");
        assert_eq!(flattened[0].content, "Synthetic system");
        assert_eq!(flattened[1].role, "assistant");
    }

    #[test]
    fn effective_content_preserves_unclosed_think_tag() {
        // An unclosed literal `<think>` tag must NOT discard the rest of the
        // response. The old `strip_think_tags()` helper saw no closing
        // `</think>` and dropped the trailing tail, collapsing
        // "Visible <think>hidden tail" to "Visible". The new path returns
        // the input unchanged.
        let json = r#"{"choices":[{"message":{"content":"Visible <think>hidden tail"}}]}"#;
        let resp: ApiChatResponse = serde_json::from_str(json).unwrap();
        let msg = &resp.choices[0].message;
        assert_eq!(msg.effective_content(), "Visible <think>hidden tail");
    }

    #[test]
    fn effective_content_preserves_multiple_think_blocks() {
        // Multiple literal `<think>` blocks in `content` survive the removal
        // intact. The old `strip_think_tags()` helper would have collapsed
        // the visible text to "Answer A  and B  done" — the double spaces
        // mark where `<think>hidden 1</think>` and `<think>hidden 2</think>`
        // used to be — losing the inter-block separators and the tag
        // delimiters themselves.
        let json = r#"{"choices":[{"message":{"content":"Answer A <think>hidden 1</think> and B <think>hidden 2</think> done"}}]}"#;
        let resp: ApiChatResponse = serde_json::from_str(json).unwrap();
        let msg = &resp.choices[0].message;
        assert_eq!(
            msg.effective_content(),
            "Answer A <think>hidden 1</think> and B <think>hidden 2</think> done"
        );
    }
    #[test]
    fn effective_content_preserves_think_tags_with_reasoning_content() {
        // When both `content` and `reasoning_content` are present,
        // the literal `<think>` blocks in `content` survive intact while
        // `reasoning_content` is preserved separately and is NOT leaked
        // into the response text.
        let json = r#"{"choices":[{"message":{"content":"Visible <think>hidden tail</think>","reasoning_content":"reasoning separately"}}]}"#;
        let resp: ApiChatResponse = serde_json::from_str(json).unwrap();
        let msg = &resp.choices[0].message;
        assert_eq!(
            msg.effective_content(),
            "Visible <think>hidden tail</think>"
        );
        assert!(!msg.effective_content().contains("reasoning separately"));
        assert_eq!(
            msg.reasoning_content.as_deref(),
            Some("reasoning separately")
        );
    }

    // ----------------------------------------------------------
    // Reasoning model fallback tests (reasoning_content)
    // ----------------------------------------------------------

    #[test]
    fn reasoning_content_does_not_leak_when_content_empty() {
        // reasoning_content must NOT leak into effective_content —
        // it is preserved separately in ChatResponse.reasoning_content
        let json = r#"{"choices":[{"message":{"content":"","reasoning_content":"Thinking output here"}}]}"#;
        let resp: ApiChatResponse = serde_json::from_str(json).unwrap();
        let msg = &resp.choices[0].message;
        assert_eq!(msg.effective_content(), "");
        assert_eq!(
            msg.reasoning_content.as_deref(),
            Some("Thinking output here")
        );
    }

    #[test]
    fn reasoning_content_does_not_leak_when_content_null() {
        let json =
            r#"{"choices":[{"message":{"content":null,"reasoning_content":"Fallback text"}}]}"#;
        let resp: ApiChatResponse = serde_json::from_str(json).unwrap();
        let msg = &resp.choices[0].message;
        assert_eq!(msg.effective_content(), "");
        assert_eq!(msg.reasoning_content.as_deref(), Some("Fallback text"));
    }

    #[test]
    fn reasoning_content_does_not_leak_when_content_missing() {
        let json = r#"{"choices":[{"message":{"reasoning_content":"Only reasoning"}}]}"#;
        let resp: ApiChatResponse = serde_json::from_str(json).unwrap();
        let msg = &resp.choices[0].message;
        assert_eq!(msg.effective_content(), "");
        assert_eq!(msg.reasoning_content.as_deref(), Some("Only reasoning"));
    }

    #[test]
    fn reasoning_content_not_used_when_content_present() {
        // Normal model: content populated, reasoning_content should be ignored
        let json = r#"{"choices":[{"message":{"content":"Normal response","reasoning_content":"Should be ignored"}}]}"#;
        let resp: ApiChatResponse = serde_json::from_str(json).unwrap();
        let msg = &resp.choices[0].message;
        assert_eq!(msg.effective_content(), "Normal response");
    }

    #[test]
    fn reasoning_content_preserved_when_content_only_think_tags() {
        // The compatible provider no longer strips literal
        // `<think>...</think>` blocks from `content`. Previously the
        // `<think>secret</think>`-only content was collapsed to the empty
        // string by `strip_think_tags()`, and `effective_content()` returned
        // `""` so the visible-text field was effectively replaced by the
        // model's chain-of-thought marker. Now the literal `<think>` tags
        // round-trip into `effective_content()` byte-for-byte, and
        // `reasoning_content` is still preserved separately and not leaked
        // into the response text.
        let json = r#"{"choices":[{"message":{"content":"<think>secret</think>","reasoning_content":"Thinking text"}}]}"#;
        let resp: ApiChatResponse = serde_json::from_str(json).unwrap();
        let msg = &resp.choices[0].message;
        assert!(msg.effective_content().contains("secret"));
        assert!(msg.effective_content().contains("<think>"));
        assert_eq!(
            msg.effective_content_optional().as_deref(),
            Some("<think>secret</think>"),
        );
        assert_eq!(msg.reasoning_content.as_deref(), Some("Thinking text"));
    }

    #[test]
    fn reasoning_content_both_absent_returns_empty() {
        // Neither content nor reasoning_content - returns empty string
        let json = r#"{"choices":[{"message":{}}]}"#;
        let resp: ApiChatResponse = serde_json::from_str(json).unwrap();
        let msg = &resp.choices[0].message;
        assert_eq!(msg.effective_content(), "");
    }

    #[test]
    fn reasoning_content_ignored_by_normal_models() {
        // Standard response without reasoning_content still works
        let json = r#"{"choices":[{"message":{"content":"Hello from Venice!"}}]}"#;
        let resp: ApiChatResponse = serde_json::from_str(json).unwrap();
        let msg = &resp.choices[0].message;
        assert!(msg.reasoning_content.is_none());
        assert_eq!(msg.effective_content(), "Hello from Venice!");
    }

    // ----------------------------------------------------------
    // SSE streaming reasoning_content fallback tests
    // ----------------------------------------------------------

    #[test]
    fn parse_sse_line_with_content() {
        let line = r#"data: {"choices":[{"delta":{"content":"hello"}}]}"#;
        let result = parse_sse_line(line).unwrap().unwrap();
        assert_eq!(result.delta, "hello");
        assert!(result.reasoning.is_none());
    }

    #[test]
    fn parse_sse_line_with_reasoning_content() {
        let line = r#"data: {"choices":[{"delta":{"reasoning_content":"thinking..."}}]}"#;
        let result = parse_sse_line(line).unwrap().unwrap();
        assert!(result.delta.is_empty());
        assert_eq!(result.reasoning.as_deref(), Some("thinking..."));
    }

    #[test]
    fn parse_sse_line_with_both_prefers_content() {
        let line = r#"data: {"choices":[{"delta":{"content":"real answer","reasoning_content":"thinking..."}}]}"#;
        let result = parse_sse_line(line).unwrap().unwrap();
        assert_eq!(result.delta, "real answer");
        assert!(result.reasoning.is_none());
    }

    #[test]
    fn parse_sse_line_with_empty_content_falls_back_to_reasoning() {
        let line =
            r#"data: {"choices":[{"delta":{"content":"","reasoning_content":"thinking..."}}]}"#;
        let result = parse_sse_line(line).unwrap().unwrap();
        assert!(result.delta.is_empty());
        assert_eq!(result.reasoning.as_deref(), Some("thinking..."));
    }

    // OpenRouter and vLLM (>= v0.16.0) emit reasoning
    // under `reasoning` rather than `reasoning_content`. Both fields must
    // be accepted on deserialization.
    #[test]
    fn parse_sse_line_accepts_reasoning_alias() {
        let line = r#"data: {"choices":[{"delta":{"reasoning":"thinking via vllm..."}}]}"#;
        let result = parse_sse_line(line).unwrap().unwrap();
        assert!(result.delta.is_empty());
        assert_eq!(result.reasoning.as_deref(), Some("thinking via vllm..."));
    }

    #[test]
    fn parse_sse_line_with_empty_content_and_reasoning_alias() {
        let line = r#"data: {"choices":[{"delta":{"content":"","reasoning":"vllm thought"}}]}"#;
        let result = parse_sse_line(line).unwrap().unwrap();
        assert!(result.delta.is_empty());
        assert_eq!(result.reasoning.as_deref(), Some("vllm thought"));
    }

    #[test]
    fn response_message_accepts_reasoning_alias_on_non_stream_path() {
        // Non-stream OpenAI Chat Completions response, vLLM/OpenRouter shape.
        let json = r#"{"content":null,"reasoning":"chain-of-thought via vllm","tool_calls":null}"#;
        let msg: ResponseMessage = serde_json::from_str(json).unwrap();
        assert!(msg.content.is_none());
        assert_eq!(
            msg.reasoning_content.as_deref(),
            Some("chain-of-thought via vllm"),
            "the `reasoning` alias must populate the canonical reasoning_content field",
        );
        // effective_content returns "" when content is None — reasoning
        // is preserved separately, not leaked into the response text.
        assert_eq!(msg.effective_content(), "");
    }

    #[test]
    fn response_message_canonical_reasoning_content_still_works() {
        // Existing providers continue to populate reasoning_content directly.
        let json = r#"{"content":null,"reasoning_content":"canonical thought","tool_calls":null}"#;
        let msg: ResponseMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.reasoning_content.as_deref(), Some("canonical thought"));
    }

    #[test]
    fn response_message_with_both_keys_prefers_canonical_reasoning_content() {
        let json = r#"{"content":null,"reasoning_content":"canonical","reasoning":"alias","tool_calls":null}"#;
        let msg: ResponseMessage = serde_json::from_str(json)
            .expect("payload with both reasoning_content and reasoning must deserialize");
        assert_eq!(
            msg.reasoning_content.as_deref(),
            Some("canonical"),
            "canonical reasoning_content must win when both fields are present",
        );
    }

    #[test]
    fn response_message_with_only_alias_populates_canonical_field() {
        // Sanity: when only the alias is present, it still flows into the
        // canonical reasoning_content field.
        let json = r#"{"content":null,"reasoning":"alias only","tool_calls":null}"#;
        let msg: ResponseMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.reasoning_content.as_deref(), Some("alias only"));
    }

    #[test]
    fn stream_delta_with_both_keys_prefers_canonical_reasoning_content() {
        // The streaming-SSE shape used the same `#[serde(alias)]` and had the
        // same duplicate-field error mode. Pin the precedence here too.
        let chunk = r#"data: {"choices":[{"delta":{"reasoning_content":"canonical","reasoning":"alias"}}]}"#;
        let result = parse_sse_line(chunk)
            .expect("parse must succeed")
            .expect("non-empty chunk");
        assert_eq!(result.reasoning.as_deref(), Some("canonical"));
    }

    // The round-trip path at to_native_messages reconstructs reasoning_content
    // from session-stored assistant-with-tool-calls JSON. Both names must work.
    #[test]
    fn round_trip_reasoning_extraction_accepts_alias() {
        fn extract_reasoning(value: &serde_json::Value) -> Option<String> {
            value
                .get("reasoning_content")
                .or_else(|| value.get("reasoning"))
                .and_then(serde_json::Value::as_str)
                .map(ToString::to_string)
        }
        let canonical: serde_json::Value =
            serde_json::from_str(r#"{"reasoning_content":"canonical","tool_calls":[]}"#).unwrap();
        let alias: serde_json::Value =
            serde_json::from_str(r#"{"reasoning":"vllm","tool_calls":[]}"#).unwrap();
        let neither: serde_json::Value = serde_json::from_str(r#"{"tool_calls":[]}"#).unwrap();
        let both: serde_json::Value = serde_json::from_str(
            r#"{"reasoning_content":"canonical","reasoning":"alias","tool_calls":[]}"#,
        )
        .unwrap();
        assert_eq!(extract_reasoning(&canonical).as_deref(), Some("canonical"));
        assert_eq!(extract_reasoning(&alias).as_deref(), Some("vllm"));
        assert_eq!(extract_reasoning(&neither), None);
        // When both are present, the canonical name wins — preserves existing
        // behavior for providers that emit `reasoning_content` plus a stray
        // `reasoning` field.
        assert_eq!(extract_reasoning(&both).as_deref(), Some("canonical"));
    }

    #[test]
    fn parse_sse_line_done_sentinel() {
        let line = "data: [DONE]";
        let result = parse_sse_line(line).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn parse_sse_chunk_with_tool_call_delta() {
        let line = r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"shell","arguments":"{\"command\":\"date\"}"}}]}}]}"#;
        let chunk = parse_sse_chunk(line)
            .unwrap()
            .expect("chunk should be parsed");
        let choice = chunk.choices.first().expect("choice should exist");
        let tool_calls = choice
            .delta
            .tool_calls
            .as_ref()
            .expect("tool call deltas should exist");
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].index, Some(0));
        assert_eq!(tool_calls[0].id.as_deref(), Some("call_1"));
        assert_eq!(
            tool_calls[0]
                .function
                .as_ref()
                .and_then(|function| function.name.as_deref()),
            Some("shell")
        );
    }

    #[test]
    fn stream_tool_call_accumulator_combines_deltas() {
        let mut acc = StreamToolCallAccumulator::default();
        acc.apply_delta(&StreamToolCallDelta {
            index: Some(0),
            id: Some("call_1".to_string()),
            function: Some(StreamFunctionDelta {
                name: Some("shell".to_string()),
                arguments: Some("{\"command\":\"".to_string()),
            }),
            name: None,
            arguments: None,
            extra_content: None,
        });
        acc.apply_delta(&StreamToolCallDelta {
            index: Some(0),
            id: None,
            function: Some(StreamFunctionDelta {
                name: None,
                arguments: Some("date\"}".to_string()),
            }),
            name: None,
            arguments: None,
            extra_content: None,
        });

        let mut used_tool_call_ids = std::collections::HashSet::new();
        let tool_call = acc
            .into_provider_tool_call(false, &mut used_tool_call_ids)
            .expect("accumulator should emit tool call");
        assert_eq!(tool_call.id, "call_1");
        assert_eq!(tool_call.name, "shell");
        assert_eq!(tool_call.arguments, r#"{"command":"date"}"#);
    }

    #[test]
    fn stream_tool_call_accumulator_mistral_normalizes_invalid_id() {
        let mut acc = StreamToolCallAccumulator::default();
        acc.apply_delta(&StreamToolCallDelta {
            index: Some(0),
            id: Some("chatcmpl-tool-abc".to_string()),
            function: Some(StreamFunctionDelta {
                name: Some("shell".to_string()),
                arguments: Some(r#"{"command":"date"}"#.to_string()),
            }),
            name: None,
            arguments: None,
            extra_content: None,
        });

        let mut used_tool_call_ids = std::collections::HashSet::new();
        let tool_call = acc
            .into_provider_tool_call(true, &mut used_tool_call_ids)
            .expect("accumulator should emit tool call");

        assert_eq!(tool_call.id.len(), 9);
        assert!(tool_call.id.chars().all(|c| c.is_ascii_alphanumeric()));
        assert_ne!(tool_call.id, "chatcmpl-tool-abc");
    }

    #[test]
    fn api_response_parses_usage() {
        let json = r#"{
            "choices": [{"message": {"content": "Hello"}}],
            "usage": {"prompt_tokens": 150, "completion_tokens": 60}
        }"#;
        let resp: ApiChatResponse = serde_json::from_str(json).unwrap();
        let usage = resp.usage.unwrap();
        assert_eq!(usage.prompt_tokens, Some(150));
        assert_eq!(usage.completion_tokens, Some(60));
    }

    #[test]
    fn api_response_parses_openai_cached_tokens() {
        let json = r#"{
            "choices": [{"message": {"content": "Hello"}}],
            "usage": {
                "prompt_tokens": 150,
                "completion_tokens": 60,
                "prompt_tokens_details": {"cached_tokens": 120}
            }
        }"#;
        let resp: ApiChatResponse = serde_json::from_str(json).unwrap();
        let usage = resp.usage.unwrap().into_provider_usage();
        assert_eq!(usage.input_tokens, Some(150));
        assert_eq!(usage.output_tokens, Some(60));
        assert_eq!(usage.cached_input_tokens, Some(120));
    }

    #[test]
    fn api_response_parses_gateway_cache_creation_tokens() {
        // Translating gateways forward Anthropic's cache-write counter inside
        // `prompt_tokens_details` when providers include that usage detail.
        let json = r#"{
            "choices": [{"message": {"content": "Hello"}}],
            "usage": {
                "prompt_tokens": 150,
                "completion_tokens": 60,
                "prompt_tokens_details": {
                    "cached_tokens": 120,
                    "cache_creation_input_tokens": 25
                }
            }
        }"#;
        let resp: ApiChatResponse = serde_json::from_str(json).unwrap();
        let usage = resp.usage.unwrap().into_provider_usage();
        assert_eq!(usage.cached_input_tokens, Some(120));
        assert_eq!(usage.cache_creation_input_tokens, Some(25));
    }

    #[test]
    fn api_response_parses_deepseek_cached_tokens() {
        let json = r#"{
            "choices": [{"message": {"content": "Hello"}}],
            "usage": {
                "prompt_tokens": 150,
                "completion_tokens": 60,
                "prompt_cache_hit_tokens": 100,
                "prompt_tokens_details": {"cached_tokens": 80}
            }
        }"#;
        let resp: ApiChatResponse = serde_json::from_str(json).unwrap();
        let usage = resp.usage.unwrap().into_provider_usage();
        assert_eq!(usage.cached_input_tokens, Some(100));
    }

    #[test]
    fn api_response_parses_non_integer_cached_tokens_lossily() {
        let json = r#"{
            "choices": [{"message": {"content": "Hello"}}],
            "usage": {
                "prompt_tokens": 150,
                "completion_tokens": 60,
                "prompt_tokens_details": {"cached_tokens": "2.5e2"}
            }
        }"#;
        let resp: ApiChatResponse = serde_json::from_str(json).unwrap();
        let usage = resp.usage.unwrap().into_provider_usage();
        assert_eq!(usage.cached_input_tokens, Some(250));
    }

    #[test]
    fn api_response_ignores_invalid_cached_tokens_without_losing_usage() {
        let json = r#"{
            "choices": [{"message": {"content": "Hello"}}],
            "usage": {
                "prompt_tokens": 150,
                "completion_tokens": 60,
                "prompt_cache_hit_tokens": -1,
                "prompt_tokens_details": {"cached_tokens": "not-a-number"}
            }
        }"#;
        let resp: ApiChatResponse = serde_json::from_str(json).unwrap();
        let usage = resp.usage.unwrap().into_provider_usage();
        assert_eq!(usage.input_tokens, Some(150));
        assert_eq!(usage.output_tokens, Some(60));
        assert_eq!(usage.cached_input_tokens, None);
    }

    #[test]
    fn stream_chunk_parses_cached_tokens() {
        let json = r#"{
            "choices": [],
            "usage": {
                "prompt_tokens": 99,
                "completion_tokens": 11,
                "prompt_tokens_details": {"cached_tokens": 42}
            }
        }"#;
        let chunk: StreamChunkResponse = serde_json::from_str(json).unwrap();
        let usage = chunk.usage.unwrap().into_provider_usage();
        assert_eq!(usage.input_tokens, Some(99));
        assert_eq!(usage.output_tokens, Some(11));
        assert_eq!(usage.cached_input_tokens, Some(42));
    }

    #[test]
    fn stream_chunk_prefers_deepseek_prompt_cache_hit_tokens() {
        let json = r#"{
            "id":"14037a3e-81f7-4559-b9ae-161bcb17c34c",
            "object":"chat.completion.chunk",
            "created":1780971871,
            "model":"deepseek-v4-flash",
            "choices":[{"index":0,"delta":{"content":"","reasoning_content":null},"finish_reason":"tool_calls"}],
            "usage": {
                "prompt_tokens": 13313,
                "completion_tokens": 175,
                "total_tokens": 13488,
                "prompt_tokens_details": {"cached_tokens": 384},
                "completion_tokens_details": {"reasoning_tokens": 100},
                "prompt_cache_hit_tokens": 384,
                "prompt_cache_miss_tokens": 12929
            }
        }"#;
        let chunk: StreamChunkResponse = serde_json::from_str(json).unwrap();
        let usage = chunk.usage.unwrap().into_provider_usage();
        assert_eq!(usage.input_tokens, Some(13313));
        assert_eq!(usage.output_tokens, Some(175));
        assert_eq!(usage.cached_input_tokens, Some(384));
    }

    #[test]
    fn api_response_parses_anthropic_cache_read_tokens() {
        let json = r#"{
            "choices": [{"message": {"content": "Hello"}}],
            "usage": {
                "prompt_tokens": 4863,
                "completion_tokens": 0,
                "cache_read_input_tokens": 4802,
                "cache_creation_input_tokens": 0
            }
        }"#;
        let resp: ApiChatResponse = serde_json::from_str(json).unwrap();
        let usage = resp.usage.unwrap().into_provider_usage();
        assert_eq!(usage.input_tokens, Some(4863));
        assert_eq!(usage.cached_input_tokens, Some(4802));
    }

    /// All three cached-token shapes on one response: the Anthropic fields
    /// are upstream-reported and win over the gateway-accounted DeepSeek
    /// and OpenAI shapes, which keep their existing relative order.
    #[test]
    fn api_response_prefers_anthropic_cache_read_across_all_shapes() {
        let json = r#"{
            "choices": [{"message": {"content": "Hello"}}],
            "usage": {
                "prompt_tokens": 4863,
                "completion_tokens": 0,
                "cache_read_input_tokens": 4802,
                "prompt_cache_hit_tokens": 100,
                "prompt_tokens_details": {"cached_tokens": 80}
            }
        }"#;
        let resp: ApiChatResponse = serde_json::from_str(json).unwrap();
        let usage = resp.usage.unwrap().into_provider_usage();
        assert_eq!(usage.cached_input_tokens, Some(4802));
    }

    /// An upstream-reported zero is authoritative, not missing data: when
    /// the Anthropic shape says zero reads, the gateway-accounted OpenAI
    /// figure must NOT be substituted (that would double-count gateway-side
    /// accounting against the upstream's authoritative answer).
    #[test]
    fn api_response_anthropic_zero_read_blocks_openai_fallback() {
        let json = r#"{
            "choices": [{"message": {"content": "Hello"}}],
            "usage": {
                "prompt_tokens": 4863,
                "completion_tokens": 0,
                "cache_read_input_tokens": 0,
                "prompt_tokens_details": {"cached_tokens": 80}
            }
        }"#;
        let resp: ApiChatResponse = serde_json::from_str(json).unwrap();
        let usage = resp.usage.unwrap().into_provider_usage();
        assert_eq!(usage.cached_input_tokens, Some(0));
    }

    #[test]
    fn stream_chunk_parses_anthropic_cache_read_tokens() {
        let json = r#"{
            "choices": [],
            "usage": {
                "prompt_tokens": 99,
                "completion_tokens": 11,
                "cache_read_input_tokens": 42,
                "cache_creation_input_tokens": 0
            }
        }"#;
        let chunk: StreamChunkResponse = serde_json::from_str(json).unwrap();
        let usage = chunk.usage.unwrap().into_provider_usage();
        assert_eq!(usage.input_tokens, Some(99));
        assert_eq!(usage.output_tokens, Some(11));
        assert_eq!(usage.cached_input_tokens, Some(42));
    }

    /// Cache-write tokens are logged for write-premium visibility. The log
    /// must carry the counts, not the prompt content.
    #[test]
    fn cache_creation_tokens_are_logged_with_counts_only() {
        let _writer_guard = clawcrew_log::__private_test_writer_lock();
        let _hook_guard = clawcrew_log::__private_test_hook_lock();
        clawcrew_log::try_install_capture_subscriber();
        let mut rx = clawcrew_log::subscribe_or_install();
        while rx.try_recv().is_ok() {}

        let json = r#"{
            "choices": [{"message": {"content": "Hello"}}],
            "usage": {
                "prompt_tokens": 4863,
                "completion_tokens": 0,
                "cache_creation_input_tokens": 4803
            }
        }"#;
        let resp: ApiChatResponse = serde_json::from_str(json).unwrap();
        let usage = resp.usage.unwrap().into_provider_usage();
        // Write-only response: upstream reported no cache read, so the
        // cached-token figure stays unset.
        assert_eq!(usage.cached_input_tokens, None);

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let found = 'search: loop {
            while let Ok(event) = rx.try_recv() {
                let matches_message = event.get("message").and_then(|value| value.as_str())
                    == Some("gateway-reported Anthropic cache write (billed at the write premium)");
                let matches_counts = event
                    .get("attributes")
                    .and_then(|attributes| attributes.get("cache_creation_input_tokens"))
                    == Some(&serde_json::json!(4803));
                if matches_message && matches_counts && matches_source_file(&event) {
                    break 'search true;
                }
            }
            if std::time::Instant::now() >= deadline {
                break 'search false;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        assert!(found, "cache-write log record with counts must be emitted");
    }

    fn matches_source_file(event: &serde_json::Value) -> bool {
        event
            .get("attributes")
            .and_then(|attributes| attributes.get("_file"))
            .and_then(|value| value.as_str())
            .is_some_and(|file| file.ends_with("compatible.rs"))
    }

    /// End to end on the tools path: a translated-gateway response carrying
    /// Anthropic-shaped usage populates `TokenUsage.cached_input_tokens`.
    /// Parsing is flag-independent (the gateway sends these fields on
    /// Anthropic-backed routes regardless of the client flag).
    #[tokio::test]
    async fn chat_with_tools_surfaces_anthropic_cache_read_tokens() {
        let (provider, _captured, server) = mock_non_streaming_response(serde_json::json!({
            "choices": [{"message": {"content": "ok"}}],
            "usage": {
                "prompt_tokens": 4863,
                "completion_tokens": 4,
                "cache_read_input_tokens": 4802,
                "cache_creation_input_tokens": 0
            }
        }))
        .await;
        let messages = vec![ChatMessage::user("hi")];
        let tools = vec![serde_json::json!({
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "Get weather",
                "parameters": {"type": "object", "properties": {}}
            }
        })];
        let response = provider
            .chat_with_tools(&messages, &tools, "test-model", None)
            .await;
        server.abort();
        let response = response.unwrap_or_else(|error| panic!("tools request failed: {error}"));
        let usage = response.usage.expect("usage must be captured");
        assert_eq!(usage.cached_input_tokens, Some(4802));
        assert_eq!(usage.input_tokens, Some(4863));
    }

    /// Flag interaction: the thinking object reaches the wire through
    /// `extra_body` (the same surface the thinking-passthrough feature
    /// uses), and it must ride alongside the cache breakpoints in one
    /// request: neither injection disturbs the other. This proves generic
    /// flattened-body coexistence on the structured path; full proof that
    /// the two operator flags compose awaits the thinking-passthrough
    /// provider flag landing in this tree.
    #[tokio::test]
    async fn cache_breakpoints_coexist_with_thinking_object_in_one_request() {
        let (provider, captured, server) = mock_streaming_cache_capture(true).await;
        // Builder-level extra_body mirrors what provider_extra supplies for
        // thinking-capable gateways; swap in the flag-side equivalent when
        // the thinking-passthrough provider flag lands in this tree.
        let provider = OpenAiCompatibleModelProvider {
            extra_body: Some(serde_json::json!({
                "thinking": {"type": "enabled", "budget_tokens": 2048},
            })),
            ..provider
        };
        let messages = vec![
            ChatMessage::system("you are brief"),
            ChatMessage::user("hi"),
            ChatMessage::assistant("hello"),
            ChatMessage::user("bye"),
        ];
        let result = provider
            .chat(
                ProviderChatRequest {
                    messages: &messages,
                    tools: None,
                    thinking: None,
                },
                "test-model",
                None,
            )
            .await;
        server.abort();
        result.unwrap_or_else(|error| panic!("combined request failed: {error}"));
        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0]["thinking"],
            serde_json::json!({"type": "enabled", "budget_tokens": 2048}),
            "the thinking object must ride at the top level untouched"
        );
        assert_eq!(
            requests[0]["messages"][0]["content"][0]["cache_control"],
            serde_json::json!({"type": "ephemeral"}),
            "system cache breakpoint present alongside the thinking object"
        );
        assert_eq!(
            requests[0]["messages"][3]["content"][0]["cache_control"],
            serde_json::json!({"type": "ephemeral"}),
            "rolling cache breakpoint present alongside the thinking object"
        );
    }

    #[test]
    fn api_response_parses_without_usage() {
        let json = r#"{"choices": [{"message": {"content": "Hello"}}]}"#;
        let resp: ApiChatResponse = serde_json::from_str(json).unwrap();
        assert!(resp.usage.is_none());
    }

    // ═══════════════════════════════════════════════════════════════════════
    // merged-system carrier + usage-capture boundary tests
    // ═══════════════════════════════════════════════════════════════════════

    /// Merged-carrier, single-turn: with `merge_system_into_user`, the
    /// system role never reaches the wire, so the merged first user message
    /// is the only system-equivalent carrier and must carry the breakpoint
    /// unconditionally. Before the repair this shape sent zero breakpoints.
    #[tokio::test]
    async fn cache_passthrough_merged_system_marks_first_user_single_turn() {
        let (provider, captured, server) = mock_streaming_cache_capture(true).await;
        let provider = OpenAiCompatibleModelProvider {
            merge_system_into_user: true,
            ..provider
        };
        let messages = vec![
            ChatMessage::system("core policy"),
            ChatMessage::user("hello"),
        ];
        let result = provider
            .chat(
                ProviderChatRequest {
                    messages: &messages,
                    tools: None,
                    thinking: None,
                },
                "test-model",
                None,
            )
            .await;
        server.abort();
        result.unwrap_or_else(|error| panic!("merged single-turn request failed: {error}"));
        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0]["messages"],
            serde_json::json!([
                {"role": "user", "content": [
                    {"type": "text", "text": "core policy\n\nhello",
                     "cache_control": {"type": "ephemeral"}},
                ]},
            ]),
            "merged user message is the system-equivalent carrier and must be marked"
        );
        assert_eq!(
            requests[0].to_string().matches("cache_control").count(),
            1,
            "single exchange carries exactly the carrier breakpoint"
        );
    }

    /// Merged-carrier, multi-turn: the carrier keeps the system breakpoint
    /// and the rolling breakpoint still lands on the last message, exactly
    /// two total, middle messages untouched.
    #[tokio::test]
    async fn cache_passthrough_merged_system_carrier_plus_rolling_multi_turn() {
        let (provider, captured, server) = mock_streaming_cache_capture(true).await;
        let provider = OpenAiCompatibleModelProvider {
            merge_system_into_user: true,
            ..provider
        };
        let messages = vec![
            ChatMessage::system("core policy"),
            ChatMessage::user("hi"),
            ChatMessage::assistant("hello"),
            ChatMessage::user("bye"),
        ];
        let result = provider
            .chat(
                ProviderChatRequest {
                    messages: &messages,
                    tools: None,
                    thinking: None,
                },
                "test-model",
                None,
            )
            .await;
        server.abort();
        result.unwrap_or_else(|error| panic!("merged multi-turn request failed: {error}"));
        let requests = captured.lock().unwrap();
        assert_eq!(
            requests[0]["messages"],
            serde_json::json!([
                {"role": "user", "content": [
                    {"type": "text", "text": "core policy\n\nhi",
                     "cache_control": {"type": "ephemeral"}},
                ]},
                {"role": "assistant", "content": "hello"},
                {"role": "user", "content": [
                    {"type": "text", "text": "bye",
                     "cache_control": {"type": "ephemeral"}},
                ]},
            ]),
            "merged carrier and rolling breakpoint with middle messages untouched"
        );
        assert_eq!(
            requests[0].to_string().matches("cache_control").count(),
            2,
            "merged multi-turn must carry exactly two breakpoints"
        );
    }

    /// Merged-carrier, assistant-first history: the merged user is not
    /// message zero; the carrier index must follow the first user wherever
    /// it sits.
    #[tokio::test]
    async fn cache_passthrough_merged_system_marks_first_user_after_assistant() {
        let (provider, captured, server) = mock_streaming_cache_capture(true).await;
        let provider = OpenAiCompatibleModelProvider {
            merge_system_into_user: true,
            ..provider
        };
        let messages = vec![
            ChatMessage::assistant("ack"),
            ChatMessage::system("core policy"),
            ChatMessage::user("hello"),
        ];
        let result = provider
            .chat(
                ProviderChatRequest {
                    messages: &messages,
                    tools: None,
                    thinking: None,
                },
                "test-model",
                None,
            )
            .await;
        server.abort();
        result.unwrap_or_else(|error| panic!("assistant-first merged request failed: {error}"));
        let requests = captured.lock().unwrap();
        assert_eq!(
            requests[0]["messages"],
            serde_json::json!([
                {"role": "assistant", "content": "ack"},
                {"role": "user", "content": [
                    {"type": "text", "text": "core policy\n\nhello",
                     "cache_control": {"type": "ephemeral"}},
                ]},
            ]),
            "carrier breakpoint lands on the merged user, not on message zero"
        );
        assert_eq!(
            requests[0].to_string().matches("cache_control").count(),
            1,
            "assistant + merged user is a single exchange: no rolling breakpoint"
        );
    }

    /// Streaming twin of the merged-carrier pin.
    #[tokio::test]
    async fn cache_passthrough_merged_system_streaming_matches_non_streaming() {
        use futures_util::StreamExt as _;

        let (provider, captured, server) = mock_streaming_cache_capture(true).await;
        let provider = OpenAiCompatibleModelProvider {
            merge_system_into_user: true,
            ..provider
        };
        let events = provider
            .stream_chat(
                ProviderChatRequest {
                    messages: &[ChatMessage::system("be brief"), ChatMessage::user("hello")],
                    tools: None,
                    thinking: None,
                },
                "test-model",
                None,
                StreamOptions {
                    enabled: true,
                    count_tokens: false,
                },
            )
            .collect::<Vec<_>>()
            .await;
        server.abort();
        assert!(
            events.iter().all(Result::is_ok),
            "merged streaming request must succeed: {events:?}"
        );
        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0]["messages"],
            serde_json::json!([
                {"role": "user", "content": [
                    {"type": "text", "text": "be brief\n\nhello",
                     "cache_control": {"type": "ephemeral"}},
                ]},
            ]),
            "streaming merged carrier carries the breakpoint identically"
        );
    }

    /// Flag-off merged: the merge behavior is unchanged and the wire stays
    /// free of markers.
    #[tokio::test]
    async fn cache_passthrough_merged_flag_off_body_unmarked() {
        let (provider, captured, server) = mock_streaming_cache_capture(false).await;
        let provider = OpenAiCompatibleModelProvider {
            merge_system_into_user: true,
            ..provider
        };
        let messages = vec![
            ChatMessage::system("core policy"),
            ChatMessage::user("hello"),
        ];
        let result = provider
            .chat(
                ProviderChatRequest {
                    messages: &messages,
                    tools: None,
                    thinking: None,
                },
                "test-model",
                None,
            )
            .await;
        server.abort();
        result.unwrap_or_else(|error| panic!("flag-off merged request failed: {error}"));
        let requests = captured.lock().unwrap();
        assert_eq!(
            requests[0]["messages"],
            serde_json::json!([
                {"role": "user", "content": "core policy\n\nhello"},
            ]),
            "flag-off merged body must be byte-identical to the pre-feature wire"
        );
        assert!(
            !requests[0].to_string().contains("cache_control"),
            "flag-off merged request must carry no cache markers"
        );
    }

    /// Capture-boundary regression: the text-only helpers must never emit
    /// cache breakpoints even with the flag on, because their responses
    /// drop usage and could never account for the premium.
    #[tokio::test]
    async fn cache_passthrough_simple_paths_emit_no_breakpoints_even_when_enabled() {
        let (provider, captured, server) = mock_streaming_cache_capture(true).await;
        let _ = provider
            .chat_with_system(Some("be brief"), "hello", "test-model", None)
            .await;
        let messages = vec![
            ChatMessage::system("you are brief"),
            ChatMessage::user("hi"),
            ChatMessage::assistant("hello"),
            ChatMessage::user("bye"),
        ];
        let _ = provider
            .chat_with_history(&messages, "test-model", None)
            .await;
        server.abort();
        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 2);
        for (index, request) in requests.iter().enumerate() {
            assert!(
                !request.to_string().contains("cache_control"),
                "simple path {index} must not emit cache breakpoints: {request}"
            );
        }
    }

    /// Capture-boundary regression, streaming side: the legacy chunk stream
    /// drops usage, so it must not trigger premium writes either.
    #[tokio::test]
    async fn cache_passthrough_legacy_stream_emits_no_breakpoints_even_when_enabled() {
        use futures_util::StreamExt as _;

        let (provider, captured, server) = mock_streaming_cache_capture(true).await;
        let events = provider
            .stream_chat_with_system(
                Some("be brief"),
                "hello",
                "test-model",
                None,
                StreamOptions {
                    enabled: true,
                    count_tokens: false,
                },
            )
            .collect::<Vec<_>>()
            .await;
        server.abort();
        let _ = events;
        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(
            !requests[0].to_string().contains("cache_control"),
            "legacy chunk stream must carry no cache markers even with the flag on"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // reasoning_content pass-through tests
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn parse_native_response_captures_reasoning_content() {
        let provider = make_model_provider("test", "https://example.com", None);
        let message = ResponseMessage {
            content: Some("answer".to_string()),
            reasoning_content: Some("thinking step".to_string()),
            tool_calls: Some(vec![ToolCall {
                id: Some("call_1".to_string()),
                kind: Some("function".to_string()),
                function: Some(Function {
                    name: Some("shell".to_string()),
                    arguments: Some(r#"{"cmd":"ls"}"#.to_string()),
                }),
                name: None,
                arguments: None,
                parameters: None,
                extra_content: None,
            }]),
        };

        let parsed = provider.parse_native_response(message);
        assert_eq!(parsed.reasoning_content.as_deref(), Some("thinking step"));
        assert_eq!(parsed.text.as_deref(), Some("answer"));
        assert_eq!(parsed.tool_calls.len(), 1);
    }

    #[test]
    fn parse_native_response_none_reasoning_content_for_normal_model() {
        let provider = make_model_provider("test", "https://example.com", None);
        let message = ResponseMessage {
            content: Some("hello".to_string()),
            reasoning_content: None,
            tool_calls: None,
        };

        let parsed = provider.parse_native_response(message);
        assert!(parsed.reasoning_content.is_none());
        assert_eq!(parsed.text.as_deref(), Some("hello"));
    }

    #[test]
    fn convert_messages_for_native_round_trips_reasoning_content() {
        // Simulate stored assistant history JSON that includes reasoning_content
        let history_json = serde_json::json!({
            "content": "I will check",
            "tool_calls": [{
                "id": "tc_1",
                "name": "shell",
                "arguments": "{\"cmd\":\"ls\"}"
            }],
            "reasoning_content": "Let me think about this..."
        });

        let messages = vec![ChatMessage::assistant(history_json.to_string())];
        let provider = make_model_provider("test", "https://example.com", None);
        let native = provider.convert_messages_for_native(&messages, true);
        assert_eq!(native.len(), 1);
        assert_eq!(native[0].role, "assistant");
        assert_eq!(
            native[0].reasoning_content.as_deref(),
            Some("Let me think about this...")
        );
        assert!(native[0].tool_calls.is_some());
    }

    #[test]
    fn convert_messages_for_native_round_trips_tool_call_extra_content() {
        let extra_content = serde_json::json!({
            "google": {
                "thought_signature": "sig_1"
            }
        });
        let history_json = serde_json::json!({
            "content": "",
            "tool_calls": [{
                "id": "tc_1",
                "name": "shell",
                "arguments": "{\"cmd\":\"ls\"}",
                "extra_content": extra_content.clone()
            }]
        });

        let messages = vec![ChatMessage::assistant(history_json.to_string())];
        let provider = make_model_provider("test", "https://example.com", None);
        let native = provider.convert_messages_for_native(&messages, true);
        let tool_calls = native[0].tool_calls.as_ref().unwrap();

        assert_eq!(tool_calls[0].extra_content.as_ref(), Some(&extra_content));
    }

    #[test]
    fn groq_outbound_omits_reasoning_replay_but_default_preserves_it() {
        let history_json = serde_json::json!({
            "content": "I will check",
            "tool_calls": [{
                "id": "tc_1",
                "name": "shell",
                "arguments": "{\"cmd\":\"ls\"}"
            }],
            "reasoning_content": "canonical thought",
            "reasoning": "alias thought"
        });

        let messages = vec![ChatMessage::assistant(history_json.to_string())];
        let default_provider =
            make_model_provider("OpenRouter", "https://openrouter.ai/api/v1", None);
        let default_request = default_provider.build_native_tool_chat_request(
            &messages,
            None,
            "openai/gpt-oss-120b",
            None,
            true,
            false,
        );
        let default_message = &default_request.messages[0];
        assert_eq!(default_message.role, "assistant");
        assert_eq!(
            default_message.reasoning_content.as_deref(),
            Some("canonical thought")
        );
        // Default provider preserves BOTH field names faithfully so the value
        // round-trips on multi-turn requests `reasoning_content` and
        // `reasoning` are carried independently, not collapsed into one.
        assert_eq!(default_message.reasoning.as_deref(), Some("alias thought"));
        assert!(default_message.tool_calls.is_some());
        let default_json = serde_json::to_value(default_message).unwrap();
        assert_eq!(
            default_json.get("reasoning_content"),
            Some(&serde_json::json!("canonical thought"))
        );
        assert_eq!(
            default_json.get("reasoning"),
            Some(&serde_json::json!("alias thought"))
        );

        let groq_provider = OpenAiCompatibleModelProvider::builder("test")
            .display_name("Groq")
            .base_url("https://api.groq.com/openai/v1")
            .credential(None)
            .auth_style(AuthStyle::Bearer)
            .without_assistant_reasoning_replay()
            .build();
        let groq_request = groq_provider.build_native_tool_chat_request(
            &messages,
            None,
            "openai/gpt-oss-120b",
            None,
            true,
            false,
        );
        let groq_message = &groq_request.messages[0];
        assert_eq!(groq_message.role, "assistant");
        assert!(groq_message.reasoning_content.is_none());
        assert!(groq_message.tool_calls.is_some());
        let groq_json = serde_json::to_value(groq_message).unwrap();
        assert_eq!(groq_json.get("role"), Some(&serde_json::json!("assistant")));
        assert_eq!(
            groq_json
                .get("tool_calls")
                .and_then(serde_json::Value::as_array)
                .map(Vec::len),
            Some(1)
        );
        assert!(groq_json.get("reasoning_content").is_none());
        assert!(groq_json.get("reasoning").is_none());
    }

    #[test]
    fn convert_messages_for_native_no_reasoning_content_when_absent() {
        // Normal model history without reasoning_content key
        let history_json = serde_json::json!({
            "content": "I will check",
            "tool_calls": [{
                "id": "tc_1",
                "name": "shell",
                "arguments": "{\"cmd\":\"ls\"}"
            }]
        });

        let messages = vec![ChatMessage::assistant(history_json.to_string())];
        let provider = make_model_provider("test", "https://example.com", None);
        let native = provider.convert_messages_for_native(&messages, true);
        assert_eq!(native.len(), 1);
        assert!(native[0].reasoning_content.is_none());
    }

    #[test]
    fn convert_messages_for_native_round_trips_reasoning_content_without_tool_calls() {
        let history_json = serde_json::json!({
            "content": "Direct answer.",
            "reasoning_content": "Let me think step by step..."
        });

        let messages = vec![ChatMessage::assistant(history_json.to_string())];
        let provider = make_model_provider("test", "https://example.com", None);
        let native = provider.convert_messages_for_native(&messages, true);
        assert_eq!(native.len(), 1);
        assert_eq!(native[0].role, "assistant");
        assert!(
            native[0].tool_calls.is_none(),
            "no tool_calls on a plain-text turn"
        );
        assert_eq!(
            native[0].reasoning_content.as_deref(),
            Some("Let me think step by step...")
        );
        match &native[0].content {
            Some(MessageContent::Text(t)) => assert_eq!(t, "Direct answer."),
            other => panic!("expected text content, got {other:?}"),
        }
    }

    #[test]
    fn convert_messages_for_native_content_only_json_falls_through() {
        let structured_answer = serde_json::json!({"content": "raw"});
        let raw_json = structured_answer.to_string();
        let messages = vec![ChatMessage::assistant(raw_json.clone())];
        let provider = make_model_provider("test", "https://example.com", None);
        let native = provider.convert_messages_for_native(&messages, true);
        assert_eq!(native.len(), 1);
        assert!(native[0].reasoning_content.is_none());
        assert!(native[0].tool_calls.is_none());
        match &native[0].content {
            Some(MessageContent::Text(t)) => assert_eq!(t.as_str(), raw_json.as_str()),
            other => panic!("expected text content from fallback, got {other:?}"),
        }
    }

    #[test]
    fn convert_messages_for_native_non_string_reasoning_content_falls_through() {
        let structured_answer = serde_json::json!({
            "content": "raw",
            "reasoning_content": null
        });
        let raw_json = structured_answer.to_string();
        let messages = vec![ChatMessage::assistant(raw_json.clone())];
        let provider = make_model_provider("test", "https://example.com", None);
        let native = provider.convert_messages_for_native(&messages, true);
        assert_eq!(native.len(), 1);
        assert!(native[0].reasoning_content.is_none());
        assert!(native[0].tool_calls.is_none());
        match &native[0].content {
            Some(MessageContent::Text(t)) => assert_eq!(t.as_str(), raw_json.as_str()),
            other => panic!("expected text content from fallback, got {other:?}"),
        }
    }

    #[test]
    fn convert_messages_for_native_unrelated_json_falls_through() {
        let unrelated = serde_json::json!({"foo": "bar"});
        let messages = vec![ChatMessage::assistant(unrelated.to_string())];
        let provider = make_model_provider("test", "https://example.com", None);
        let native = provider.convert_messages_for_native(&messages, true);
        assert_eq!(native.len(), 1);
        assert!(native[0].reasoning_content.is_none());
        assert!(native[0].tool_calls.is_none());
        match &native[0].content {
            Some(MessageContent::Text(t)) => {
                assert!(
                    t.contains("\"foo\""),
                    "expected raw JSON in fallback content, got {t:?}"
                );
            }
            other => panic!("expected text content from fallback, got {other:?}"),
        }
    }

    #[test]
    fn convert_messages_for_native_omits_empty_tool_call_content() {
        let empty_history_json = serde_json::json!({
            "content": "",
            "tool_calls": [{
                "id": "tc_1",
                "name": "shell",
                "arguments": "{\"cmd\":\"ls\"}"
            }]
        });
        let non_empty_history_json = serde_json::json!({
            "content": "I will check",
            "tool_calls": [{
                "id": "tc_2",
                "name": "shell",
                "arguments": "{\"cmd\":\"pwd\"}"
            }]
        });

        let messages = vec![
            ChatMessage::assistant(empty_history_json.to_string()),
            ChatMessage::assistant(non_empty_history_json.to_string()),
        ];
        let provider = make_model_provider("test", "https://example.com", None);
        let native = provider.convert_messages_for_native(&messages, true);
        let empty_json = serde_json::to_value(&native[0]).unwrap();
        let non_empty_json = serde_json::to_value(&native[1]).unwrap();

        assert_eq!(empty_json.get("content"), None);
        assert_ne!(
            empty_json.get("content"),
            Some(&serde_json::Value::String(String::new()))
        );
        assert_eq!(
            non_empty_json.get("content"),
            Some(&serde_json::json!("I will check"))
        );
    }

    #[test]
    fn convert_messages_for_native_sends_string_tool_call_content_on_cloudflare() {
        // Cloudflare Workers AI rejects an assistant tool-call message whose
        // `content` is absent or null (HTTP 400, AiError 5006). Measured:
        // content=null -> 400, content omitted -> 400, content="" -> 200.
        // Every other backend keeps the omitting behaviour pinned by
        // convert_messages_for_native_omits_empty_tool_call_content.
        let history_json = serde_json::json!({
            "content": "",
            "tool_calls": [{
                "id": "tc_1",
                "name": "realms_proposal_firewall",
                "arguments": "{}"
            }]
        });
        let messages = vec![ChatMessage::assistant(history_json.to_string())];

        let cloudflare = make_model_provider(
            "workers_ai",
            "https://api.cloudflare.com/client/v4/accounts/acct/ai/v1/chat/completions",
            None,
        );
        let native = cloudflare.convert_messages_for_native(&messages, true);
        let json = serde_json::to_value(&native[0]).unwrap();
        assert_eq!(
            json.get("content"),
            Some(&serde_json::Value::String(String::new())),
            "Cloudflare must receive content as a string, not an omitted field"
        );

        let other = make_model_provider("test", "https://example.com", None);
        let native = other.convert_messages_for_native(&messages, true);
        let json = serde_json::to_value(&native[0]).unwrap();
        assert_eq!(
            json.get("content"),
            None,
            "non-Cloudflare backends keep the existing omitting behaviour"
        );
    }

    #[test]
    fn convert_messages_for_native_reasoning_content_serialized_only_when_present() {
        // Verify skip_serializing_if works: reasoning_content omitted from JSON when None
        let msg_without = NativeMessage {
            role: "assistant".to_string(),
            content: Some(MessageContent::Text("hi".to_string())),
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
            reasoning: None,
            name: None,
        };
        let json = serde_json::to_string(&msg_without).unwrap();
        assert!(
            !json.contains("reasoning_content"),
            "reasoning_content should be omitted when None"
        );

        let msg_with = NativeMessage {
            role: "assistant".to_string(),
            content: Some(MessageContent::Text("hi".to_string())),
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: Some("thinking...".to_string()),
            reasoning: None,
            name: None,
        };
        let json = serde_json::to_string(&msg_with).unwrap();
        assert!(
            json.contains("reasoning_content"),
            "reasoning_content should be present when Some"
        );
        assert!(json.contains("thinking..."));
    }

    #[test]
    fn default_timeout_is_120s() {
        let p = make_model_provider("test", "https://example.com", None);
        assert_eq!(p.timeout_secs, 120);
    }

    #[test]
    fn stream_idle_timeout_keeps_300s_floor_when_timeout_secs_is_lower() {
        assert_eq!(
            crate::stream_idle_timeout(120),
            crate::StreamIdleBound::Configurable(std::time::Duration::from_secs(300))
        );
        assert_eq!(
            crate::stream_idle_timeout(300),
            crate::StreamIdleBound::Configurable(std::time::Duration::from_secs(300))
        );
    }

    #[test]
    fn stream_idle_timeout_raises_bound_when_timeout_secs_exceeds_floor() {
        assert_eq!(
            crate::stream_idle_timeout(301),
            crate::StreamIdleBound::Configurable(std::time::Duration::from_secs(301))
        );
        assert_eq!(
            crate::stream_idle_timeout(3600),
            crate::StreamIdleBound::Configurable(std::time::Duration::from_secs(3600))
        );
    }

    #[tokio::test]
    async fn stream_idle_error_message_names_bound_on_read_timeout() {
        // Accept the connection, then never write a byte: the read-idle bound
        // fires while the streaming client waits for response headers.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = ::clawcrew_spawn::spawn!(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            futures_util::future::pending::<()>().await;
        });
        let client = reqwest::Client::builder()
            .read_timeout(std::time::Duration::from_millis(100))
            .build()
            .unwrap();
        let err = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.get(format!("http://{addr}/stream")).send(),
        )
        .await
        .expect("stalled headers must hit the read-idle bound")
        .unwrap_err();
        server.abort();
        assert!(
            err.is_timeout(),
            "stalled headers must surface as a timeout: {err}"
        );
        assert!(!err.is_connect(), "the connection itself succeeded: {err}");
        let message = crate::stream_idle_error_message(
            &err,
            crate::StreamIdleBound::Configurable(std::time::Duration::from_secs(300)),
        );
        assert!(
            message.contains("no data from provider for 300s"),
            "idle message must name the bound that fired: {message}"
        );
        assert!(
            message.contains("stream idle timeout"),
            "idle message must name the idle timeout: {message}"
        );
        assert!(
            message.contains("raise timeout_secs above 300s"),
            "idle message must name the knob that raises the bound: {message}"
        );
        assert!(
            message.contains(&crate::format_error_chain(&err)),
            "the underlying reqwest error must stay in the chain: {message}"
        );
    }

    #[tokio::test]
    async fn stream_idle_error_message_leaves_connect_errors_unchanged() {
        let err = reqwest::Client::new()
            .get("http://127.0.0.1:1/stream")
            .send()
            .await
            .unwrap_err();
        assert!(
            err.is_connect(),
            "a refused local port is a connect error: {err}"
        );
        let message = crate::stream_idle_error_message(
            &err,
            crate::StreamIdleBound::Configurable(std::time::Duration::from_secs(300)),
        );
        assert_eq!(
            message,
            crate::format_error_chain(&err),
            "connect errors must keep the plain error chain"
        );
    }

    #[tokio::test]
    async fn sse_chunk_stream_names_idle_bound_when_body_goes_silent() {
        use axum::{Router, response::IntoResponse, routing::get};
        use futures_util::StreamExt as _;

        let app = Router::new().route(
            "/stream",
            get(|| async {
                let first = futures_util::stream::once(async {
                    Ok::<_, std::convert::Infallible>(axum::body::Bytes::from_static(
                        b"data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n",
                    ))
                });
                let open = futures_util::stream::pending::<
                    Result<axum::body::Bytes, std::convert::Infallible>,
                >();
                axum::body::Body::from_stream(first.chain(open)).into_response()
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = ::clawcrew_spawn::spawn!(async move {
            axum::serve(listener, app).await.unwrap();
        });
        // A 1 s read-idle bound gives the header exchange and first chunk a
        // comfortable window on a slow runner; the silent body still trips it
        // in about a second, keeping the whole test under ~2 s.
        let client = reqwest::Client::builder()
            .read_timeout(std::time::Duration::from_secs(1))
            .build()
            .unwrap();
        let response = client
            .get(format!("http://{addr}/stream"))
            .send()
            .await
            .unwrap();
        let mut stream = sse_bytes_to_chunks(
            response,
            false,
            crate::StreamIdleBound::Fixed(std::time::Duration::from_secs(300)),
        );
        let first = tokio::time::timeout(std::time::Duration::from_secs(2), stream.next())
            .await
            .expect("first chunk must arrive before the stall")
            .expect("chunk stream must yield a chunk")
            .expect("first chunk must be valid");
        assert_eq!(first.delta, "hi");
        let item = tokio::time::timeout(std::time::Duration::from_secs(2), stream.next())
            .await
            .expect("stall must hit the read-idle bound")
            .expect("stalled chunk stream must yield an item");
        server.abort();
        let message = match item {
            Err(StreamError::Http(message)) => message,
            Err(other) => panic!("expected an HTTP stream error, got {other:?}"),
            Ok(_) => panic!("expected an error after the stalled body"),
        };
        assert!(
            message.contains("no data from provider for 300s"),
            "idle message must name the bound that fired: {message}"
        );
        assert!(
            message.contains("stream idle timeout"),
            "idle message must name the idle timeout: {message}"
        );
        assert!(
            !message.contains("timeout_secs"),
            "a fixed idle bound has no knob advice: {message}"
        );
    }

    #[test]
    fn timeout_secs_overrides_default() {
        let p = OpenAiCompatibleModelProvider::builder("test")
            .display_name("test")
            .base_url("https://example.com")
            .credential(None)
            .auth_style(AuthStyle::Bearer)
            .timeout_secs(300)
            .build();
        assert_eq!(p.timeout_secs, 300);
    }

    #[test]
    fn extra_headers_default_empty() {
        let p = make_model_provider("test", "https://example.com", None);
        assert!(p.extra_headers.is_empty());
    }

    #[test]
    fn extra_headers_sets_headers() {
        let mut headers = std::collections::HashMap::new();
        headers.insert("X-Title".to_string(), "clawcrew".to_string());
        headers.insert(
            "HTTP-Referer".to_string(),
            "https://example.com".to_string(),
        );
        let p = OpenAiCompatibleModelProvider::builder("test")
            .display_name("test")
            .base_url("https://example.com")
            .credential(None)
            .auth_style(AuthStyle::Bearer)
            .extra_headers(headers)
            .build();
        assert_eq!(p.extra_headers.len(), 2);
        assert_eq!(p.extra_headers.get("X-Title").unwrap(), "clawcrew");
        assert_eq!(
            p.extra_headers.get("HTTP-Referer").unwrap(),
            "https://example.com"
        );
    }

    #[test]
    fn http_client_with_extra_headers_builds_successfully() {
        let mut headers = std::collections::HashMap::new();
        headers.insert("X-Title".to_string(), "clawcrew".to_string());
        headers.insert("User-Agent".to_string(), "TestAgent/1.0".to_string());
        let p = OpenAiCompatibleModelProvider::builder("test")
            .display_name("test")
            .base_url("https://example.com")
            .credential(None)
            .auth_style(AuthStyle::Bearer)
            .extra_headers(headers)
            .build();
        // Should not panic
        let _client = p.http_client();
    }

    #[test]
    fn http_client_without_extra_headers_or_user_agent() {
        let p = make_model_provider("test", "https://example.com", None);
        // Should use the cached proxy client path
        let _client = p.http_client();
    }

    #[test]
    fn extra_headers_combined_with_user_agent() {
        let mut headers = std::collections::HashMap::new();
        headers.insert("X-Title".to_string(), "clawcrew".to_string());
        let p = OpenAiCompatibleModelProvider::builder("test")
            .display_name("test")
            .base_url("https://example.com")
            .credential(None)
            .auth_style(AuthStyle::Bearer)
            .user_agent("CustomAgent/1.0")
            .extra_headers(headers)
            .build();
        assert_eq!(p.user_agent.as_deref(), Some("CustomAgent/1.0"));
        assert_eq!(p.extra_headers.len(), 1);
        // Should not panic
        let _client = p.http_client();
    }

    #[test]
    fn tool_call_none_fields_omitted_from_json() {
        // Ensures model_providers like Mistral that reject extra fields (e.g. "name": null)
        // don't receive them when the ToolCall compat fields are None.
        let tc = ToolCall {
            id: Some("call_1".to_string()),
            kind: Some("function".to_string()),
            function: Some(Function {
                name: Some("shell".to_string()),
                arguments: Some("{\"command\":\"ls\"}".to_string()),
            }),
            name: None,
            arguments: None,
            parameters: None,
            extra_content: None,
        };
        let json = serde_json::to_value(&tc).unwrap();
        assert!(!json.as_object().unwrap().contains_key("name"));
        assert!(!json.as_object().unwrap().contains_key("arguments"));
        assert!(!json.as_object().unwrap().contains_key("parameters"));
        // Standard fields must be present
        assert!(json.as_object().unwrap().contains_key("id"));
        assert!(json.as_object().unwrap().contains_key("type"));
        assert!(json.as_object().unwrap().contains_key("function"));
    }

    #[test]
    fn tool_call_with_compat_fields_serializes_them() {
        // When compat fields are Some, they should appear in the output.
        let tc = ToolCall {
            id: None,
            kind: None,
            function: None,
            name: Some("shell".to_string()),
            arguments: Some("{\"command\":\"ls\"}".to_string()),
            parameters: None,
            extra_content: None,
        };
        let json = serde_json::to_value(&tc).unwrap();
        assert_eq!(json["name"], "shell");
        assert_eq!(json["arguments"], "{\"command\":\"ls\"}");
        // None fields should be omitted
        assert!(!json.as_object().unwrap().contains_key("id"));
        assert!(!json.as_object().unwrap().contains_key("type"));
        assert!(!json.as_object().unwrap().contains_key("function"));
        assert!(!json.as_object().unwrap().contains_key("parameters"));
    }

    // ── parse_proxy_tool_event tests ──

    #[test]
    fn proxy_tool_start_valid() {
        let line = r#"data: {"x_tool_start":{"name":"bash","arguments":"{\"cmd\":\"ls\"}"}}"#;
        let event = parse_proxy_tool_event(line);
        assert!(matches!(
            event,
            Some(StreamEvent::PreExecutedToolCall { ref name, ref args })
            if name == "bash" && args == r#"{"cmd":"ls"}"#
        ));
    }

    #[test]
    fn proxy_tool_start_missing_name_returns_none() {
        let line = r#"data: {"x_tool_start":{"arguments":"{}"}}"#;
        assert!(parse_proxy_tool_event(line).is_none());
    }

    #[test]
    fn proxy_tool_start_missing_arguments_defaults() {
        let line = r#"data: {"x_tool_start":{"name":"read"}}"#;
        let event = parse_proxy_tool_event(line);
        assert!(matches!(
            event,
            Some(StreamEvent::PreExecutedToolCall { ref name, ref args })
            if name == "read" && args == "{}"
        ));
    }

    #[test]
    fn proxy_tool_result_valid() {
        let line = r#"data: {"x_tool_result":{"name":"bash","output":"hello world"}}"#;
        let event = parse_proxy_tool_event(line);
        assert!(matches!(
            event,
            Some(StreamEvent::PreExecutedToolResult { ref name, ref output })
            if name == "bash" && output == "hello world"
        ));
    }

    #[test]
    fn proxy_tool_result_missing_fields_uses_defaults() {
        let line = r#"data: {"x_tool_result":{}}"#;
        let event = parse_proxy_tool_event(line);
        assert!(matches!(
            event,
            Some(StreamEvent::PreExecutedToolResult { ref name, ref output })
            if name == "unknown" && output.is_empty()
        ));
    }

    #[test]
    fn proxy_tool_event_non_json_returns_none() {
        assert!(parse_proxy_tool_event("data: not json").is_none());
    }

    #[test]
    fn proxy_tool_event_no_data_prefix_returns_none() {
        let line = r#"{"x_tool_start":{"name":"bash"}}"#;
        assert!(parse_proxy_tool_event(line).is_none());
    }

    #[test]
    fn proxy_tool_event_standard_openai_chunk_returns_none() {
        let line = r#"data: {"id":"chatcmpl-1","choices":[{"delta":{"content":"hi"}}]}"#;
        assert!(parse_proxy_tool_event(line).is_none());
    }

    #[test]
    fn proxy_tool_event_done_sentinel_returns_none() {
        assert!(parse_proxy_tool_event("data: [DONE]").is_none());
    }

    #[test]
    fn strip_native_tool_messages_coalesces_adjacent_assistants() {
        let messages = vec![
            ChatMessage::user("search for cats"),
            ChatMessage::assistant(
                r#"{"content":"I'll search","tool_calls":[{"id":"t1","name":"web_search","arguments":"{}"}]}"#,
            ),
            ChatMessage::tool(r#"{"tool_call_id":"t1","content":"Found 10 results"}"#),
            ChatMessage::assistant("Here are the results about cats"),
        ];
        let p = OpenAiCompatibleModelProvider::builder("test")
            .display_name("MiniMax")
            .base_url("https://api.minimax.chat/v1")
            .credential(Some("k"))
            .auth_style(AuthStyle::Bearer)
            .merge_system_into_user()
            .build();
        let stripped = p.strip_native_tool_messages(&messages);
        let roles: Vec<&str> = stripped.iter().map(|m| m.role.as_str()).collect();
        assert!(
            !roles.windows(2).any(|w| w[0] == w[1]),
            "no two consecutive messages should share a role; got {roles:?}"
        );
        // Sanity: user turn and merged assistant content both survive.
        assert_eq!(roles, vec!["user", "assistant"]);
        assert_eq!(stripped[0].content, "search for cats");
        assert!(
            stripped[1].content.contains("I'll search")
                && stripped[1]
                    .content
                    .contains("Here are the results about cats"),
            "merged assistant should preserve both the pre-tool narration and the final reply; \
             got {:?}",
            stripped[1].content
        );
    }

    #[test]
    fn strip_native_tool_messages_coalesces_adjacent_users() {
        let messages = vec![
            ChatMessage::user("summarize this build output"),
            ChatMessage::assistant(
                r#"{"content":"","tool_calls":[{"id":"t1","name":"shell","arguments":"{}"}]}"#,
            ),
            ChatMessage::tool(r#"{"tool_call_id":"t1","content":"cargo output"}"#),
            ChatMessage::user("go on"),
        ];
        let p = OpenAiCompatibleModelProvider::builder("test")
            .display_name("Anthropic-compatible")
            .base_url("https://example.test/v1")
            .credential(Some("k"))
            .auth_style(AuthStyle::Bearer)
            .merge_system_into_user()
            .build();
        let stripped = p.strip_native_tool_messages(&messages);
        let roles: Vec<&str> = stripped.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(roles, vec!["user"]);
        assert!(
            stripped[0].content.contains("summarize this build output")
                && stripped[0].content.contains("go on"),
            "merged user message should preserve the original prompt and continuation; got {:?}",
            stripped[0].content
        );
    }

    #[test]
    fn strip_native_tool_messages_drops_internal_pruning_markers_before_coalescing() {
        let messages = vec![
            ChatMessage {
                role: "assistant".to_string(),
                content: ChatMessage::pruned_tool_exchange_summary(1),
            },
            ChatMessage::pruned_context_separator(),
            ChatMessage::assistant(
                r#"{"content":"","tool_calls":[{"id":"t1","name":"shell","arguments":"{}"}]}"#,
            ),
            ChatMessage::tool(r#"{"tool_call_id":"t1","content":"cargo output"}"#),
            ChatMessage::user("go on"),
        ];
        let p = OpenAiCompatibleModelProvider::builder("test")
            .display_name("Anthropic-compatible")
            .base_url("https://example.test/v1")
            .credential(Some("k"))
            .auth_style(AuthStyle::Bearer)
            .merge_system_into_user()
            .build();
        let stripped = p.strip_native_tool_messages(&messages);
        let roles: Vec<&str> = stripped.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(roles, vec!["user"]);
        assert_eq!(stripped[0].content, "go on");
    }

    #[test]
    fn strip_native_tool_messages_drops_empty_narration_cleanly() {
        let messages = vec![
            ChatMessage::user("search for cats"),
            ChatMessage::assistant(
                r#"{"content":"","tool_calls":[{"id":"t1","name":"web_search","arguments":"{}"}]}"#,
            ),
            ChatMessage::tool(r#"{"tool_call_id":"t1","content":"Found"}"#),
            ChatMessage::assistant("Here are the results"),
        ];
        let p = OpenAiCompatibleModelProvider::builder("test")
            .display_name("MiniMax")
            .base_url("https://api.minimax.chat/v1")
            .credential(Some("k"))
            .auth_style(AuthStyle::Bearer)
            .merge_system_into_user()
            .build();
        let stripped = p.strip_native_tool_messages(&messages);
        assert_eq!(
            stripped.iter().map(|m| m.role.as_str()).collect::<Vec<_>>(),
            vec!["user", "assistant"]
        );
        assert_eq!(stripped[1].content, "Here are the results");
    }

    #[tokio::test]
    async fn stream_chat_with_tools_sends_typed_tools_in_streaming_body() {
        use axum::response::IntoResponse;
        use axum::{Json, Router, routing::post};
        use futures_util::StreamExt as _;
        use std::sync::Mutex;
        use tokio::net::TcpListener;

        // Pins the stream_chat call-site wiring into
        // build_streaming_native_tool_request (helper-level tests alone
        // would not catch e.g. swapped bool arguments or tools dropped at
        // the call site).
        let captured: std::sync::Arc<Mutex<Option<serde_json::Value>>> =
            std::sync::Arc::new(Mutex::new(None));
        let captured_clone = captured.clone();

        let app = Router::new().route(
            "/chat/completions",
            post(move |Json(body): Json<serde_json::Value>| {
                let cap = captured_clone.clone();
                async move {
                    *cap.lock().unwrap() = Some(body);
                    (
                        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                        "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\ndata: [DONE]\n\n",
                    )
                        .into_response()
                }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_handle = ::clawcrew_spawn::spawn!(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let provider = make_model_provider("vllm", &format!("http://{addr}"), Some("key"));
        let tools = vec![clawcrew_api::tool::ToolSpec::new(
            "get_weather",
            "Fetch the weather",
            serde_json::json!({ "type": "object", "properties": {} }),
        )];

        let mut stream = provider.stream_chat(
            crate::traits::ChatRequest {
                messages: &[ChatMessage::user("hi")],
                tools: Some(&tools),
                thinking: None,
            },
            "test-model",
            None,
            StreamOptions {
                enabled: true,
                count_tokens: false,
            },
        );
        while stream.next().await.is_some() {}

        let body = captured
            .lock()
            .unwrap()
            .take()
            .expect("no streaming request captured");
        assert_eq!(body["stream"], serde_json::json!(true));
        assert_eq!(body["tool_choice"], serde_json::json!("auto"));
        assert_eq!(
            body["tools"],
            serde_json::json!([{
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Fetch the weather",
                    "parameters": { "type": "object", "properties": {} }
                }
            }]),
            "streaming request body must carry the converted typed tools"
        );

        server_handle.abort();
    }

    #[tokio::test]
    async fn chat_with_tools_forwards_raw_specs_without_validation_or_sanitizing() {
        use axum::{Json, Router, routing::post};
        use std::sync::Mutex;
        use tokio::net::TcpListener;

        let captured: std::sync::Arc<Mutex<Option<serde_json::Value>>> =
            std::sync::Arc::new(Mutex::new(None));
        let captured_clone = captured.clone();

        let app = Router::new().route(
            "/chat/completions",
            post(move |Json(body): Json<serde_json::Value>| {
                let cap = captured_clone.clone();
                async move {
                    *cap.lock().unwrap() = Some(body);
                    Json(serde_json::json!({
                        "id": "chatcmpl-test",
                        "choices": [{
                            "index": 0,
                            "message": { "role": "assistant", "content": "ok" },
                            "finish_reason": "stop"
                        }],
                        "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
                    }))
                }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_handle = ::clawcrew_spawn::spawn!(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let provider = OpenAiCompatibleModelProvider::builder("lmstudio")
            .display_name("lmstudio")
            .base_url(&format!("http://{addr}"))
            .credential(Some("key"))
            .auth_style(AuthStyle::Bearer)
            .local_model_tool_sanitize()
            .build();
        let messages = vec![ChatMessage::user("hello")];
        let tools = vec![
            // OpenAI permits both description and parameters to be omitted.
            serde_json::json!({
                "type": "function",
                "function": { "name": "get_weather" }
            }),
            // Raw callers historically controlled vendor extensions and
            // schema shape. Even with local sanitization configured, this
            // entry must not be parsed or cleaned in this allocation-only PR.
            serde_json::json!({
                "type": "vendor_extension",
                "function": {
                    "name": "lookup",
                    "parameters": {
                        "$defs": { "Id": { "type": "string" } },
                        "additionalProperties": false
                    }
                },
                "x_vendor_hint": "keep-me"
            }),
        ];

        let result = provider
            .chat_with_tools(&messages, &tools, "gemma-4-9b-it", None)
            .await;
        assert!(
            result.is_ok(),
            "raw compatible-provider specs must be forwarded: {:?}",
            result.err()
        );

        let body = captured
            .lock()
            .unwrap()
            .take()
            .expect("no request captured by mock server");
        assert_eq!(
            body["tools"],
            serde_json::json!(tools),
            "raw tools must reach the request body byte-shape-equivalent, \
             including optional-field omissions and sanitizer-sensitive keys"
        );
        assert_eq!(
            body["tool_choice"],
            serde_json::json!("auto"),
            "tool_choice must be auto when tools are present"
        );

        server_handle.abort();
    }

    #[tokio::test]
    async fn dropping_stream_aborts_forwarder_and_closes_upstream_socket() {
        use axum::Router;
        use axum::response::IntoResponse;
        use axum::routing::post;
        use futures_util::StreamExt as _;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        use tokio::net::TcpListener;

        let handler_dropped = Arc::new(AtomicBool::new(false));
        let handler_dropped_for_route = Arc::clone(&handler_dropped);

        let app = Router::new().route(
            "/chat/completions",
            post(move || {
                let dropped = Arc::clone(&handler_dropped_for_route);
                async move {
                    let sentinel = scopeguard::guard((), move |()| {
                        dropped.store(true, Ordering::SeqCst);
                    });
                    let first = futures_util::stream::once(async {
                        Ok::<_, std::convert::Infallible>(axum::body::Bytes::from(
                            "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n",
                        ))
                    });
                    let park = futures_util::stream::poll_fn(move |_cx| {
                        let _ = &sentinel;
                        std::task::Poll::Pending
                    });
                    axum::body::Body::from_stream(first.chain(park)).into_response()
                }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = ::clawcrew_spawn::spawn!(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let provider = OpenAiCompatibleModelProvider::builder("test")
            .display_name("test")
            .base_url(&format!("http://{addr}"))
            .credential(Some("k"))
            .auth_style(AuthStyle::Bearer)
            .build();

        let mut stream = provider.stream_chat(
            crate::traits::ChatRequest {
                messages: &[ChatMessage::user("hi")],
                tools: None,
                thinking: None,
            },
            "gpt-test",
            Some(0.0),
            StreamOptions {
                enabled: true,
                count_tokens: false,
            },
        );

        let first = stream.next().await;
        assert!(first.is_some(), "expected at least the first SSE chunk");

        drop(stream);

        let observed = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if handler_dropped.load(Ordering::SeqCst) {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await;

        server.abort();
        assert!(
            observed.is_ok(),
            "dropped stream must abort the forwarder and close the upstream socket"
        );
    }

    fn minimal_request(temperature: Option<f64>) -> ApiChatRequest {
        ApiChatRequest {
            model: "any-model".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::Text("hi".to_string()),
            }],
            temperature,
            stream: None,
            stream_options: None,
            reasoning_effort: None,
            tool_stream: None,
            tools: None,
            tool_choice: None,
            max_tokens: None,
            extra_body: None,
        }
    }

    #[test]
    fn unset_temperature_is_omitted_from_wire() {
        // `None` must honor the `Option<f64>` contract: no `temperature` field
        // on the wire, regardless of model name. Generalizes the former
        // kimi-k2-only special case
        let body = serde_json::to_value(minimal_request(None)).unwrap();
        assert!(
            body.get("temperature").is_none(),
            "unset temperature must be omitted from the request body, got: {body}"
        );
    }

    #[test]
    fn explicit_temperature_is_sent_on_wire() {
        let body = serde_json::to_value(minimal_request(Some(0.5))).unwrap();
        assert_eq!(
            body.get("temperature").and_then(|v| v.as_f64()),
            Some(0.5),
            "explicit temperature must be sent verbatim, got: {body}"
        );
    }

    #[test]
    fn models_dev_to_model_info_returns_no_pricing() {
        // The models.dev catalog does not serve pricing data; every entry
        // must have `pricing: None`. This documents the intentional contract.
        let ids = vec![
            ("openai/gpt-4o".to_string(), None),
            ("anthropic/claude-sonnet-4-6".to_string(), None),
        ];
        let models = models_dev_to_model_info(ids);
        assert_eq!(models.len(), 2);
        // Preserves input order (no sorting — caller decides).
        assert_eq!(models[0].id, "openai/gpt-4o");
        assert!(models[0].pricing.is_none());
        assert_eq!(models[1].id, "anthropic/claude-sonnet-4-6");
        assert!(models[1].pricing.is_none());
    }

    #[test]
    fn models_dev_to_model_info_carries_context_window() {
        // The catalog's `limit.context` must survive the mapping, and a model
        // the catalog gives no limit for must stay `None` — not a stub value.
        let ids = vec![
            ("anthropic/claude-opus-4-8".to_string(), Some(1_000_000)),
            ("some/unknown-model".to_string(), None),
        ];
        let models = models_dev_to_model_info(ids);
        assert_eq!(models[0].context_window, Some(1_000_000));
        assert_eq!(models[1].context_window, None);
    }

    #[test]
    fn public_model_listing_flag_defaults_false() {
        // Providers without explicit public_model_listing must default to false,
        // preserving existing behavior for all established providers.
        let p = make_model_provider("test", "https://example.com", None);
        assert!(!p.public_model_listing);
    }

    #[test]
    fn public_model_listing_flag_can_be_set() {
        // Verify the builder correctly enables public_model_listing.
        let p = OpenAiCompatibleModelProvider::builder("test")
            .display_name("test")
            .base_url("https://example.com")
            .credential(None)
            .auth_style(AuthStyle::Bearer)
            .public_model_listing()
            .build();
        assert!(p.public_model_listing);
    }

    #[test]
    fn token_count_u64_positive() {
        assert_eq!(normalize_token_count_value(serde_json::json!(42)), Some(42));
    }

    #[test]
    fn token_count_u64_zero() {
        assert_eq!(normalize_token_count_value(serde_json::json!(0)), Some(0));
    }

    #[test]
    fn token_count_large_u64() {
        assert_eq!(
            normalize_token_count_value(serde_json::json!(u64::MAX)),
            Some(u64::MAX)
        );
    }

    #[test]
    fn token_count_i64_positive() {
        assert_eq!(
            normalize_token_count_value(serde_json::json!(100i64)),
            Some(100)
        );
    }

    #[test]
    fn token_count_i64_negative() {
        assert_eq!(
            normalize_token_count_value(serde_json::json!(-1i64)),
            None,
            "negative token counts must be rejected"
        );
    }

    #[test]
    fn token_count_f64_positive_integer() {
        assert_eq!(
            normalize_token_count_value(serde_json::json!(15.0)),
            Some(15)
        );
    }

    #[test]
    fn token_count_f64_fractional() {
        assert_eq!(
            normalize_token_count_value(serde_json::json!(3.7)),
            Some(3),
            "fractional floats floor toward zero"
        );
    }

    #[test]
    fn token_count_f64_less_than_one() {
        assert_eq!(
            normalize_token_count_value(serde_json::json!(0.5)),
            Some(0),
            "fractional token < 1 counts as zero (avoids noise)"
        );
    }

    #[test]
    fn token_count_f64_negative() {
        assert_eq!(
            normalize_token_count_value(serde_json::json!(-0.5)),
            None,
            "negative float token counts must be rejected"
        );
    }

    #[test]
    fn token_count_f64_nan() {
        let nan: f64 = f64::NAN;
        assert_eq!(
            normalize_token_count_value(serde_json::json!(nan)),
            None,
            "NaN must be rejected"
        );
    }

    #[test]
    fn token_count_f64_infinity() {
        assert_eq!(
            normalize_token_count_value(serde_json::json!(f64::INFINITY)),
            None,
            "+Infinity must be rejected"
        );
    }

    #[test]
    fn token_count_f64_neg_infinity() {
        assert_eq!(
            normalize_token_count_value(serde_json::json!(f64::NEG_INFINITY)),
            None,
            "-Infinity must be rejected"
        );
    }

    #[test]
    fn token_count_f64_exceeds_u64_max() {
        assert_eq!(
            normalize_token_count_value(serde_json::json!(u64::MAX as f64 * 2.0)),
            None,
            "value > u64::MAX must be rejected"
        );
    }

    #[test]
    fn token_count_string_integer() {
        assert_eq!(
            normalize_token_count_value(serde_json::json!("15")),
            Some(15)
        );
    }

    #[test]
    fn token_count_string_float() {
        assert_eq!(
            normalize_token_count_value(serde_json::json!("3.7")),
            Some(3)
        );
    }

    #[test]
    fn token_count_string_whitespace() {
        assert_eq!(
            normalize_token_count_value(serde_json::json!(" 20 ")),
            Some(20)
        );
    }

    #[test]
    fn token_count_string_negative() {
        assert_eq!(normalize_token_count_value(serde_json::json!("-5")), None);
    }

    #[test]
    fn token_count_string_garbage() {
        assert_eq!(
            normalize_token_count_value(serde_json::json!("not-a-number")),
            None
        );
    }

    #[test]
    fn token_count_null() {
        assert_eq!(normalize_token_count_value(serde_json::Value::Null), None);
    }

    #[test]
    fn token_count_bool() {
        assert_eq!(
            normalize_token_count_value(serde_json::json!(true)),
            None,
            "boolean must not be misinterpreted as token count"
        );
    }

    #[test]
    fn token_count_array() {
        assert_eq!(
            normalize_token_count_value(serde_json::json!([1, 2, 3])),
            None
        );
    }

    #[test]
    fn token_count_object() {
        assert_eq!(
            normalize_token_count_value(serde_json::json!({"count": 10})),
            None
        );
    }

    // ── `deserialize_optional_token_count` round-trip tests ────────────
    // Validate the full deserialize path through a UsageInfo-shaped struct
    // so the serde attribute wiring is exercised as well.

    #[derive(Debug, Deserialize)]
    struct TestUsage {
        #[serde(default, deserialize_with = "deserialize_optional_token_count")]
        prompt_cache_hit_tokens: Option<u64>,
        #[serde(default, deserialize_with = "deserialize_optional_token_count")]
        prompt_tokens: Option<u64>,
    }

    #[test]
    fn deserialize_token_count_integer() {
        let usage: TestUsage =
            serde_json::from_str(r#"{"prompt_tokens": 5000, "prompt_cache_hit_tokens": 3000}"#)
                .unwrap();
        assert_eq!(usage.prompt_tokens, Some(5000));
        assert_eq!(usage.prompt_cache_hit_tokens, Some(3000));
    }

    #[test]
    fn deserialize_token_count_float() {
        let usage: TestUsage =
            serde_json::from_str(r#"{"prompt_tokens": 12.8, "prompt_cache_hit_tokens": 100.3}"#)
                .unwrap();
        assert_eq!(usage.prompt_tokens, Some(12));
        assert_eq!(usage.prompt_cache_hit_tokens, Some(100));
    }

    #[test]
    fn deserialize_token_count_string() {
        let usage: TestUsage =
            serde_json::from_str(r#"{"prompt_tokens": "1000", "prompt_cache_hit_tokens": "500"}"#)
                .unwrap();
        assert_eq!(usage.prompt_tokens, Some(1000));
        assert_eq!(usage.prompt_cache_hit_tokens, Some(500));
    }

    #[test]
    fn deserialize_token_count_null() {
        let usage: TestUsage =
            serde_json::from_str(r#"{"prompt_tokens": null, "prompt_cache_hit_tokens": null}"#)
                .unwrap();
        assert_eq!(usage.prompt_tokens, None);
        assert_eq!(usage.prompt_cache_hit_tokens, None);
    }

    #[test]
    fn deserialize_token_count_negative() {
        let usage: TestUsage =
            serde_json::from_str(r#"{"prompt_tokens": -1, "prompt_cache_hit_tokens": 0}"#).unwrap();
        assert_eq!(
            usage.prompt_tokens, None,
            "negative values must be rejected"
        );
        assert_eq!(usage.prompt_cache_hit_tokens, Some(0));
    }

    #[test]
    fn deserialize_token_count_missing_field() {
        let usage: TestUsage = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(usage.prompt_tokens, None);
        assert_eq!(usage.prompt_cache_hit_tokens, None);
    }

    // ── `UsageInfo::cached_input_tokens` priority tests ─────────────────
    // `prompt_cache_hit_tokens` (DeepSeek-style) takes priority over
    // `prompt_tokens_details.cached_tokens` (OpenAI-style).

    #[test]
    fn usageinfo_cached_input_prefers_prompt_cache_hit_tokens() {
        let json = serde_json::json!({
            "prompt_tokens": 100,
            "completion_tokens": 50,
            "prompt_cache_hit_tokens": 60,
            "prompt_tokens_details": {"cached_tokens": 20}
        });
        let usage: UsageInfo = serde_json::from_value(json).unwrap();
        assert_eq!(usage.cached_input_tokens(), Some(60));
    }

    #[test]
    fn usageinfo_cached_input_falls_back_to_details() {
        let json = serde_json::json!({
            "prompt_tokens": 100,
            "completion_tokens": 50,
            "prompt_tokens_details": {"cached_tokens": 20}
        });
        let usage: UsageInfo = serde_json::from_value(json).unwrap();
        assert_eq!(usage.cached_input_tokens(), Some(20));
    }

    #[test]
    fn usageinfo_cached_input_returns_none_when_absent() {
        let json = serde_json::json!({
            "prompt_tokens": 100,
            "completion_tokens": 50
        });
        let usage: UsageInfo = serde_json::from_value(json).unwrap();
        assert_eq!(usage.cached_input_tokens(), None);
    }

    #[test]
    fn usageinfo_into_provider_usage_forwards_cached_tokens() {
        let json = serde_json::json!({
            "prompt_tokens": 1000,
            "completion_tokens": 200,
            "prompt_cache_hit_tokens": 400
        });
        let usage: UsageInfo = serde_json::from_value(json).unwrap();
        let out = usage.into_provider_usage();
        assert_eq!(out.input_tokens, Some(1000));
        assert_eq!(out.output_tokens, Some(200));
        assert_eq!(out.cached_input_tokens, Some(400));
    }

    #[test]
    fn convert_messages_for_native_strips_reasoning_when_replay_disabled() {
        let provider = OpenAiCompatibleModelProvider::builder("test")
            .display_name("test")
            .base_url("https://example.com")
            .credential(None)
            .auth_style(AuthStyle::Bearer)
            .without_assistant_reasoning_replay()
            .build();
        let messages = vec![ChatMessage::assistant(
            r#"{"content":"ok","reasoning_content":"step 1"}"#.to_string(),
        )];
        let native = provider.convert_messages_for_native(&messages, true);
        assert_eq!(native.len(), 1);
        assert_eq!(native[0].role, "assistant");
        assert_eq!(native[0].reasoning_content, None);
        assert_eq!(native[0].reasoning, None);
    }

    #[test]
    fn convert_messages_for_native_tool_fallbacks_to_last_assistant_tool_call_id() {
        let provider = make_model_provider("test", "https://example.com", None);
        let messages = vec![
            ChatMessage::assistant(
                r#"{"content":null,"tool_calls":[{"id":"fc_123","name":"search","arguments":"{}"}]}"#.to_string(),
            ),
            ChatMessage::tool(
                r#"{"content":"result"}"#.to_string(), // missing tool_call_id
            ),
        ];
        let native = provider.convert_messages_for_native(&messages, true);
        assert_eq!(native.len(), 2);
        assert_eq!(native[1].role, "tool");
        assert_eq!(native[1].tool_call_id.as_deref(), Some("fc_123"));
    }

    #[test]
    fn convert_messages_for_native_tool_uses_explicit_id_when_present() {
        let provider = make_model_provider("test", "https://example.com", None);
        let messages = vec![
            ChatMessage::assistant(
                r#"{"content":null,"tool_calls":[{"id":"fc_123","name":"search","arguments":"{}"}]}"#.to_string(),
            ),
            ChatMessage::tool(
                r#"{"tool_call_id":"fc_456","content":"result"}"#.to_string(),
            ),
        ];
        let native = provider.convert_messages_for_native(&messages, true);
        assert_eq!(native.len(), 2);
        assert_eq!(native[1].role, "tool");
        assert_eq!(native[1].tool_call_id.as_deref(), Some("fc_456"));
    }
