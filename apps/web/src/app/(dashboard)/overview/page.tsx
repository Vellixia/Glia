"use client";

import { useQuery } from "@tanstack/react-query";
import { graphqlClient } from "@/lib/graphql-client";
import { gql } from "graphql-request";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import { Bot, Wrench, Key, RefreshCw } from "lucide-react";

const OVERVIEW_QUERY = gql`
  query Overview {
    agents {
      name
      status
      model
    }
    skills {
      id
      name
      status
    }
    secrets {
      credId
      ready
      provider
    }
    syncStatus {
      pending
      inProgress
      completed
      failed
    }
  }
`;

interface OverviewData {
  agents: Array<{ name: string; status: string; model: string }>;
  skills: Array<{ id: string; name: string; status: string }>;
  secrets: Array<{ credId: string; ready: boolean; provider: string | null }>;
  syncStatus: {
    pending: number;
    inProgress: number;
    completed: number;
    failed: number;
  };
}

export default function OverviewPage() {
  const { data, isLoading } = useQuery<OverviewData>({
    queryKey: ["overview"],
    queryFn: () => graphqlClient.request(OVERVIEW_QUERY),
    refetchInterval: 30_000,
  });

  const activeAgents = data?.agents?.filter((a) => a.status === "running").length ?? 0;
  const totalSkills = data?.skills?.length ?? 0;
  const syncPending = data?.syncStatus?.pending ?? 0;
  const syncCompleted = data?.syncStatus?.completed ?? 0;
  const totalSecrets = data?.secrets?.length ?? 0;

  const stats = [
    {
      title: "Active Agents",
      testId: "kpi-active-agents",
      value: activeAgents,
      icon: Bot,
      description: "Running in sandbox",
      status: activeAgents > 0 ? "healthy" : "neutral",
    },
    {
      title: "Skills",
      testId: "kpi-skills",
      value: totalSkills,
      icon: Wrench,
      description: "Embedded in HelixDB",
      status: totalSkills > 0 ? "healthy" : "neutral",
    },
    {
      title: "Secrets",
      testId: "kpi-secrets",
      value: totalSecrets === 0 ? "0" : String(totalSecrets),
      icon: Key,
      description: "Managed by OpenBao",
      status: totalSecrets > 0 ? "healthy" : "neutral",
    },
    {
      title: "Sync",
      testId: "kpi-sync",
      value: syncCompleted,
      icon: RefreshCw,
      description: syncPending > 0 ? `${syncPending} pending` : "Up to date",
      status: syncPending > 0 ? "warning" : "healthy",
    },
  ];

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-3xl font-bold tracking-tight">Overview</h1>
        <p className="text-muted-foreground">
          Glia cognitive control plane status
        </p>
      </div>

      <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
        {isLoading
          ? Array.from({ length: 4 }).map((_, i) => (
              <Card key={i}>
                <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                  <Skeleton className="h-4 w-24" />
                  <Skeleton className="h-4 w-4" />
                </CardHeader>
                <CardContent>
                  <Skeleton className="h-8 w-16 mb-1" />
                  <Skeleton className="h-3 w-32" />
                </CardContent>
              </Card>
            ))
          : stats.map((stat) => (
              <Card key={stat.title} data-testid={stat.testId}>
                <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                  <CardTitle className="text-sm font-medium">
                    {stat.title}
                  </CardTitle>
                  <stat.icon className="h-4 w-4 text-muted-foreground" />
                </CardHeader>
                <CardContent>
                  <div className="text-2xl font-bold">{stat.value}</div>
                  <p className="text-xs text-muted-foreground flex items-center gap-1.5">
                    <Badge
                      variant={
                        stat.status === "healthy"
                          ? "default"
                          : stat.status === "warning"
                          ? "destructive"
                          : "secondary"
                      }
                      className="text-[10px] px-1 py-0"
                    >
                      {stat.status === "healthy"
                        ? "Healthy"
                        : stat.status === "warning"
                        ? "Attention"
                        : "N/A"}
                    </Badge>
                    {stat.description}
                  </p>
                </CardContent>
              </Card>
            ))}
      </div>

      <div className="grid gap-4 md:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle className="text-sm font-medium">
              Agent Status
            </CardTitle>
          </CardHeader>
          <CardContent>
            {isLoading ? (
              <Skeleton className="h-16 w-full" />
            ) : data?.agents && data.agents.length > 0 ? (
              <div className="space-y-2">
                {data.agents.slice(0, 5).map((agent) => (
                  <div
                    key={agent.name}
                    className="flex items-center justify-between text-sm"
                  >
                    <span className="font-medium">{agent.name}</span>
                    <Badge
                      variant={
                        agent.status === "running" ? "default" : "secondary"
                      }
                    >
                      {agent.status}
                    </Badge>
                  </div>
                ))}
              </div>
            ) : (
              <p className="text-sm text-muted-foreground">
                No agents connected. Start an agent to see activity.
              </p>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="text-sm font-medium">
              Recent Skills
            </CardTitle>
          </CardHeader>
          <CardContent>
            {isLoading ? (
              <Skeleton className="h-16 w-full" />
            ) : data?.skills && data.skills.length > 0 ? (
              <div className="space-y-2">
                {data.skills.slice(0, 5).map((skill) => (
                  <div
                    key={skill.id}
                    className="flex items-center justify-between text-sm"
                  >
                    <span className="font-medium">{skill.name}</span>
                    <Badge
                      variant={
                        skill.status === "ACTIVE" ? "default" : "secondary"
                      }
                    >
                      {skill.status.toLowerCase()}
                    </Badge>
                  </div>
                ))}
              </div>
            ) : (
              <p className="text-sm text-muted-foreground">
                No skills embedded yet. Run{" "}
                <code className="text-xs bg-muted px-1 rounded">
                  glia save-skill
                </code>{" "}
                to get started.
              </p>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
