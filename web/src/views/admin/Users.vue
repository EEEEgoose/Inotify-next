<template>
  <div>
    <div class="page-head">
      <h2>用户管理</h2>
      <el-button type="primary" :icon="Plus" @click="create.open = true">添加用户</el-button>
    </div>

    <el-table v-loading="loading" :data="users">
      <el-table-column label="用户名" min-width="140">
        <template #default="{ row }">
          {{ row.username }}
          <el-tag v-if="row.id === session.user?.id" size="small" type="info">我</el-tag>
        </template>
      </el-table-column>
      <el-table-column label="GitHub" min-width="120">
        <template #default="{ row }">{{ row.github_login ?? '—' }}</template>
      </el-table-column>
      <el-table-column label="管理员" width="90">
        <template #default="{ row }">
          <el-switch
            :model-value="row.is_admin"
            :disabled="row.id === session.user?.id"
            @change="(v: string | number | boolean) => update(row, { is_admin: Boolean(v) })"
          />
        </template>
      </el-table-column>
      <el-table-column label="启用" width="80">
        <template #default="{ row }">
          <el-switch
            :model-value="row.active"
            :disabled="row.id === session.user?.id"
            @change="(v: string | number | boolean) => update(row, { active: Boolean(v) })"
          />
        </template>
      </el-table-column>
      <el-table-column label="创建时间" prop="created_at" min-width="160" />
      <el-table-column label="操作" width="160" align="right" fixed="right">
        <template #default="{ row }">
          <el-button link type="primary" @click="resetPassword(row)">重置密码</el-button>
          <el-popconfirm
            v-if="row.id !== session.user?.id"
            title="删除用户会同时删除其所有渠道，确定？"
            width="240"
            @confirm="remove(row)"
          >
            <template #reference>
              <el-button link type="danger">删除</el-button>
            </template>
          </el-popconfirm>
        </template>
      </el-table-column>
    </el-table>

    <el-dialog v-model="create.open" title="添加用户" width="min(420px, 94vw)">
      <el-form label-position="top">
        <el-form-item label="用户名" required><el-input v-model="create.username" /></el-form-item>
        <el-form-item label="密码（至少 8 位）" required>
          <el-input v-model="create.password" type="password" show-password autocomplete="new-password" />
        </el-form-item>
        <el-checkbox v-model="create.is_admin">管理员</el-checkbox>
      </el-form>
      <template #footer>
        <el-button @click="create.open = false">取消</el-button>
        <el-button type="primary" @click="submitCreate">添加</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { onMounted, reactive, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Plus } from '@element-plus/icons-vue'
import { api, type User } from '../../api'
import { session } from '../../session'

const users = ref<User[]>([])
const loading = ref(true)
const create = reactive({ open: false, username: '', password: '', is_admin: false })

async function load() {
  loading.value = true
  try {
    users.value = await api.users()
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    loading.value = false
  }
}
onMounted(load)

async function submitCreate() {
  try {
    await api.createUser({ username: create.username, password: create.password, is_admin: create.is_admin })
    Object.assign(create, { open: false, username: '', password: '', is_admin: false })
    await load()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

async function update(u: User, change: { active?: boolean; is_admin?: boolean; password?: string }) {
  try {
    Object.assign(u, await api.updateUser(u.id, change))
    return true
  } catch (e) {
    ElMessage.error((e as Error).message)
    return false
  }
}

async function resetPassword(u: User) {
  const { value } = await ElMessageBox.prompt(`为 ${u.username} 设置新密码（至少 8 位）`, '重置密码', {
    inputType: 'password',
  }).catch(() => ({ value: '' }))
  if (value && (await update(u, { password: value }))) ElMessage.success('密码已重置')
}

async function remove(u: User) {
  try {
    await api.deleteUser(u.id)
    users.value = users.value.filter((x) => x.id !== u.id)
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}
</script>
