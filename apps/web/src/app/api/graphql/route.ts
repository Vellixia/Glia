import { NextRequest, NextResponse } from "next/server";
import { auth } from "@/auth";

const HUB_URL = process.env.HUB_URL ?? "http://127.0.0.1:3000";

export async function POST(request: NextRequest) {
  const session = await auth();
  const body = await request.json();

  const headers: Record<string, string> = {
    "Content-Type": "application/json",
  };

  if (session?.hubToken) {
    headers["Authorization"] = `Bearer ${session.hubToken}`;
  }

  try {
    const res = await fetch(`${HUB_URL}/graphql`, {
      method: "POST",
      headers,
      body: JSON.stringify(body),
    });

    const data = await res.json();

    // Hub async-graphql always returns 200, check error extensions for auth failures
    if (data?.errors?.some((e: any) => e?.extensions?.code === "UNAUTHENTICATED")) {
      return NextResponse.json(data, { status: 401 });
    }

    return NextResponse.json(data, { status: res.status });
  } catch {
    return NextResponse.json(
      { errors: [{ message: "Hub unreachable", extensions: { code: "DOWNSTREAM_ERROR" } }] },
      { status: 502 },
    );
  }
}
