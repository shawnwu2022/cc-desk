//! Bounded loopback observation intake. A loopback address is not authentication.
use crate::hook_events::HookPayload;
use crate::observer_registry::{
    parse_observer_headers, ObserverAccept, ObserverDelivery, ObserverRegistry,
    MAX_OBSERVER_PAYLOAD,
};
use axum::{
    body::to_bytes,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

pub(crate) type Publisher =
    Arc<dyn Fn(&ObserverDelivery, HookPayload) -> Result<(), ()> + Send + Sync>;
#[derive(Clone)]
struct Intake {
    registry: Arc<ObserverRegistry>,
    publish: Publisher,
    concurrent: Arc<Semaphore>,
}

pub(crate) fn router(registry: Arc<ObserverRegistry>, publish: Publisher) -> Router {
    Router::new()
        .route("/observer", post(receive))
        // Old unauthenticated scripts must fail closed, never become run authority.
        .route(
            "/hook",
            post(|| async { reject(StatusCode::FORBIDDEN, "OBSERVER_AUTH_REQUIRED") }),
        )
        .with_state(Intake {
            registry,
            publish,
            concurrent: Arc::new(Semaphore::new(8)),
        })
}
fn reject(status: StatusCode, code: &str) -> Response {
    (status, Json(json!({"ok":false,"code":code}))).into_response()
}
async fn receive(State(state): State<Intake>, request: Request) -> Response {
    let Ok(_permit) = state.concurrent.try_acquire() else {
        return reject(StatusCode::TOO_MANY_REQUESTS, "OBSERVER_BUSY");
    };
    let headers = request.headers();
    if headers.contains_key("origin") {
        return reject(StatusCode::FORBIDDEN, "OBSERVER_ORIGIN_REJECTED");
    }
    let (binding, event_id) = match parse_observer_headers(headers) {
        Ok(value) => value,
        Err(_) => return reject(StatusCode::BAD_REQUEST, "OBSERVER_INVALID_HEADERS"),
    };
    // Authenticate before accepting/allocating the body, then check again before publication.
    if state.registry.check_binding(&binding).is_err() {
        return reject(StatusCode::UNAUTHORIZED, "OBSERVER_FORBIDDEN");
    }
    let body = match tokio::time::timeout(
        Duration::from_secs(3),
        to_bytes(request.into_body(), MAX_OBSERVER_PAYLOAD),
    )
    .await
    {
        Err(_) => return reject(StatusCode::REQUEST_TIMEOUT, "OBSERVER_BODY_TIMEOUT"),
        Ok(Err(_)) => return reject(StatusCode::PAYLOAD_TOO_LARGE, "OBSERVER_PAYLOAD_TOO_LARGE"),
        Ok(Ok(value)) => value,
    };
    match state.registry.accept_event(&binding, &event_id, &body) {
        Ok(ObserverAccept::Duplicate) => Json(json!({"ok":true,"duplicate":true})).into_response(),
        Ok(ObserverAccept::Accepted(event)) => {
            let delivery = match state.registry.delivery(&binding) {
                Ok(value) => value,
                Err(_) => return reject(StatusCode::UNAUTHORIZED, "OBSERVER_FORBIDDEN"),
            };
            let mut payload = HookPayload::from_validated(event);
            payload.pty_id = delivery.legacy_pty.clone();
            if (state.publish)(&delivery, payload).is_err() {
                return reject(StatusCode::SERVICE_UNAVAILABLE, "OBSERVER_UNAVAILABLE");
            }
            Json(json!({"ok":true})).into_response()
        }
        Err(failure) => {
            let status = match failure.code.as_str() {
                "OBSERVER_FORBIDDEN" => StatusCode::UNAUTHORIZED,
                "OBSERVER_EVENT_CAPACITY" => StatusCode::TOO_MANY_REQUESTS,
                _ => StatusCode::BAD_REQUEST,
            };
            reject(status, &failure.code)
        }
    }
}
