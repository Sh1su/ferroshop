import { createRouter, createWebHistory } from 'vue-router'
import { isLoggedIn } from './api/client'

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
      ],
    },
  ],
})

router.beforeEach((to) => {
  if (!to.meta.public && !isLoggedIn()) {
    return { name: 'login', query: { redirect: to.fullPath } }
  }
})
