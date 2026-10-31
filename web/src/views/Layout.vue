<template>
  <el-container class="shell">
    <el-header class="header">
      <div class="brand">
        <el-button class="menu-toggle" text :icon="Menu" @click="drawer = true" />
        <strong>Inotify</strong>
      </div>
      <el-dropdown v-if="session.user" @command="onCommand">
        <span class="user">
          <el-avatar :size="28" :src="session.user.avatar_url ?? undefined">
            {{ session.user.username.slice(0, 1).toUpperCase() }}
          </el-avatar>
          <span class="name">{{ session.user.username }}</span>
        </span>
        <template #dropdown>
          <el-dropdown-menu>
            <el-dropdown-item command="account">账号设置</el-dropdown-item>
            <el-dropdown-item command="logout" divided>退出登录</el-dropdown-item>
          </el-dropdown-menu>
        </template>
      </el-dropdown>
    </el-header>
    <el-container>
      <el-aside class="aside" width="200px">
        <nav-menu />
      </el-aside>
      <el-drawer v-model="drawer" direction="ltr" size="220px" :with-header="false">
        <nav-menu @select="drawer = false" />
      </el-drawer>
      <el-main class="main">
        <router-view />
      </el-main>
    </el-container>
  </el-container>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { Menu } from '@element-plus/icons-vue'
import NavMenu from '../components/NavMenu.vue'
import { logout, session } from '../session'

const router = useRouter()
const drawer = ref(false)

function onCommand(command: string) {
  if (command === 'logout') {
    logout()
    router.replace({ name: 'login' })
  } else {
    router.push('/account')
  }
}
</script>

<style scoped>
.shell {
  min-height: 100%;
}
.header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  border-bottom: 1px solid var(--el-border-color-light);
  background: var(--el-bg-color);
  padding: 0 16px;
}
.brand {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 18px;
}
.user {
  display: flex;
  align-items: center;
  gap: 8px;
  cursor: pointer;
  outline: none;
}
.aside {
  background: var(--el-bg-color);
  border-right: 1px solid var(--el-border-color-light);
}
.main {
  padding: 20px;
  max-width: 1100px;
  /* Let wide tables scroll inside the page instead of widening it. */
  min-width: 0;
}
.menu-toggle {
  display: none;
}
@media (max-width: 768px) {
  .aside {
    display: none;
  }
  .menu-toggle {
    display: inline-flex;
  }
  .main {
    padding: 16px;
  }
  .name {
    display: none;
  }
}
</style>
