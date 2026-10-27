<script setup lang="ts">
import {
  MtBanner,
  MtButton,
  MtCard,
  MtLoader,
  MtNumberField,
  MtSelect,
  MtSwitch,
  MtTextarea,
  MtTextField,
} from '@shopware-ag/meteor-component-library'
import { computed, onMounted, reactive, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import {
  ApiError,
  repository,
  type Category,
  type Price,
  type Product,
  type ProductManufacturer,
  type Tax,
} from '../api/client'
import { CURRENCY_DEFAULT } from '../api/defaults'

const props = defineProps<{ id?: string }>()
const { t, n, te } = useI18n()
const router = useRouter()

const products = repository<Product>('product')
const isNew = computed(() => !props.id)

const loading = ref(true)
const saving = ref(false)
const notice = ref<{ variant: 'positive' | 'critical'; text: string } | null>(null)
const errors = ref<Record<string, { code: string; detail: string }>>({})

const taxes = ref<Tax[]>([])
const manufacturers = ref<ProductManufacturer[]>([])
const categories = ref<Category[]>([])
const children = ref<Product[]>([])
const parent = ref<Product | null>(null)
const originalCategoryIds = ref<string[]>([])

const form = reactive({
  name: '' as string | null,
  productNumber: '',
  active: true as boolean | null,
  stock: 0 as number | null,
  ean: '' as string | null,
  taxId: null as string | null,
  manufacturerId: null as string | null,
  description: '' as string | null,
  gross: null as number | null,
  net: null as number | null,
  categoryIds: [] as string[],
})

const isVariant = computed(() => parent.value !== null)
const taxRate = computed(() => {
  const taxId = form.taxId ?? parent.value?.taxId
  return taxes.value.find((tax) => tax.id === taxId)?.taxRate ?? 0
})

const round = (value: number) => Math.round(value * 100) / 100
let syncing = false
watch(
  () => form.gross,
  (gross) => {
    if (syncing) return
    syncing = true
    form.net = gross === null ? null : round(gross / (1 + taxRate.value / 100))
    syncing = false
  },
)
watch(
  () => form.net,
  (net) => {
    if (syncing) return
    syncing = true
    form.gross = net === null ? null : round(net * (1 + taxRate.value / 100))
    syncing = false
  },
)
watch(taxRate, () => {
  if (form.gross !== null) form.net = round(form.gross / (1 + taxRate.value / 100))
})

function fill(product: Product) {
  const price = product.price?.find((p) => p.currencyId === CURRENCY_DEFAULT) ?? product.price?.[0]
  Object.assign(form, {
    name: product.name ?? null,
    productNumber: product.productNumber,
    active: product.active ?? null,
    stock: product.stock ?? null,
    ean: product.ean ?? null,
    taxId: product.taxId ?? null,
    manufacturerId: product.manufacturerId ?? null,
    description: product.description ?? null,
    categoryIds: (product.categories ?? []).map((c) => c.id!),
  })
  syncing = true
  form.gross = price?.gross ?? null
  form.net = price?.net ?? null
  syncing = false
  originalCategoryIds.value = [...form.categoryIds]
  children.value = [...(product.children ?? [])].sort((a, b) => a.productNumber.localeCompare(b.productNumber))
}

async function load() {
  loading.value = true
  try {
    const [taxResult, manufacturerResult, categoryResult] = await Promise.all([
      repository<Tax>('tax').search({ sort: [{ field: 'position', order: 'ASC' }] }),
      repository<ProductManufacturer>('product-manufacturer').search({ sort: [{ field: 'name', order: 'ASC' }] }),
      repository<Category>('category').search({ sort: [{ field: 'name', order: 'ASC' }] }),
    ])
    taxes.value = taxResult.data
    manufacturers.value = manufacturerResult.data
    categories.value = categoryResult.data
    if (props.id) {
      const product = await products.get(props.id, { associations: { categories: {}, children: {} } })
      parent.value = product.parentId ? await products.get(product.parentId) : null
      fill(product)
    } else {
      form.taxId = taxes.value[0]?.id ?? null
    }
  } catch (e) {
    notice.value = { variant: 'critical', text: e instanceof ApiError && e.status === 404 ? t('common.notFound') : t('common.error') }
  } finally {
    loading.value = false
  }
}

function payload(): Partial<Product> {
  const blank = (value: string | null) => (value === '' ? null : value)
  const price: Price[] | null =
    form.gross === null ? null : [{ currencyId: CURRENCY_DEFAULT, gross: form.gross, net: form.net ?? 0, linked: true }]
  const added = form.categoryIds.filter((id) => !originalCategoryIds.value.includes(id))
  return {
    name: blank(form.name),
    productNumber: form.productNumber,
    active: form.active,
    stock: form.stock ?? undefined,
    ean: blank(form.ean),
    taxId: form.taxId,
    manufacturerId: form.manufacturerId,
    description: blank(form.description),
    ...(price || !isVariant.value ? { price } : { price: null }),
    categories: added.map((id) => ({ id })) as Category[],
  } as Partial<Product>
}

async function save() {
  saving.value = true
  errors.value = {}
  notice.value = null
  try {
    if (isNew.value) {
      const created = await products.create(payload())
      await router.replace({ name: 'product.detail', params: { id: created.id } })
      return
    }
    const removed = originalCategoryIds.value.filter((id) => !form.categoryIds.includes(id))
    await products.update(props.id!, payload())
    await Promise.all(removed.map((categoryId) => products.unlink(props.id!, 'categories', categoryId)))
    fill(await products.get(props.id!, { associations: { categories: {}, children: {} } }))
    notice.value = { variant: 'positive', text: t('common.saved') }
  } catch (e) {
    if (e instanceof ApiError) {
      // Show backend error codes in the interface language, falling back to the server's text.
      errors.value = Object.fromEntries(
        Object.entries(e.byPointer()).map(([pointer, err]) => [
          pointer,
          { ...err, detail: te(`errors.${err.code}`) ? t(`errors.${err.code}`) : err.detail },
        ]),
      )
      notice.value = { variant: 'critical', text: e.status === 400 ? t('errors.checkInput') : e.message }
    } else {
      notice.value = { variant: 'critical', text: t('common.error') }
    }
  } finally {
    saving.value = false
  }
}

async function remove() {
  if (!props.id || !confirm(t('common.confirmDelete', { name: form.name ?? form.productNumber }))) return
  await products.delete(props.id)
  await router.push({ name: 'product.list' })
}

/** Placeholder showing the parent's value for inherited (empty) variant fields. */
const inherited = (value: unknown) => (isVariant.value && value !== null && value !== undefined ? String(value) : undefined)

const title = computed(() =>
  isNew.value ? t('product.new') : form.name || parent.value?.translated?.name || form.productNumber,
)

onMounted(load)
</script>

<template>
  <section class="fs-page">
    <div class="fs-page-header">
      <div>
        <RouterLink :to="parent ? { name: 'product.detail', params: { id: parent.id } } : { name: 'product.list' }">
          ← {{ parent ? parent.translated?.name : t('product.list') }}
        </RouterLink>
        <h1>{{ title }}</h1>
      </div>
      <div class="actions">
        <MtButton v-if="!isNew" variant="critical" ghost @click="remove">{{ t('common.delete') }}</MtButton>
        <MtButton variant="primary" :is-loading="saving" :disabled="loading" @click="save">{{ t('common.save') }}</MtButton>
      </div>
    </div>

    <MtBanner v-if="notice" :variant="notice.variant" closable @close="notice = null">{{ notice.text }}</MtBanner>
    <MtBanner v-if="isVariant" variant="info">{{ t('product.inheritedHint') }}</MtBanner>
    <MtLoader v-if="loading" />

    <form v-else @submit.prevent="save">
      <MtCard :title="t('product.general')">
        <div class="fs-grid">
          <MtTextField
            v-model="form.name"
            :label="t('product.name')"
            :placeholder="inherited(parent?.translated?.name)"
            :error="errors['/name']"
            :required="!isVariant"
          />
          <MtTextField
            v-model="form.productNumber"
            :label="t('product.productNumber')"
            :error="errors['/productNumber']"
            required
          />
        </div>
        <MtTextarea
          v-model="form.description"
          :label="t('product.description')"
          :placeholder="inherited(parent?.translated?.description)"
          :error="errors['/description']"
        />
        <MtSwitch v-model="form.active" :label="t('product.active')" />
      </MtCard>

      <MtCard :title="t('product.pricesStock')">
        <div class="fs-grid">
          <MtSelect
            v-model="form.taxId"
            :label="t('product.tax')"
            :options="taxes.map((tax) => ({ id: tax.id, label: `${tax.name} (${tax.taxRate} %)` }))"
            label-property="label"
            value-property="id"
            :error="errors['/taxId']"
          />
          <MtNumberField
            v-model="form.gross"
            :label="`${t('product.gross')} (EUR)`"
            number-type="float"
            :digits="2"
            :min="0"
            :placeholder="inherited(parent?.price?.[0]?.gross)"
            :error="errors['/price']"
          />
          <MtNumberField
            v-model="form.net"
            :label="`${t('product.net')} (EUR)`"
            number-type="float"
            :digits="2"
            :min="0"
            :placeholder="inherited(parent?.price?.[0]?.net)"
          />
          <MtNumberField
            v-model="form.stock"
            :label="t('product.stock')"
            number-type="int"
            :error="errors['/stock']"
          />
          <MtTextField v-model="form.ean" :label="t('product.ean')" :placeholder="inherited(parent?.ean)" />
        </div>
      </MtCard>

      <MtCard :title="t('product.assignment')">
        <div class="fs-grid">
          <MtSelect
            v-model="form.manufacturerId"
            :label="t('product.manufacturer')"
            :options="manufacturers.map((m) => ({ id: m.id, label: m.translated?.name }))"
            label-property="label"
            value-property="id"
          />
          <MtSelect
            v-model="form.categoryIds"
            :label="t('product.categories')"
            :options="categories.map((c) => ({ id: c.id, label: c.translated?.name }))"
            label-property="label"
            value-property="id"
            enable-multi-selection
          />
        </div>
      </MtCard>

      <MtCard v-if="children.length" :title="`${t('product.variants')} (${children.length})`">
        <table class="fs-table">
          <thead>
            <tr>
              <th>{{ t('product.productNumber') }}</th>
              <th class="num">{{ t('product.stock') }}</th>
              <th class="num">{{ t('product.price') }}</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="child in children" :key="child.id">
              <td>
                <RouterLink :to="{ name: 'product.detail', params: { id: child.id } }">{{ child.productNumber }}</RouterLink>
              </td>
              <td class="num">{{ child.stock }}</td>
              <td class="num">
                {{ child.price?.[0] ? n(child.price[0].gross, { style: 'currency', currency: 'EUR' }) : `↳ ${t('product.parent')}` }}
              </td>
            </tr>
          </tbody>
        </table>
      </MtCard>
    </form>
  </section>
</template>

<style scoped>
.actions { display: flex; gap: 8px; }
form { display: flex; flex-direction: column; }
</style>
