<template>
  <el-menu router :default-active="route.path" class="nav" @select="emit('select')">
    <el-menu-item v-for="i in items" :key="i.path" :index="i.path">{{ i.label }}</el-menu-item>
    <el-menu-item-group v-if="session.user?.is_admin" title="管理">
      <el-menu-item v-for="i in adminItems" :key="i.path" :index="i.path">{{ i.label }}</el-menu-item>
    </el-menu-item-group>
  </el-menu>
</template>

<script setup lang="ts">
import { useRoute } from 'vue-router'
import { session } from '../session'

const emit = defineEmits<{ select: [] }>()
const route = useRoute()

const items = [
  { path: '/channels', label: '消息渠道' },
  { path: '/send', label: '发送接口' },
  { path: '/account', label: '账号' },
]
const adminItems = [
  { path: '/admin/users', label: '用户管理' },
  { path: '/admin/settings', label: '系统设置' },
  { path: '/admin/stats', label: '发送统计' },
]
</script>

<style scoped>
.nav {
  border-right: none;
}
</style>
