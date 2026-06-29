"use client";

import { LogSocketProvider } from "@/components/log-socket-provider";
import { LogViewer } from "@/components/log-viewer";

export default function LogsPage() {
  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-3xl font-bold tracking-tight">Logs</h1>
        <p className="text-muted-foreground">
          Real-time log stream from hub and agents
        </p>
      </div>

      <LogSocketProvider>
        <div className="rounded-lg border">
          <LogViewer />
        </div>
      </LogSocketProvider>
    </div>
  );
}
