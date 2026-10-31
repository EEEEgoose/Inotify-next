<template>
  <div>
    <div ref="container" />
    <div v-if="error" class="error">{{ error }}</div>
  </div>
</template>

<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { loadTurnstile } from '../turnstile'

const props = defineProps<{ siteKey: string }>()
// An empty string means there is no valid token (not solved yet, expired or failed).
const emit = defineEmits<{ token: [token: string] }>()

const container = ref<HTMLElement>()
const error = ref('')
let api: Turnstile | undefined
let widgetId: string | undefined

onMounted(async () => {
  try {
    api = await loadTurnstile()
  } catch (e) {
    error.value = (e as Error).message
    return
  }
  if (!container.value) return
  widgetId = api.render(container.value, {
    sitekey: props.siteKey,
    theme: 'auto',
    language: 'zh-cn',
    callback: (token: string) => emit('token', token),
    'expired-callback': () => emit('token', ''),
    'error-callback': () => emit('token', ''),
  })
})

onBeforeUnmount(() => {
  if (api && widgetId) api.remove(widgetId)
})

/** Tokens are single-use: get a new one after each attempt. */
function reset() {
  emit('token', '')
  if (api && widgetId) api.reset(widgetId)
}

defineExpose({ reset })
</script>

<style scoped>
.error {
  color: var(--el-color-danger);
  font-size: 13px;
}
</style>
