use axum::{
    response::sse::{Event, KeepAlive, Sse},
};
use futures_util::Stream;
use serde::Serialize;
use std::convert::Infallible;
use std::sync::LazyLock;
use tokio::sync::broadcast;

use crate::auth::AuthUserSse;

/// Broadcast channel for dashboard state-change events.
///
/// Capacity 128 — dashboard events are sparse (mutations, not log streams);
/// if a subscriber falls behind, it gets a `lag-detected` event and the
/// connection stays alive.
static DASHBOARD_CHANNEL: LazyLock<broadcast::Sender<DashboardEvent>> =
    LazyLock::new(|| {
        let (tx, _) = broadcast::channel::<DashboardEvent>(128);
        tx
    });

/// Subscribe to the dashboard event broadcast channel.
pub fn subscribe_dashboard_events() -> broadcast::Receiver<DashboardEvent> {
    DASHBOARD_CHANNEL.subscribe()
}

/// Publish a dashboard event to all SSE subscribers.
///
/// The publish is best-effort: if there are no active subscribers, the
/// `send` call returns an error which we silently ignore. This is correct
/// for fire-and-forget UI notifications — we never want to block a
/// mutation on UI subscribers being present.
pub fn publish_dashboard_event(event: DashboardEvent) {
    let _ = DASHBOARD_CHANNEL.send(event);
}

/// All event variants that flow to dashboard subscribers.
///
/// Each variant maps 1:1 to a query-key invalidation prefix on the
/// frontend. Keep the payload small — only what the UI needs to decide
/// what to refetch (typically the affected entity ID).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum DashboardEvent {
    /// A skill was installed from the catalog.
    SkillInstalled {
        /// The installed skill's Hub ID.
        id: String,
        /// The display name (used in the UI notification).
        name: String,
    },
    /// A skill was removed.
    SkillUninstalled {
        /// The removed skill's Hub ID.
        id: String,
    },
    /// A skill's enabled/disabled state changed.
    SkillToggled {
        /// The affected skill's Hub ID.
        id: String,
        /// New enabled state.
        enabled: bool,
    },
    /// Background skill sync run finished successfully.
    SkillSyncSucceeded {
        /// Number of skills updated.
        count: u32,
    },
    /// Background skill sync run finished with errors.
    SkillSyncFailed {
        /// Number of skills that failed.
        count: u32,
        /// Error summary.
        error: String,
    },
    /// Hub settings changed.
    ConfigChanged,
    /// A new OAuth provider was registered.
    ProviderRegistered {
        /// The new provider's ID.
        id: String,
    },
    /// A stored credential entry was deleted.
    SecretDeleted {
        /// The deleted credential's ID.
        cred_id: String,
    },
    /// A new credential entry was created (e.g. OAuth callback completed).
    SecretAdded {
        /// The new credential's ID.
        cred_id: String,
        /// Provider the credential is associated with, if any.
        provider: Option<String>,
    },
    /// A catalog source was added.
    CatalogSourceAdded {
        /// The source name.
        name: String,
    },
    /// A catalog source was removed.
    CatalogSourceRemoved {
        /// The source name.
        name: String,
    },
}

impl DashboardEvent {
    /// The SSE `event:` field name, used by the browser's
    /// `EventSource.addEventListener(name, handler)`.
    pub fn event_name(&self) -> &'static str {
        match self {
            Self::SkillInstalled { .. } => "skill-installed",
            Self::SkillUninstalled { .. } => "skill-uninstalled",
            Self::SkillToggled { .. } => "skill-toggled",
            Self::SkillSyncSucceeded { .. } => "skill-sync-succeeded",
            Self::SkillSyncFailed { .. } => "skill-sync-failed",
            Self::ConfigChanged => "config-changed",
            Self::ProviderRegistered { .. } => "provider-registered",
            Self::SecretDeleted { .. } => "secret-deleted",
            Self::SecretAdded { .. } => "secret-added",
            Self::CatalogSourceAdded { .. } => "catalog-source-added",
            Self::CatalogSourceRemoved { .. } => "catalog-source-removed",
        }
    }
}

/// SSE endpoint handler — streams real-time dashboard state changes to the
/// client.
///
/// Mount at `GET /api/events` in the Hub router. The `AuthUserSse` extractor
/// accepts either a `Authorization: Bearer <jwt>` header (proxy scenario) or
/// a `?token=<jwt>` query param (raw `EventSource` clients).
pub async fn events_stream_handler(
    _user: AuthUserSse,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = subscribe_dashboard_events();

    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        match rx.recv().await {
            Ok(event) => {
                let name = event.event_name();
                let event = Event::default()
                    .event(name)
                    .json_data(&event)
                    .unwrap_or_else(|_| {
                        Event::default()
                            .event(name)
                            .data(serde_json::to_string(&event).unwrap_or_default())
                    });
                Some((Ok(event), rx))
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("SSE dashboard event stream lagged by {n} events");
                let event = Event::default()
                    .event("lag-detected")
                    .data(format!("stream lagged by {n} events"));
                Some((Ok(event), rx))
            }
            Err(broadcast::error::RecvError::Closed) => None,
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dashboard_channel_publish_subscribe() {
        let mut rx = subscribe_dashboard_events();
        let event = DashboardEvent::SkillToggled {
            id: "abc-123".into(),
            enabled: true,
        };
        publish_dashboard_event(event.clone());
        let received = rx.recv().await.expect("should receive event");
        assert_eq!(received.event_name(), "skill-toggled");
        match received {
            DashboardEvent::SkillToggled { id, enabled } => {
                assert_eq!(id, "abc-123");
                assert!(enabled);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn event_names_match_kebab_case() {
        let cases: Vec<(DashboardEvent, &str)> = vec![
            (
                DashboardEvent::SkillInstalled { id: "1".into(), name: "n".into() },
                "skill-installed",
            ),
            (DashboardEvent::SkillUninstalled { id: "1".into() }, "skill-uninstalled"),
            (
                DashboardEvent::SkillToggled { id: "1".into(), enabled: false },
                "skill-toggled",
            ),
            (DashboardEvent::SkillSyncSucceeded { count: 1 }, "skill-sync-succeeded"),
            (
                DashboardEvent::SkillSyncFailed { count: 1, error: "e".into() },
                "skill-sync-failed",
            ),
            (DashboardEvent::ConfigChanged, "config-changed"),
            (DashboardEvent::ProviderRegistered { id: "1".into() }, "provider-registered"),
            (DashboardEvent::SecretDeleted { cred_id: "1".into() }, "secret-deleted"),
            (
                DashboardEvent::SecretAdded { cred_id: "1".into(), provider: None },
                "secret-added",
            ),
            (DashboardEvent::CatalogSourceAdded { name: "n".into() }, "catalog-source-added"),
            (DashboardEvent::CatalogSourceRemoved { name: "n".into() }, "catalog-source-removed"),
        ];
        for (event, expected) in cases {
            assert_eq!(event.event_name(), expected);
        }
    }

    #[test]
    fn event_json_shape() {
        let event = DashboardEvent::SkillToggled { id: "x".into(), enabled: true };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "skill-toggled");
        assert_eq!(json["id"], "x");
        assert_eq!(json["enabled"], true);
    }
}
