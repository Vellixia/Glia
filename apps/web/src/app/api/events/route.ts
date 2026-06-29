import { auth } from "@/auth";

const HUB_URL = process.env.HUB_URL ?? "http://127.0.0.1:3000";

export const dynamic = "force-dynamic";

/**
 * SSE proxy — streams Hub dashboard events to the browser.
 *
 * The browser's `EventSource` API cannot set custom request headers, so we
 * read the session here, attach the JWT as `Authorization: Bearer`, and
 * pass the upstream `text/event-stream` body straight back to the client.
 *
 * `cache: "no-store"` is required: without it, fetch (and Next.js) may
 * buffer the response body, breaking the streaming contract.
 *
 * `dynamic = "force-dynamic"` prevents Next.js from caching the response.
 */
export async function GET(request: Request) {
  const session = await auth();

  const headers: Record<string, string> = {
    Accept: "text/event-stream",
  };

  if (session?.hubToken) {
    headers["Authorization"] = `Bearer ${session.hubToken}`;
  }

  let upstream: Response;
  try {
    upstream = await fetch(`${HUB_URL}/api/events`, {
      method: "GET",
      headers,
      cache: "no-store",
      signal: request.signal,
    });
  } catch {
    return new Response(
      `event: error\ndata: ${JSON.stringify({ message: "Hub unreachable" })}\n\n`,
      {
        status: 502,
        headers: {
          "Content-Type": "text/event-stream; charset=utf-8",
          "Cache-Control": "no-cache, no-transform",
        },
      },
    );
  }

  if (!upstream.ok || !upstream.body) {
    const status = upstream.ok ? 502 : upstream.status;
    return new Response(
      `event: error\ndata: ${JSON.stringify({ message: "Hub events stream unavailable" })}\n\n`,
      {
        status,
        headers: {
          "Content-Type": "text/event-stream; charset=utf-8",
          "Cache-Control": "no-cache, no-transform",
        },
      },
    );
  }

  return new Response(upstream.body, {
    status: 200,
    headers: {
      "Content-Type": "text/event-stream; charset=utf-8",
      "Cache-Control": "no-cache, no-transform",
      Connection: "keep-alive",
      "X-Accel-Buffering": "no",
    },
  });
}
