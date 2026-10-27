<script setup lang="ts">
import { RefreshCw, Unplug } from "@lucide/vue";
import { Button } from "@/components/ui/button";
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";

defineProps<{ detail?: string }>();
const emit = defineEmits<{ retry: [] }>();
</script>

<template>
  <div class="flex min-h-dvh items-center justify-center p-6">
    <Empty class="surface max-w-md border">
      <EmptyHeader>
        <EmptyMedia variant="icon"><Unplug /></EmptyMedia>
        <EmptyTitle>Can't reach the Navree API</EmptyTitle>
        <EmptyDescription>
          {{
            detail ??
            "Start crm-api (pnpm api serves it on 127.0.0.1:8092) or set NAVREE_API_URL, or run the app with NUXT_PUBLIC_API_MOCKS=1 to use demo data."
          }}
        </EmptyDescription>
      </EmptyHeader>
      <EmptyContent>
        <Button variant="outline" @click="emit('retry')">
          <RefreshCw />
          Try again
        </Button>
      </EmptyContent>
    </Empty>
  </div>
</template>
