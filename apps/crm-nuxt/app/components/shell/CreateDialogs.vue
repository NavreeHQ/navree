<script setup lang="ts">
import TaskFormDialog from "@/features/activities/TaskFormDialog.vue";
import CompanyFormDialog from "@/features/companies/CompanyFormDialog.vue";
import ContactFormDialog from "@/features/contacts/ContactFormDialog.vue";
import DealFormDialog from "@/features/deals/DealFormDialog.vue";
import { type CreateKind, useUiStore } from "@/stores/ui";

/** The create dialogs the palette, the quick-create menu and the `c` shortcuts open. */
const ui = useUiStore();

function model(kind: CreateKind) {
  return computed({
    get: () => ui.creating === kind,
    set: (open: boolean) => {
      ui.creating = open ? kind : null;
    },
  });
}
const deal = model("deal");
const company = model("company");
const contact = model("contact");
const task = model("task");
</script>

<template>
  <DealFormDialog v-model:open="deal" />
  <CompanyFormDialog v-model:open="company" />
  <ContactFormDialog v-model:open="contact" />
  <TaskFormDialog v-model:open="task" />
</template>
