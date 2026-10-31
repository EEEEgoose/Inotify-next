import { createRouter, createWebHashHistory } from 'vue-router'
import { loadUser, session } from './session'

// Hash history: the server only has to serve index.html at `/`.
export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: '/login', name: 'login', component: () => import('./views/Login.vue'), meta: { public: true } },
    {
      path: '/',
      component: () => import('./views/Layout.vue'),
      redirect: '/channels',
      children: [
        { path: 'channels', name: 'channels', component: () => import('./views/Channels.vue'), meta: { title: '消息渠道' } },
        { path: 'send', name: 'send', component: () => import('./views/Send.vue'), meta: { title: '发送接口' } },
        { path: 'account', name: 'account', component: () => import('./views/Account.vue'), meta: { title: '账号' } },
        {
          path: 'admin/users',
          name: 'users',
          component: () => import('./views/admin/Users.vue'),
          meta: { title: '用户管理', admin: true },
        },
        {
          path: 'admin/settings',
          name: 'settings',
          component: () => import('./views/admin/Settings.vue'),
          meta: { title: '系统设置', admin: true },
        },
        {
          path: 'admin/stats',
          name: 'stats',
          component: () => import('./views/admin/Stats.vue'),
          meta: { title: '发送统计', admin: true },
        },
      ],
    },
    { path: '/:pathMatch(.*)*', redirect: '/channels' },
  ],
})

router.beforeEach(async (to) => {
  if (to.meta.public) return true
  if (!(await loadUser())) return { name: 'login', query: { redirect: to.fullPath } }
  if (to.meta.admin && !session.user?.is_admin) return { name: 'channels' }
  return true
})

router.afterEach((to) => {
  document.title = to.meta.title ? `${to.meta.title} - Inotify` : 'Inotify'
})
