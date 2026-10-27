<script setup lang="ts">
import "vue-sonner/style.css";
import { Toaster } from "@/components/ui/sonner";
import { TooltipProvider } from "@/components/ui/tooltip";

useHead({ titleTemplate: (title) => (title ? `${title} | Navree CRM` : "Navree CRM") });
</script>

<template>
  <TooltipProvider :delay-duration="300">
    <NuxtLayout>
      <NuxtPage />
    </NuxtLayout>
    <Toaster position="bottom-right" close-button :theme="'dark'" />
  </TooltipProvider>
</template>
