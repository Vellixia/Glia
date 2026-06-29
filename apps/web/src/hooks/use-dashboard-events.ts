"use client";

import { useEffect } from "react";
import { eventBus } from "@/services/event-bus";

/**
 * React lifecycle for the dashboard event bus.
 *
 * Mount → call `eventBus.connect()` (idempotent).
 * Unmount → call `eventBus.disconnect()` to release the EventSource and
 *   reset the connection-state store.
 *
 * The hook is designed to be called **once** at the dashboard layout
 * level. Calling it on every page would tear down the connection on
 * every navigation.
 */
export function useDashboardEvents(): void {
  useEffect(() => {
    eventBus.connect();
    return () => {
      eventBus.disconnect();
    };
  }, []);
}
