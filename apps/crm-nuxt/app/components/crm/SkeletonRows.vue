<script setup lang="ts">
import { Skeleton } from "@/components/ui/skeleton";
import { TableCell, TableRow } from "@/components/ui/table";

/** Placeholder rows while a table loads; widths vary so it reads as data, not stripes. */
withDefaults(defineProps<{ columns: number; rows?: number }>(), { rows: 8 });
const widths = ["w-40", "w-24", "w-28", "w-20", "w-24", "w-16", "w-20"];
</script>

<template>
  <TableRow v-for="r in rows" :key="r" class="hover:bg-transparent" aria-hidden="true">
    <TableCell v-for="c in columns" :key="c" class="py-3.5" :class="c === 1 ? 'pl-4' : ''">
      <Skeleton class="h-3.5" :class="widths[(c + r) % widths.length]" />
    </TableCell>
  </TableRow>
</template>
