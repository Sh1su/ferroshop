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
  generateVariants,
  type Price,
  type Product,
  type ProductManufacturer,
  type Tax,
  type ProductMedia,
  uploadMedia,
  type Media,
  type PropertyGroupOption,
} from '../api/client'
import { can } from '../api/session'
import MediaThumb from '../components/MediaThumb.vue'
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
const propertyOptions = ref<PropertyGroupOption[]>([])
const variantOptionIds = ref<string[]>([])
const generating = ref(false)
const children = ref<Product[]>([])
const images = ref<ProductMedia[]>([])
const coverId = ref<string | null>(null)
const uploading = ref(false)
const imageInput = ref<HTMLInputElement | null>(null)

const DETAIL_ASSOCIATIONS = {
  categories: {},
  properties: {},
  children: { associations: { options: { associations: { group: {} } } } },
  media: { associations: { media: {} }, sort: [{ field: 'position', order: 'ASC' }] },
}
const parent = ref<Product | null>(null)
const originalCategoryIds = ref<string[]>([])
const originalPropertyIds = ref<string[]>([])

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
  propertyIds: [] as string[],
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
    propertyIds: (product.properties ?? []).map((o) => o.id!),
  })
  syncing = true
  form.gross = price?.gross ?? null
  form.net = price?.net ?? null
  syncing = false
  originalCategoryIds.value = [...form.categoryIds]
  originalPropertyIds.value = [...form.propertyIds]
  images.value = (product.media ?? []) as ProductMedia[]
  coverId.value = product.coverId ?? null
  children.value = [...(product.children ?? [])].sort((a, b) => a.productNumber.localeCompare(b.productNumber))
}

