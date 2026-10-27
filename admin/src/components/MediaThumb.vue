<script setup lang="ts">
import { computed } from 'vue'
import type { Media } from '../api/client'

const props = defineProps<{ media: Media | null | undefined; size?: number }>()

/** Smallest thumbnail that is at least `size` wide, falling back to the original. */
const src = computed(() => {
  const media = props.media
  if (!media?.url || !media.mimeType?.startsWith('image/')) return null
  const thumbs = ((media.thumbnails as unknown as { width: number; url: string }[]) ?? []).sort((a, b) => a.width - b.width)
  return thumbs.find((t) => t.width >= (props.size ?? 400))?.url ?? media.url
})
</script>

<template>
  <div class="thumb">
    <img v-if="src" :src="src" :alt="media?.translated?.alt ?? ''" loading="lazy" />
    <span v-else class="placeholder">{{ media?.fileExtension?.toUpperCase() ?? '?' }}</span>
  </div>
</template>

<style scoped>
.thumb {
  aspect-ratio: 1; border-radius: 8px; overflow: hidden; background: var(--fs-bg); border: 1px solid var(--fs-border);
  display: grid; place-items: center;
}
img { width: 100%; height: 100%; object-fit: contain; }
.placeholder { font-weight: 700; color: var(--fs-muted); }
</style>
