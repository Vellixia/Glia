import { GraphQLClient } from "graphql-request";

function getEndpoint(): string {
  if (typeof window !== "undefined") {
    return "/api/graphql";
  }
  return process.env.HUB_URL ?? process.env.NEXT_PUBLIC_HUB_URL ?? "http://127.0.0.1:3000";
}

export const graphqlClient = new GraphQLClient(getEndpoint());
