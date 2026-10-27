<script setup lang="ts">
import { useI18n } from 'vue-i18n'

export interface OrgMember {
  id: string
  name: string
  jobTitle: string
  department: string
  active: boolean
  color: string
  reports: OrgMember[]
  /** All members below this one, not only direct reports. */
  teamSize: number
}

defineProps<{ member: OrgMember; collapsed: Set<string> }>()
const emit = defineEmits<{ toggle: [id: string] }>()
const { t } = useI18n()

const initials = (name: string) =>
  name
    .split(' ')
    .map((part) => part[0])
    .slice(0, 2)
    .join('')
</script>

<template>
  <li>
    <div class="card" :class="{ inactive: !member.active }" :style="{ '--dept': member.color }">
      <span class="avatar">{{ initials(member.name) }}</span>
      <div class="text">
        <strong>{{ member.name }}</strong>
        <span v-if="member.jobTitle" class="title">{{ member.jobTitle }}</span>
        <span v-if="member.department" class="dept">{{ member.department }}</span>
        <span v-if="!member.active" class="badge">{{ t('organigram.inactive') }}</span>
      </div>
      <button
        v-if="member.reports.length"
        type="button"
        class="toggle"
        :aria-expanded="!collapsed.has(member.id)"
        :title="t(collapsed.has(member.id) ? 'organigram.expand' : 'organigram.collapse')"
        @click="emit('toggle', member.id)"
      >
        {{ collapsed.has(member.id) ? `+${member.teamSize}` : '−' }}
      </button>
    </div>
    <ul v-if="member.reports.length && !collapsed.has(member.id)">
      <OrgNode
        v-for="report in member.reports"
        :key="report.id"
        :member="report"
        :collapsed="collapsed"
        @toggle="(id) => emit('toggle', id)"
      />
    </ul>
  </li>
</template>

<style scoped>
.card {
  position: relative; display: flex; align-items: flex-start; gap: 10px; width: 210px; padding: 10px 12px;
  background: var(--fs-surface); border: 1px solid var(--fs-border); border-top: 3px solid var(--dept);
  border-radius: 8px; text-align: left; box-shadow: 0 1px 2px rgb(0 0 0 / .04);
}
.card.inactive { opacity: .55; border-style: dashed; border-top-style: solid; }
.avatar {
  flex-shrink: 0; width: 34px; height: 34px; border-radius: 50%; display: grid; place-items: center;
  background: var(--dept); color: #fff; font-size: 13px; font-weight: 600;
}
.text { display: flex; flex-direction: column; min-width: 0; font-size: 13px; line-height: 1.35; }
.text strong { font-size: 14px; }
.title { color: var(--fs-text); }
.dept { color: var(--fs-muted); font-size: 12px; }
.badge { align-self: flex-start; margin-top: 4px; font-size: 11px; padding: 1px 6px; border-radius: 4px; background: var(--fs-bg); color: var(--fs-muted); }
.toggle {
  position: absolute; left: 50%; bottom: -11px; transform: translateX(-50%); z-index: 1;
  min-width: 22px; height: 22px; padding: 0 6px; border-radius: 11px; border: 1px solid var(--fs-border);
  background: var(--fs-surface); color: var(--fs-muted); font-size: 12px; line-height: 20px; cursor: pointer;
}
.toggle:hover { color: var(--fs-accent); border-color: var(--fs-accent); }

/* Connector lines: each child draws its half of the horizontal bar plus a vertical stub. */
ul { display: flex; justify-content: center; padding: 24px 0 0; margin: 0; position: relative; }
li { list-style: none; display: flex; flex-direction: column; align-items: center; position: relative; padding: 24px 8px 0; }
ul::before { content: ''; position: absolute; top: 0; left: 50%; height: 24px; border-left: 1px solid var(--fs-border); }
li::before, li::after { content: ''; position: absolute; top: 0; width: 50%; height: 24px; border-top: 1px solid var(--fs-border); }
li::before { right: 50%; }
li::after { left: 50%; border-left: 1px solid var(--fs-border); }
li:first-child::before, li:last-child::after { border-top: 0; }
li:only-child::before { display: none; }
li:only-child { padding-top: 0; }
li:only-child::after { border-top: 0; height: 0; }
</style>
