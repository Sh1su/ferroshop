<script setup lang="ts">
import { MtBanner, MtButton, MtCard, MtPasswordField, MtTextField } from '@shopware-ag/meteor-component-library'
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import { login } from '../api/client'

const { t } = useI18n()
const router = useRouter()
const route = useRoute()

const username = ref('')
const password = ref('')
const loading = ref(false)
const failed = ref(false)

async function submit() {
  loading.value = true
  failed.value = false
  try {
    await login(username.value, password.value)
    const redirect = typeof route.query.redirect === 'string' ? route.query.redirect : '/'
    await router.replace(redirect)
  } catch {
    failed.value = true
  } finally {
    loading.value = false
  }
}
</script>

<template>
  <main class="login">
    <form class="login-form" @submit.prevent="submit">
      <p class="brand">ferroshop</p>
      <MtCard :title="t('login.title')">
        <MtBanner v-if="failed" variant="critical">{{ t('login.failed') }}</MtBanner>
        <MtTextField v-model="username" :label="t('login.username')" name="username" required />
        <MtPasswordField v-model="password" :label="t('login.password')" name="password" required />
        <MtButton variant="primary" type="submit" block :is-loading="loading">{{ t('login.submit') }}</MtButton>
      </MtCard>
    </form>
  </main>
</template>

<style scoped>
.login { min-height: 100vh; display: grid; place-items: center; padding: 16px; }
.login-form { width: 100%; max-width: 400px; }
.brand { font-size: 28px; font-weight: 700; text-align: center; margin: 0 0 16px; letter-spacing: -.02em; }
</style>
