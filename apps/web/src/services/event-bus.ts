import { getQueryClient } from "@/lib/query-client";
import {
  agentKeys,
  catalogKeys,
  secretsKeys,
  settingsKeys,
  skillKeys,
  syncKeys,
} from "@/lib/query-keys";
import { useEventStore } from "@/stores/event-store";

/**
 * Browser-side singleton wrapping an `EventSource` that streams Hub
 * dashboard events. Survives Next.js HMR via the `globalThis` pattern.
 *
 * Connection lifecycle:
 *   - `connect()` — opens the `EventSource`, updates `connectionState` on
 *     `onopen`/`onerror`. EventSource handles auto-reconnect natively
 *     (~3s default, server can override with `retry:`).
 *   - `disconnect()` — closes the EventSource. Used on logout / page
 *     unmount.
 *
 * Event handling:
 *   - On `onopen` (initial OR reconnect): invalidate ALL active queries
 *     to catch up on anything missed while disconnected.
 *   - On named event: targeted `invalidateQueries()` against the
 *     prefixes affected by that event type.
 *
 * SSR safety:
 *   - All EventSource interaction is gated by `typeof window`. The
 *     singleton is never instantiated server-side.
 */
type EventBusGlobal = typeof globalThis & {
  __gliaEventBus?: EventBusService;
};

const global = globalThis as EventBusGlobal;

export class EventBusService {
  private eventSource: EventSource | null = null;
  private connecting = false;

  static getInstance(): EventBusService {
    if (!global.__gliaEventBus) {
      global.__gliaEventBus = new EventBusService();
    }
    return global.__gliaEventBus;
  }

  /**
   * Open the EventSource. Idempotent — repeated calls are no-ops once
   * the connection is open or in flight.
   */
  connect(): void {
    if (typeof window === "undefined") return;
    if (this.eventSource || this.connecting) return;

    this.connecting = true;
    const store = useEventStore.getState();

    if (!store.hasEverConnected) {
      store.setState("connecting");
    } else {
      store.setState("reconnecting");
    }

    const es = new EventSource("/api/events");
    this.eventSource = es;

    es.onopen = () => {
      this.connecting = false;
      useEventStore.getState().setState("connected");
      this.invalidateAll();
    };

    es.onerror = () => {
      // EventSource auto-reconnects natively. We just track the state.
      // readyState === 2 (CLOSED) means the server told us to stop
      // (e.g. 401 or 204).
      if (es.readyState === EventSource.CLOSED) {
        this.connecting = false;
        useEventStore.getState().setState("disconnected");
      } else {
        this.connecting = false;
        useEventStore.getState().setState("reconnecting");
      }
    };

    es.addEventListener("skill-installed", () => this.invalidateSkills());
    es.addEventListener("skill-uninstalled", () => this.invalidateSkills());
    es.addEventListener("skill-toggled", () => this.invalidateSkills());
    es.addEventListener("skill-sync-succeeded", () => {
      this.invalidateSync();
      this.invalidateSkills();
    });
    es.addEventListener("skill-sync-failed", () => {
      this.invalidateSync();
      this.invalidateSkills();
    });
    es.addEventListener("config-changed", () => this.invalidateSettings());
    es.addEventListener("provider-registered", () => this.invalidateSecrets());
    es.addEventListener("secret-deleted", () => this.invalidateSecrets());
    es.addEventListener("secret-added", () => this.invalidateSecrets());
    es.addEventListener("catalog-source-added", () => this.invalidateCatalog());
    es.addEventListener("catalog-source-removed", () => this.invalidateCatalog());

    es.addEventListener("lag-detected", () => {
      // Substantial gap in the stream — catch up.
      this.invalidateAll();
    });
  }

  /**
   * Close the EventSource. Safe to call multiple times.
   */
  disconnect(): void {
    this.eventSource?.close();
    this.eventSource = null;
    this.connecting = false;
    useEventStore.getState().reset();
  }

  isConnected(): boolean {
    return this.eventSource?.readyState === EventSource.OPEN;
  }

  private recordEvent(): void {
    useEventStore.getState().recordEvent();
  }

  private invalidateAll(): void {
    if (typeof window === "undefined") return;
    this.recordEvent();
    getQueryClient().invalidateQueries();
  }

  private invalidateSkills(): void {
    this.recordEvent();
    const qc = getQueryClient();
    qc.invalidateQueries({ queryKey: skillKeys.all });
    qc.invalidateQueries({ queryKey: catalogKeys.all });
  }

  private invalidateSync(): void {
    this.recordEvent();
    getQueryClient().invalidateQueries({ queryKey: syncKeys.all });
  }

  private invalidateSettings(): void {
    this.recordEvent();
    getQueryClient().invalidateQueries({ queryKey: settingsKeys.all });
  }

  private invalidateSecrets(): void {
    this.recordEvent();
    const qc = getQueryClient();
    qc.invalidateQueries({ queryKey: secretsKeys.all });
    // A new provider may also affect the agent list (provider → agent binding).
    qc.invalidateQueries({ queryKey: agentKeys.all });
  }

  private invalidateCatalog(): void {
    this.recordEvent();
    getQueryClient().invalidateQueries({ queryKey: catalogKeys.all });
  }
}

export const eventBus = EventBusService.getInstance();
