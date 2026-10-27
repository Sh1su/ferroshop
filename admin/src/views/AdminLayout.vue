<script setup lang="ts">
import { MtButton, MtLoader, MtSelect } from '@shopware-ag/meteor-component-library'
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute } from 'vue-router'
import { contentLanguage, logout, repository, type Language } from '../api/client'
import { LANGUAGE_SYSTEM } from '../api/defaults'
import { can, loadSession, session } from '../api/session'
import { moduleByKey } from '../entities'
import { setLocale } from '../i18n'

const { t, locale } = useI18n()
const route = useRoute()
const languages = ref<Language[]>([])
const languageId = ref(contentLanguage.id)
/** Bumped on content-language change so the current view reloads. */
const languageVersion = ref(0)
const menuOpen = ref(false)

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
watch(() => route.fullPath, () => (menuOpen.value = false))

/** Mirrors `PUBLIC_READ` in the backend's acl.rs. */
const REFERENCE_DATA = ['language', 'currency', 'tax', 'country', 'customer_group', 'payment_method', 'shipping_method', 'sales_channel', 'media']

interface NavItem {
  label: string
  to: object
  entity: string
  prefix: string
}

const entry = (key: string): NavItem => ({
  label: `modules.${key}`,
  to: { name: 'entity.list', params: { module: key } },
  entity: moduleByKey(key)!.entity,
  prefix: `/${key}`,
})

const sections = computed(() =>
  [
    {
      label: 'nav.catalog',
      items: [
        { label: 'modules.products', to: { name: 'product.list' }, entity: 'product', prefix: '/products' },
        entry('categories'),
        entry('manufacturers'),
        { label: 'modules.media', to: { name: 'media' }, entity: 'media', prefix: '/media' },
      ],
    },
    { label: 'nav.sales', items: [entry('orders'), entry('customers')] },
    {
      label: 'nav.settings',
      items: ['taxes', 'currencies', 'languages', 'countries', 'paymentMethods', 'shippingMethods', 'customerGroups', 'salesChannels', 'users', 'roles'].map(entry),
    },
  ]
    // Reference data is readable by everyone; only list it for people who can maintain it.
    .map((section) => ({
      ...section,
      items: section.items.filter((item) => can(item.entity, REFERENCE_DATA.includes(item.entity) ? 'update' : 'read')),
    }))
    .filter((section) => section.items.length),
)

onMounted(async () => {
  await loadSession()
  if (can('language')) {
    languages.value = (await repository<Language>('language').search({ sort: [{ field: 'name', order: 'ASC' }] })).data
  }
})
</script>

<template>
  <MtLoader v-if="!session.user" />
  <div v-else class="layout">
    <aside class="sidebar" :class="{ open: menuOpen }">
      <div class="sidebar-head">
        <RouterLink to="/" class="brand">ferroshop</RouterLink>
        <button class="menu-toggle" type="button" :aria-expanded="menuOpen" @click="menuOpen = !menuOpen">☰</button>
      </div>
      <nav>
        <div v-for="section in sections" :key="section.label" class="section">
          <p class="section-label">{{ t(section.label) }}</p>
          <RouterLink v-for="item in section.items" :key="item.label" :to="item.to" :class="{ active: route.path.startsWith(item.prefix) }">
            {{ t(item.label) }}
          </RouterLink>
        </div>
      </nav>
      <div class="sidebar-footer">
        <MtSelect v-model="uiLocale" :label="t('nav.uiLanguage')" :options="uiLocales" label-property="label" value-property="value" hide-clearable-button />
        <RouterLink :to="{ name: 'profile' }" class="user">{{ session.user.firstName || session.user.username }}</RouterLink>
        <MtButton variant="secondary" size="small" block @click="logout">{{ t('nav.logout') }}</MtButton>
      </div>
    </aside>
    <div class="content">
      <header v-if="languages.length" class="topbar">
        <div class="language">
          <MtSelect v-model="languageId" :label="t('nav.contentLanguage')" :options="languages" label-property="name" value-property="id" hide-clearable-button />
        </div>
      </header>
      <RouterView :key="`${languageVersion}-${route.path}`" />
    </div>
  </div>
</template>

<style scoped>
.layout { display: flex; min-height: 100vh; }
.sidebar {
  width: 240px; flex-shrink: 0; background: var(--fs-surface); border-right: 1px solid var(--fs-border);
  padding: 20px 16px; display: flex; flex-direction: column; gap: 16px; position: sticky; top: 0; height: 100vh; overflow-y: auto;
}
.sidebar-head { display: flex; justify-content: space-between; align-items: center; }
.brand { font-size: 20px; font-weight: 700; text-decoration: none; color: var(--fs-text); letter-spacing: -.02em; }
.menu-toggle { display: none; background: none; border: 1px solid var(--fs-border); border-radius: 6px; padding: 4px 10px; font-size: 18px; cursor: pointer; color: var(--fs-text); }
.section { margin-bottom: 14px; }
.section-label { font-size: 12px; text-transform: uppercase; color: var(--fs-muted); margin: 0 0 4px; letter-spacing: .04em; }
nav a { display: block; padding: 6px 10px; border-radius: 6px; color: var(--fs-text); text-decoration: none; font-size: 14px; }
nav a.active { background: var(--fs-bg); color: var(--fs-accent); font-weight: 600; }
.sidebar-footer { margin-top: auto; }
.user { display: block; font-size: 14px; color: var(--fs-muted); margin: 8px 0; }
.content { flex: 1; min-width: 0; }
.topbar { display: flex; justify-content: flex-end; padding: 12px 32px 0; }
.language { width: 220px; }

@media (max-width: 720px) {
  .layout { flex-direction: column; }
  .sidebar { width: auto; height: auto; position: static; border-right: 0; border-bottom: 1px solid var(--fs-border); padding: 12px 16px; gap: 8px; }
  .menu-toggle { display: block; }
  .sidebar nav, .sidebar-footer { display: none; }
  .sidebar.open nav, .sidebar.open .sidebar-footer { display: block; }
  .topbar { padding: 12px 16px 0; }
}
</style>
