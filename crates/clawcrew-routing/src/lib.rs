//! Dynamic routing crate for plugin self-registration.
//!
//! Provides `RouteHandler` trait and `DynamicRouter` for runtime route registration.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};
use axum::{extract::Request, response::Response};

/// Trait for handler implementations that can be registered dynamically.
#[async_trait::async_trait]
pub trait RouteHandler: Send + Sync + std::fmt::Debug {
    /// Handle the incoming request and return a response.
    async fn handle(&self, req: Request) -> Result<Response, String>;
    /// Returns the path pattern this handler matches (e.g., "/api/config").
    fn path_pattern(&self) -> &str;
    /// Returns the HTTP method this handler accepts (e.g., "GET", "POST").
    fn http_method(&self) -> &str;
    /// Returns a unique name for this handler.
    fn name(&self) -> &str;
    /// Returns the capability this handler provides (for discovery/authorization).
    fn capability(&self) -> Option<&str> {
        None
    }
}

/// Dynamic router that holds registered handlers and dispatches requests.
#[derive(Debug)]
pub struct DynamicRouter {
    routes: Arc<RwLock<HashMap<String, Box<dyn RouteHandler>>>>,
    fallback: Option<Box<dyn RouteHandler>>,
}

impl DynamicRouter {
    /// Creates a new empty DynamicRouter.
    pub fn new() -> Self {
        info!("Creating new DynamicRouter");
        Self {
            routes: Arc::new(RwLock::new(HashMap::new())),
            fallback: None,
        }
    }

    /// Registers a handler with the router.
    ///
    /// Returns an error if a handler for the same path_pattern already exists.
    pub async fn register(&self, handler: Box<dyn RouteHandler>) -> Result<(), String> {
        let path = handler.path_pattern().to_string();
        let name = handler.name().to_string();

        let mut routes = self.routes.write().await;

        if routes.contains_key(&path) {
            let err = format!("Route collision: handler '{}' for '{}' already registered", name, path);
            error!("{}", err);
            return Err(err);
        }

        routes.insert(path.clone(), handler);
        info!("Registered handler '{}' for '{}'", name, path);
        Ok(())
    }

    /// Sets a fallback handler for unmatched routes.
    pub async fn set_fallback(&mut self, handler: Box<dyn RouteHandler>) {
        self.fallback = Some(handler);
        info!("Fallback handler set");
    }

    /// Dispatches a request to the appropriate handler.
    ///
    /// Returns the result from the handler or an error if no handler matches.
    pub async fn dispatch(&self, req: Request) -> Result<Response, String> {
        let path = req.uri().path().to_string();
        let method = req.method().to_string();

        {
            let routes = self.routes.read().await;
            if let Some(handler) = routes.get(&path) {
                info!("Dispatching {} {} to handler '{}'", method, path, handler.name());
                return handler.handle(req).await;
            }
        }

        // No exact match found
        if let Some(ref fallback) = self.fallback {
            warn!("No handler for '{}', using fallback '{}'", path, fallback.name());
            return fallback.handle(req).await;
        }

        let err = format!("No handler registered for path '{}'", path);
        error!("{}", err);
        Err(err)
    }

    /// Returns the number of registered handlers.
    pub async fn len(&self) -> usize {
        self.routes.read().await.len()
    }

    /// Returns true if no handlers are registered.
    pub async fn is_empty(&self) -> bool {
        self.len().await == 0
    }
}

impl Default for DynamicRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{Method, Uri};
    use axum::body::Body;

    #[derive(Debug)]
    struct TestHandler {
        name: String,
        path: String,
        method: String,
    }

    #[async_trait::async_trait]
    impl RouteHandler for TestHandler {
        async fn handle(&self, _req: Request) -> Result<Response, String> {
            Ok(Response::new(Body::from("ok")))
        }

        fn path_pattern(&self) -> &str {
            &self.path
        }

        fn http_method(&self) -> &str {
            &self.method
        }

        fn name(&self) -> &str {
            &self.name
        }
    }

    #[tokio::test]
    async fn test_register_success() {
        let router = DynamicRouter::new();
        let handler = Box::new(TestHandler {
            name: "test".to_string(),
            path: "/api/test".to_string(),
            method: "GET".to_string(),
        });

        let result = router.register(handler).await;
        assert!(result.is_ok());
        assert_eq!(router.len().await, 1);
    }

    #[tokio::test]
    async fn test_register_collision() {
        let router = DynamicRouter::new();
        let handler1 = Box::new(TestHandler {
            name: "handler1".to_string(),
            path: "/api/test".to_string(),
            method: "GET".to_string(),
        });
        let handler2 = Box::new(TestHandler {
            name: "handler2".to_string(),
            path: "/api/test".to_string(),
            method: "POST".to_string(),
        });

        assert!(router.register(handler1).await.is_ok());
        let result = router.register(handler2).await;
        assert!(result.is_err());
        assert_eq!(router.len().await, 1);
    }

    #[tokio::test]
    async fn test_dispatch_no_match() {
        let router = DynamicRouter::new();
        let uri = Uri::try_from("/api/unknown").unwrap();
        let req = Request::builder()
            .method(Method::GET)
            .uri(uri)
            .body(Body::empty())
            .unwrap();

        let result = router.dispatch(req).await;
        assert!(result.is_err());
    }
}
