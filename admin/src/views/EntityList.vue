<script setup lang="ts">
import { MtBanner, MtButton, MtCard, MtEmptyState, MtLoader, MtPagination, MtSearch } from '@shopware-ag/meteor-component-library'
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import { repository } from '../api/client'
import { can, canChange, session } from '../api/session'
import StateBadge from '../components/StateBadge.vue'
import { at, moduleByKey, parseSort, type Column } from '../entities'
import { useFormat } from '../format'

const props = defineProps<{ module: string }>()
const { t } = useI18n()
const router = useRouter()
const fmt = useFormat()

const config = computed(() => moduleByKey(props.module)!)
const repo = computed(() => repository<Record<string, unknown>>(config.value.entity.replace(/_/g, '-')))

const limit = 25
const rows = ref<Record<string, unknown>[]>([])
const total = ref(0)
const page = ref(1)
const term = ref('')
const sort = ref(config.value.defaultSort)
const loading = ref(false)
const error = ref('')

async function load() {
  loading.value = true
  error.value = ''
  try {
    const result = await repo.value.search({
      page: page.value,
      limit,
      term: term.value || undefined,
      sort: [parseSort(sort.value)],
      associations: config.value.associations,
      'total-count-mode': 1,
    })
    rows.value = result.data
    total.value = result.total
  } catch (e) {
    error.value = e instanceof Error ? e.message : t('common.error')
  } finally {
    loading.value = false
  }
}

let timer: ReturnType<typeof setTimeout> | undefined
watch(term, () => {
  clearTimeout(timer)
  timer = setTimeout(() => {
    page.value = 1
    void load()
  }, 300)
})
watch(
  () => props.module,
  () => {
    page.value = 1
    term.value = ''
    sort.value = config.value.defaultSort
    void load()
  },
  { immediate: true },
)

function toggleSort(column: Column) {
  if (!column.sort) return
  sort.value = sort.value === column.sort ? `-${column.sort}` : column.sort
  void load()
}

function sortIndicator(column: Column) {
  if (sort.value === column.sort) return ' ↑'
  if (sort.value === `-${column.sort}`) return ' ↓'
  return ''
}

function display(row: Record<string, unknown>, column: Column) {
  const value = at(row, column.path)
  switch (column.format) {
    case 'currency':
      return fmt.currency(value)
    case 'date':
      return fmt.date(value)
    case 'bool':
      return fmt.bool(value)
    case 'percent':
      return fmt.percent(value)
    default:
      return value === null || value === undefined || value === '' ? '–' : String(value)
  }
}

function open(row: Record<string, unknown>) {
  const id = row.id as string
  if (config.value.detailRoute) router.push({ name: config.value.detailRoute, params: { id } })
  else router.push({ name: 'entity.detail', params: { module: props.module, id } })
}

async function remove(row: Record<string, unknown>) {
  if (!confirm(t('common.confirmDelete', { name: String(at(row, config.value.titleField) ?? '') }))) return
  try {
    await repo.value.delete(row.id as string)
    await load()
  } catch (e) {
    error.value = e instanceof Error ? e.message : t('common.error')
  }
}

const canCreate = computed(() => !config.value.noCreate && can(config.value.entity, 'create'))
const canDelete = computed(() => can(config.value.entity, 'delete'))
</script>

<template>
  <section class="fs-page">
    <div class="fs-page-header">
      <h1>{{ t(`modules.${module}`) }} <span class="fs-muted">({{ total }})</span></h1>
      <MtButton
        v-if="canCreate"
        variant="primary"
        @click="config.detailRoute ? router.push({ name: config.detailRoute, params: { id: 'new' } }) : router.push({ name: 'entity.detail', params: { module, id: 'new' } })"
      >
        {{ t('common.create') }}
      </MtButton>
    </div>
    <MtBanner v-if="error" variant="critical">{{ error }}</MtBanner>
    <MtCard>
      <MtSearch v-model="term" :placeholder="t('common.search')" />
      <MtLoader v-if="loading && !rows.length" />
      <MtEmptyState v-else-if="!rows.length" :headline="t('common.empty')" icon="regular-search" />
      <div v-else class="fs-table-scroll">
        <table class="fs-table">
          <thead>
            <tr>
              <th v-for="column in config.columns" :key="column.path" :class="{ 'hide-sm': column.hideSmall, num: column.format === 'currency' }">
                <a v-if="column.sort" href="#" @click.prevent="toggleSort(column)">{{ t(column.label) }}{{ sortIndicator(column) }}</a>
                <template v-else>{{ t(column.label) }}</template>
              </th>
              <th v-if="canDelete"></th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="row in rows" :key="row.id as string">
              <td v-for="(column, i) in config.columns" :key="column.path" :class="{ 'hide-sm': column.hideSmall, num: column.format === 'currency' }">
                <StateBadge v-if="column.format === 'state'" :state="String(at(row, column.path))" />
                <a v-else-if="i === 0" href="#" @click.prevent="open(row)">{{ display(row, column) }}</a>
                <template v-else>{{ display(row, column) }}</template>
              </td>
              <td v-if="canDelete" class="num">
                <MtButton v-if="canChange(config.entity, row, 'delete') && row.id !== session.user?.id" variant="critical" size="x-small" ghost @click="remove(row)">{{ t('common.delete') }}</MtButton>
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
