<script setup lang="ts">
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Spinner } from "@/components/ui/spinner";
import { cn } from "@/lib/utils";

/** The dialog chrome every create and edit form shares. The trigger goes in the `trigger` slot. */
defineProps<{
  title: string;
  description?: string;
  submitLabel: string;
  pending?: boolean;
  rootError?: string | null;
  wide?: boolean;
}>();
const open = defineModel<boolean>("open", { default: false });
const emit = defineEmits<{ submit: [event: Event] }>();
</script>

<template>
  <Dialog v-model:open="open">
    <DialogTrigger v-if="$slots.trigger" as-child>
      <slot name="trigger" />
    </DialogTrigger>
    <!-- Header and footer stay put; only the fields scroll, so the submit button is always in reach on a phone. -->
    <DialogContent
      :class="cn('flex max-h-[min(90svh,52rem)] flex-col gap-0 overflow-hidden p-0', wide ? 'sm:max-w-2xl' : 'sm:max-w-lg')"
    >
      <form novalidate class="flex min-h-0 flex-1 flex-col" @submit.prevent="emit('submit', $event)">
        <DialogHeader class="shrink-0 gap-1 border-b border-line px-6 pt-5 pb-4 pr-12 text-left">
          <DialogTitle class="font-sans text-lg font-semibold tracking-[-0.01em] text-fg">{{ title }}</DialogTitle>
          <DialogDescription v-if="description" class="text-sm text-fg-2">{{ description }}</DialogDescription>
        </DialogHeader>
        <div class="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto px-6 py-5 scroll-thin">
          <Alert v-if="rootError" variant="destructive">
            <AlertDescription>{{ rootError }}</AlertDescription>
          </Alert>
          <slot />
        </div>
        <DialogFooter class="shrink-0 gap-2 border-t border-line px-6 py-3.5">
          <DialogClose as-child>
            <Button type="button" variant="ghost">Cancel</Button>
          </DialogClose>
          <Button type="submit" :disabled="pending">
            <Spinner v-if="pending" />
            {{ submitLabel }}
          </Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
</template>
