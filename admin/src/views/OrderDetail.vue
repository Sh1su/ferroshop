<script setup lang="ts">
import { MtBanner, MtButton, MtCard, MtLoader } from '@shopware-ag/meteor-component-library'
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { ApiError, orderTransitions, repository, transitionOrder, type Order, type OrderAddress, type OrderStates } from '../api/client'
import { can } from '../api/session'
import StateBadge from '../components/StateBadge.vue'
import { useFormat } from '../format'

const props = defineProps<{ id: string }>()
const { t } = useI18n()
const fmt = useFormat()

const order = ref<Order | null>(null)
const states = ref<OrderStates | null>(null)
const loading = ref(true)
const busy = ref('')
const error = ref('')

const machines = [
  { key: 'state', api: 'state', label: 'fields.state' },
  { key: 'paymentState', api: 'payment-state', label: 'fields.paymentState' },
  { key: 'deliveryState', api: 'delivery-state', label: 'fields.deliveryState' },
] as const

async function load() {
  loading.value = true
  try {
    const [o, s] = await Promise.all([
      repository<Order>('order').get(props.id, {
        associations: {
          lineItems: { sort: [{ field: 'position', order: 'ASC' }] },
          billingAddress: { associations: { country: {} } },
          shippingAddress: { associations: { country: {} } },
          paymentMethod: {},
          shippingMethod: {},
          salesChannel: {},
        },
      }),
      orderTransitions(props.id),
    ])
    order.value = o
    states.value = s
  } catch (e) {
    error.value = e instanceof ApiError && e.status === 404 ? t('common.notFound') : t('common.error')
  } finally {
    loading.value = false
  }
}

async function transition(machine: string, name: string) {
  busy.value = `${machine}.${name}`
  error.value = ''
  try {
    states.value = await transitionOrder(props.id, machine, name)
  } catch (e) {
    error.value = e instanceof Error ? e.message : t('common.error')
  } finally {
    busy.value = ''
  }
}

function addressLines(address: OrderAddress | null | undefined): string[] {
  if (!address) return []
  return [
    address.company,
    `${address.firstName} ${address.lastName}`,
    address.street,
    `${address.zipcode ?? ''} ${address.city}`.trim(),
    (address as OrderAddress & { country?: { translated?: { name?: string } } }).country?.translated?.name,
  ].filter((line): line is string => !!line)
}

const canUpdate = computed(() => can('order', 'update'))
onMounted(load)
</script>

