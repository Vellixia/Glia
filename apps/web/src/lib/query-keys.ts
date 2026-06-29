export const skillKeys = {
  all: ["skills"] as const,
  lists: () => [...skillKeys.all, "list"] as const,
  list: (filters: Record<string, unknown>, first: number) =>
    [...skillKeys.lists(), { filters, first }] as const,
  details: () => [...skillKeys.all, "detail"] as const,
  detail: (id: string) => [...skillKeys.details(), id] as const,
};

export const agentKeys = {
  all: ["agents"] as const,
  lists: () => [...agentKeys.all, "list"] as const,
  list: (filters?: Record<string, unknown>) =>
    [...agentKeys.lists(), filters] as const,
};

export const settingsKeys = {
  all: ["settings"] as const,
};

export const syncKeys = {
  all: ["sync"] as const,
  status: () => [...syncKeys.all, "status"] as const,
};

export const catalogKeys = {
  all: ["catalog"] as const,
  tools: () => [...catalogKeys.all, "tools"] as const,
  installed: () => [...catalogKeys.all, "installed"] as const,
};

export const secretsKeys = {
  all: ["secrets"] as const,
  providers: () => [...secretsKeys.all, "providers"] as const,
  credentials: () => [...secretsKeys.all, "credentials"] as const,
};
