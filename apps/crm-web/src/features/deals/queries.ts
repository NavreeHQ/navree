"use client";

import { keepPreviousData, useMutation, useQuery, useQueryClient, type QueryKey } from "@tanstack/react-query";
import { api, unwrap } from "@/lib/api/client";
import { keys } from "@/lib/api/keys";
import type { DealBoard, DealInput, DealListQuery, DealMoveInput, DealUpdateInput } from "@/lib/api/types";
import { stageProbability } from "@/lib/api/types";

/** `filter` (stage, owner_id, company_id), `search`, `sort`, `page`, `per_page`: sent as `filter[stage]=won`. */
export type DealListParams = DealListQuery;

export function useDeals(params: DealListParams, enabled = true) {
  return useQuery({
    queryKey: keys.deals.list(params),
    queryFn: () => unwrap(api.GET("/api/v1/deals", { params: { query: params } })),
    placeholderData: keepPreviousData,
    enabled,
  });
}

export function useDealBoard(params: { owner_id?: string }) {
  return useQuery({
    queryKey: keys.deals.board(params),
    queryFn: () => unwrap(api.GET("/api/v1/deals/board", { params: { query: params } })),
    placeholderData: keepPreviousData,
  });
}

export function useDeal(id: string) {
  return useQuery({
    queryKey: keys.deals.detail(id),
    queryFn: () => unwrap(api.GET("/api/v1/deals/{id}", { params: { path: { id } } })),
  });
}

function invalidateDealData(queryClient: ReturnType<typeof useQueryClient>) {
  void queryClient.invalidateQueries({ queryKey: keys.deals.all });
  void queryClient.invalidateQueries({ queryKey: keys.dashboard });
  void queryClient.invalidateQueries({ queryKey: keys.companies.all });
}

export function useCreateDeal() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: DealInput) => unwrap(api.POST("/api/v1/deals", { body })),
    onSuccess: () => invalidateDealData(queryClient),
  });
}

export function useUpdateDeal(id: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: DealUpdateInput) => unwrap(api.PATCH("/api/v1/deals/{id}", { params: { path: { id } }, body })),
    onSuccess: (deal) => {
      queryClient.setQueryData(keys.deals.detail(id), deal);
      invalidateDealData(queryClient);
    },
  });
}

export function useDeleteDeal() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => unwrap(api.DELETE("/api/v1/deals/{id}", { params: { path: { id } } })),
    onSuccess: () => invalidateDealData(queryClient),
  });
}

/** Moves a deal card inside a cached board: removes it, inserts it at `position` in `stage`, recomputes totals. */
export function moveOnBoard(board: DealBoard, dealId: string, move: DealMoveInput): DealBoard {
  const deal = board.stages.flatMap((s) => s.deals).find((d) => d.id === dealId);
  if (!deal) return board;
  const moved = {
    ...deal,
    stage: move.stage,
    probability: stageProbability(move.stage),
    lost_reason: move.stage === "lost" ? (move.lost_reason ?? null) : null,
  };
  return {
    stages: board.stages.map((column) => {
      const deals = column.deals.filter((d) => d.id !== dealId);
      if (column.stage === move.stage) {
        const index = Math.min(move.position ?? deals.length, deals.length);
        deals.splice(index, 0, moved);
      }
      const positioned = deals.map((d, position) => ({ ...d, position }));
      return {
        ...column,
        deals: positioned,
        count: positioned.length,
        amount_cents: positioned.reduce((sum, d) => sum + d.amount_cents, 0),
      };
    }),
  };
}

/**
 * `POST /api/v1/deals/{id}/move`, optimistically applied to every cached board
 * so the card lands immediately; rolled back if the API refuses.
 */
export function useMoveDeal() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, ...body }: DealMoveInput & { id: string }) =>
      unwrap(api.POST("/api/v1/deals/{id}/move", { params: { path: { id } }, body })),
    onMutate: async ({ id, ...move }) => {
      await queryClient.cancelQueries({ queryKey: [...keys.deals.all, "board"] });
      const snapshots = queryClient.getQueriesData<DealBoard>({ queryKey: [...keys.deals.all, "board"] });
      for (const [key, board] of snapshots) {
        if (board) queryClient.setQueryData<DealBoard>(key, moveOnBoard(board, id, move));
      }
      return { snapshots };
    },
    onError: (_error, _vars, context) => {
      for (const [key, board] of (context?.snapshots ?? []) as [QueryKey, DealBoard | undefined][]) {
        queryClient.setQueryData(key, board);
      }
    },
    onSuccess: (deal) => {
      queryClient.setQueryData(keys.deals.detail(deal.id), deal);
    },
    onSettled: () => invalidateDealData(queryClient),
  });
}
