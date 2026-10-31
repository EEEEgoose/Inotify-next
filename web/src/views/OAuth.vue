<template>
  <div class="muted" style="padding: 32px; text-align: center">正在登录…</div>
</template>

<script setup lang="ts">
import { onMounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { startSession } from '../session'

// The server's GitHub callback redirects here with the session token in the URL fragment.
const route = useRoute()
const router = useRouter()

onMounted(async () => {
  const token = typeof route.query.token === 'string' ? route.query.token : ''
  if (token && (await startSession(token))) router.replace('/channels')
  else router.replace({ name: 'login', query: { error: '登录失败' } })
})
</script>
