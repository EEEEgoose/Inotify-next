<template>
  <div>
    <div class="page-head">
      <h2>消息渠道</h2>
      <div>
        <el-button :icon="Download" :disabled="channels.length === 0" @click="openBackup('export')">导出</el-button>
        <el-button :icon="Upload" @click="openBackup('import')">导入</el-button>
        <el-button type="primary" :icon="Plus" @click="openCreate">添加渠道</el-button>
      </div>
    </div>

    <el-alert v-if="!loading && channels.length === 0" type="info" :closable="false" show-icon>
      还没有渠道。点「添加渠道」配置企业微信、钉钉、Telegram 等；iPhone 用户可在
      <router-link to="/send">发送接口</router-link> 页把 Bark App 连接到本服务。
    </el-alert>

    <el-table v-else v-loading="loading" :data="channels" row-key="id">
      <el-table-column type="expand">
        <template #default="{ row }">
          <div class="expand">
            <div class="muted">渠道 Key：用它代替 send token 发送，消息只会发到这个渠道</div>
            <div class="key-line">
              <span class="mono">{{ row.key }}</span>
              <el-button link type="primary" @click="copyText(row.key)">复制</el-button>
            </div>
          </div>
        </template>
      </el-table-column>
      <el-table-column label="名称" prop="name" min-width="140" />
      <el-table-column label="类型" min-width="120">
        <template #default="{ row }">{{ kindName(row.kind) }}</template>
      </el-table-column>
      <el-table-column label="启用" width="80">
        <template #default="{ row }">
          <el-switch :model-value="row.active" @change="(v: string | number | boolean) => toggle(row, Boolean(v))" />
        </template>
      </el-table-column>
      <el-table-column label="操作" width="150" align="right" fixed="right">
        <template #default="{ row }">
          <el-button link type="primary" :loading="testing === row.id" @click="test(row)">测试</el-button>
          <el-button link type="primary" @click="openEdit(row)">编辑</el-button>
          <el-popconfirm title="确定删除这个渠道？" @confirm="remove(row)">
            <template #reference>
              <el-button link type="danger">删除</el-button>
            </template>
          </el-popconfirm>
        </template>
      </el-table-column>
    </el-table>

    <el-dialog v-model="dialog.open" :title="dialog.id ? '编辑渠道' : '添加渠道'" width="min(560px, 94vw)">
      <el-form label-position="top">
        <el-form-item v-if="!dialog.id" label="类型" required>
          <el-select v-model="dialog.kind" style="width: 100%" @change="resetConfig">
            <el-option v-for="k in creatableKinds" :key="k.kind" :label="k.name" :value="k.kind" />
          </el-select>
        </el-form-item>
        <el-form-item label="名称">
          <el-input v-model="dialog.name" :placeholder="currentKind?.name" />
        </el-form-item>
      </el-form>
      <channel-form v-if="currentKind" v-model="dialog.config" :fields="currentKind.fields" />
      <template #footer>
        <el-button @click="dialog.open = false">取消</el-button>
        <el-button type="primary" :loading="saving" @click="save">保存</el-button>
      </template>
    </el-dialog>

    <el-dialog
      v-model="backup.open"
      :title="backup.mode === 'export' ? '导出渠道' : '导入渠道'"
      width="min(480px, 94vw)"
    >
      <el-form label-position="top" @submit.prevent>
        <el-form-item v-if="backup.mode === 'import'" label="备份文件" required>
          <input ref="fileInput" type="file" accept=".json,application/json" @change="pickFile" />
        </el-form-item>
        <el-form-item label="备份密码" required>
          <el-input v-model="backup.password" type="password" show-password autocomplete="new-password" />
          <div v-if="backup.mode === 'export'" class="muted">
            导出文件包含渠道的全部配置（含密钥），用此密码加密，至少 8 位。忘记密码将无法恢复。
          </div>
          <div v-else class="muted">已有相同 Key 的渠道会跳过；Key 被占用时生成新 Key（旧的推送地址需更新）。</div>
        </el-form-item>
        <el-form-item v-if="backup.mode === 'export'" label="确认密码" required>
          <el-input v-model="backup.confirm" type="password" show-password autocomplete="new-password" />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="backup.open = false">取消</el-button>
        <el-button type="primary" :loading="backup.busy" @click="runBackup">
          {{ backup.mode === 'export' ? '导出' : '导入' }}
        </el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Download, Plus, Upload } from '@element-plus/icons-vue'
