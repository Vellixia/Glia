import { auth } from "@/auth";

const HUB_URL = process.env.HUB_URL ?? "http://127.0.0.1:3000";

export async function serverGqlFetch<T>(
  query: string,
  variables?: Record<string, unknown>,
): Promise<T> {
  const session = await auth();

  const headers: Record<string, string> = {
    "Content-Type": "application/json",
  };

  if (session?.hubToken) {
    headers["Authorization"] = `Bearer ${session.hubToken}`;
  }

  const res = await fetch(`${HUB_URL}/graphql`, {
    method: "POST",
    headers,
    body: JSON.stringify({ query, variables }),
  });

  const json = await res.json();

  if (json.errors) {
    throw new Error(json.errors[0]?.message ?? "GraphQL error");
  }

  return json.data as T;
}
