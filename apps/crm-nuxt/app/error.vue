<script setup lang="ts">
import type { NuxtError } from "#app";
import NavreeMark from "@/components/brand/NavreeMark.vue";
import { Button } from "@/components/ui/button";

const props = defineProps<{ error: NuxtError }>();
const notFound = computed(() => props.error.statusCode === 404);
useHead({ title: notFound.value ? "Not found" : "Something went wrong" });
</script>

<template>
  <main class="flex min-h-dvh flex-col items-center justify-center gap-6 px-6 text-center">
    <NavreeMark class="size-8 text-accent" />
    <p class="font-mono text-xs text-fg-3">Error {{ error.statusCode }}</p>
    <h1 class="font-sans text-2xl leading-8 font-semibold tracking-[-0.02em] text-fg">
      {{ notFound ? "This page isn't here" : "Something went wrong" }}
    </h1>
    <p class="max-w-md text-sm text-fg-2">
      {{ notFound ? "The link may be old, or the address has a typo." : "Reload the page to try again. If it keeps happening, the API may be down." }}
    </p>
    <Button @click="clearError({ redirect: '/' })">Back to the dashboard</Button>
  </main>
</template>
