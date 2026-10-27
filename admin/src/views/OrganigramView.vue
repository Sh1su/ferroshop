<script setup lang="ts">
import { MtBanner, MtButton, MtCard, MtEmptyState, MtLoader } from '@shopware-ag/meteor-component-library'
import { computed, nextTick, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { repository, type User } from '../api/client'
import OrgNode, { type OrgMember } from '../components/OrgNode.vue'

const { t } = useI18n()
const users = repository<User>('user')

const items = ref<User[]>([])
const loading = ref(false)
const error = ref('')
const collapsed = ref(new Set<string>())
const zoom = ref(1)
const chartEl = ref<HTMLElement | null>(null)

/** Fixed hues so a department keeps its colour across reloads; order follows first appearance. */
const palette = ['#0870ff', '#00a37a', '#d9480f', '#9c36b5', '#b26b00', '#1098ad', '#c2255c', '#5c7cfa']

async function load() {
  loading.value = true
  error.value = ''
  try {
    const result = await users.search({
      limit: 500,
      sort: [
        { field: 'lastName', order: 'ASC' },
        { field: 'firstName', order: 'ASC' },
      ],
    })
    items.value = result.data
    await nextTick()
    centerTop()
  } catch (e) {
    error.value = e instanceof Error ? e.message : t('common.error')
  } finally {
    loading.value = false
  }
}

const departments = computed(() => {
  const names = [...new Set(items.value.map((u) => u.department).filter((d): d is string => !!d))]
  return new Map(names.map((name, i) => [name, palette[i % palette.length]]))
})

/** Trees of members with reports, plus everyone without manager and without reports. */
const chart = computed(() => {
  const byId = new Map<string, OrgMember>()
  for (const u of items.value) {
    byId.set(u.id!, {
      id: u.id!,
      name: [u.firstName, u.lastName].filter(Boolean).join(' ') || u.username,
      jobTitle: u.jobTitle ?? '',
      department: u.department ?? '',
      active: u.active ?? true,
      color: (u.department && departments.value.get(u.department)) || 'var(--fs-muted)',
      reports: [],
      teamSize: 0,
    })
  }
  const roots: OrgMember[] = []
  for (const u of items.value) {
    const member = byId.get(u.id!)!
    const manager = u.managerId ? byId.get(u.managerId) : undefined
    if (manager) manager.reports.push(member)
    else roots.push(member)
  }
  // Reporting cycles would leave members unreachable from any root; show them as roots instead.
  const seen = new Set<string>()
  const size = (m: OrgMember): number => {
    seen.add(m.id)
    m.teamSize = m.reports.reduce((sum, r) => sum + 1 + size(r), 0)
    return m.teamSize
  }
  roots.forEach(size)
  for (const m of byId.values()) {
    if (!seen.has(m.id)) {
      roots.push(m)
      size(m)
    }
  }
  return {
    trees: roots.filter((m) => m.reports.length).sort((a, b) => b.teamSize - a.teamSize),
    unassigned: roots.filter((m) => !m.reports.length),
  }
})

const levels = computed(() => {
  const depth = (m: OrgMember): number => 1 + Math.max(0, ...m.reports.map(depth))
  return Math.max(0, ...chart.value.trees.map(depth))
})

function toggle(id: string) {
  const next = new Set(collapsed.value)
  if (!next.delete(id)) next.add(id)
  collapsed.value = next
}

function collapseAll() {
  const ids = new Set<string>()
  const walk = (m: OrgMember) => m.reports.forEach((r) => {
    if (r.reports.length) ids.add(r.id)
    walk(r)
  })
  chart.value.trees.forEach(walk)
  collapsed.value = ids
}

/** The tree is usually wider than the page; start with the top of the first tree in view. */
function centerTop() {
  const chart = chartEl.value
  const card = chart?.querySelector<HTMLElement>('.card')
  if (!chart || !card) return
  const box = chart.getBoundingClientRect()
  const target = card.getBoundingClientRect()
  chart.scrollLeft += target.left + target.width / 2 - (box.left + box.width / 2)
}

function setZoom(value: number) {
  zoom.value = Math.min(1.25, Math.max(0.3, Math.round(value * 100) / 100))
  void nextTick(centerTop)
}

function fit() {
  const chart = chartEl.value
  if (chart) setZoom(Math.min(1, (zoom.value * chart.clientWidth) / chart.scrollWidth))
}

onMounted(load)
</script>

<template>
  <section class="fs-page organigram">
    <div class="fs-page-header">
      <h1>
        {{ t('organigram.title') }}
        <span class="fs-muted">({{ t('organigram.summary', { members: items.length, levels }) }})</span>
      </h1>
      <div class="actions">
        <MtButton variant="secondary" size="small" :disabled="zoom <= 0.3" @click="setZoom(zoom - 0.15)">−</MtButton>
        <MtButton variant="secondary" size="small" @click="setZoom(1)">{{ Math.round(zoom * 100) }} %</MtButton>
        <MtButton variant="secondary" size="small" :disabled="zoom >= 1.25" @click="setZoom(zoom + 0.15)">+</MtButton>
        <MtButton variant="secondary" size="small" @click="fit">{{ t('organigram.fit') }}</MtButton>
        <MtButton variant="secondary" size="small" @click="collapsed = new Set()">{{ t('organigram.expandAll') }}</MtButton>
        <MtButton variant="secondary" size="small" @click="collapseAll">{{ t('organigram.collapseAll') }}</MtButton>
      </div>
    </div>
    <MtBanner v-if="error" variant="critical">{{ error }}</MtBanner>
    <MtLoader v-if="loading && !items.length" />
    <MtEmptyState v-else-if="!chart.trees.length" :headline="t('organigram.empty')" icon="regular-users" />
    <template v-else>
      <ul class="legend">
        <li v-for="[name, color] in departments" :key="name"><span :style="{ background: color }"></span>{{ name }}</li>
      </ul>
      <MtCard>
        <div ref="chartEl" class="chart">
          <div :style="{ zoom }">
            <ul v-for="root in chart.trees" :key="root.id" class="tree">
              <OrgNode :member="root" :collapsed="collapsed" @toggle="toggle" />
            </ul>
          </div>
        </div>
      </MtCard>
      <MtCard v-if="chart.unassigned.length" :title="t('organigram.unassigned')">
        <ul class="unassigned">
          <OrgNode v-for="m in chart.unassigned" :key="m.id" :member="m" :collapsed="collapsed" />
        </ul>
      </MtCard>
    </template>
  </section>
</template>

<style scoped>
.organigram.fs-page { max-width: none; }
.actions { display: flex; flex-wrap: wrap; gap: 8px; }
.legend { display: flex; flex-wrap: wrap; gap: 6px 16px; list-style: none; padding: 0; margin: 0 0 12px; font-size: 13px; color: var(--fs-muted); }
.legend li { display: flex; align-items: center; gap: 6px; }
.legend span { width: 10px; height: 10px; border-radius: 3px; }
/* Wide trees scroll inside the card instead of widening the page. */
.chart { overflow-x: auto; padding: 8px 0 16px; }
.tree { display: flex; justify-content: center; width: max-content; min-width: 100%; padding: 0; margin: 0 0 32px; }
.tree:last-child { margin-bottom: 0; }
.unassigned { display: flex; flex-wrap: wrap; gap: 12px; padding: 0; margin: 0; }
.unassigned :deep(li) { padding: 0; }
.unassigned :deep(li::before), .unassigned :deep(li::after) { display: none; }
</style>
