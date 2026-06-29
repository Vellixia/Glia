"use client";

import { useEffect, useRef, useCallback } from "react";
import { useLogStore, type LogEntry } from "@/stores/log-store";

const FLUSH_INTERVAL_MS = 100;

export function LogSocketProvider({
  children,
}: {
  children: React.ReactNode;
}) {
  const bufferRef = useRef<LogEntry[]>([]);
  const addLogs = useLogStore((s) => s.addLogs);
  const setStreaming = useLogStore((s) => s.setStreaming);
  const esRef = useRef<EventSource | null>(null);
  const idCounter = useRef(0);

  const connect = useCallback(() => {
    const baseUrl =
      process.env.NEXT_PUBLIC_HUB_URL ?? "http://127.0.0.1:3000";
    const es = new EventSource(`${baseUrl}/api/logs`);
    esRef.current = es;

    es.onopen = () => setStreaming(true);

    es.addEventListener("log", (event) => {
      try {
        const raw = JSON.parse(event.data);
        const entry: LogEntry = {
          id: String(++idCounter.current),
          timestamp: raw.timestamp ?? new Date().toISOString(),
          level: raw.level ?? "info",
          message: raw.message ?? "",
          source: raw.source,
        };
        bufferRef.current.push(entry);
      } catch {
        // skip malformed entries
      }
    });

    es.onerror = () => {
      setStreaming(false);
      es.close();
      // Auto-reconnect after 3s
      setTimeout(connect, 3000);
    };
  }, [setStreaming]);

  useEffect(() => {
    connect();

    // Flush buffer at fixed interval
    const flushTimer = setInterval(() => {
      if (bufferRef.current.length > 0) {
        addLogs(bufferRef.current);
        bufferRef.current = [];
      }
    }, FLUSH_INTERVAL_MS);

    return () => {
      clearInterval(flushTimer);
      esRef.current?.close();
    };
  }, [connect, addLogs]);

  return <>{children}</>;
}
