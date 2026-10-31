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

    <el-card shadow="never">
      <template #header>GitHub</template>
      <template v-if="session.user.github_login">
        <p>已绑定 <strong>{{ session.user.github_login }}</strong>，可以用 GitHub 登录。</p>
        <el-button :disabled="!session.user.has_password" @click="unlink">解绑</el-button>
        <span v-if="!session.user.has_password" class="muted">　先设置密码才能解绑</span>
      </template>
      <template v-else>
        <p class="muted">绑定后可以用 GitHub 登录这个账号。</p>
        <el-button @click="link">绑定 GitHub</el-button>
      </template>
    </el-card>
  </div>
</template>

<script setup lang="ts">
import { onMounted, reactive, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { ElMessage } from 'element-plus'
import { api } from '../api'
import { session } from '../session'

const route = useRoute()
const router = useRouter()
const pw = reactive({ old: '', new: '' })
const saving = ref(false)

onMounted(async () => {
  if (route.query.github === 'linked') {
    ElMessage.success('GitHub 已绑定')
    session.user = await api.me()
    router.replace({ query: {} })
  }
})

async function changePassword() {
  saving.value = true
  try {
    await api.changePassword(pw.old, pw.new)
    Object.assign(pw, { old: '', new: '' })
    session.user = await api.me()
    ElMessage.success('密码已更新')
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    saving.value = false
  }
}

async function link() {
  try {
    window.location.href = (await api.linkGithub()).url
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

async function unlink() {
  try {
    session.user = await api.unlinkGithub()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}
</script>
