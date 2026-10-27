<script setup lang="ts">
import { Skeleton } from "@/components/ui/skeleton";
</script>

<template>
  <div class="flex flex-col gap-4" aria-busy="true" aria-label="Loading">
    <Skeleton class="h-4 w-24" />
    <Skeleton class="h-9 w-80 max-w-full" />
    <Skeleton class="h-4 w-96 max-w-full" />
    <div class="mt-4 grid gap-6 xl:grid-cols-[minmax(0,1fr)_20rem]">
      <div class="flex flex-col gap-6">
        <Skeleton class="h-20 rounded-lg" />
        <Skeleton class="h-80 rounded-lg" />
      </div>
      <Skeleton class="h-96 rounded-lg" />
    </div>
  </div>
</template>
