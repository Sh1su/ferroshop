<script setup lang="ts">
import { MtBanner, MtButton, MtCard, MtCheckbox, MtLoader, MtTextarea, MtTextField } from '@shopware-ag/meteor-component-library'
import { computed, reactive, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import { ApiError, repository, type AclRole, type Action } from '../api/client'
import { session } from '../api/session'

const props = defineProps<{ id: string }>()
const { t, te } = useI18n()
const router = useRouter()
const roles = repository<AclRole>('acl-role')
const isNew = computed(() => props.id === 'new')

/** Entities shown in the matrix, grouped like the navigation. */
const groups: { label: string; entities: string[] }[] = [
  // Line items, addresses and product images share the privileges of their owner (see backend `acl::owner`).
  { label: 'nav.catalog', entities: ['product', 'category', 'product_manufacturer', 'media'] },
  { label: 'nav.sales', entities: ['order', 'customer'] },
  {
    label: 'nav.settings',
    entities: ['tax', 'currency', 'language', 'country', 'payment_method', 'shipping_method', 'customer_group', 'sales_channel', 'user', 'acl_role', 'integration'],
  },
]
const actions: Action[] = ['read', 'create', 'update', 'delete']

const loading = ref(true)
const saving = ref(false)
const notice = ref<{ variant: 'positive' | 'critical'; text: string } | null>(null)
const errors = ref<Record<string, { code: string; detail: string }>>({})
const form = reactive({ name: '', description: '' as string | null })
const privileges = ref(new Set<string>())

const isAdmin = computed(() => session.user?.admin ?? false)

async function load() {
  loading.value = true
  try {
    if (isNew.value) {
      privileges.value = new Set()
      return
    }
    const role = await roles.get(props.id)
    form.name = role.name
    form.description = role.description ?? ''
    privileges.value = new Set(((role.privileges as unknown as string[]) ?? []).filter((p) => typeof p === 'string'))
  } catch (e) {
    notice.value = { variant: 'critical', text: e instanceof ApiError && e.status === 404 ? t('common.notFound') : t('common.error') }
  } finally {
    loading.value = false
  }
}

const has = (entity: string, action: Action) => privileges.value.has('*') || privileges.value.has(`${entity}:${action}`)

function toggle(entity: string, action: Action, on: boolean) {
  const next = new Set(privileges.value)
  const key = `${entity}:${action}`
  if (on) {
    next.add(key)
    // Writing without reading makes no sense in the UI.
    if (action !== 'read') next.add(`${entity}:read`)
  } else {
    next.delete(key)
    if (action === 'read') actions.forEach((a) => next.delete(`${entity}:${a}`))
  }
  privileges.value = next
}

function toggleRow(entity: string, on: boolean) {
  actions.forEach((a) => toggle(entity, a, on))
}

async function save() {
  saving.value = true
  notice.value = null
  errors.value = {}
  const payload = { name: form.name, description: form.description || null, privileges: [...privileges.value].sort() }
  try {
    if (isNew.value) {
      const created = await roles.create(payload as unknown as Partial<AclRole>)
      await router.replace({ name: 'role.detail', params: { id: created.id } })
      return
    }
    await roles.update(props.id, payload as unknown as Partial<AclRole>)
    notice.value = { variant: 'positive', text: t('common.saved') }
  } catch (e) {
    if (e instanceof ApiError) {
      errors.value = Object.fromEntries(
        Object.entries(e.byPointer()).map(([p, err]) => [p, { ...err, detail: te(`errors.${err.code}`) ? t(`errors.${err.code}`) : err.detail }]),
      )
      notice.value = { variant: 'critical', text: e.status === 400 ? t('errors.checkInput') : e.message }
    }
  } finally {
    saving.value = false
  }
}

watch(() => props.id, load, { immediate: true })
</script>

<template>
  <section class="fs-page">
    <div class="fs-page-header">
      <div>
        <RouterLink :to="{ name: 'entity.list', params: { module: 'roles' } }">← {{ t('modules.roles') }}</RouterLink>
        <h1>{{ isNew ? t('role.new') : form.name }}</h1>
      </div>
      <MtButton v-if="isAdmin" variant="primary" :is-loading="saving" :disabled="loading" @click="save">{{ t('common.save') }}</MtButton>
    </div>
    <MtBanner v-if="!isAdmin" variant="info">{{ t('role.adminOnly') }}</MtBanner>
    <MtBanner v-if="notice" :variant="notice.variant" closable @close="notice = null">{{ notice.text }}</MtBanner>
    <MtLoader v-if="loading" />
    <template v-else>
      <MtCard :title="t('role.general')">
        <MtTextField v-model="form.name" :label="t('fields.name')" required :disabled="!isAdmin" :error="errors['/name']" />
        <MtTextarea v-model="form.description" :label="t('fields.description')" :disabled="!isAdmin" />
      </MtCard>
      <MtCard :title="t('role.privileges')">
        <MtBanner v-if="privileges.has('*')" variant="info">{{ t('role.allPrivileges') }}</MtBanner>
        <p class="fs-muted hint">{{ t('role.hint') }}</p>
        <div class="fs-table-scroll">
          <table class="fs-table matrix">
            <template v-for="group in groups" :key="group.label">
              <thead>
                <tr>
                  <th>{{ t(group.label) }}</th>
                  <th v-for="action in actions" :key="action" class="center">{{ t(`actions.${action}`) }}</th>
                  <th class="center">{{ t('role.all') }}</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="entity in group.entities" :key="entity">
                  <td>{{ t(`entities.${entity}`) }}</td>
                  <td v-for="action in actions" :key="action" class="center">
                    <MtCheckbox
                      :checked="has(entity, action)"
                      :disabled="!isAdmin || privileges.has('*')"
                      @change="(on: boolean) => toggle(entity, action, on)"
                    />
                  </td>
                  <td class="center">
                    <MtCheckbox
                      :checked="actions.every((a) => has(entity, a))"
                      :disabled="!isAdmin || privileges.has('*')"
                      @change="(on: boolean) => toggleRow(entity, on)"
                    />
                  </td>
                </tr>
              </tbody>
            </template>
          </table>
        </div>
      </MtCard>
    </template>
  </section>
</template>

<style scoped>
.center { text-align: center; }
.hint { font-size: 13px; margin: 0 0 12px; }
.matrix :deep(.mt-field--checkbox__container), .matrix :deep(.mt-field--checkbox) { margin: 0; display: inline-flex; }
</style>
