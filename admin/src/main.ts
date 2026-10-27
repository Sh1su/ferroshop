import { createApp } from 'vue'
import '@shopware-ag/meteor-component-library/font.css'
import '@shopware-ag/meteor-component-library/styles.css'
import './style.css'
import App from './App.vue'
import { setLogoutHandler } from './api/client'
import { i18n } from './i18n'
import { router } from './router'

setLogoutHandler(() => router.push({ name: 'login' }))

createApp(App).use(i18n).use(router).mount('#app')
