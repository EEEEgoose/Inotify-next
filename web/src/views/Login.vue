<template>
  <div class="login">
    <el-card class="box" shadow="never">
      <h1>Inotify</h1>
      <p class="muted">消息推送网关</p>
      <el-alert v-if="error" :title="error" type="error" show-icon :closable="false" class="alert" />
      <el-form :model="form" label-position="top" @submit.prevent="submit">
        <el-form-item label="用户名">
          <el-input v-model="form.username" autocomplete="username" autofocus />
        </el-form-item>
        <el-form-item label="密码">
          <el-input v-model="form.password" type="password" show-password autocomplete="current-password" />
        </el-form-item>
        <el-button type="primary" native-type="submit" :loading="loading" class="wide">登录</el-button>
      </el-form>
      <template v-if="github">
        <el-divider>或</el-divider>
        <el-button class="wide" tag="a" href="/api/v1/oauth/github/start">使用 GitHub 登录</el-button>
      </template>
    </el-card>
  </div>
</template>

<script setup lang="ts">
import { onMounted, reactive, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { api } from '../api'
import { startSession } from '../session'

const route = useRoute()
const router = useRouter()
const form = reactive({ username: '', password: '' })
const loading = ref(false)
const github = ref(false)
const error = ref(typeof route.query.error === 'string' ? route.query.error : '')

onMounted(async () => {
  github.value = (await api.authOptions().catch(() => ({ github: false }))).github
})

async function submit() {
  loading.value = true
  error.value = ''
  try {
    const { token } = await api.login(form.username, form.password)
    await startSession(token)
    const redirect = typeof route.query.redirect === 'string' ? route.query.redirect : '/channels'
    router.replace(redirect)
  } catch (e) {
    error.value = (e as Error).message
  } finally {
    loading.value = false
  }
}
</script>

<style scoped>
.login {
  min-height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 16px;
  box-sizing: border-box;
}
.box {
  width: 100%;
  max-width: 380px;
}
h1 {
  margin: 0;
  font-size: 26px;
}
.alert {
  margin-bottom: 16px;
}
.wide {
  width: 100%;
}
</style>
