"use client";

import { useState, useMemo } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { gql } from "graphql-request";
import { graphqlClient } from "@/lib/graphql-client";
import { catalogKeys } from "@/lib/query-keys";
import { useDebounce } from "@/hooks/use-debounce";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { EmptyState } from "@/components/empty-state";
import { ErrorState } from "@/components/error-state";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
  DialogClose,
} from "@/components/ui/dialog";
import { toast } from "sonner";
import { Store, Search, LayoutGrid, List, AlertTriangle } from "lucide-react";

// --- Types ---

interface CatalogEntry {
  name: string;
  display: string;
  description: string;
  path: string;
  stacks: string[];
  creds: string[];
  version: string;
}

interface CatalogIndex {
  version: number;
  tools: CatalogEntry[];
}

interface Skill {
  id: string;
  name: string;
  description: string;
  version: string;
  status: string;
  installed_at: string;
}

interface InstalledSkillsResponse {
  installedSkills: Skill[];
}

// --- GraphQL ---

const INSTALLED_QUERY = gql`
  query GetInstalledSkills {
    installedSkills {
      id
      name
      description
      version
      status
      installed_at
    }
  }
`;

const INSTALL_TOOL_MUTATION = gql`
  mutation InstallTool($name: String!) {
    installTool(name: $name) {
      id
      name
      status
      version
      installed_at
    }
  }
`;

const REMOVE_SKILL_MUTATION = gql`
  mutation RemoveSkill($id: ID!) {
    removeSkill(id: $id)
  }
`;

// --- Helpers ---

const CATALOG_URL =
  "https://raw.githubusercontent.com/Vellixia/community-catalog/main/catalog.json";

async function fetchCatalog(): Promise<CatalogIndex> {
  const res = await fetch(CATALOG_URL);
  if (!res.ok) {
    throw new Error(`Failed to fetch catalog: ${res.statusText}`);
  }
  return res.json();
}

// --- Merged type ---

interface MergedEntry extends CatalogEntry {
  isInstalled: boolean;
  installedSkillId: string | null;
  installedStatus: string | null;
}

// --- Component ---

