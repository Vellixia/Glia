"use client";

import { useRef, useEffect, useMemo, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useLogStore } from "@/stores/log-store";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { ArrowDown, Wifi, WifiOff } from "lucide-react";

const levelColors: Record<string, string> = {
  info: "bg-blue-100 text-blue-700 dark:bg-blue-900 dark:text-blue-300",
  warn: "bg-yellow-100 text-yellow-700 dark:bg-yellow-900 dark:text-yellow-300",
  error: "bg-red-100 text-red-700 dark:bg-red-900 dark:text-red-300",
  debug: "bg-gray-100 text-gray-700 dark:bg-gray-800 dark:text-gray-300",
};

export function LogViewer() {
  const logs = useLogStore((s) => s.logs);
  const filter = useLogStore((s) => s.filter);
  const isStreaming = useLogStore((s) => s.isStreaming);
  const setFilter = useLogStore((s) => s.setFilter);
  const parentRef = useRef<HTMLDivElement>(null);
  const [autoScroll, setAutoScroll] = useState(true);
  const prevCountRef = useRef(0);

  const filteredLogs = useMemo(() => {
    return logs.filter((log) => {
      if (filter.level && log.level !== filter.level) return false;
      if (
        filter.search &&
        !log.message.toLowerCase().includes(filter.search.toLowerCase())
      )
        return false;
      return true;
    });
  }, [logs, filter]);

  const virtualizer = useVirtualizer({
    count: filteredLogs.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 24,
    overscan: 50,
  });

  // Auto-scroll to bottom when new logs arrive
  useEffect(() => {
    if (
      autoScroll &&
      filteredLogs.length > prevCountRef.current &&
      parentRef.current
    ) {
      virtualizer.scrollToIndex(filteredLogs.length - 1, { align: "end" });
    }
    prevCountRef.current = filteredLogs.length;
  }, [filteredLogs.length, autoScroll, virtualizer]);

  const handleScroll = () => {
    if (!parentRef.current) return;
    const { scrollTop, scrollHeight, clientHeight } = parentRef.current;
    const isAtBottom = scrollHeight - scrollTop - clientHeight < 50;
    setAutoScroll(isAtBottom);
  };

  return (
    <div className="flex flex-col h-[calc(100vh-12rem)]">
      {/* Toolbar */}
      <div className="flex items-center gap-2 p-2 border-b">
        <Input
          placeholder="Filter logs..."
          value={filter.search ?? ""}
          onChange={(e) => setFilter({ search: e.target.value })}
          className="flex-1"
        />
        <select
          value={filter.level ?? ""}
          onChange={(e) => setFilter({ level: e.target.value || undefined })}
          className="h-9 rounded-md border px-2 text-sm"
        >
          <option value="">All levels</option>
          <option value="error">Error</option>
          <option value="warn">Warning</option>
          <option value="info">Info</option>
          <option value="debug">Debug</option>
        </select>
        <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
          {isStreaming ? (
            <Wifi className="h-3 w-3 text-green-500" />
          ) : (
            <WifiOff className="h-3 w-3 text-red-500" />
          )}
          <span>
            {isStreaming ? "Connected" : "Disconnected"}
          </span>
        </div>
        <span className="text-xs text-muted-foreground">
          {filteredLogs.length.toLocaleString()} entries
        </span>
      </div>

      {/* Virtual log list */}
      <div
        ref={parentRef}
        className="flex-1 overflow-auto font-mono text-xs"
        onScroll={handleScroll}
      >
        <div
          style={{
            height: `${virtualizer.getTotalSize()}px`,
            position: "relative",
          }}
        >
          {virtualizer.getVirtualItems().map((virtualRow) => {
            const log = filteredLogs[virtualRow.index];
            return (
              <div
                key={virtualRow.key}
                style={{
                  position: "absolute",
                  top: 0,
                  left: 0,
                  width: "100%",
                  height: `${virtualRow.size}px`,
                  transform: `translateY(${virtualRow.start}px)`,
                }}
                className={`flex items-center px-2 border-b border-border/50 hover:bg-muted/50 ${
                  log.level === "error"
                    ? "text-red-500"
                    : log.level === "warn"
                    ? "text-yellow-500"
                    : ""
                }`}
              >
                <span className="w-20 shrink-0 text-muted-foreground">
                  {new Date(log.timestamp).toLocaleTimeString()}
                </span>
                <span className="w-12 shrink-0">
                  <Badge
                    variant="outline"
                    className={`text-[10px] px-1 py-0 ${
                      levelColors[log.level] ?? ""
                    }`}
                  >
                    {log.level.toUpperCase()}
                  </Badge>
                </span>
                <span className="flex-1 truncate ml-2">{log.message}</span>
              </div>
            );
          })}
        </div>
      </div>

      {/* Auto-scroll indicator */}
      {!autoScroll && (
        <div className="sticky bottom-0 flex justify-center py-2">
          <Button
            size="sm"
            onClick={() => {
              setAutoScroll(true);
              virtualizer.scrollToIndex(filteredLogs.length - 1, {
                align: "end",
              });
            }}
          >
            <ArrowDown className="mr-1 h-3 w-3" />
            Scroll to bottom
          </Button>
        </div>
      )}
    </div>
  );
}
