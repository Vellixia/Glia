import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  output: "standalone",

  /** Proxy /api/graphql → glia-hub inside the Docker compose network.
   *
   *  In docker-compose.yml the `web` service sets HUB_URL to the internal
   *  Docker hostname.  Outside Docker, NEXT_LOCAL_HUB_URL (or the fallback
   *  http://127.0.0.1:3000) is used for local development.
   *
   *  For the browser-side GraphQL client, the generated standalone bundle
   *  uses NEXT_PUBLIC_HUB_URL (baked at build time).  In production the
   *  client should call a relative path instead — see §graphql-client.ts. */
  async rewrites() {
    return [
      {
        source: "/api/graphql",
        destination: `${process.env.HUB_URL ?? process.env.NEXT_LOCAL_HUB_URL ?? "http://127.0.0.1:3000"}/graphql`,
      },
    ];
  },
};

export default nextConfig;
