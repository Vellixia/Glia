"use client";

import { Button } from "@/components/ui/button";

export default function GlobalError({
  error,
  unstable_retry,
}: {
  error: Error & { digest?: string };
  unstable_retry: () => void;
}) {
  return (
    <html>
      <body>
        <div className="flex min-h-screen flex-col items-center justify-center gap-4">
          <h1 className="text-4xl font-bold">Something went wrong</h1>
          <p className="text-muted-foreground">{error.message}</p>
          <Button onClick={unstable_retry}>Try again</Button>
        </div>
      </body>
    </html>
  );
}
