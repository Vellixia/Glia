"use client";

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { graphqlClient } from "@/lib/graphql-client";
import { gql } from "graphql-request";
import { settingsKeys } from "@/lib/query-keys";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import {
  settingsSchema,
  type SettingsFormValues,
} from "@/lib/schemas/settings";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { toast } from "sonner";
import { Settings } from "lucide-react";

const SETTINGS_QUERY = gql`
  query GetSettings {
    settings {
      hubUrl
      theme
      logLevel
    }
  }
`;

const UPDATE_SETTINGS_MUTATION = gql`
  mutation UpdateSettings($input: UpdateSettingsInput!) {
    updateSettings(input: $input) {
      hubUrl
      theme
      logLevel
    }
  }
`;

type SettingsData = {
  settings: {
    hubUrl: string;
    theme: string;
    logLevel: string;
  };
};

export default function SettingsPage() {
  const queryClient = useQueryClient();

  const { data, isLoading } = useQuery<SettingsData>({
    queryKey: settingsKeys.all,
    queryFn: () => graphqlClient.request(SETTINGS_QUERY),
  });

  const form = useForm<SettingsFormValues>({
    resolver: zodResolver(settingsSchema),
    values: {
      hubUrl: data?.settings?.hubUrl ?? "http://127.0.0.1:3000",
      theme: (data?.settings?.theme?.toLowerCase() as "light" | "dark" | "system") ?? "system",
      logLevel: (data?.settings?.logLevel?.toLowerCase() as "info" | "warn" | "error" | "debug") ?? "info",
    },
  });

  const updateMutation = useMutation({
    mutationFn: (variables: { input: { theme?: string; logLevel?: string } }) =>
      graphqlClient.request(UPDATE_SETTINGS_MUTATION, variables),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: settingsKeys.all });
    },
  });

  function onSubmit(values: SettingsFormValues) {
    const promise = updateMutation.mutateAsync({
      input: {
        theme: values.theme,
        logLevel: values.logLevel,
      },
    });
    toast.promise(promise, {
      loading: "Saving settings...",
      success: "Settings saved",
      error: (err) => `Failed: ${err.message}`,
    });
  }

  if (isLoading) {
    return (
      <div className="space-y-6">
        <div>
          <h1 className="text-3xl font-bold tracking-tight">Settings</h1>
          <p className="text-muted-foreground">Hub configuration and preferences</p>
        </div>
        <Card>
          <CardHeader>
            <Skeleton className="h-5 w-32" />
            <Skeleton className="h-4 w-48" />
          </CardHeader>
          <CardContent className="space-y-4">
            {[1, 2, 3].map((i) => (
              <div key={i} className="space-y-2">
                <Skeleton className="h-4 w-24" />
                <Skeleton className="h-9 w-full" />
              </div>
            ))}
            <Skeleton className="h-9 w-24" />
          </CardContent>
        </Card>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-3xl font-bold tracking-tight">Settings</h1>
        <p className="text-muted-foreground">Hub configuration and preferences</p>
      </div>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Settings className="h-5 w-5" />
            General Settings
          </CardTitle>
          <CardDescription>
            Configure your Glia hub connection and preferences.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <form onSubmit={form.handleSubmit(onSubmit)} className="space-y-6">
            <div className="space-y-2">
              <Label htmlFor="hubUrl">Hub URL</Label>
              <Input
                id="hubUrl"
                {...form.register("hubUrl")}
                disabled
                className="font-mono"
              />
              <p className="text-xs text-muted-foreground">
                Hub URL is configured via environment variables.
              </p>
            </div>

            <div className="space-y-2">
              <Label htmlFor="theme">Theme</Label>
              <Select
                value={form.watch("theme")}
                onValueChange={(val) =>
                  form.setValue("theme", val as "light" | "dark" | "system")
                }
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="light">Light</SelectItem>
                  <SelectItem value="dark">Dark</SelectItem>
                  <SelectItem value="system">System</SelectItem>
                </SelectContent>
              </Select>
            </div>

            <div className="space-y-2">
              <Label htmlFor="logLevel">Log Level</Label>
              <Select
                value={form.watch("logLevel")}
                onValueChange={(val) =>
                  form.setValue("logLevel", val as "info" | "warn" | "error" | "debug")
                }
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="debug">Debug</SelectItem>
                  <SelectItem value="info">Info</SelectItem>
                  <SelectItem value="warn">Warning</SelectItem>
                  <SelectItem value="error">Error</SelectItem>
                </SelectContent>
              </Select>
            </div>

            <Button type="submit" disabled={updateMutation.isPending}>
              {updateMutation.isPending ? "Saving..." : "Save Settings"}
            </Button>
          </form>
        </CardContent>
      </Card>
    </div>
  );
}
