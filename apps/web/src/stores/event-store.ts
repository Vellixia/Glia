import { create } from "zustand";

/**
 * Real-time dashboard event bus connection state.
 *
 * - `connecting` — initial connection attempt, no `onopen` received yet
 * - `connected` — stream is open
 * - `reconnecting` — was previously connected, EventSource fired `onerror`
 *   and is auto-retrying (built-in, ~3s default)
 * - `disconnected` — terminal state (server sent non-200, or 204)
 */
export type ConnectionState =
  | "connecting"
  | "connected"
  | "reconnecting"
  | "disconnected";

interface EventStoreState {
  connectionState: ConnectionState;
  lastEventAt: string | null;
  eventCount: number;
  hasEverConnected: boolean;

  setState: (state: ConnectionState) => void;
  recordEvent: () => void;
  reset: () => void;
}

export const useEventStore = create<EventStoreState>((set, get) => ({
  connectionState: "connecting",
  lastEventAt: null,
  eventCount: 0,
  hasEverConnected: false,

  setState: (state) => {
    const hasEverConnected = get().hasEverConnected || state === "connected";
    set({ connectionState: state, hasEverConnected });
  },

  recordEvent: () =>
    set({
      lastEventAt: new Date().toISOString(),
      eventCount: get().eventCount + 1,
    }),

  reset: () =>
    set({
      connectionState: "connecting",
      lastEventAt: null,
      eventCount: 0,
      hasEverConnected: false,
    }),
}));
