<template>
  <div v-if="session.user" class="stack">
    <div class="page-head"><h2>发送接口</h2></div>

    <el-card shadow="never">
      <template #header>Send Token</template>
      <div class="token-line">
        <span class="mono">{{ token }}</span>
        <el-button link type="primary" @click="copyText(token)">复制</el-button>
        <el-popconfirm title="重置后旧 token 立即失效，Bark App 也需重新添加服务器。继续？" width="260" @confirm="reset">
          <template #reference>
            <el-button link type="danger">重置</el-button>
          </template>
        </el-popconfirm>
      </div>
      <p class="muted">用它发送的消息会发到你所有已启用的渠道；换成某个渠道的 Key 则只发到那个渠道。</p>
    </el-card>

    <el-card shadow="never">
      <template #header>调用示例</template>
      <el-tabs>
        <el-tab-pane label="POST（推荐）">
          <pre class="code">{{ postExample }}</pre>
        </el-tab-pane>
        <el-tab-pane label="GET">
          <pre class="code">{{ getExample }}</pre>
          <p class="muted">标题和内容需要 URL 编码。与 Inotify 2.x 的 URL 格式兼容，旧脚本只需换成新 token。</p>
        </el-tab-pane>
        <el-tab-pane label="Bark 格式">
          <pre class="code">{{ barkExample }}</pre>
          <p class="muted">支持 Bark 推送 API 的工具（Home Assistant、青龙等）可把服务器填成上面的地址，Key 填 send token。</p>
        </el-tab-pane>
      </el-tabs>
    </el-card>

    <el-card shadow="never">
      <template #header>连接 Bark App（iPhone）</template>
      <div class="bark">
        <canvas ref="qr" class="qr" />
        <div>
          <p>在 Bark App 的「服务器」页添加下面的地址（或扫码），App 会自动注册成一个渠道：</p>
          <div class="token-line">
            <span class="mono">{{ barkServer }}</span>
            <el-button link type="primary" @click="copyText(barkServer)">复制</el-button>
          </div>
          <p class="muted">iOS 要求 HTTPS 地址，请通过反向代理开启 HTTPS。</p>
        </div>
      </div>
    </el-card>

    <el-card shadow="never">
      <template #header>发送测试</template>
      <el-form label-position="top" @submit.prevent="sendTest">
        <el-form-item label="标题"><el-input v-model="test.title" /></el-form-item>
        <el-form-item label="内容"><el-input v-model="test.body" type="textarea" :rows="3" /></el-form-item>
        <el-button type="primary" native-type="submit" :loading="sending">发送到所有渠道</el-button>
      </el-form>
    </el-card>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onMounted, reactive, ref, watch } from 'vue'
import { ElMessage } from 'element-plus'
import QRCode from 'qrcode'
import { api } from '../api'
import { copy, session } from '../session'

const origin = window.location.origin
const token = computed(() => session.user?.send_token ?? '')
const barkServer = computed(() => `${origin}/bark/${token.value}`)
const postExample = computed(
  () => `curl -X POST ${origin}/api/v1/send \\
  -H 'Authorization: Bearer ${token.value}' \\
  -H 'Content-Type: application/json' \\
  -d '{"title": "标题", "body": "内容"}'`,
)
const getExample = computed(
  () => `${origin}/${token.value}.send/标题/内容
${origin}/api/send?token=${token.value}&title=标题&data=内容`,
)
const barkExample = computed(
  () => `${barkServer.value}/${token.value}/标题/内容?group=ops&url=https://example.com`,
)

const qr = ref<HTMLCanvasElement>()
async function drawQr() {
  await nextTick()
  if (qr.value && token.value) await QRCode.toCanvas(qr.value, barkServer.value, { width: 160, margin: 1 })
}
onMounted(drawQr)
watch(token, drawQr)

async function reset() {
  try {
    session.user = await api.resetSendToken()
    ElMessage.success('已重置')
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

const test = reactive({ title: 'Inotify', body: '这是一条测试消息' })
const sending = ref(false)
async function sendTest() {
  sending.value = true
  try {
    await api.send(token.value, test)
    ElMessage.success('已加入发送队列')
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    sending.value = false
  }
}

async function copyText(text: string) {
  await copy(text)
  ElMessage.success('已复制')
}
</script>

<style scoped>
.token-line {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.bark {
  display: flex;
  gap: 20px;
  align-items: flex-start;
  flex-wrap: wrap;
}
.bark p {
  margin-top: 0;
}
.qr {
  border-radius: 6px;
  background: #fff;
}
</style>
