use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, watch};
use warp_ztna_nbrpc::prelude::{MeshAgentRpc, ReconnectBackoff, StatusFrame, StatusView};

use crate::state_name;

pub(crate) const STATUS_EVENT: &str = "agent-status";

const OFFLINE_GRACE: Duration = Duration::from_secs(20);
const RECONNECT_BASE: Duration = Duration::from_secs(1);
const RECONNECT_MAX: Duration = Duration::from_secs(30);
const FRAME_WATCHDOG: Duration = Duration::from_mins(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StreamHealth {
    Live,
    Reconnecting,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct StatusDto {
    pub(crate) state: &'static str,
    pub(crate) peer_ip: Option<String>,
    pub(crate) hostname: Option<String>,
    pub(crate) detail: Option<String>,
    pub(crate) daemon_version: Option<String>,
    pub(crate) session_expires_at: Option<i64>,
    pub(crate) routes: Vec<String>,
    pub(crate) link_ok: bool,
    pub(crate) stream: StreamHealth,
}

impl StatusDto {
    pub(crate) fn from_view(view: StatusView, routes: Vec<String>, stream: StreamHealth) -> Self {
        Self {
            state: state_name(view.state),
            peer_ip: view.peer_ip,
            hostname: view.hostname,
            detail: view.detail,
            daemon_version: view.daemon_version,
            session_expires_at: view.session_expires_at,
            routes,
            link_ok: view.link_ok,
            stream,
        }
    }

    pub(crate) fn cacheable(&self) -> bool {
        self.state != "connecting"
    }

    pub(crate) fn unknown(stream: StreamHealth) -> Self {
        Self {
            state: "unknown",
            peer_ip: None,
            hostname: None,
            detail: None,
            daemon_version: None,
            session_expires_at: None,
            routes: Vec::new(),
            link_ok: false,
            stream,
        }
    }

    pub(crate) fn disconnected(stream: StreamHealth) -> Self {
        Self {
            state: "disconnected",
            peer_ip: None,
            hostname: None,
            detail: None,
            daemon_version: None,
            session_expires_at: None,
            routes: Vec::new(),
            link_ok: false,
            stream,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct RouteCache {
    revision: Option<u64>,
    routes: Vec<String>,
}

impl RouteCache {
    fn needs_fetch(&self, revision: Option<u64>) -> bool {
        self.routes.is_empty() || revision.is_none() || self.revision != revision
    }

    fn recorded(&mut self, revision: Option<u64>, routes: &[String]) {
        self.routes.clear();
        self.routes.extend_from_slice(routes);
        if revision.is_some() {
            self.revision = revision;
        }
    }
}

pub(crate) struct StatusHub {
    latest: Mutex<Option<StatusDto>>,
    routes: Mutex<RouteCache>,
    connected_tx: watch::Sender<bool>,
}

impl Default for StatusHub {
    fn default() -> Self {
        let (connected_tx, _) = watch::channel(false);
        Self { latest: Mutex::new(None), routes: Mutex::new(RouteCache::default()), connected_tx }
    }
}

impl StatusHub {
    pub(crate) async fn publish(&self, dto: StatusDto) {
        let connected = dto.state == "connected";
        *self.latest.lock().await = Some(dto);
        let _ = self.connected_tx.send(connected);
    }

    pub(crate) fn subscribe_connected(&self) -> watch::Receiver<bool> {
        self.connected_tx.subscribe()
    }

    pub(crate) async fn cached_if_live(&self) -> Option<StatusDto> {
        self.latest.lock().await.clone().filter(|dto| dto.stream == StreamHealth::Live)
    }

    pub(crate) async fn latest(&self) -> Option<StatusDto> {
        self.latest.lock().await.clone()
    }

    pub(crate) async fn stream_health(&self) -> StreamHealth {
        self.latest.lock().await.as_ref().map_or(StreamHealth::Unavailable, |dto| dto.stream)
    }

    pub(crate) async fn republish(
        &self,
        stream: StreamHealth,
        state: Option<&'static str>,
    ) -> StatusDto {
        let mut latest = self.latest.lock().await;
        let dto = latest.get_or_insert_with(|| StatusDto::unknown(stream));
        dto.stream = stream;
        if let Some(state) = state {
            dto.state = state;
        }
        let dto = dto.clone();
        let _ = self.connected_tx.send(dto.state == "connected");
        dto
    }

    async fn routes_for(
        &self,
        client: &mut MeshAgentRpc,
        revision: Option<u64>,
        fallback: Vec<String>,
    ) -> Vec<String> {
        let mut cache = self.routes.lock().await;
        if !cache.needs_fetch(revision) {
            return cache.routes.clone();
        }
        match client.list_network_addresses().await {
            Ok(routes) => {
                cache.recorded(revision, &routes);
                routes
            }
            Err(_) => {
                if cache.routes.is_empty() {
                    fallback
                } else {
                    cache.routes.clone()
                }
            }
        }
    }
}

pub(crate) fn emit_status(app: &AppHandle, dto: &StatusDto) {
    let _ = app.emit(STATUS_EVENT, dto);
}

pub(crate) async fn publish_view(
    app: &AppHandle,
    hub: &StatusHub,
    client: &mut MeshAgentRpc,
    view: StatusView,
) -> StatusDto {
    let stream = hub.stream_health().await;
    let routes = client.list_network_addresses().await.unwrap_or_else(|_| view.routes.clone());
    let dto = StatusDto::from_view(view, routes, stream);
    hub.publish(dto.clone()).await;
    emit_status(app, &dto);
    dto
}

pub(crate) async fn run_status_stream(app: AppHandle, addr: String, hub: Arc<StatusHub>) {
    let mut backoff = ReconnectBackoff::new(RECONNECT_BASE, RECONNECT_MAX);
    let mut offline_since: Option<Instant> = None;
    let mut escalated = false;
    let mut revision: Option<u64> = None;

    loop {
        let mut client = match MeshAgentRpc::connect_str(&addr).await {
            Ok(client) => client,
            Err(error) => {
                log::debug!("agent status stream: connect failed: {error}");
                degrade(&app, &hub, StreamHealth::Reconnecting, &mut offline_since, &mut escalated)
                    .await;
                tokio::time::sleep(backoff.delay()).await;
                continue;
            }
        };

        let mut quiet_stream = false;
        match client.subscribe_status().await {
            Ok(mut stream) => {
                log::info!("agent status stream: subscribed");
                loop {
                    match tokio::time::timeout(FRAME_WATCHDOG, stream.next()).await {
                        Ok(Some(Ok(frame))) => {
                            offline_since = None;
                            escalated = false;
                            backoff.reset();
                            revision = frame.networks_revision;
                            publish_frame(&app, &hub, &mut client, frame).await;
                        }
                        Ok(Some(Err(error))) => {
                            log::warn!("agent status stream: {error}");
                            break;
                        }
                        Ok(None) => {
                            log::info!("agent status stream: closed");
                            break;
                        }
                        Err(_) => {
                            if reconcile(&app, &hub, &mut client, revision).await {
                                quiet_stream = true;
                                break;
                            }
                        }
                    }
                }
                if !quiet_stream {
                    degrade(
                        &app,
                        &hub,
                        StreamHealth::Reconnecting,
                        &mut offline_since,
                        &mut escalated,
                    )
                    .await;
                }
            }
            Err(error) if error.is_unimplemented() => {
                log::warn!(
                    "agent status stream: daemon lacks SubscribeStatus; relying on on-demand refresh"
                );
                degrade(&app, &hub, StreamHealth::Unavailable, &mut offline_since, &mut escalated)
                    .await;
                return;
            }
            Err(error) => {
                log::warn!("agent status stream: subscribe failed: {error}");
                degrade(&app, &hub, StreamHealth::Reconnecting, &mut offline_since, &mut escalated)
                    .await;
            }
        }

        if !quiet_stream {
            tokio::time::sleep(backoff.delay()).await;
        }
    }
}

async fn reconcile(
    app: &AppHandle,
    hub: &StatusHub,
    client: &mut MeshAgentRpc,
    revision: Option<u64>,
) -> bool {
    let Ok(view) = client.status().await else {
        log::warn!(
            "agent status stream: quiet for {FRAME_WATCHDOG:?} and the daemon does not answer; re-subscribing"
        );
        return true;
    };
    let routes = hub.routes_for(client, revision, view.routes.clone()).await;
    let fresh = StatusDto::from_view(view, routes, StreamHealth::Live);
    if hub.cached_if_live().await.is_some_and(|cached| cached == fresh) {
        log::debug!("agent status stream: quiet for {FRAME_WATCHDOG:?} but the daemon agrees");
        return false;
    }
    log::warn!(
        "agent status stream: quiet for {FRAME_WATCHDOG:?} and the snapshot moved on; publishing it and re-subscribing"
    );
    hub.publish(fresh.clone()).await;
    emit_status(app, &fresh);
    true
}

async fn publish_frame(
    app: &AppHandle,
    hub: &StatusHub,
    client: &mut MeshAgentRpc,
    frame: StatusFrame,
) {
    let revision = frame.networks_revision;
    let routes = hub.routes_for(client, revision, frame.view.routes.clone()).await;
    let dto = StatusDto::from_view(frame.view, routes, StreamHealth::Live);
    log::debug!("agent status stream: frame state={} revision={revision:?}", dto.state);
    hub.publish(dto.clone()).await;
    emit_status(app, &dto);
}

async fn degrade(
    app: &AppHandle,
    hub: &StatusHub,
    stream: StreamHealth,
    offline_since: &mut Option<Instant>,
    escalated: &mut bool,
) {
    let since = *offline_since.get_or_insert_with(Instant::now);
    log::debug!("agent status stream: degraded as {:?} for {:?}", stream, since.elapsed());
    let state = if since.elapsed() >= OFFLINE_GRACE && !*escalated {
        *escalated = true;
        Some("error")
    } else {
        None
    };
    let dto = hub.republish(stream, state).await;
    emit_status(app, &dto);
}

#[cfg(test)]
mod tests {
    use super::{RouteCache, StatusDto, StatusHub, StreamHealth};

    #[test]
    fn route_cache_fetches_only_when_the_revision_moves() {
        let mut cache = RouteCache::default();
        assert!(cache.needs_fetch(Some(22)), "an empty cache always fetches");

        cache.recorded(Some(22), &["10.0.0.0/8".to_owned()]);
        assert!(!cache.needs_fetch(Some(22)));
        assert!(cache.needs_fetch(Some(23)));
        assert!(cache.needs_fetch(None));
    }

    #[test]
    fn route_cache_keeps_a_zero_revision_distinct_from_absent() {
        let mut cache = RouteCache::default();
        cache.recorded(Some(0), &["10.0.0.0/8".to_owned()]);

        assert!(!cache.needs_fetch(Some(0)));
        assert!(cache.needs_fetch(None), "a frame without a revision must not reuse the cache");
    }

    #[test]
    fn route_cache_does_not_advance_on_a_frame_without_revision() {
        let mut cache = RouteCache::default();
        cache.recorded(None, &["10.0.0.0/8".to_owned()]);

        assert!(cache.needs_fetch(Some(0)));
    }

    #[tokio::test]
    async fn a_reconnecting_hub_does_not_serve_the_cache() {
        let hub = StatusHub::default();
        assert!(hub.cached_if_live().await.is_none());
        assert_eq!(hub.stream_health().await, StreamHealth::Unavailable);

        let mut dto = StatusDto::disconnected(StreamHealth::Live);
        dto.hostname = Some("mac".to_owned());
        hub.publish(dto).await;

        assert_eq!(hub.cached_if_live().await.and_then(|dto| dto.hostname), Some("mac".to_owned()));
        assert_eq!(hub.stream_health().await, StreamHealth::Live);

        let republished = hub.republish(StreamHealth::Reconnecting, None).await;
        assert_eq!(republished.state, "disconnected");
        assert!(hub.cached_if_live().await.is_none());

        let escalated = hub.republish(StreamHealth::Reconnecting, Some("error")).await;
        assert_eq!(escalated.state, "error");
    }

    #[tokio::test]
    async fn a_connecting_snapshot_is_not_cacheable() {
        let hub = StatusHub::default();
        let dto = hub.republish(StreamHealth::Live, Some("connecting")).await;
        assert!(!dto.cacheable());
        let dto = hub.republish(StreamHealth::Live, Some("disconnected")).await;
        assert!(dto.cacheable());
    }

    #[tokio::test]
    async fn an_empty_hub_still_reports_a_degraded_stream() {
        let hub = StatusHub::default();

        let dto = hub.republish(StreamHealth::Reconnecting, None).await;
        assert_eq!(dto.state, "unknown");
        assert_eq!(dto.stream, StreamHealth::Reconnecting);
        assert_eq!(hub.stream_health().await, StreamHealth::Reconnecting);

        let escalated = hub.republish(StreamHealth::Reconnecting, Some("error")).await;
        assert_eq!(escalated.state, "error");
    }
}
