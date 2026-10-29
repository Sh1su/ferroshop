<script setup lang="ts">
import {
  MtBanner,
  MtButton,
  MtCard,
  MtLoader,
  MtNumberField,
  MtPasswordField,
  MtSelect,
  MtSwitch,
  MtTextarea,
  MtTextField,
} from '@shopware-ag/meteor-component-library'
import { computed, reactive, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import { ApiError, repository } from '../api/client'
import { CURRENCY_DEFAULT } from '../api/defaults'
import { can, canChange, session } from '../api/session'
import { at, moduleByKey, type FormField } from '../entities'

const props = defineProps<{ module: string; id: string }>()
const { t, te } = useI18n()
const router = useRouter()

const config = computed(() => moduleByKey(props.module)!)
const apiName = computed(() => config.value.entity.replace(/_/g, '-'))
const repo = computed(() => repository<Record<string, unknown>>(apiName.value))
const isNew = computed(() => props.id === 'new')

const loading = ref(true)
const saving = ref(false)
const notice = ref<{ variant: 'positive' | 'critical'; text: string } | null>(null)
const errors = ref<Record<string, { code: string; detail: string }>>({})
const form = reactive<Record<string, unknown>>({})
const original = ref<Record<string, unknown>>({})
const options = reactive<Record<string, { value: string; label: string }[]>>({})

const isAdmin = computed(() => session.user?.admin ?? false)
const fields = computed(() =>
  config.value.fields.filter((f) => (!f.only || (f.only === 'create') === isNew.value) && (!f.adminOnly || isAdmin.value)),
)
const readOnly = computed(() =>
  isNew.value ? !can(config.value.entity, 'create') : !canChange(config.value.entity, original.value),
)
// Allowed in general, but this record (an administrator) is reserved for administrators.
const protectedTarget = computed(() => !isNew.value && can(config.value.entity, 'update') && readOnly.value)

const salesChannelDefaults: { field: string; entity: string; mustBeActive?: boolean }[] = [
  { field: 'languageId', entity: 'language' },
  { field: 'currencyId', entity: 'currency' },
  { field: 'navigationCategoryId', entity: 'category', mustBeActive: true },
  { field: 'customerGroupId', entity: 'customer_group' },
  { field: 'countryId', entity: 'country', mustBeActive: true },
  { field: 'paymentMethodId', entity: 'payment_method', mustBeActive: true },
  { field: 'shippingMethodId', entity: 'shipping_method', mustBeActive: true },
]

async function validateSalesChannelActivation(): Promise<boolean> {
  if (config.value.key !== 'salesChannels' || form.active !== true) return true
  const nextErrors: Record<string, { code: string; detail: string }> = {}
  for (const { field } of salesChannelDefaults) {
    if (!form[field]) nextErrors[`/${field}`] = { code: 'invalid', detail: t('salesChannel.activationRequired') }
  }
  if (Object.keys(nextErrors).length) {
    errors.value = nextErrors
    notice.value = { variant: 'critical', text: t('salesChannel.activationIncomplete') }
    return false
  }
  if (isNew.value) {
    nextErrors['/active'] = { code: 'invalid', detail: t('salesChannel.activationRequiresDomain') }
    errors.value = nextErrors
    notice.value = { variant: 'critical', text: t('salesChannel.activationIncomplete') }
    return false
  }
  await Promise.all(salesChannelDefaults.map(async ({ field, entity, mustBeActive }) => {
    try {
      const record = await repository<Record<string, unknown>>(entity.replace(/_/g, '-')).get(String(form[field]))
      if (mustBeActive && record.active !== true) nextErrors[`/${field}`] = { code: 'invalid', detail: t('salesChannel.referenceInactive') }
    } catch {
      nextErrors[`/${field}`] = { code: 'invalid', detail: t('salesChannel.referenceUnavailable') }
    }
  }))
  if (!Object.keys(nextErrors).length) {
    try {
      const domains = await repository<Record<string, unknown>>('sales-channel-domain').search({
        limit: 1,
        filter: [{ type: 'equals', field: 'salesChannelId', value: props.id }],
      })
      if (domains.data.length === 0) {
        nextErrors['/active'] = { code: 'invalid', detail: t('salesChannel.activationRequiresDomain') }
      }
    } catch {
      nextErrors['/active'] = { code: 'invalid', detail: t('salesChannel.domainCheckFailed') }
    }
  }
  if (Object.keys(nextErrors).length) {
    errors.value = nextErrors
    notice.value = { variant: 'critical', text: t('salesChannel.activationIncomplete') }
    return false
  }
  return true
}

function initial(field: FormField, entity?: Record<string, unknown>): unknown {
  const value = entity?.[field.name]
  switch (field.type) {
    case 'bool':
      return value ?? field.default ?? true
    case 'multi':
      return ((value as { id: string }[] | undefined) ?? []).map((v) => v.id)
    case 'price': {
      const price = (value as { currencyId: string; gross: number; net: number }[] | null | undefined)?.find(
        (p) => p.currencyId === CURRENCY_DEFAULT,
      )
      return { gross: price?.gross ?? null, net: price?.net ?? null }
    }
    case 'password':
      return ''
    default:
    return entity ? value ?? null : field.default ?? null
  }
}

async function loadOptions() {
  await Promise.all(
    config.value.fields
      .filter((f) => f.entity)
      .map(async (f) => {
        const result = await repository<Record<string, unknown>>(f.entity!.replace(/_/g, '-')).search({ limit: 500 })
        options[f.name] = result.data
          .filter((row) => !(f.entity === config.value.entity && row.id === props.id))
          .map((row) => ({ value: row.id as string, label: String(at(row, f.optionLabel ?? 'name') ?? row.id) }))
      }),
  )
  for (const f of config.value.fields.filter((f) => f.options)) options[f.name] = f.options!
}

async function load() {
  loading.value = true
  notice.value = null
  errors.value = {}
  try {
    await loadOptions()
    const multi = Object.fromEntries(config.value.fields.filter((f) => f.type === 'multi').map((f) => [f.name, {}]))
    const entity = isNew.value ? undefined : await repo.value.get(props.id, { associations: multi })
    original.value = entity ?? {}
    for (const field of config.value.fields) form[field.name] = initial(field, entity)
  } catch (e) {
    notice.value = { variant: 'critical', text: e instanceof ApiError && e.status === 404 ? t('common.notFound') : t('common.error') }
  } finally {
    loading.value = false
  }
}

function payload(): Record<string, unknown> {
  const out: Record<string, unknown> = {}
  for (const field of fields.value) {
    const value = form[field.name]
    switch (field.type) {
      case 'password':
        if (value) out[field.name] = value
        break
      case 'multi': {
        const before = initial(field, original.value) as string[]
        out[field.name] = (value as string[]).filter((id) => !before.includes(id)).map((id) => ({ id }))
        break
      }
      case 'price': {
        const { gross, net } = value as { gross: number | null; net: number | null }
        out[field.name] = gross === null ? null : [{ currencyId: CURRENCY_DEFAULT, gross, net: net ?? gross, linked: false }]
        break
      }
      case 'text':
      case 'textarea':
        out[field.name] = value === '' ? null : value
        break
      default:
        out[field.name] = value
    }
  }
  // New entities: leave empty fields out so database defaults apply.
  if (isNew.value) {
    for (const key of Object.keys(out)) if (out[key] === null || out[key] === undefined) delete out[key]
  }
  // Only send what changed so permission checks (e.g. admin flag) don't trip over untouched fields.
  if (!isNew.value) {
    for (const key of Object.keys(out)) {
      const field = fields.value.find((f) => f.name === key)!
      if (field.type === 'multi' && (out[key] as unknown[]).length === 0) delete out[key]
      else if (!['multi', 'price', 'password'].includes(field.type) && out[key] === (original.value[key] ?? null)) delete out[key]
    }
  }
  return out
}

async function save() {
  if (saving.value) return
  saving.value = true
  errors.value = {}
  notice.value = null
  try {
    if (!(await validateSalesChannelActivation())) return
    if (isNew.value) {
      const created = await repo.value.create(payload())
      await router.replace({ name: 'entity.detail', params: { module: props.module, id: created.id as string } })
      return
    }
    await repo.value.update(props.id, payload())
    for (const field of fields.value.filter((f) => f.type === 'multi')) {
      const before = initial(field, original.value) as string[]
      const removed = before.filter((id) => !(form[field.name] as string[]).includes(id))
      await Promise.all(removed.map((ref) => repo.value.unlink(props.id, field.name, ref)))
    }
    await load()
    notice.value = { variant: 'positive', text: t('common.saved') }
  } catch (e) {
    if (e instanceof ApiError) {
      errors.value = Object.fromEntries(
        Object.entries(e.byPointer()).map(([p, err]) => [p, { ...err, detail: te(`errors.${err.code}`) ? t(`errors.${err.code}`) : err.detail }]),
      )
      // Errors without a matching form field (e.g. database constraints) are shown verbatim.
      const unmatched = e.errors.find((err) => !fields.value.some((f) => err.source?.pointer === `/${f.name}`))
      const text = unmatched && te(`errors.${unmatched.code}`) ? t(`errors.${unmatched.code}`) : (unmatched?.detail ?? e.message)
      notice.value = { variant: 'critical', text: e.status === 400 && !unmatched ? t('errors.checkInput') : text }
    } else {
      notice.value = { variant: 'critical', text: t('common.error') }
    }
  } finally {
    saving.value = false
  }
}

const title = computed(() =>
  isNew.value ? t('common.create') : String(at(original.value, config.value.titleField) ?? form[config.value.fields[0]?.name ?? ''] ?? ''),
)

watch(() => [props.module, props.id], load, { immediate: true })
</script>

<template>
  <section class="fs-page">
    <div class="fs-page-header">
      <div>
        <RouterLink :to="{ name: 'entity.list', params: { module } }">← {{ t(`modules.${module}`) }}</RouterLink>
        <h1>{{ title }}</h1>
      </div>
      <MtButton v-if="!readOnly" variant="primary" :is-loading="saving" :disabled="loading || saving" @click="save">
        {{ t('common.save') }}
      </MtButton>
    </div>
    <MtBanner v-if="protectedTarget && !loading" variant="info">{{ t('common.adminTarget') }}</MtBanner>
    <MtBanner v-if="notice" :variant="notice.variant" closable @close="notice = null">{{ notice.text }}</MtBanner>
    <MtLoader v-if="loading" />
    <MtCard v-else :title="t(`modules.${module}`)">
      <form class="fs-grid" @submit.prevent="save">
        <template v-for="field in fields" :key="field.name">
          <MtTextField
            v-if="field.type === 'text'"
            v-model="form[field.name] as string"
            :label="t(field.label)"
            :required="field.required"
            :disabled="readOnly"
            :error="errors[`/${field.name}`]"
          />
          <MtTextarea
            v-else-if="field.type === 'textarea'"
            v-model="form[field.name] as string"
            class="wide"
            :label="t(field.label)"
            :disabled="readOnly"
            :error="errors[`/${field.name}`]"
          />
          <MtPasswordField
            v-else-if="field.type === 'password'"
            v-model="form[field.name] as string"
            :label="t(field.label)"
            :placeholder="isNew ? '' : t('fields.passwordUnchanged')"
            :required="isNew"
            :disabled="readOnly"
            :error="errors[`/${field.name}`]"
          />
          <MtNumberField
            v-else-if="field.type === 'int' || field.type === 'float'"
            v-model="form[field.name] as number"
            :label="t(field.label)"
            :number-type="field.type"
            :required="field.required"
            :disabled="readOnly"
            :error="errors[`/${field.name}`]"
          />
          <MtSwitch
            v-else-if="field.type === 'bool'"
            v-model="form[field.name] as boolean"
            :label="t(field.label)"
            :disabled="readOnly"
          />
          <MtSelect
            v-else-if="field.type === 'select' || field.type === 'multi'"
            v-model="form[field.name] as string"
            :label="t(field.label)"
            :options="options[field.name] ?? []"
            label-property="label"
            value-property="value"
            :enable-multi-selection="field.type === 'multi'"
            :disabled="readOnly"
            :error="errors[`/${field.name}`]"
          />
          <div v-else-if="field.type === 'price'" class="price">
            <MtNumberField
              v-model="(form[field.name] as { gross: number | null }).gross"
              :label="`${t(field.label)} – ${t('product.gross')} (EUR)`"
              number-type="float"
              :digits="2"
              :disabled="readOnly"
            />
            <MtNumberField
              v-model="(form[field.name] as { net: number | null }).net"
              :label="`${t('product.net')} (EUR)`"
              number-type="float"
              :digits="2"
              :disabled="readOnly"
            />
          </div>
        </template>
      </form>
    </MtCard>
  </section>
</template>

<style scoped>
.wide, .price { grid-column: 1 / -1; }
.price { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 0 16px; }
</style>
