<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'

const props = defineProps<{ state: string }>()
const { t } = useI18n()

const tone = computed(() => {
  switch (props.state) {
    case 'paid':
    case 'completed':
    case 'delivered':
      return 'positive'
    case 'cancelled':
    case 'refunded':
    case 'returned':
      return 'critical'
    case 'in_progress':
    case 'shipped':
      return 'info'
    default:
      return 'neutral'
  }
})
</script>

<template>
  <span class="badge" :class="tone">{{ t(`states.${state}`) }}</span>
</template>

<style scoped>
.badge {
  display: inline-block; padding: 2px 8px; border-radius: 999px; font-size: 12px; font-weight: 600; white-space: nowrap;
  background: var(--color-background-secondary-default, #eef1f4); color: var(--fs-muted);
}
.positive { background: var(--color-background-positive-default, #e3f6ec); color: var(--color-text-positive-default, #0b7a3e); }
.critical { background: var(--color-background-critical-default, #fdecec); color: var(--color-text-critical-default, #c81919); }
.info { background: var(--color-background-brand-default, #e6f0ff); color: var(--color-text-brand-default, #0658d6); }
</style>
