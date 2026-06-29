"use client";

import { useState, useEffect } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { graphqlClient } from "@/lib/graphql-client";
import { gql } from "graphql-request";
import { skillKeys } from "@/lib/query-keys";
import { DataTable } from "@/components/data-table";
import { EmptyState } from "@/components/empty-state";
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { useDebounce } from "@/hooks/use-debounce";
import { toast } from "sonner";
import { createColumnHelper, type SortingState, type PaginationState } from "@tanstack/react-table";
import { Wrench, Search } from "lucide-react";

const SKILLS_QUERY = gql`
  query GetSkills($first: Int!, $offset: Int!, $search: String) {
    skills {
      id
      name
      description
      status
      version
    }
  }
`;

const TOGGLE_SKILL_MUTATION = gql`
  mutation ToggleSkill($id: ID!, $enabled: Boolean!) {
    toggleSkill(id: $id, enabled: $enabled) {
      id
      status
    }
  }
`;

type Skill = {
  id: string;
  name: string;
  description: string;
  status: string;
  version: string;
};

type SkillsResponse = {
  skills: Skill[];
};

const columnHelper = createColumnHelper<Skill>();

export default function SkillsPage() {
  const queryClient = useQueryClient();
  const [search, setSearch] = useState("");
  const debouncedSearch = useDebounce(search, 300);
  const [sorting, setSorting] = useState<SortingState>([]);
  const [pagination, setPagination] = useState<PaginationState>({
    pageIndex: 0,
    pageSize: 25,
  });

  // Reset page on search change
  useEffect(() => {
    setPagination((prev) => ({ ...prev, pageIndex: 0 }));
  }, [debouncedSearch]);

  const { data, isLoading, isFetching } = useQuery<SkillsResponse>({
    queryKey: skillKeys.list({ search: debouncedSearch }, pagination.pageSize),
    queryFn: () =>
      graphqlClient.request(SKILLS_QUERY, {
        first: pagination.pageSize,
        offset: pagination.pageIndex * pagination.pageSize,
        search: debouncedSearch || undefined,
      }),
    placeholderData: (prev) => prev,
  });

  const toggleMutation = useMutation({
    mutationFn: (variables: { id: string; enabled: boolean }) =>
      graphqlClient.request(TOGGLE_SKILL_MUTATION, variables),
    onMutate: async (variables) => {
      await queryClient.cancelQueries({
        queryKey: skillKeys.all,
      });

      const previousSkills = queryClient.getQueryData(skillKeys.all);

      queryClient.setQueriesData(
        { queryKey: skillKeys.all },
        (old: SkillsResponse | undefined) => {
          if (!old) return old;
          return {
            ...old,
            skills: old.skills.map((skill) =>
              skill.id === variables.id
                ? {
                    ...skill,
                    status: variables.enabled ? "ACTIVE" : "DISABLED",
                  }
                : skill
            ),
          };
        }
      );

      return { previousSkills };
    },
    onError: (_err, _variables, context) => {
      if (context?.previousSkills) {
        queryClient.setQueryData(skillKeys.all, context.previousSkills);
      }
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: skillKeys.all });
    },
  });

  const skills = data?.skills ?? [];

  const columns = [
    columnHelper.accessor("name", {
      header: "Name",
      cell: (info) => (
        <span className="font-medium">{info.getValue()}</span>
      ),
    }),
    columnHelper.accessor("description", {
      header: "Description",
      cell: (info) => (
        <span className="text-muted-foreground line-clamp-1 max-w-[300px] block">
          {info.getValue() || "—"}
        </span>
      ),
      enableSorting: false,
    }),
    columnHelper.accessor("version", {
      header: "Version",
      cell: (info) => (
        <code className="text-xs bg-muted px-1.5 py-0.5 rounded">
          {info.getValue()}
        </code>
      ),
    }),
    columnHelper.accessor("status", {
      header: "Status",
      cell: (info) => {
        const status = info.getValue();
        return (
          <Badge variant={status === "ACTIVE" ? "default" : "secondary"}>
            {status?.toLowerCase() ?? "unknown"}
          </Badge>
        );
      },
    }),
    columnHelper.display({
      id: "toggle",
      header: "Enabled",
      cell: (info) => {
        const skill = info.row.original;
        const isEnabled = skill.status === "ACTIVE";
        return (
          <Switch
            checked={isEnabled}
            onCheckedChange={(checked) => {
              const promise = toggleMutation.mutateAsync({
                id: skill.id,
                enabled: checked,
              });
              toast.promise(promise, {
                loading: "Updating skill...",
                success: `Skill ${checked ? "enabled" : "disabled"}`,
                error: (err) => `Failed: ${err.message}`,
              });
            }}
            disabled={toggleMutation.isPending}
          />
        );
      },
    }),
  ];

  if (skills.length === 0 && !isLoading) {
    return (
      <div className="space-y-6">
        <div>
          <h1 className="text-3xl font-bold tracking-tight">Skills</h1>
          <p className="text-muted-foreground">
            Manage embedded skills in HelixDB
          </p>
        </div>
        <EmptyState
          title="No skills embedded yet"
          description="Skills are embedded rules and knowledge that guide AI agents. Run glia save-skill to get started."
          icon={<Wrench className="h-6 w-6 text-muted-foreground" />}
        />
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-3xl font-bold tracking-tight">Skills</h1>
          <p className="text-muted-foreground">
            Manage embedded skills in HelixDB
          </p>
        </div>
        <div className="flex items-center gap-2">
          <div className="relative">
            <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
            <Input
              placeholder="Search skills..."
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              className="pl-8 w-64"
            />
          </div>
          <Button
            variant="outline"
            size="sm"
            onClick={() => queryClient.invalidateQueries({ queryKey: skillKeys.all })}
            disabled={isFetching}
          >
            Refresh
          </Button>
        </div>
      </div>

      <DataTable
        data={skills}
        columns={columns}
        isLoading={isLoading}
        isFetching={isFetching}
        pagination={pagination}
        sorting={sorting}
        onPaginationChange={setPagination}
        onSortingChange={setSorting}
      />
    </div>
  );
}