export default function CatalogPage() {
  const queryClient = useQueryClient();
  const [view, setView] = useState<"grid" | "list">("grid");
  const [search, setSearch] = useState("");
  const [activeStack, setActiveStack] = useState<string | null>(null);
  const [selectedEntry, setSelectedEntry] = useState<MergedEntry | null>(null);
  const [confirmUninstall, setConfirmUninstall] = useState<MergedEntry | null>(null);

  const debouncedSearch = useDebounce(search, 300);

  // Queries
  const {
    data: catalogData,
    isLoading: catalogLoading,
    error: catalogError,
    refetch: refetchCatalog,
  } = useQuery<CatalogIndex>({
    queryKey: catalogKeys.tools(),
    queryFn: fetchCatalog,
    staleTime: 5 * 60 * 1000,
    retry: 2,
  });

  const {
    data: installedData,
    isLoading: installedLoading,
  } = useQuery<InstalledSkillsResponse>({
    queryKey: catalogKeys.installed(),
    queryFn: () => graphqlClient.request(INSTALLED_QUERY),
  });

  // Mutations
  const installMutation = useMutation({
    mutationFn: (name: string) =>
      graphqlClient.request(INSTALL_TOOL_MUTATION, { name }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: catalogKeys.installed() });
      queryClient.invalidateQueries({ queryKey: ["skills"] });
    },
  });

  const uninstallMutation = useMutation({
    mutationFn: (id: string) =>
      graphqlClient.request(REMOVE_SKILL_MUTATION, { id }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: catalogKeys.installed() });
      queryClient.invalidateQueries({ queryKey: ["skills"] });
    },
  });

  // Merge catalog entries with installed state
  const mergedEntries: MergedEntry[] = useMemo(() => {
    if (!catalogData?.tools) return [];
    const installed = installedData?.installedSkills ?? [];
    return catalogData.tools.map((entry) => {
      const match = installed.find((s) => s.name === entry.name);
      return {
        ...entry,
        isInstalled: !!match,
        installedSkillId: match?.id ?? null,
        installedStatus: match?.status ?? null,
      };
    });
  }, [catalogData, installedData]);

  // Extract unique stacks from catalog
  const allStacks = useMemo(() => {
    const set = new Set<string>();
    catalogData?.tools.forEach((e) => e.stacks.forEach((s) => set.add(s)));
    return Array.from(set).sort();
  }, [catalogData]);

  // Client-side filtering
  const filtered = useMemo(() => {
    let result = mergedEntries;
    if (debouncedSearch) {
      const q = debouncedSearch.toLowerCase();
      result = result.filter(
        (e) =>
          e.name.toLowerCase().includes(q) ||
          e.display.toLowerCase().includes(q) ||
          e.description.toLowerCase().includes(q) ||
          e.stacks.some((s) => s.toLowerCase().includes(q))
      );
    }
    if (activeStack) {
      result = result.filter((e) => e.stacks.includes(activeStack));
    }
    return result;
  }, [mergedEntries, debouncedSearch, activeStack]);

  // Handlers
  const handleInstall = (name: string) => {
    toast.promise(installMutation.mutateAsync(name), {
      loading: `Installing ${name}...`,
      success: `${name} installed`,
      error: (err: Error) => `Failed: ${err.message}`,
    });
  };

  const handleUninstallConfirm = () => {
    if (!confirmUninstall?.installedSkillId) return;
    const name = confirmUninstall.display;
    toast.promise(uninstallMutation.mutateAsync(confirmUninstall.installedSkillId), {
      loading: `Removing ${name}...`,
      success: `${name} removed`,
      error: (err: Error) => `Failed: ${err.message}`,
    });
    setConfirmUninstall(null);
  };

  // Loading state
  if (catalogLoading || installedLoading) {
    return (
      <div className="space-y-6">
        <div>
          <h1 className="text-3xl font-bold tracking-tight">Catalog</h1>
          <p className="text-muted-foreground">Browse community tools from GitHub catalog</p>
        </div>
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
          {Array.from({ length: 6 }).map((_, i) => (
            <Card key={i}>
              <CardHeader>
                <Skeleton className="h-5 w-32" />
                <Skeleton className="h-4 w-full mt-2" />
              </CardHeader>
              <CardContent className="space-y-3">
                <Skeleton className="h-4 w-16" />
                <div className="flex gap-2">
                  <Skeleton className="h-5 w-16" />
                  <Skeleton className="h-5 w-20" />
                </div>
                <Skeleton className="h-8 w-full" />
              </CardContent>
            </Card>
          ))}
        </div>
      </div>
    );
  }

  // Error state
  if (catalogError) {
    return (
      <div className="space-y-6">
        <div>
          <h1 className="text-3xl font-bold tracking-tight">Catalog</h1>
          <p className="text-muted-foreground">Browse community tools from GitHub catalog</p>
        </div>
        <ErrorState
          title="Failed to load catalog"
          description={catalogError instanceof Error ? catalogError.message : "Could not fetch community catalog"}
          onRetry={() => refetchCatalog()}
        />
      </div>
    );
  }

  // Empty catalog
  if (!catalogData?.tools.length) {
    return (
      <div className="space-y-6">
        <div>
          <h1 className="text-3xl font-bold tracking-tight">Catalog</h1>
          <p className="text-muted-foreground">Browse community tools from GitHub catalog</p>
        </div>
        <EmptyState
          title="No tools available"
          description="The community catalog is empty or could not be loaded."
          icon={<Store className="h-6 w-6 text-muted-foreground" />}
        />
      </div>
    );
  }

  // No filtered results
  if (!filtered.length) {
    return (
      <div className="space-y-6">
        <div>
          <h1 className="text-3xl font-bold tracking-tight">Catalog</h1>
          <p className="text-muted-foreground">Browse community tools from GitHub catalog</p>
        </div>
        <div className="flex items-center gap-4 flex-wrap">
          <div className="relative flex-1 max-w-sm">
            <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
            <Input
              placeholder="Search catalog..."
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              className="pl-8"
            />
          </div>
          <div className="inline-flex items-center rounded-lg border p-0.5 gap-0.5">
            <Button
              variant={view === "grid" ? "default" : "ghost"}
              size="icon-sm"
              onClick={() => setView("grid")}
            >
              <LayoutGrid className="h-4 w-4" />
            </Button>
            <Button
              variant={view === "list" ? "default" : "ghost"}
              size="icon-sm"
              onClick={() => setView("list")}
            >
              <List className="h-4 w-4" />
            </Button>
          </div>
        </div>
        <div className="flex flex-wrap gap-2">
          {allStacks.map((stack) => (
            <Badge
              key={stack}
              variant={activeStack === stack ? "default" : "outline"}
              className="cursor-pointer"
              onClick={() => setActiveStack(activeStack === stack ? null : stack)}
            >
              {stack}
            </Badge>
          ))}
        </div>
        <EmptyState
          title="No tools match your search"
          description="Try adjusting your search terms or filters."
          icon={<Search className="h-6 w-6 text-muted-foreground" />}
        />
      </div>
    );
  }

  // Main content
  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-3xl font-bold tracking-tight">Catalog</h1>
        <p className="text-muted-foreground">Browse community tools from GitHub catalog</p>
      </div>

      {/* Toolbar */}
      <div className="flex items-center gap-4 flex-wrap">
        <div className="relative flex-1 max-w-sm">
          <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
          <Input
            placeholder="Search catalog..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            className="pl-8"
          />
        </div>
        <div className="inline-flex items-center rounded-lg border p-0.5 gap-0.5">
          <Button
            variant={view === "grid" ? "default" : "ghost"}
            size="icon-sm"
            onClick={() => setView("grid")}
          >
            <LayoutGrid className="h-4 w-4" />
          </Button>
          <Button
            variant={view === "list" ? "default" : "ghost"}
            size="icon-sm"
            onClick={() => setView("list")}
          >
            <List className="h-4 w-4" />
          </Button>
        </div>
        <Button
          variant="outline"
          size="sm"
          onClick={() => {
            refetchCatalog();
            queryClient.invalidateQueries({ queryKey: catalogKeys.installed() });
          }}
        >
          Refresh
        </Button>
      </div>

      {/* Stack badges */}
      <div className="flex flex-wrap gap-2">
        {allStacks.map((stack) => (
          <Badge
            key={stack}
            variant={activeStack === stack ? "default" : "outline"}
            className="cursor-pointer"
            onClick={() => setActiveStack(activeStack === stack ? null : stack)}
          >
            {stack}
          </Badge>
        ))}
      </div>

      {/* Grid view */}
      {view === "grid" && (
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
          {filtered.map((entry) => (
            <Card key={entry.name} className="flex flex-col">
              <CardHeader className="pb-3 flex-1">
                <div className="flex items-start justify-between gap-2">
                  <CardTitle className="text-base font-medium leading-tight">
                    {entry.display}
                  </CardTitle>
                  <Badge variant="outline" className="shrink-0 text-xs">
                    v{entry.version}
                  </Badge>
                </div>
                <p className="text-sm text-muted-foreground line-clamp-2 mt-1">
                  {entry.description}
                </p>
              </CardHeader>
              <CardContent className="space-y-3 pt-0">
                <div className="flex flex-wrap gap-1">
                  {entry.stacks.map((stack) => (
                    <Badge key={stack} variant="secondary" className="text-xs">
                      {stack}
                    </Badge>
                  ))}
                </div>
                <div className="flex gap-2">
                  {entry.isInstalled ? (
                    <Button
                      variant="destructive"
                      size="sm"
                      className="flex-1"
                      onClick={() => setConfirmUninstall(entry)}
                    >
                      Uninstall
                    </Button>
                  ) : (
                    <Button
                      variant="default"
                      size="sm"
                      className="flex-1"
                      onClick={() => handleInstall(entry.name)}
                      disabled={installMutation.isPending}
                    >
                      Install
                    </Button>
                  )}
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => setSelectedEntry(entry)}
                  >
                    Details
                  </Button>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>
      )}

      {/* List view */}
      {view === "list" && (
        <div className="flex flex-col gap-2">
          {filtered.map((entry) => (
            <Card key={entry.name}>
              <div className="flex items-center gap-4 p-4">
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="font-medium">{entry.display}</span>
                    <Badge variant="outline" className="text-xs shrink-0">
                      v{entry.version}
                    </Badge>
                  </div>
                  <p className="text-sm text-muted-foreground line-clamp-1 mt-0.5">
                    {entry.description}
                  </p>
                </div>
                <div className="flex flex-wrap gap-1 shrink-0">
                  {entry.stacks.map((stack) => (
                    <Badge key={stack} variant="secondary" className="text-xs">
                      {stack}
                    </Badge>
                  ))}
                </div>
                <div className="flex gap-2 shrink-0">
                  {entry.isInstalled ? (
                    <Button
                      variant="destructive"
                      size="sm"
                      onClick={() => setConfirmUninstall(entry)}
                    >
                      Uninstall
                    </Button>
                  ) : (
                    <Button
                      variant="default"
                      size="sm"
                      onClick={() => handleInstall(entry.name)}
                      disabled={installMutation.isPending}
                    >
                      Install
                    </Button>
                  )}
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => setSelectedEntry(entry)}
                  >
                    Details
                  </Button>
                </div>
              </div>
            </Card>
          ))}
        </div>
      )}

      {/* Detail Dialog */}
      {selectedEntry && (
        <Dialog
          open={!!selectedEntry}
          onOpenChange={(open) => {
            if (!open) setSelectedEntry(null);
          }}
        >
          <DialogContent>
            <DialogHeader>
              <DialogTitle>{selectedEntry.display}</DialogTitle>
              <DialogDescription>{selectedEntry.description}</DialogDescription>
            </DialogHeader>
            <div className="space-y-4">
              <div className="flex items-center gap-2">
                <span className="text-sm font-medium">Version:</span>
                <Badge variant="outline" className="text-xs">
                  v{selectedEntry.version}
                </Badge>
              </div>
              {selectedEntry.stacks.length > 0 && (
                <div>
                  <span className="text-sm font-medium block mb-1.5">Stacks</span>
                  <div className="flex flex-wrap gap-1">
                    {selectedEntry.stacks.map((stack) => (
                      <Badge key={stack} variant="secondary" className="text-xs">
                        {stack}
                      </Badge>
                    ))}
                  </div>
                </div>
              )}
              {selectedEntry.creds.length > 0 && (
                <div>
                  <span className="text-sm font-medium block mb-1.5">Credentials</span>
                  <div className="flex flex-wrap gap-1">
                    {selectedEntry.creds.map((cred) => (
                      <Badge key={cred} variant="outline" className="text-xs">
                        {cred}
                      </Badge>
                    ))}
                  </div>
                </div>
              )}
            </div>
            <DialogFooter>
              {selectedEntry.isInstalled ? (
                <Button
                  variant="destructive"
                  onClick={() => {
                    setSelectedEntry(null);
                    setConfirmUninstall(selectedEntry);
                  }}
                >
                  Uninstall
                </Button>
              ) : (
                <Button
                  variant="default"
                  onClick={() => {
                    setSelectedEntry(null);
                    handleInstall(selectedEntry.name);
                  }}
                  disabled={installMutation.isPending}
                >
                  Install
                </Button>
              )}
              <DialogClose render={<Button variant="outline" />}>
                Close
              </DialogClose>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      )}

      {/* Uninstall Confirmation Dialog */}
      {confirmUninstall && (
        <Dialog
          open={!!confirmUninstall}
          onOpenChange={(open) => {
            if (!open) setConfirmUninstall(null);
          }}
        >
          <DialogContent>
            <DialogHeader>
              <div className="flex items-center gap-3">
                <div className="rounded-full bg-destructive/10 p-2">
                  <AlertTriangle className="h-5 w-5 text-destructive" />
                </div>
                <div>
                  <DialogTitle>Uninstall {confirmUninstall.display}</DialogTitle>
                  <DialogDescription>
                    This will remove the tool and its associated data. This action cannot be undone.
                  </DialogDescription>
                </div>
              </div>
            </DialogHeader>
            <DialogFooter>
              <DialogClose render={<Button variant="outline" />}>
                Cancel
              </DialogClose>
              <Button
                variant="destructive"
                onClick={handleUninstallConfirm}
                disabled={uninstallMutation.isPending}
              >
                {uninstallMutation.isPending ? "Removing..." : "Uninstall"}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      )}
    </div>
  );
}
