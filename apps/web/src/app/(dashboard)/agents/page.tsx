"use client";

import { useQuery } from "@tanstack/react-query";
import { graphqlClient } from "@/lib/graphql-client";
import { gql } from "graphql-request";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import { EmptyState } from "@/components/empty-state";
import { Bot } from "lucide-react";

const AGENTS_QUERY = gql`
  query GetAgents {
    agents {
      name
      status
      model
    }
  }
`;

type Agent = {
  name: string;
  status: string;
  model: string;
};

type AgentsResponse = {
  agents: Agent[];
};

const statusColors: Record<string, string> = {
  running: "bg-green-100 text-green-700 dark:bg-green-900 dark:text-green-300",
  stopped: "bg-gray-100 text-gray-700 dark:bg-gray-800 dark:text-gray-300",
  error: "bg-red-100 text-red-700 dark:bg-red-900 dark:text-red-300",
};

export default function AgentsPage() {
  const { data, isLoading } = useQuery<AgentsResponse>({
    queryKey: ["agents"],
    queryFn: () => graphqlClient.request(AGENTS_QUERY),
    refetchInterval: 10_000,
  });

  const agents = data?.agents ?? [];

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-3xl font-bold tracking-tight">Agents</h1>
        <p className="text-muted-foreground">
          View running agents and their status
        </p>
      </div>

      {isLoading ? (
        <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
          {Array.from({ length: 3 }).map((_, i) => (
            <Card key={i}>
              <CardHeader>
                <Skeleton className="h-5 w-32" />
                <Skeleton className="h-4 w-24" />
              </CardHeader>
              <CardContent>
                <Skeleton className="h-4 w-full" />
              </CardContent>
            </Card>
          ))}
        </div>
      ) : agents.length === 0 ? (
        <EmptyState
          title="No agents connected"
          description="Agents will appear here when they connect to the hub."
          icon={<Bot className="h-6 w-6 text-muted-foreground" />}
        />
      ) : (
        <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
          {agents.map((agent) => (
            <Card key={agent.name}>
              <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                <CardTitle className="text-sm font-medium">
                  {agent.name}
                </CardTitle>
                <Badge
                  className={statusColors[agent.status] ?? "bg-gray-100 text-gray-700"}
                >
                  {agent.status}
                </Badge>
              </CardHeader>
              <CardContent>
                <p className="text-sm text-muted-foreground">
                  Model:{" "}
                  <code className="text-xs bg-muted px-1 rounded">
                    {agent.model}
                  </code>
                </p>
              </CardContent>
            </Card>
          ))}
        </div>
      )}
    </div>
  );
}
