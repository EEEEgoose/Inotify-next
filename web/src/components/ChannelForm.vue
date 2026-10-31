<template>
  <el-form label-position="top">
    <el-form-item v-for="f in fields" :key="f.name" :label="f.label" :required="f.required && !f.readonly">
      <el-switch v-if="f.kind === 'bool'" v-model="model[f.name] as boolean" :disabled="f.readonly" />
      <el-select
        v-else-if="f.kind === 'select'"
        v-model="model[f.name] as string"
        :disabled="f.readonly"
        style="width: 100%"
      >
        <el-option v-for="o in f.options" :key="o" :label="o === '' ? '（默认）' : o" :value="o" />
      </el-select>
      <el-input
        v-else
        v-model="model[f.name] as string"
        :type="f.kind === 'textarea' ? 'textarea' : f.kind === 'secret' ? 'password' : 'text'"
        :show-password="f.kind === 'secret'"
        :autosize="f.kind === 'textarea' ? { minRows: 3, maxRows: 10 } : undefined"
        :readonly="f.readonly"
        :class="{ mono: f.kind === 'textarea' || f.readonly }"
      />
      <div v-if="f.help" class="muted">{{ f.help }}</div>
    </el-form-item>
  </el-form>
</template>

<script setup lang="ts">
import type { ConfigValue, Field } from '../api'

defineProps<{ fields: Field[] }>()
const model = defineModel<Record<string, ConfigValue>>({ required: true })
</script>

