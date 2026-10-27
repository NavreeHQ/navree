<script setup lang="ts">
/**
 * The Navree mark: a hull in profile with its sheer rising to the bow, cut by
 * the waterline, over a swept fin. The same paths as navree.com's logo.
 */
defineProps<{ title?: string }>();

const paths = [
  "M1.5 9.5 L30.5 7 C29.6 9.4 28.6 11.3 27.4 12.7 L3.4 12.7 C2.6 11.8 2 10.8 1.5 9.5Z",
  "M4.4 14.5 L26 14.5 C24.4 16.5 22.4 17.8 19.5 18.3 L10 18.3 C7.6 17.8 5.8 16.5 4.4 14.5Z",
  "M13 18.3H19.4L16.4 26H14.4Z",
];
</script>

<template>
  <svg
    viewBox="0 0 32 32"
    fill="currentColor"
    :role="title ? 'img' : undefined"
    :aria-hidden="title ? undefined : true"
    :aria-label="title"
  >
    <path v-for="d in paths" :key="d" :d="d" />
  </svg>
</template>
