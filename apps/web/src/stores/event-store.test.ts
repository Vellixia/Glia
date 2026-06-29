import { describe, it, expect, beforeEach } from "vitest";
import { useEventStore } from "./event-store";

describe("event-store", () => {
  beforeEach(() => {
    useEventStore.getState().reset();
  });

  it("starts in connecting state", () => {
    expect(useEventStore.getState().connectionState).toBe("connecting");
    expect(useEventStore.getState().hasEverConnected).toBe(false);
  });

  it("reset clears all state", () => {
    useEventStore.getState().setState("connected");
    useEventStore.getState().recordEvent();
    useEventStore.getState().reset();
    const s = useEventStore.getState();
    expect(s.connectionState).toBe("connecting");
    expect(s.lastEventAt).toBeNull();
    expect(s.eventCount).toBe(0);
    expect(s.hasEverConnected).toBe(false);
  });

  it("setState to connected marks hasEverConnected", () => {
    useEventStore.getState().setState("connected");
    const s = useEventStore.getState();
    expect(s.connectionState).toBe("connected");
    expect(s.hasEverConnected).toBe(true);
  });

  it("reconnecting does not overwrite hasEverConnected", () => {
    useEventStore.getState().setState("connected");
    useEventStore.getState().setState("reconnecting");
    expect(useEventStore.getState().hasEverConnected).toBe(true);
  });

  it("recordEvent updates lastEventAt and eventCount", () => {
    expect(useEventStore.getState().eventCount).toBe(0);
    useEventStore.getState().recordEvent();
    expect(useEventStore.getState().eventCount).toBe(1);
    expect(useEventStore.getState().lastEventAt).not.toBeNull();
    useEventStore.getState().recordEvent();
    expect(useEventStore.getState().eventCount).toBe(2);
  });

  it("handles the full lifecycle", () => {
    const store = useEventStore.getState();
    expect(store.connectionState).toBe("connecting");

    store.setState("connected");
    store.recordEvent();
    expect(useEventStore.getState().connectionState).toBe("connected");
    expect(useEventStore.getState().hasEverConnected).toBe(true);
    expect(useEventStore.getState().eventCount).toBe(1);

    store.setState("reconnecting");
    expect(useEventStore.getState().hasEverConnected).toBe(true);

    store.setState("connected");
    store.recordEvent();
    expect(useEventStore.getState().eventCount).toBe(2);

    store.setState("disconnected");
    expect(useEventStore.getState().connectionState).toBe("disconnected");
  });
});
