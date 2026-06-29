import { QueryCache, QueryClient, MutationCache } from "@tanstack/react-query";
import { toast } from "sonner";

let browserQueryClient: QueryClient | undefined;

function is401(error: Error): boolean {
  const err = error as any;
  return err?.response?.status === 401;
}

function handleError(error: Error) {
  if (is401(error)) {
    // Dynamic import to avoid bundling next-auth/react on the server
    import("next-auth/react").then(({ signOut }) => {
      signOut({ callbackUrl: "/login" });
    });
    return;
  }
  const message = error instanceof Error ? error.message : "An unexpected error occurred";
  toast.error(message, { duration: 5000 });
}

function makeQueryClient() {
  return new QueryClient({
    queryCache: new QueryCache({
      onError: handleError,
    }),
    mutationCache: new MutationCache({
      onError: handleError,
    }),
    defaultOptions: {
      queries: {
        staleTime: 60 * 1000,
        retry: 1,
      },
    },
  });
}

export function getQueryClient() {
  if (typeof window === "undefined") {
    return makeQueryClient();
  }
  if (!browserQueryClient) {
    browserQueryClient = makeQueryClient();
  }
  return browserQueryClient;
}
