"use client";

import { ErrorState } from "@/components/error-state";

export default function CatalogError({
  error,
  unstable_retry,
}: {
  error: Error & { digest?: string };
  unstable_retry: () => void;
}) {
  return (
    <ErrorState
      title="Failed to load catalog"
      description={error.message}
      onRetry={unstable_retry}
    />
  );
}
