<script setup lang="ts">
import { useField } from "vee-validate";
import type { HTMLAttributes } from "vue";
import { Field, FieldDescription, FieldError, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";

/** A labelled text input bound to the surrounding VeeValidate form, with client and server errors. */
const props = withDefaults(
  defineProps<{
    name: string;
    label: string;
    type?: string;
    placeholder?: string;
    description?: string;
    autofocus?: boolean;
    autocomplete?: string;
    multiline?: boolean;
    required?: boolean;
    inputmode?: HTMLAttributes["inputmode"];
    list?: string;
    inputClass?: HTMLAttributes["class"];
  }>(),
  { type: "text" },
);

const { value, errorMessage, handleBlur } = useField<string>(() => props.name);
const id = computed(() => `field-${props.name}`);
const describedBy = computed(() => (props.description ? `${id.value}-description` : undefined));
</script>

<template>
  <Field :data-invalid="errorMessage ? true : undefined">
    <FieldLabel :for="id">
      {{ label }}
      <span v-if="required" class="font-normal text-fg-3">(required)</span>
    </FieldLabel>
    <Textarea
      v-if="multiline"
      :id="id"
      v-model="value"
      :placeholder="placeholder"
      :aria-invalid="errorMessage ? true : undefined"
      :aria-describedby="describedBy"
      :class="inputClass"
      @blur="handleBlur"
    />
    <Input
      v-else
      :id="id"
      v-model="value"
      :type="type"
      :placeholder="placeholder"
      :autofocus="autofocus"
      :autocomplete="autocomplete"
      :inputmode="inputmode"
      :list="list"
      :aria-invalid="errorMessage ? true : undefined"
      :aria-describedby="describedBy"
      :class="inputClass"
      @blur="handleBlur"
    />
    <FieldDescription v-if="description" :id="describedBy">{{ description }}</FieldDescription>
    <FieldError v-if="errorMessage">{{ errorMessage }}</FieldError>
  </Field>
</template>
