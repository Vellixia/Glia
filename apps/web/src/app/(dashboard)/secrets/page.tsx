"use client";

import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { graphqlClient } from "@/lib/graphql-client";
import { gql } from "graphql-request";
import { secretsKeys } from "@/lib/query-keys";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { createColumnHelper } from "@tanstack/react-table";
import type { PaginationState, SortingState } from "@tanstack/react-table";

import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { DataTable } from "@/components/data-table";
import { EmptyState } from "@/components/empty-state";
import { ErrorState } from "@/components/error-state";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
  DialogClose,
} from "@/components/ui/dialog";
import { toast } from "sonner";
import { Key, Plus, Eye, EyeOff, Trash2 } from "lucide-react";

const GET_PROVIDERS = gql`
  query GetProviders {
    oauthProviders {
      id
      name
      authUrl
      tokenUrl
      clientId
      scopes
    }
  }
`;

const GET_SECRETS = gql`
  query GetSecrets {
    secrets {
      credId
      ready
      provider
      createdAt
    }
  }
`;

const REGISTER_PROVIDER = gql`
  mutation RegisterProvider($input: ProviderInput!) {
    registerOauthProvider(input: $input) {
      id
    }
  }
`;

const DELETE_PROVIDER = gql`
  mutation DeleteProvider($id: String!) {
    deleteOauthProvider(id: $id)
  }
`;

const DELETE_SECRET = gql`
  mutation DeleteSecret($credId: String!) {
    deleteSecret(credId: $credId)
  }
`;

interface Provider {
  id: string;
  name: string;
  authUrl: string;
  tokenUrl: string;
  clientId: string;
  scopes: string[];
}

interface SecretEntry {
  credId: string;
  ready: boolean;
  provider: string | null;
  createdAt: string;
}

const providerSchema = z.object({
  providerId: z.string().min(1, "Provider ID is required"),
  name: z.string().min(1, "Name is required"),
  authUrl: z.string().url("Must be a valid URL"),
  tokenUrl: z.string().url("Must be a valid URL"),
  clientId: z.string().min(1, "Client ID is required"),
  clientSecret: z.string().min(1, "Client Secret is required"),
  scopes: z.string().optional(),
});

type ProviderFormValues = z.infer<typeof providerSchema>;

const columnHelper = createColumnHelper<Provider>();

const providersColumns = [
  columnHelper.accessor("name", {
    header: "Name",
  }),
  columnHelper.accessor("clientId", {
    header: "Client ID",
    cell: (info) => (
      <span className="max-w-[180px] truncate block font-mono text-xs" title={info.getValue()}>
        {info.getValue()}
      </span>
    ),
  }),
  columnHelper.accessor("authUrl", {
    header: "Auth URL",
    cell: (info) => (
      <span className="max-w-[200px] truncate block font-mono text-xs" title={info.getValue()}>
        {info.getValue()}
      </span>
    ),
  }),
  columnHelper.accessor("scopes", {
    header: "Scopes",
    cell: (info) => info.getValue()?.join(", ") ?? "-",
  }),
  columnHelper.display({
    id: "actions",
    header: "Actions",
    cell: (info) => <DeleteProviderCell provider={info.row.original} />,
  }),
];

