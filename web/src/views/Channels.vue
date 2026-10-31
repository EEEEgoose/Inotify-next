<template>
  <div>
    <div class="page-head">
      <h2>消息渠道</h2>
      <el-button type="primary" :icon="Plus" @click="openCreate">添加渠道</el-button>
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
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Plus } from '@element-plus/icons-vue'
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
