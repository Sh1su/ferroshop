<script setup lang="ts">
import { MtButton, MtSelect } from '@shopware-ag/meteor-component-library'
import { onMounted, provide, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { contentLanguage, logout, me, repository, type Language, type User } from '../api/client'
import { LANGUAGE_SYSTEM } from '../api/defaults'
import { useRoute } from 'vue-router'
import { setLocale } from '../i18n'

const { t, locale } = useI18n()
const route = useRoute()
const user = ref<User | null>(null)
const languages = ref<Language[]>([])
const languageId = ref(contentLanguage.id)
/** Incremented on content-language change so views reload their data. */
const languageVersion = ref(0)
provide('languageVersion', languageVersion)

const uiLocales = [
  { value: 'de', label: 'Deutsch' },
  { value: 'en', label: 'English' },
]
const uiLocale = ref(locale.value)
watch(uiLocale, (value) => setLocale(value as 'de' | 'en'))

watch(languageId, (id) => {
  contentLanguage.id = id || LANGUAGE_SYSTEM
  languageVersion.value++
})

onMounted(async () => {
  const [profile, langs] = await Promise.all([
    me(),
    repository<Language>('language').search({ sort: [{ field: 'name', order: 'ASC' }] }),
  ])
  user.value = profile
  languages.value = langs.data
})
</script>

<template>
  <div class="layout">
    <aside class="sidebar">
      <RouterLink to="/" class="brand">ferroshop</RouterLink>
      <nav>
        <p class="section">{{ t('nav.catalog') }}</p>
        <RouterLink :to="{ name: 'product.list' }" :class="{ active: route.path.startsWith('/products') }">
          {{ t('nav.products') }}
        </RouterLink>
        <p class="section">{{ t('nav.team') }}</p>
        <RouterLink :to="{ name: 'organigram' }" :class="{ active: route.name === 'organigram' }">{{ t('nav.organigram') }}</RouterLink>
      </nav>
      <div class="sidebar-footer">
        <MtSelect
          v-model="uiLocale"
          :label="t('nav.uiLanguage')"
          :options="uiLocales"
          label-property="label"
          value-property="value"
          hide-clearable-button
        />
        <p v-if="user" class="user">{{ user.firstName || user.username }}</p>
        <MtButton variant="secondary" size="small" block @click="logout">{{ t('nav.logout') }}</MtButton>
      </div>
    </aside>
    <div class="content">
      <header class="topbar">
        <div class="language">
          <MtSelect
            v-model="languageId"
            :label="t('nav.contentLanguage')"
            :options="languages"
            label-property="name"
            value-property="id"
            hide-clearable-button
          />
        </div>
      </header>
      <RouterView :key="languageVersion" />
    </div>
  </div>
</template>

<style scoped>
.layout { display: flex; min-height: 100vh; }
.sidebar {
  width: 240px; flex-shrink: 0; background: var(--fs-surface); border-right: 1px solid var(--fs-border);
  padding: 20px 16px; display: flex; flex-direction: column; gap: 16px;
}
.brand { font-size: 20px; font-weight: 700; text-decoration: none; color: var(--fs-text); letter-spacing: -.02em; }
.section { font-size: 12px; text-transform: uppercase; color: var(--fs-muted); margin: 0 0 6px; letter-spacing: .04em; }
nav .section:not(:first-child) { margin-top: 12px; }
nav a { display: block; padding: 8px 10px; border-radius: 6px; color: var(--fs-text); text-decoration: none; }
nav a.active { background: var(--fs-bg); color: var(--fs-accent); font-weight: 600; }
.sidebar-footer { margin-top: auto; }
.user { font-size: 14px; color: var(--fs-muted); margin: 8px 0; }
.content { flex: 1; min-width: 0; }
.topbar { display: flex; justify-content: flex-end; padding: 12px 32px 0; }
.language { width: 220px; }

@media (max-width: 720px) {
  .layout { flex-direction: column; }
  .sidebar { width: auto; border-right: 0; border-bottom: 1px solid var(--fs-border); }
  .topbar { padding: 12px 16px 0; }
}
</style>