function DeleteProviderCell({ provider }: { provider: Provider }) {
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);

  const deleteMutation = useMutation({
    mutationFn: (id: string) =>
      graphqlClient.request(DELETE_PROVIDER, { id }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: secretsKeys.providers() });
      toast.success(`Provider "${provider.name}" deleted`);
      setOpen(false);
    },
    onError: () => {
      toast.error(`Failed to delete provider "${provider.name}"`);
    },
  });

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger
        render={
          <Button variant="destructive" size="sm">
            <Trash2 className="h-4 w-4" />
          </Button>
        }
      />
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Delete {provider.name}?</DialogTitle>
          <DialogDescription>
            This will permanently delete the OAuth provider and all associated
            credentials. This action cannot be undone.
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <DialogClose render={<Button variant="outline">Cancel</Button>} />
          <Button
            variant="destructive"
            onClick={() => deleteMutation.mutate(provider.id)}
            disabled={deleteMutation.isPending}
          >
            {deleteMutation.isPending ? "Deleting..." : "Delete"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function DeleteSecretCell({ secret }: { secret: SecretEntry }) {
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);

  const deleteMutation = useMutation({
    mutationFn: (credId: string) =>
      graphqlClient.request(DELETE_SECRET, { credId }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: secretsKeys.credentials() });
      toast.success("Credential deleted");
      setOpen(false);
    },
    onError: () => {
      toast.error("Failed to delete credential");
    },
  });

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger
        render={
          <Button variant="ghost" size="icon" className="h-8 w-8 -mr-2 -mt-1">
            <Trash2 className="h-4 w-4 text-destructive" />
          </Button>
        }
      />
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Delete credential?</DialogTitle>
          <DialogDescription>
            This will permanently delete the credential{" "}
            <span className="font-mono font-medium text-foreground">
              {secret.credId}
            </span>
            {secret.provider && (
              <>
                {" "}
                from provider{" "}
                <span className="font-medium text-foreground">
                  {secret.provider}
                </span>
              </>
            )}
            . This action cannot be undone.
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <DialogClose render={<Button variant="outline">Cancel</Button>} />
          <Button
            variant="destructive"
            onClick={() => deleteMutation.mutate(secret.credId)}
            disabled={deleteMutation.isPending}
          >
            {deleteMutation.isPending ? "Deleting..." : "Delete"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function AddProviderDialog() {
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const [showSecret, setShowSecret] = useState(false);

  const form = useForm<ProviderFormValues>({
    resolver: zodResolver(providerSchema),
    defaultValues: {
      providerId: "",
      name: "",
      authUrl: "",
      tokenUrl: "",
      clientId: "",
      clientSecret: "",
      scopes: "",
    },
  });

  const registerMutation = useMutation({
    mutationFn: (input: ProviderFormValues) =>
      graphqlClient.request(REGISTER_PROVIDER, {
        input: {
          providerId: input.providerId,
          name: input.name,
          authUrl: input.authUrl,
          tokenUrl: input.tokenUrl,
          clientId: input.clientId,
          clientSecret: input.clientSecret,
          scopes: input.scopes
            ? input.scopes.split(",").map((s) => s.trim()).filter(Boolean)
            : [],
        },
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: secretsKeys.providers() });
      toast.success("Provider registered successfully");
      setOpen(false);
      form.reset();
    },
    onError: () => {
      toast.error("Failed to register provider");
    },
  });

  function onSubmit(values: ProviderFormValues) {
    registerMutation.mutate(values);
  }

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger
        render={
          <Button data-testid="add-provider-trigger">
            <Plus className="h-4 w-4" />
            Add Provider
          </Button>
        }
      />
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Add OAuth Provider</DialogTitle>
          <DialogDescription>
            Register a new OAuth provider for credential management.
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={form.handleSubmit(onSubmit)} className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="providerId">Provider ID</Label>
            <Input
              id="providerId"
              placeholder="github"
              {...form.register("providerId")}
            />
            {form.formState.errors.providerId && (
              <p className="text-xs text-destructive">
                {form.formState.errors.providerId.message}
              </p>
            )}
          </div>
          <div className="space-y-2">
            <Label htmlFor="name">Name</Label>
            <Input
              id="name"
              placeholder="GitHub"
              {...form.register("name")}
            />
            {form.formState.errors.name && (
              <p className="text-xs text-destructive">
                {form.formState.errors.name.message}
              </p>
            )}
          </div>
          <div className="space-y-2">
            <Label htmlFor="authUrl">Auth URL</Label>
            <Input
              id="authUrl"
              placeholder="https://github.com/login/oauth/authorize"
              {...form.register("authUrl")}
            />
            {form.formState.errors.authUrl && (
              <p className="text-xs text-destructive">
                {form.formState.errors.authUrl.message}
              </p>
            )}
          </div>
          <div className="space-y-2">
            <Label htmlFor="tokenUrl">Token URL</Label>
            <Input
              id="tokenUrl"
              placeholder="https://github.com/login/oauth/access_token"
              {...form.register("tokenUrl")}
            />
            {form.formState.errors.tokenUrl && (
              <p className="text-xs text-destructive">
                {form.formState.errors.tokenUrl.message}
              </p>
            )}
          </div>
          <div className="space-y-2">
            <Label htmlFor="clientId">Client ID</Label>
            <Input
              id="clientId"
              placeholder="your-client-id"
              {...form.register("clientId")}
            />
            {form.formState.errors.clientId && (
              <p className="text-xs text-destructive">
                {form.formState.errors.clientId.message}
              </p>
            )}
          </div>
          <div className="space-y-2">
            <Label htmlFor="clientSecret">Client Secret</Label>
            <div className="relative">
              <Input
                id="clientSecret"
                type={showSecret ? "text" : "password"}
                placeholder="your-client-secret"
                {...form.register("clientSecret")}
              />
              <Button
                type="button"
                variant="ghost"
                size="icon"
                className="absolute right-0 top-0 h-full px-3"
                onClick={() => setShowSecret(!showSecret)}
              >
                {showSecret ? (
                  <EyeOff className="h-4 w-4" />
                ) : (
                  <Eye className="h-4 w-4" />
                )}
              </Button>
            </div>
            {form.formState.errors.clientSecret && (
              <p className="text-xs text-destructive">
                {form.formState.errors.clientSecret.message}
              </p>
            )}
          </div>
          <div className="space-y-2">
            <Label htmlFor="scopes">Scopes (comma-separated)</Label>
            <Input
              id="scopes"
              placeholder="repo, user"
              {...form.register("scopes")}
            />
          </div>
          <DialogFooter>
            <DialogClose render={<Button variant="outline" type="button">Cancel</Button>} />
            <Button
              type="submit"
              data-testid="provider-submit"
              disabled={registerMutation.isPending}
            >
              {registerMutation.isPending ? "Registering..." : "Register Provider"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export default function SecretsPage() {
  const [pagination, setPagination] = useState<PaginationState>({
    pageIndex: 0,
    pageSize: 50,
  });
  const [sorting, setSorting] = useState<SortingState>([]);

  const providersQuery = useQuery<{ oauthProviders: Provider[] }>({
    queryKey: secretsKeys.providers(),
    queryFn: () => graphqlClient.request(GET_PROVIDERS),
  });

  const secretsQuery = useQuery<{ secrets: SecretEntry[] }>({
    queryKey: secretsKeys.credentials(),
    queryFn: () => graphqlClient.request(GET_SECRETS),
  });

  const providers = providersQuery.data?.oauthProviders ?? [];
  const secrets = secretsQuery.data?.secrets ?? [];

  const isLoading = providersQuery.isLoading && secretsQuery.isLoading;

  if (isLoading) {
    return (
      <div className="space-y-6">
        <div>
          <h1 className="text-3xl font-bold tracking-tight">Secrets</h1>
          <p className="text-muted-foreground">
            Manage secrets via OpenBao integration
          </p>
        </div>
        <div className="space-y-4">
          <Skeleton className="h-7 w-40" />
          <div className="rounded-lg border">
            <div className="space-y-3 p-4">
              {[1, 2, 3, 4].map((i) => (
                <Skeleton key={i} className="h-8 w-full" />
              ))}
            </div>
          </div>
          <Skeleton className="h-7 w-40 mt-8" />
          <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
            {[1, 2, 3].map((i) => (
              <Skeleton key={i} className="h-32 rounded-lg" />
            ))}
          </div>
        </div>
      </div>
    );
  }

  const hasProvidersError = providersQuery.isError;
  const hasSecretsError = secretsQuery.isError;

  if (hasProvidersError && hasSecretsError) {
    return (
      <div className="space-y-6">
        <div>
          <h1 className="text-3xl font-bold tracking-tight">Secrets</h1>
          <p className="text-muted-foreground">
            Manage secrets via OpenBao integration
          </p>
        </div>
        <ErrorState
          title="Failed to load secrets data"
          description="Unable to fetch providers or credentials. Please try again."
          onRetry={() => {
            providersQuery.refetch();
            secretsQuery.refetch();
          }}
        />
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-3xl font-bold tracking-tight">Secrets</h1>
        <p className="text-muted-foreground">
          Manage secrets via OpenBao integration
        </p>
      </div>

      <div className="space-y-4">
        <div className="flex items-center justify-between">
          <h2 className="text-xl font-semibold">OAuth Providers</h2>
          <AddProviderDialog />
        </div>

        {hasProvidersError ? (
          <ErrorState
            title="Failed to load providers"
            onRetry={() => providersQuery.refetch()}
          />
        ) : providers.length === 0 ? (
          <Card>
            <CardContent className="pt-6">
              <EmptyState
                title="No providers configured"
                description="Add an OAuth provider to start managing credentials."
                icon={<Key className="h-6 w-6 text-muted-foreground" />}
              />
            </CardContent>
          </Card>
        ) : (
          <DataTable
            data={providers.slice(
              pagination.pageIndex * pagination.pageSize,
              (pagination.pageIndex + 1) * pagination.pageSize
            )}
            columns={providersColumns}
            pageCount={Math.ceil(providers.length / pagination.pageSize)}
            isLoading={providersQuery.isLoading}
            pagination={pagination}
            sorting={sorting}
            onPaginationChange={setPagination}
            onSortingChange={setSorting}
          />
        )}
      </div>

      <div className="space-y-4">
        <h2 className="text-xl font-semibold">Credentials</h2>

        {hasSecretsError ? (
          <ErrorState
            title="Failed to load credentials"
            onRetry={() => secretsQuery.refetch()}
          />
        ) : secrets.length === 0 ? (
          <Card>
            <CardContent className="pt-6">
              <EmptyState
                title="No credentials found"
                description="Credentials will appear here once OAuth flows are completed."
                icon={<Key className="h-6 w-6 text-muted-foreground" />}
              />
            </CardContent>
          </Card>
        ) : (
          <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
            {secrets.map((secret) => (
              <Card key={secret.credId}>
                <CardHeader className="pb-3">
                  <div className="flex items-start justify-between">
                    <div className="min-w-0 flex-1">
                      <CardTitle className="text-sm font-medium truncate">
                        {secret.provider ?? "Direct"}
                      </CardTitle>
                      <CardDescription className="font-mono text-xs mt-1 truncate">
                        {secret.credId}
                      </CardDescription>
                    </div>
                    <DeleteSecretCell secret={secret} />
                  </div>
                </CardHeader>
                <CardContent>
                  <div className="flex items-center justify-between">
                    <span className="text-xs text-muted-foreground">
                      {new Date(secret.createdAt).toLocaleDateString()}
                    </span>
                    <span
                      className={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ${
                        secret.ready
                          ? "bg-green-100 text-green-800 dark:bg-green-900/30 dark:text-green-400"
                          : "bg-yellow-100 text-yellow-800 dark:bg-yellow-900/30 dark:text-yellow-400"
                      }`}
                    >
                      {secret.ready ? "Ready" : "Pending"}
                    </span>
                  </div>
                </CardContent>
              </Card>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
