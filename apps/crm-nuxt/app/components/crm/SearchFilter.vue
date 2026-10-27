<script setup lang="ts">
import { Search, X } from "@lucide/vue";
import type { HTMLAttributes } from "vue";
import { InputGroup, InputGroupAddon, InputGroupButton, InputGroupInput } from "@/components/ui/input-group";
import { cn } from "@/lib/utils";

/**
 * A search box bound to a URL parameter: it applies 250 ms after typing stops,
 * or at once on Enter or blur, and follows the URL when something else changes
 * it ("Clear filters", the back button).
 */
const props = defineProps<{ value: string | undefined; placeholder: string; class?: HTMLAttributes["class"] }>();
const emit = defineEmits<{ change: [value: string] }>();

const draft = ref(props.value ?? "");
let timer: ReturnType<typeof setTimeout> | undefined;

watch(
  () => props.value,
  (value) => {
    if ((value ?? "") !== draft.value) draft.value = value ?? "";
  },
);
onBeforeUnmount(() => clearTimeout(timer));

function commit(next: string) {
  clearTimeout(timer);
  if (next === (props.value ?? "")) return;
  emit("change", next);
}

function onInput(value: string | number) {
  draft.value = String(value);
  clearTimeout(timer);
  timer = setTimeout(() => commit(draft.value), 250);
}
</script>

<template>
  <InputGroup :class="cn('h-8 w-full sm:w-64', props.class)">
    <InputGroupAddon>
      <Search class="size-4 text-fg-3" />
    </InputGroupAddon>
    <InputGroupInput
      type="search"
      :aria-label="placeholder"
      :placeholder="placeholder"
      :model-value="draft"
      class="text-[13px]"
      @update:model-value="onInput"
      @blur="commit(draft)"
      @keydown.enter="commit(draft)"
    />
    <InputGroupAddon v-if="draft" align="inline-end">
      <InputGroupButton
        size="icon-xs"
        aria-label="Clear search"
        class="text-fg-3 hover:bg-ink-3 hover:text-fg"
        @click="
          draft = '';
          commit('');
        "
      >
        <X />
      </InputGroupButton>
    </InputGroupAddon>
  </InputGroup>
</template>
