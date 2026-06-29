"use client";

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { graphqlClient } from "@/lib/graphql-client";
import { gql } from "graphql-request";
import type { TypedDocumentNode } from "@graphql-typed-document-node/core";
import { print } from "graphql";

export function useGqlQuery<TData, TVariables extends Record<string, unknown> = Record<string, never>>(
  document: TypedDocumentNode<TData, TVariables>,
  variables?: TVariables,
  options?: { enabled?: boolean }
) {
  const queryKey = [print(document), variables] as const;

  return useQuery<TData>({
    queryKey,
    queryFn: () => graphqlClient.request<TData>(document as any, variables),
    enabled: options?.enabled ?? true,
  });
}

export function useGqlMutation<TData, TVariables extends Record<string, unknown> = Record<string, never>>(
  document: TypedDocumentNode<TData, TVariables>
) {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (variables: TVariables) =>
      graphqlClient.request<TData>(document as any, variables),
    onSuccess: () => {
      queryClient.invalidateQueries();
    },
  });
}
