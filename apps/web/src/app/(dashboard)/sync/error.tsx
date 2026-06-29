"use client";

import { ErrorState } from "@/components/error-state";

export default function SyncError({
  error,
  unstable_retry,
}: {
  error: Error & { digest?: string };
  unstable_retry: () => void;
}) {
  return (
    <ErrorState
      title="Failed to load sync status"
      description={error.message}
      onRetry={unstable_retry}
    />
  );
}
