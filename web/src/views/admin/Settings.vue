<template>
  <div v-if="loaded" class="stack">
    <div class="page-head"><h2>系统设置</h2></div>

    <el-form label-position="top" style="max-width: 560px" @submit.prevent="save">
      <el-card shadow="never">
        <template #header>基本</template>
        <el-form-item label="外部访问地址">
          <el-input v-model="cfg.public_url" :placeholder="placeholders.publicUrl" />
          <div class="muted">用于生成 GitHub 回调地址；留空则按请求的 Host 推断（反向代理需传递 X-Forwarded-Host/Proto）。</div>
        </el-form-item>
        <el-form-item label="出站代理">
          <el-input v-model="cfg.proxy" :placeholder="placeholders.proxy" />
          <div class="muted">所有渠道的 HTTP 请求（含 APNs、GitHub）都经过此代理，邮件除外。留空不使用。</div>
        </el-form-item>
      </el-card>

      <el-card shadow="never" style="margin-top: 16px">
        <template #header>GitHub 登录</template>
        <el-form-item>
          <el-switch v-model="cfg.github_enabled" active-text="启用" />
        </el-form-item>
        <el-form-item label="Client ID">
          <el-input v-model="cfg.github_client_id" />
        </el-form-item>
        <el-form-item label="Client Secret">
          <el-input
            v-model="cfg.github_client_secret"
            type="password"
            show-password
            autocomplete="off"
            :placeholder="cfg.github_client_secret_set ? '已设置，留空保持不变' : ''"
          />
        </el-form-item>
        <el-form-item>
          <el-checkbox v-model="cfg.github_allow_signup">允许未绑定的 GitHub 用户自动注册（普通用户权限）</el-checkbox>
        </el-form-item>
        <div class="muted">
          在 GitHub → Settings → Developer settings → OAuth Apps 创建应用，回调地址填：
          <div class="mono callback">{{ cfg.github_callback_url }}</div>
          GitHub 账号只按 ID 关联用户。已有账号请先用密码登录，再到「账号」页绑定。
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
  github_enabled: false,
  github_client_id: '',
  github_client_secret: '',
  github_client_secret_set: false,
  github_allow_signup: false,
  github_callback_url: '',
  proxy: '',
  public_url: '',
})
// Kept out of the template: a `//` inside a template string trips vue-tsc 3.3's type check.
const placeholders = {
  publicUrl: 'https://inotify.example.com',
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

<style scoped>
.callback {
  margin: 6px 0;
}
</style>
