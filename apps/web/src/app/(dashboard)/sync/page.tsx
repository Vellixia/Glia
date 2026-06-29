"use client";

import { useQuery } from "@tanstack/react-query";
import { graphqlClient } from "@/lib/graphql-client";
import { gql } from "graphql-request";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { syncKeys } from "@/lib/query-keys";
import { RefreshCw } from "lucide-react";

const SYNC_STATUS_QUERY = gql`
  query GetSyncStatus {
    syncStatus {
      pending
      inProgress
      completed
      failed
    }
  }
`;

type SyncData = {
  syncStatus: {
    pending: number;
    inProgress: number;
    completed: number;
    failed: number;
  };
};

export default function SyncPage() {
  const { data, isLoading, refetch, isFetching } = useQuery<SyncData>({
    queryKey: syncKeys.status(),
    queryFn: () => graphqlClient.request(SYNC_STATUS_QUERY),
    refetchInterval: (query) => {
      const status = query.state.data?.syncStatus;
      if (!status) return 5_000;
      if (status.inProgress > 0) return 1_000;
      return 30_000;
    },
  });

  const sync = data?.syncStatus;
  const total = sync ? sync.pending + sync.inProgress + sync.completed + sync.failed : 0;
  const progress = total > 0 ? Math.round((sync!.completed / total) * 100) : 0;
  const isActive = sync ? sync.inProgress > 0 || sync.pending > 0 : false;

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-3xl font-bold tracking-tight">Sync</h1>
          <p className="text-muted-foreground">
            Hub synchronization status and controls
          </p>
        </div>
        <Button
          variant="outline"
          size="sm"
          onClick={() => refetch()}
          disabled={isFetching}
        >
          <RefreshCw className={`mr-1 h-4 w-4 ${isFetching ? "animate-spin" : ""}`} />
          Refresh
        </Button>
      </div>

      {isLoading ? (
        <div className="space-y-4">
          <Card>
            <CardHeader>
              <Skeleton className="h-5 w-32" />
            </CardHeader>
            <CardContent className="space-y-4">
              <Skeleton className="h-4 w-full" />
              <Skeleton className="h-4 w-3/4" />
            </CardContent>
          </Card>
        </div>
      ) : (
        <>
          <Card>
            <CardHeader>
              <CardTitle className="text-sm font-medium">
                Sync Progress
              </CardTitle>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="flex items-center justify-between text-sm">
                <span>
                  {isActive ? "Syncing..." : "Idle"}
                </span>
                <span className="text-muted-foreground">
                  {sync?.completed ?? 0} / {total} completed
                </span>
              </div>
              <Progress value={progress} />
              {sync?.failed !== undefined && sync.failed > 0 && (
                <Badge variant="destructive">
                  {sync.failed} failed
                </Badge>
              )}
            </CardContent>
          </Card>

          <div className="grid gap-4 md:grid-cols-4">
            <Card>
              <CardHeader className="pb-2">
                <CardTitle className="text-sm font-medium text-muted-foreground">
                  Pending
                </CardTitle>
              </CardHeader>
              <CardContent>
                <div className="text-2xl font-bold">{sync?.pending ?? 0}</div>
              </CardContent>
            </Card>
            <Card>
              <CardHeader className="pb-2">
                <CardTitle className="text-sm font-medium text-muted-foreground">
                  In Progress
                </CardTitle>
              </CardHeader>
              <CardContent>
                <div className="text-2xl font-bold">{sync?.inProgress ?? 0}</div>
              </CardContent>
            </Card>
            <Card>
              <CardHeader className="pb-2">
                <CardTitle className="text-sm font-medium text-muted-foreground">
                  Completed
                </CardTitle>
              </CardHeader>
              <CardContent>
                <div className="text-2xl font-bold text-green-600">{sync?.completed ?? 0}</div>
              </CardContent>
            </Card>
            <Card>
              <CardHeader className="pb-2">
                <CardTitle className="text-sm font-medium text-muted-foreground">
                  Failed
                </CardTitle>
              </CardHeader>
              <CardContent>
                <div className="text-2xl font-bold text-red-600">{sync?.failed ?? 0}</div>
              </CardContent>
            </Card>
          </div>

          <Card>
            <CardHeader>
              <CardTitle className="text-sm font-medium">
                Manual Sync
              </CardTitle>
            </CardHeader>
            <CardContent>
              <p className="text-sm text-muted-foreground mb-4">
                Trigger a manual synchronization with the hub.
              </p>
              <Button disabled variant="outline">
                Trigger Sync (Coming Soon)
              </Button>
            </CardContent>
          </Card>
        </>
      )}
    </div>
  );
}
