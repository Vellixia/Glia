import * as React from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, type RenderOptions } from "@testing-library/react";

function createTestQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0, staleTime: 0 },
      mutations: { retry: false },
    },
  });
}

interface WrapperOptions {
  queryClient?: QueryClient;
}

function createWrapper({ queryClient }: WrapperOptions = {}) {
  const client = queryClient ?? createTestQueryClient();
  return function TestWrapper({ children }: { children: React.ReactNode }) {
    return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
  };
}

export function renderWithProviders(
  ui: React.ReactElement,
  options?: Omit<RenderOptions, "wrapper"> & WrapperOptions
) {
  const { queryClient, ...renderOptions } = options ?? {};
  return render(ui, { wrapper: createWrapper({ queryClient }), ...renderOptions });
}

export function createQueryClientForTest() {
  return createTestQueryClient();
}