<template>
  <section class="fs-page">
    <div class="fs-page-header">
      <div>
        <RouterLink :to="{ name: 'entity.list', params: { module: 'orders' } }">← {{ t('modules.orders') }}</RouterLink>
        <h1>{{ t('order.title', { number: order?.orderNumber ?? '' }) }}</h1>
      </div>
    </div>
    <MtBanner v-if="error" variant="critical" closable @close="error = ''">{{ error }}</MtBanner>
    <MtLoader v-if="loading" />
    <template v-else-if="order && states">
      <MtCard :title="t('order.status')">
        <div class="states">
          <div v-for="machine in machines" :key="machine.key" class="state">
            <p class="fs-muted label">{{ t(machine.label) }}</p>
            <StateBadge :state="states[machine.key].current" />
            <div v-if="canUpdate" class="actions">
              <MtButton
                v-for="name in states[machine.key].transitions"
                :key="name"
                size="small"
                :variant="name === 'cancel' ? 'critical' : 'secondary'"
                :is-loading="busy === `${machine.api}.${name}`"
                @click="transition(machine.api, name)"
              >
                {{ t(`transitions.${name}`) }}
              </MtButton>
            </div>
          </div>
        </div>
      </MtCard>

      <MtCard :title="t('order.positions')">
        <div class="fs-table-scroll">
          <table class="fs-table">
            <thead>
              <tr>
                <th>{{ t('fields.label') }}</th>
                <th class="num">{{ t('fields.quantity') }}</th>
                <th class="num">{{ t('fields.unitPrice') }}</th>
                <th class="num hide-sm">{{ t('fields.taxRate') }}</th>
                <th class="num">{{ t('fields.totalPrice') }}</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="item in order.lineItems" :key="item.id">
                <td>
                  <RouterLink v-if="item.productId" :to="{ name: 'product.detail', params: { id: item.productId } }">{{ item.label }}</RouterLink>
                  <template v-else>{{ item.label }}</template>
                </td>
                <td class="num">{{ item.quantity }}</td>
                <td class="num">{{ fmt.currency(item.unitPrice) }}</td>
                <td class="num hide-sm">{{ fmt.percent(item.taxRate) }}</td>
                <td class="num">{{ fmt.currency(item.totalPrice) }}</td>
              </tr>
            </tbody>
            <tfoot>
              <tr><td colspan="4" class="num">{{ t('order.positionPrice') }}</td><td class="num">{{ fmt.currency(order.positionPrice) }}</td></tr>
              <tr><td colspan="4" class="num">{{ t('order.shipping') }}</td><td class="num">{{ fmt.currency(order.shippingTotal) }}</td></tr>
              <tr><td colspan="4" class="num">{{ t('order.net') }}</td><td class="num">{{ fmt.currency(order.amountNet) }}</td></tr>
              <tr class="total"><td colspan="4" class="num">{{ t('fields.amountTotal') }}</td><td class="num">{{ fmt.currency(order.amountTotal) }}</td></tr>
            </tfoot>
          </table>
        </div>
      </MtCard>

      <div class="columns">
        <MtCard :title="t('order.customer')">
          <p>
            <RouterLink v-if="order.customerId" :to="{ name: 'customer.detail', params: { id: order.customerId } }">
              {{ order.customerFirstName }} {{ order.customerLastName }}
            </RouterLink>
            <template v-else>{{ order.customerFirstName }} {{ order.customerLastName }}</template>
          </p>
          <p class="fs-muted">{{ order.customerEmail }}</p>
          <dl>
            <dt>{{ t('fields.orderDate') }}</dt><dd>{{ fmt.date(order.orderDateTime) }}</dd>
            <dt>{{ t('fields.paymentMethod') }}</dt><dd>{{ order.paymentMethod?.translated?.name ?? '–' }}</dd>
            <dt>{{ t('fields.shippingMethod') }}</dt><dd>{{ order.shippingMethod?.translated?.name ?? '–' }}</dd>
            <dt>{{ t('fields.salesChannel') }}</dt><dd>{{ order.salesChannel?.translated?.name ?? '–' }}</dd>
          </dl>
          <p v-if="order.customerComment" class="comment">{{ order.customerComment }}</p>
        </MtCard>
        <MtCard :title="t('order.billingAddress')">
          <p v-for="line in addressLines(order.billingAddress)" :key="line" class="line">{{ line }}</p>
        </MtCard>
        <MtCard :title="t('order.shippingAddress')">
          <p v-for="line in addressLines(order.shippingAddress)" :key="line" class="line">{{ line }}</p>
        </MtCard>
      </div>
    </template>
  </section>
</template>

<style scoped>
.states { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 16px; }
.label { margin: 0 0 6px; font-size: 13px; }
.actions { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 10px; }
.columns { display: grid; grid-template-columns: repeat(auto-fit, minmax(260px, 1fr)); gap: 0 16px; }
.line { margin: 0 0 4px; }
dl { display: grid; grid-template-columns: auto 1fr; gap: 4px 12px; font-size: 14px; }
dt { color: var(--fs-muted); }
dd { margin: 0; }
tfoot td { border-bottom: 0; padding-top: 6px; padding-bottom: 6px; }
tfoot .total td { font-weight: 700; border-top: 1px solid var(--fs-border); }
.comment { background: var(--fs-bg); padding: 8px 12px; border-radius: 6px; }
</style>
