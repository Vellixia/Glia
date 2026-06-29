import { describe, it, expect } from "vitest";
import { useLogStore, type LogEntry } from "./log-store";

function makeEntry(overrides?: Partial<LogEntry>): LogEntry {
  return {
    id: "1",
    timestamp: "2025-01-01T00:00:00Z",
    level: "info",
    message: "test log",
    ...overrides,
  };
}

describe("useLogStore", () => {
  beforeEach(() => {
    useLogStore.setState({ logs: [], filter: {}, isStreaming: false });
  });

  it("starts with empty logs", () => {
    const { logs } = useLogStore.getState();
    expect(logs).toEqual([]);
  });

  it("addLog adds a single entry", () => {
    const entry = makeEntry();
    useLogStore.getState().addLog(entry);
    const { logs } = useLogStore.getState();
    expect(logs).toHaveLength(1);
    expect(logs[0]).toEqual(entry);
  });

  it("addLogs adds multiple entries", () => {
    const entries = [
      makeEntry({ id: "1", message: "first" }),
      makeEntry({ id: "2", message: "second" }),
    ];
    useLogStore.getState().addLogs(entries);
    const { logs } = useLogStore.getState();
    expect(logs).toHaveLength(2);
    expect(logs[0].message).toBe("first");
    expect(logs[1].message).toBe("second");
  });

  it("respects maxLogs cap and drops oldest entries", () => {
    const maxLogs = useLogStore.getState().maxLogs;
    const fill: LogEntry[] = [];
    for (let i = 0; i < maxLogs; i++) {
      fill.push(makeEntry({ id: String(i), message: `entry-${i}` }));
    }
    useLogStore.getState().addLogs(fill);
    expect(useLogStore.getState().logs).toHaveLength(maxLogs);

    const overflow = [
      makeEntry({ id: "overflow-1", message: "overflow-1" }),
      makeEntry({ id: "overflow-2", message: "overflow-2" }),
      makeEntry({ id: "overflow-3", message: "overflow-3" }),
    ];
    useLogStore.getState().addLogs(overflow);
    const { logs } = useLogStore.getState();
    expect(logs).toHaveLength(maxLogs);
    expect(logs[0].message).toBe("entry-3");
    expect(logs[logs.length - 1].message).toBe("overflow-3");
  });

  it("clearLogs empties the buffer", () => {
    useLogStore.getState().addLog(makeEntry());
    useLogStore.getState().addLog(makeEntry({ id: "2" }));
    expect(useLogStore.getState().logs).toHaveLength(2);
    useLogStore.getState().clearLogs();
    expect(useLogStore.getState().logs).toEqual([]);
  });

  it("setFilter merges partial updates correctly", () => {
    useLogStore.getState().setFilter({ level: "error" });
    expect(useLogStore.getState().filter).toEqual({ level: "error" });
    useLogStore.getState().setFilter({ search: "timeout" });
    expect(useLogStore.getState().filter).toEqual({
      level: "error",
      search: "timeout",
    });
  });

  it("setFilter overwrites existing keys", () => {
    useLogStore.getState().setFilter({ level: "error" });
    useLogStore.getState().setFilter({ level: "warn" });
    expect(useLogStore.getState().filter).toEqual({ level: "warn" });
  });

  it("setStreaming toggles correctly", () => {
    useLogStore.getState().setStreaming(true);
    expect(useLogStore.getState().isStreaming).toBe(true);
    useLogStore.getState().setStreaming(false);
    expect(useLogStore.getState().isStreaming).toBe(false);
  });

  it("each log entry has required fields", () => {
    const entry = makeEntry();
    useLogStore.getState().addLog(entry);
    const log = useLogStore.getState().logs[0];
    expect(log).toHaveProperty("id");
    expect(log).toHaveProperty("timestamp");
    expect(log).toHaveProperty("level");
    expect(log).toHaveProperty("message");
  });
});
