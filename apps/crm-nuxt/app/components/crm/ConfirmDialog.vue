<script setup lang="ts">
import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";

/** Asks before something destructive. The trigger goes in the `trigger` slot. */
defineProps<{
  title: string;
  description: string;
  confirmLabel: string;
  pending?: boolean;
}>();
const emit = defineEmits<{ confirm: [] }>();
const open = ref(false);
</script>

<template>
  <AlertDialog v-model:open="open">
    <AlertDialogTrigger as-child>
      <slot name="trigger" />
    </AlertDialogTrigger>
    <AlertDialogContent>
      <AlertDialogHeader>
        <AlertDialogTitle class="font-sans text-lg font-semibold tracking-[-0.01em]">{{ title }}</AlertDialogTitle>
        <AlertDialogDescription class="text-sm leading-relaxed text-fg-2">{{ description }}</AlertDialogDescription>
      </AlertDialogHeader>
      <AlertDialogFooter>
        <AlertDialogCancel as-child>
          <Button variant="ghost">Keep it</Button>
        </AlertDialogCancel>
        <Button variant="destructive" :disabled="pending" @click="emit('confirm')">
          <Spinner v-if="pending" />
          {{ confirmLabel }}
        </Button>
      </AlertDialogFooter>
    </AlertDialogContent>
  </AlertDialog>
</template>
