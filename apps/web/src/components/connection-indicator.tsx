"use client";

import { useEventStore, type ConnectionState } from "@/stores/event-store";
import { cn } from "@/lib/utils";

const LABEL: Record<ConnectionState, string> = {
  connecting: "Connecting",
  connected: "Live",
  reconnecting: "Reconnecting",
  disconnected: "Disconnected",
};

const DOT: Record<ConnectionState, string> = {
  connecting: "bg-yellow-500 animate-pulse",
  connected: "bg-emerald-500",
  reconnecting: "bg-yellow-500 animate-pulse",
  disconnected: "bg-red-500",
};

export function ConnectionIndicator() {
  const connectionState = useEventStore((s) => s.connectionState);

  return (
    <div
      data-testid="connection-indicator"
      data-state={connectionState}
      className="flex items-center gap-2 text-xs text-muted-foreground"
      title={`Dashboard event stream: ${LABEL[connectionState]}`}
    >
      <span
        aria-hidden="true"
        className={cn("h-2 w-2 rounded-full", DOT[connectionState])}
      />
      <span>{LABEL[connectionState]}</span>
    </div>
  );
}
