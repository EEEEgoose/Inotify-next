<template>
  <div v-if="session.user" class="stack">
    <div class="page-head"><h2>账号</h2></div>

    <el-card shadow="never">
      <template #header>{{ session.user.has_password ? '修改密码' : '设置密码' }}</template>
      <el-form label-position="top" style="max-width: 360px" @submit.prevent="changePassword">
        <el-form-item v-if="session.user.has_password" label="原密码">
          <el-input v-model="pw.old" type="password" show-password autocomplete="current-password" />
        </el-form-item>
        <el-form-item label="新密码（至少 8 位）">
          <el-input v-model="pw.new" type="password" show-password autocomplete="new-password" />
        </el-form-item>
        <el-button type="primary" native-type="submit" :loading="saving">保存</el-button>
      </el-form>
    </el-card>
  </div>
</template>

<script setup lang="ts">
import { reactive, ref } from 'vue'
import { ElMessage } from 'element-plus'
import { api, setSession } from '../api'
import { session } from '../session'

const pw = reactive({ old: '', new: '' })
const saving = ref(false)

async function changePassword() {
  saving.value = true
  try {
    // Other sessions end; this one continues with the new token.
    setSession((await api.changePassword(pw.old, pw.new)).token)
    Object.assign(pw, { old: '', new: '' })
    session.user = await api.me()
    ElMessage.success('密码已更新')
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    saving.value = false
  }
}
</script>
