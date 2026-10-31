<template>
  <router-view />
</template>

<script setup lang="ts">
// Follow the system color scheme with Element Plus's dark CSS variables.
const media = window.matchMedia('(prefers-color-scheme: dark)')
const apply = () => document.documentElement.classList.toggle('dark', media.matches)
apply()
media.addEventListener('change', apply)
</script>
