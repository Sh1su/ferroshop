import { createRouter, createWebHistory } from 'vue-router'
import { isLoggedIn } from './api/client'
import { modules } from './entities'

const moduleKeys = modules.map((m) => m.key).join('|')

export const router = createRouter({
  history: createWebHistory('/admin/'),
  routes: [
    { path: '/login', name: 'login', component: () => import('./views/LoginView.vue'), meta: { public: true } },
    {
      path: '/',
      component: () => import('./views/AdminLayout.vue'),
      children: [
        { path: '', redirect: { name: 'product.list' } },
        { path: 'products', name: 'product.list', component: () => import('./views/ProductList.vue') },
        { path: 'products/new', name: 'product.create', component: () => import('./views/ProductDetail.vue') },
        { path: 'products/:id', name: 'product.detail', component: () => import('./views/ProductDetail.vue'), props: true },
        { path: 'media', name: 'media', component: () => import('./views/MediaLibrary.vue') },
        { path: 'orders/:id', name: 'order.detail', component: () => import('./views/OrderDetail.vue'), props: true },
        { path: 'customers/:id', name: 'customer.detail', component: () => import('./views/CustomerDetail.vue'), props: true },
        { path: 'roles/:id', name: 'role.detail', component: () => import('./views/RoleDetail.vue'), props: true },
        { path: 'profile', name: 'profile', component: () => import('./views/ProfileView.vue') },
        { path: `:module(${moduleKeys})`, name: 'entity.list', component: () => import('./views/EntityList.vue'), props: true },
        {
          path: `:module(${moduleKeys})/:id`,
          name: 'entity.detail',
          component: () => import('./views/EntityDetail.vue'),
          props: true,
        },
      ],
    },
  ],
})

router.beforeEach((to) => {
  if (!to.meta.public && !isLoggedIn()) {
    return { name: 'login', query: { redirect: to.fullPath } }
  }
})