async function load() {
  loading.value = true
  try {
    const [taxResult, manufacturerResult, categoryResult, optionResult] = await Promise.all([
      repository<Tax>('tax').search({ sort: [{ field: 'position', order: 'ASC' }] }),
      repository<ProductManufacturer>('product-manufacturer').search({ sort: [{ field: 'name', order: 'ASC' }] }),
      repository<Category>('category').search({ sort: [{ field: 'name', order: 'ASC' }] }),
      repository<PropertyGroupOption>('property-group-option').search({
        associations: { group: {} },
        sort: [{ field: 'group.name', order: 'ASC' }, { field: 'position', order: 'ASC' }, { field: 'name', order: 'ASC' }],
      }),
    ])
    propertyOptions.value = optionResult.data
    taxes.value = taxResult.data
    manufacturers.value = manufacturerResult.data
    categories.value = categoryResult.data
    if (props.id) {
      const product = await products.get(props.id, { associations: DETAIL_ASSOCIATIONS })
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
  const addedProperties = form.propertyIds.filter((id) => !originalPropertyIds.value.includes(id))
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
    properties: addedProperties.map((id) => ({ id })) as PropertyGroupOption[],
  } as Partial<Product>
}

const optionLabel = (option: PropertyGroupOption) => `${option.group?.translated?.name}: ${option.translated?.name}`
const optionChoices = computed(() => propertyOptions.value.map((o) => ({ id: o.id, label: optionLabel(o) })))
const variantLabel = (child: Product) =>
  (child.options ?? []).map((o) => o.translated?.name).filter(Boolean).join(' / ')

async function generate() {
  generating.value = true
  notice.value = null
  try {
    const created = await generateVariants(props.id!, variantOptionIds.value)
    variantOptionIds.value = []
    fill(await products.get(props.id!, { associations: DETAIL_ASSOCIATIONS }))
    notice.value = { variant: 'positive', text: t('product.variantsGenerated', { count: created.length }) }
  } catch (e) {
    notice.value = { variant: 'critical', text: e instanceof ApiError ? e.message : t('common.error') }
  } finally {
    generating.value = false
  }
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
    const removedProperties = originalPropertyIds.value.filter((id) => !form.propertyIds.includes(id))
    await Promise.all([
      ...removed.map((categoryId) => products.unlink(props.id!, 'categories', categoryId)),
      ...removedProperties.map((optionId) => products.unlink(props.id!, 'properties', optionId)),
    ])
    fill(await products.get(props.id!, { associations: DETAIL_ASSOCIATIONS }))
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

async function reloadImages() {
  const product = await products.get(props.id!, { associations: { media: DETAIL_ASSOCIATIONS.media } })
  images.value = (product.media ?? []) as ProductMedia[]
  coverId.value = product.coverId ?? null
}

/** Uploads files as new media and attaches them; the first image becomes the cover. */
async function addImages(files: FileList | null) {
  if (!files?.length || !props.id) return
  uploading.value = true
  notice.value = null
  try {
    let position = Math.max(0, ...images.value.map((i) => i.position ?? 0))
    for (const file of Array.from(files)) {
      const media = await repository<Media>('media').create({ title: file.name.replace(/\.[^.]+$/, '') } as Partial<Media>)
      await uploadMedia(media.id!, file)
      const productMediaId = crypto.randomUUID().replace(/-/g, '')
      await products.update(props.id, {
        media: [{ id: productMediaId, mediaId: media.id, position: ++position }],
        ...(coverId.value ? {} : { coverId: productMediaId }),
      } as unknown as Partial<Product>)
      coverId.value ??= productMediaId
    }
    await reloadImages()
  } catch (e) {
    notice.value = { variant: 'critical', text: e instanceof Error ? e.message : t('common.error') }
  } finally {
    uploading.value = false
    if (imageInput.value) imageInput.value.value = ''
  }
}

async function setCover(image: ProductMedia) {
  await products.update(props.id!, { coverId: image.id })
  coverId.value = image.id!
}

async function removeImage(image: ProductMedia) {
  await repository<ProductMedia>('product-media').delete(image.id!)
  await reloadImages()
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
          <MtSelect
            v-model="form.propertyIds"
            :label="t('product.properties')"
            :options="optionChoices"
            label-property="label"
            value-property="id"
            enable-multi-selection
          />
        </div>
      </MtCard>

      <MtCard v-if="!isNew" :title="t('product.images')">
        <ul v-if="images.length" class="images">
          <li v-for="image in images" :key="image.id" :class="{ cover: image.id === coverId }">
            <MediaThumb :media="image.media" :size="400" />
            <span v-if="image.id === coverId" class="cover-tag">{{ t('product.cover') }}</span>
            <div class="image-actions">
              <MtButton v-if="image.id !== coverId" size="x-small" variant="secondary" @click="setCover(image)">{{ t('product.setCover') }}</MtButton>
              <MtButton size="x-small" variant="critical" ghost @click="removeImage(image)">{{ t('common.delete') }}</MtButton>
            </div>
          </li>
        </ul>
        <p v-else class="fs-muted">{{ t('product.noImages') }}</p>
        <template v-if="can('media', 'create')">
          <input ref="imageInput" type="file" accept="image/*" multiple hidden @change="addImages(($event.target as HTMLInputElement).files)" />
          <MtButton variant="secondary" :is-loading="uploading" @click="imageInput?.click()">{{ t('product.addImages') }}</MtButton>
        </template>
      </MtCard>

      <MtCard v-if="!isNew && !isVariant" :title="`${t('product.variants')} (${children.length})`">
        <div v-if="can('product', 'create')" class="generate">
          <MtSelect
            v-model="variantOptionIds"
            :label="t('product.variantOptions')"
            :options="optionChoices"
            label-property="label"
            value-property="id"
            enable-multi-selection
          />
          <MtButton
            variant="secondary"
            :is-loading="generating"
            :disabled="!variantOptionIds.length"
            @click="generate"
          >
            {{ t('product.generateVariants') }}
          </MtButton>
        </div>
        <table v-if="children.length" class="fs-table">
          <thead>
            <tr>
              <th>{{ t('product.productNumber') }}</th>
              <th>{{ t('product.variantOptions') }}</th>
              <th class="num">{{ t('product.stock') }}</th>
              <th class="num">{{ t('product.price') }}</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="child in children" :key="child.id">
              <td>
                <RouterLink :to="{ name: 'product.detail', params: { id: child.id } }">{{ child.productNumber }}</RouterLink>
              </td>
              <td>{{ variantLabel(child) }}</td>
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
.generate { display: grid; grid-template-columns: 1fr auto; gap: 12px; align-items: end; margin-bottom: 16px; }
.images { list-style: none; padding: 0; margin: 0 0 16px; display: grid; grid-template-columns: repeat(auto-fill, minmax(140px, 1fr)); gap: 12px; }
.images li { position: relative; }
.images li.cover :deep(.thumb) { outline: 2px solid var(--fs-accent); outline-offset: 2px; }
.cover-tag { position: absolute; top: 6px; left: 6px; background: var(--fs-accent); color: #fff; font-size: 11px; font-weight: 600; padding: 2px 6px; border-radius: 4px; }
.image-actions { display: flex; flex-wrap: wrap; gap: 4px; margin-top: 6px; }
form { display: flex; flex-direction: column; }
</style>
