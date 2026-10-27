<script setup lang="ts">
import { MtBanner, MtButton, MtCard, MtPasswordField, MtTextField } from '@shopware-ag/meteor-component-library'
import { reactive, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { ApiError, updateMe } from '../api/client'
import { loadSession, session } from '../api/session'

const { t, te } = useI18n()
const profile = reactive({
  firstName: session.user?.firstName ?? '',
  lastName: session.user?.lastName ?? '',
  email: session.user?.email ?? '',
})
const passwords = reactive({ currentPassword: '', password: '', confirm: '' })
const notice = ref<{ variant: 'positive' | 'critical'; text: string } | null>(null)
const errors = ref<Record<string, { code: string; detail: string }>>({})
const saving = ref(false)

async function submit(payload: Record<string, unknown>) {
  saving.value = true
  notice.value = null
  errors.value = {}
  try {
    await updateMe(payload)
    await loadSession()
    notice.value = { variant: 'positive', text: t('common.saved') }
    return true
  } catch (e) {
    if (e instanceof ApiError) {
      errors.value = Object.fromEntries(
        Object.entries(e.byPointer()).map(([p, err]) => [p, { ...err, detail: te(`errors.${err.code}`) ? t(`errors.${err.code}`) : err.detail }]),
      )
      notice.value = { variant: 'critical', text: e.status === 400 ? t('errors.checkInput') : e.message }
    }
    return false
  } finally {
    saving.value = false
  }
}

async function changePassword() {
  if (passwords.password !== passwords.confirm) {
    errors.value = { '/confirm': { code: 'mismatch', detail: t('profile.mismatch') } }
    return
  }
  if (await submit({ currentPassword: passwords.currentPassword, password: passwords.password })) {
    Object.assign(passwords, { currentPassword: '', password: '', confirm: '' })
  }
}
</script>

<template>
  <section class="fs-page">
    <div class="fs-page-header"><h1>{{ t('profile.title') }}</h1></div>
    <MtBanner v-if="notice" :variant="notice.variant" closable @close="notice = null">{{ notice.text }}</MtBanner>
    <MtCard :title="t('profile.data')">
      <form class="fs-grid" @submit.prevent="submit(profile)">
        <MtTextField v-model="profile.firstName" :label="t('fields.firstName')" />
        <MtTextField v-model="profile.lastName" :label="t('fields.lastName')" />
        <MtTextField v-model="profile.email" :label="t('fields.email')" :error="errors['/email']" />
        <div class="actions"><MtButton variant="primary" type="submit" :is-loading="saving">{{ t('common.save') }}</MtButton></div>
      </form>
    </MtCard>
    <MtCard :title="t('profile.password')">
      <form class="fs-grid" @submit.prevent="changePassword">
        <MtPasswordField v-model="passwords.currentPassword" :label="t('profile.currentPassword')" required :error="errors['/currentPassword']" />
        <MtPasswordField v-model="passwords.password" :label="t('profile.newPassword')" required :error="errors['/password']" />
        <MtPasswordField v-model="passwords.confirm" :label="t('profile.confirmPassword')" required :error="errors['/confirm']" />
        <div class="actions"><MtButton variant="primary" type="submit" :is-loading="saving">{{ t('profile.changePassword') }}</MtButton></div>
      </form>
    </MtCard>
  </section>
</template>

<style scoped>
.actions { display: flex; align-items: end; padding-bottom: 20px; }
</style>
