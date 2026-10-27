<script setup lang="ts">
import { MtBanner, MtButton, MtCard, MtEmptyState, MtLoader, MtPagination, MtSearch } from '@shopware-ag/meteor-component-library'
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import { repository, type Product } from '../api/client'
import { CURRENCY_DEFAULT } from '../api/defaults'

const { t, n } = useI18n()
const route = useRoute()
const router = useRouter()
const products = repository<Product>('product')

const limit = 25
const items = ref<Product[]>([])
const total = ref(0)
const loading = ref(false)
const error = ref('')
const term = ref(typeof route.query.term === 'string' ? route.query.term : '')
const page = ref(Number(route.query.page) || 1)

const sortField = ref('productNumber')
const sortDesc = ref(false)

async function load() {
  loading.value = true
  error.value = ''
  try {
    const result = await products.search({
      page: page.value,
      limit,
      term: term.value || undefined,
      filter: [{ type: 'equals', field: 'parentId', value: null }],
      sort: [{ field: sortField.value, order: sortDesc.value ? 'DESC' : 'ASC' }],
      associations: { manufacturer: {} },
      'total-count-mode': 1,
    })
    items.value = result.data
    total.value = result.total
  } catch (e) {
    error.value = e instanceof Error ? e.message : t('common.error')
  } finally {
    loading.value = false
  }
}

let searchTimer: ReturnType<typeof setTimeout> | undefined
watch(term, () => {
  clearTimeout(searchTimer)
  searchTimer = setTimeout(() => {
    page.value = 1
    void load()
  }, 300)
})
watch([page, term], () => router.replace({ query: { page: String(page.value), term: term.value || undefined } }))

function sortBy(field: string) {
  if (sortField.value === field) sortDesc.value = !sortDesc.value
  else {
    sortField.value = field
    sortDesc.value = false
  }
  void load()
}

function grossPrice(product: Product): string {
  const price = product.price?.find((p) => p.currencyId === CURRENCY_DEFAULT) ?? product.price?.[0]
  return price ? n(price.gross, { style: 'currency', currency: 'EUR' }) : '–'
}

async function remove(product: Product) {
  if (!confirm(t('common.confirmDelete', { name: product.translated?.name ?? product.productNumber }))) return
  try {
    await products.delete(product.id!)
    await load()
  } catch (e) {
    error.value = e instanceof Error ? e.message : t('common.error')
  }
}

const columns = computed(() => [
  { field: 'name', label: t('product.name') },
  { field: 'productNumber', label: t('product.productNumber') },
  { field: 'manufacturer.name', label: t('product.manufacturer'), className: 'hide-sm' },
  { field: 'stock', label: t('product.stock'), className: 'num' },
])

onMounted(load)
</script>

<template>
  <section class="fs-page">
    <div class="fs-page-header">
      <h1>{{ t('product.list') }} <span class="fs-muted">({{ total }})</span></h1>
      <MtButton variant="primary" @click="router.push({ name: 'product.create' })">{{ t('product.new') }}</MtButton>
    </div>
    <MtBanner v-if="error" variant="critical">{{ error }}</MtBanner>
    <MtCard>
      <MtSearch v-model="term" :placeholder="t('common.search')" />
      <MtLoader v-if="loading && !items.length" />
      <MtEmptyState v-else-if="!items.length" :headline="t('product.empty')" icon="regular-products" />
      <div v-else class="fs-table-scroll">
      <table class="fs-table">
        <thead>
          <tr>
            <th v-for="col in columns" :key="col.field" :class="col.className">
              <a href="#" @click.prevent="sortBy(col.field)">
                {{ col.label }}<template v-if="sortField === col.field">{{ sortDesc ? ' ↓' : ' ↑' }}</template>
              </a>
            </th>
            <th class="num">{{ t('product.price') }}</th>
            <th class="hide-sm">{{ t('product.active') }}</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="product in items" :key="product.id">
            <td>
              <RouterLink :to="{ name: 'product.detail', params: { id: product.id } }">
                {{ product.translated?.name ?? product.name }}
              </RouterLink>
              <span v-if="product.childCount" class="fs-muted"> · {{ product.childCount }} {{ t('product.variants') }}</span>
            </td>
            <td>{{ product.productNumber }}</td>
            <td class="hide-sm">{{ product.manufacturer?.translated?.name ?? '–' }}</td>
            <td class="num">{{ product.stock }}</td>
            <td class="num">{{ grossPrice(product) }}</td>
            <td class="hide-sm">{{ product.active ? t('common.yes') : t('common.no') }}</td>
            <td class="num">
              <MtButton variant="critical" size="x-small" ghost @click="remove(product)">{{ t('common.delete') }}</MtButton>
            </td>
          </tr>
        </tbody>
      </table>
      </div>
      <MtPagination
        v-if="total > limit"
        :current-page="page"
        :limit="limit"
        :total-items="total"
        @change-current-page="(p: number) => { page = p; load() }"
      />
    </MtCard>
  </section>
</template>