import { api, type Channel, type ConfigValue, type KindInfo } from '../api'
import { copy } from '../session'
import ChannelForm from '../components/ChannelForm.vue'

const channels = ref<Channel[]>([])
const kinds = ref<KindInfo[]>([])
const loading = ref(true)
const saving = ref(false)
const testing = ref<number | null>(null)
const dialog = reactive({
  open: false,
  id: null as number | null,
  kind: '',
  name: '',
  config: {} as Record<string, ConfigValue>,
})

const creatableKinds = computed(() => kinds.value.filter((k) => k.user_creatable))
const currentKind = computed(() => kinds.value.find((k) => k.kind === dialog.kind))
const kindName = (kind: string) => kinds.value.find((k) => k.kind === kind)?.name ?? kind

async function load() {
  loading.value = true
  try {
    ;[channels.value, kinds.value] = await Promise.all([api.channels(), api.kinds()])
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    loading.value = false
  }
}
onMounted(load)

function resetConfig() {
  const config: Record<string, ConfigValue> = {}
  for (const f of currentKind.value?.fields ?? []) {
    config[f.name] = f.default ?? (f.kind === 'bool' ? false : '')
  }
  dialog.config = config
}

function openCreate() {
  Object.assign(dialog, { open: true, id: null, kind: creatableKinds.value[0]?.kind ?? '', name: '' })
  resetConfig()
}

function openEdit(c: Channel) {
  Object.assign(dialog, { open: true, id: c.id, kind: c.kind, name: c.name, config: { ...c.config } })
}

async function save() {
  saving.value = true
  try {
    if (dialog.id) {
      await api.updateChannel(dialog.id, { name: dialog.name, config: dialog.config })
    } else {
      await api.createChannel({ kind: dialog.kind, name: dialog.name, config: dialog.config, active: true })
    }
    dialog.open = false
    ElMessage.success('已保存')
    await load()
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    saving.value = false
  }
}

async function toggle(c: Channel, active: boolean) {
  try {
    Object.assign(c, await api.updateChannel(c.id, { active }))
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

async function test(c: Channel) {
  testing.value = c.id
  try {
    const result = await api.testChannel(c.id)
    if (result.ok) ElMessage.success('测试消息已发送')
    else ElMessageBox.alert(result.error ?? '未知错误', '发送失败', { type: 'error' })
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    testing.value = null
  }
}

async function remove(c: Channel) {
  try {
    await api.deleteChannel(c.id)
    channels.value = channels.value.filter((x) => x.id !== c.id)
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

const fileInput = ref<HTMLInputElement>()
const backup = reactive({
  open: false,
  mode: 'export' as 'export' | 'import',
  password: '',
  confirm: '',
  file: null as File | null,
  busy: false,
})

function openBackup(mode: 'export' | 'import') {
  Object.assign(backup, { open: true, mode, password: '', confirm: '', file: null })
  if (fileInput.value) fileInput.value.value = ''
}

function pickFile(e: Event) {
  backup.file = (e.target as HTMLInputElement).files?.[0] ?? null
}

async function runBackup() {
  if (backup.mode === 'export') {
    if (backup.password.length < 8) return ElMessage.warning('备份密码至少 8 位')
    if (backup.password !== backup.confirm) return ElMessage.warning('两次输入的密码不一致')
  } else {
    if (!backup.file) return ElMessage.warning('请选择备份文件')
    if (!backup.password) return ElMessage.warning('请输入备份密码')
  }
  backup.busy = true
  try {
    if (backup.mode === 'export') {
      const data = await api.exportChannels(backup.password)
      const url = URL.createObjectURL(new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' }))
      const a = document.createElement('a')
      a.href = url
      a.download = `inotify-channels-${new Date().toISOString().slice(0, 10)}.json`
      a.click()
      URL.revokeObjectURL(url)
      ElMessage.success('已导出')
    } else {
      let file: unknown
      try {
        file = JSON.parse(await backup.file!.text())
      } catch {
        return ElMessage.error('不是有效的备份文件')
      }
      const r = await api.importChannels(backup.password, file)
      ElMessage.success(
        `导入 ${r.imported} 个，跳过 ${r.skipped} 个` + (r.rekeyed ? `，其中 ${r.rekeyed} 个生成了新 Key` : ''),
      )
      await load()
    }
    backup.open = false
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    backup.busy = false
  }
}

async function copyText(text: string) {
  await copy(text)
  ElMessage.success('已复制')
}
</script>

<style scoped>
.expand {
  padding: 4px 16px;
}
.key-line {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 4px;
}
</style>
