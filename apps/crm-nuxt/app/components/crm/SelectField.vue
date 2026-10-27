<script setup lang="ts">
import { useField } from "vee-validate";
import { Field, FieldError, FieldLabel } from "@/components/ui/field";
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";

const NONE = "__none";

/** A labelled select bound to the surrounding form. An empty string means "none". */
const props = defineProps<{
  name: string;
  label: string;
  options: readonly { value: string; label: string }[];
  placeholder?: string;
  /** Offers an explicit "none" choice, stored as "". */
  noneLabel?: string;
}>();

const { value, errorMessage } = useField<string>(() => props.name);
const id = computed(() => `field-${props.name}`);
const selected = computed({
  get: () => (value.value ? String(value.value) : props.noneLabel ? NONE : undefined),
  set: (v) => {
    value.value = v === NONE || v === undefined ? "" : v;
  },
});
</script>

<template>
  <Field :data-invalid="errorMessage ? true : undefined">
    <FieldLabel :for="id">{{ label }}</FieldLabel>
    <Select v-model="selected">
      <SelectTrigger :id="id" class="w-full" :aria-invalid="errorMessage ? true : undefined">
        <SelectValue :placeholder="placeholder" />
      </SelectTrigger>
      <SelectContent>
        <SelectGroup>
          <SelectItem v-if="noneLabel" :value="NONE">{{ noneLabel }}</SelectItem>
          <SelectItem v-for="o in options" :key="o.value" :value="o.value">{{ o.label }}</SelectItem>
        </SelectGroup>
      </SelectContent>
    </Select>
    <FieldError v-if="errorMessage">{{ errorMessage }}</FieldError>
  </Field>
</template>
