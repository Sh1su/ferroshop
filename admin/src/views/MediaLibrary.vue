<script setup lang="ts">
import { MtBanner, MtButton, MtCard, MtEmptyState, MtLoader, MtPagination, MtSearch } from '@shopware-ag/meteor-component-library'
import { onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { repository, uploadMedia, type Media } from '../api/client'
import { can } from '../api/session'
import MediaThumb from '../components/MediaThumb.vue'

const { t, n } = useI18n()
const repo = repository<Media>('media')

const limit = 48
const items = ref<Media[]>([])
const total = ref(0)
const page = ref(1)
const term = ref('')
const loading = ref(false)
const uploading = ref(0)
const error = ref('')
const fileInput = ref<HTMLInputElement | null>(null)

async function load() {
  loading.value = true
  try {
    const result = await repo.search({
      page: page.value,
      limit,
      term: term.value || undefined,
      sort: [{ field: 'createdAt', order: 'DESC' }],
      'total-count-mode': 1,
    })
    items.value = result.data
    total.value = result.total
  } finally {
    loading.value = false
  }
}

/** Creates one media entity per file, then uploads the file into it. */
async function upload(files: FileList | null) {
  if (!files?.length) return
  error.value = ''
  uploading.value = files.length
  for (const file of Array.from(files)) {
    try {
      const media = await repo.create({ title: file.name.replace(/\.[^.]+$/, '') } as Partial<Media>)
      try {
        await uploadMedia(media.id!, file)
      } catch (e) {
        await repo.delete(media.id!)
        throw e
      }
    } catch (e) {
      error.value = `${file.name}: ${e instanceof Error ? e.message : t('common.error')}`
    } finally {
      uploading.value--
    }
  }
  if (fileInput.value) fileInput.value.value = ''
  page.value = 1
  await load()
}

async function remove(media: Media) {
  if (!confirm(t('common.confirmDelete', { name: media.fileName ?? media.translated?.title ?? '' }))) return
  await repo.delete(media.id!)
  await load()
}

const size = (bytes?: number | null) => (bytes ? `${n(bytes / 1024, { maximumFractionDigits: 0 })} KB` : '')

let timer: ReturnType<typeof setTimeout> | undefined
watch(term, () => {
  clearTimeout(timer)
  timer = setTimeout(() => {
    page.value = 1
    void load()
  }, 300)
})
onMounted(load)
</script>

<template>
  <section class="fs-page">
    <div class="fs-page-header">
      <h1>{{ t('modules.media') }} <span class="fs-muted">({{ total }})</span></h1>
      <div v-if="can('media', 'create')">
        <input ref="fileInput" type="file" multiple accept="image/*,application/pdf" hidden @change="upload(($event.target as HTMLInputElement).files)" />
        <MtButton variant="primary" :is-loading="uploading > 0" @click="fileInput?.click()">{{ t('media.upload') }}</MtButton>
      </div>
    </div>
    <MtBanner v-if="error" variant="critical" closable @close="error = ''">{{ error }}</MtBanner>
    <MtCard>
      <MtSearch v-model="term" :placeholder="t('common.search')" />
      <MtLoader v-if="loading && !items.length" />
      <MtEmptyState v-else-if="!items.length" :headline="t('media.empty')" icon="regular-image" />
      <ul v-else class="grid">
        <li v-for="media in items" :key="media.id" class="item">
          <a :href="media.url ?? '#'" target="_blank" rel="noopener"><MediaThumb :media="media" /></a>
          <p class="name" :title="media.fileName ?? ''">{{ media.translated?.title || media.fileName || '–' }}</p>
          <p class="meta fs-muted">
            {{ media.fileExtension?.toUpperCase() }}
            <template v-if="media.width"> · {{ media.width }}×{{ media.height }}</template>
            · {{ size(media.fileSize) }}
          </p>
          <MtButton v-if="can('media', 'delete')" size="x-small" variant="critical" ghost @click="remove(media)">{{ t('common.delete') }}</MtButton>
        </li>
      </ul>
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

<style scoped>
.grid { list-style: none; padding: 0; margin: 16px 0 0; display: grid; grid-template-columns: repeat(auto-fill, minmax(160px, 1fr)); gap: 16px; }
.item { min-width: 0; }
.name { margin: 8px 0 0; font-size: 14px; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.meta { margin: 2px 0 6px; font-size: 12px; }
</style>
