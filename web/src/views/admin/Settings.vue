<template>
  <div v-if="loaded" class="stack">
    <div class="page-head"><h2>系统设置</h2></div>

    <el-form label-position="top" style="max-width: 560px" @submit.prevent="save">
      <el-card shadow="never">
        <template #header>基本</template>
        <el-form-item label="出站代理">
          <el-input v-model="cfg.proxy" :placeholder="placeholders.proxy" />
          <div class="muted">
            Telegram、企业微信、APNs、Turnstile 的请求经过此代理。Webhook、钉钉、飞书等用户填写的地址直连，
            防止经代理绕过内网访问限制（设置了 INOTIFY_ALLOW_PRIVATE_TARGETS 时也走代理）。邮件不走代理。留空不使用。
          </div>
        </el-form-item>
      </el-card>

      <el-card shadow="never" style="margin-top: 16px">
        <template #header>登录人机验证（Cloudflare Turnstile）</template>
        <el-form-item>
          <el-switch v-model="cfg.turnstile_enabled" active-text="启用" />
        </el-form-item>
        <el-form-item label="Site Key">
          <el-input v-model="cfg.turnstile_site_key" />
        </el-form-item>
        <el-form-item label="Secret Key">
          <el-input
            v-model="cfg.turnstile_secret_key"
            type="password"
            show-password
            autocomplete="off"
            :placeholder="cfg.turnstile_secret_key_set ? '已设置，留空保持不变' : ''"
          />
        </el-form-item>
        <div class="muted">
          在 Cloudflare 控制台 → Turnstile 添加站点（域名填本站域名）获取密钥。启用后密码登录需先通过验证。
          若密钥填错或无法访问 Cloudflare 导致无法登录，可设置环境变量 INOTIFY_DISABLE_TURNSTILE=1 重启后临时跳过验证。
        </div>
      </el-card>

      <el-button type="primary" native-type="submit" :loading="saving" style="margin-top: 16px">保存</el-button>
    </el-form>
  </div>
</template>

<script setup lang="ts">
import { onMounted, reactive, ref } from 'vue'
import { ElMessage } from 'element-plus'
import { api, type Settings } from '../../api'

const cfg = reactive<Settings>({
  proxy: '',
  turnstile_enabled: false,
  turnstile_site_key: '',
  turnstile_secret_key: '',
  turnstile_secret_key_set: false,
})
// Kept out of the template: a `//` inside a template string trips vue-tsc 3.3's type check.
const placeholders = {
  proxy: 'http://127.0.0.1:7890 或 socks5://127.0.0.1:1080',
}
const loaded = ref(false)
const saving = ref(false)

onMounted(async () => {
  try {
    Object.assign(cfg, await api.settings())
    loaded.value = true
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
})

async function save() {
  saving.value = true
  try {
    Object.assign(cfg, await api.saveSettings(cfg))
    ElMessage.success('已保存')
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    saving.value = false
  }
}
</script>
