<script setup lang="ts">
import {
  MtBanner,
  MtButton,
  MtCard,
  MtLoader,
  MtSelect,
  MtSwitch,
  MtTextField,
} from '@shopware-ag/meteor-component-library'
import { computed, reactive, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import { ApiError, repository, type Customer, type CustomerAddress, type Order } from '../api/client'
import { can } from '../api/session'
import StateBadge from '../components/StateBadge.vue'
import { useFormat } from '../format'

const props = defineProps<{ id: string }>()
const { t, te } = useI18n()
const router = useRouter()
const fmt = useFormat()
const customers = repository<Customer>('customer')
const addresses = repository<CustomerAddress>('customer-address')
const isNew = computed(() => props.id === 'new')

const DEFAULT_GROUP = '0191a6f0f4a07c7e9b1c2d3e4f5a6e01'
const DEFAULT_CHANNEL = '98432def39fc4624b33213a56b8c944d'
const SYSTEM_LANGUAGE = '2fbb5fe2e29a4d70aa5854ce7ce3e20b'
const GERMANY = '0191a6f0f4a07c7e9b1c2d3e4f5a7001'

const loading = ref(true)
const saving = ref(false)
const notice = ref<{ variant: 'positive' | 'critical'; text: string } | null>(null)
const errors = ref<Record<string, { code: string; detail: string }>>({})
const customer = ref<Customer | null>(null)
const orders = ref<Order[]>([])
const groups = ref<{ value: string; label: string }[]>([])
const countries = ref<{ value: string; label: string }[]>([])

const form = reactive({
  firstName: '',
  lastName: '',
  email: '',
  company: '' as string | null,
  groupId: DEFAULT_GROUP,
  active: true,
})
const newAddress = reactive({ firstName: '', lastName: '', street: '', zipcode: '', city: '', countryId: GERMANY })
const showAddressForm = ref(false)

async function load() {
  loading.value = true
  errors.value = {}
  try {
    const [g, c] = await Promise.all([
      repository<{ id: string; translated: { name: string } }>('customer-group').search({}),
      repository<{ id: string; translated: { name: string } }>('country').search({ sort: [{ field: 'position', order: 'ASC' }] }),
    ])
    groups.value = g.data.map((x) => ({ value: x.id, label: x.translated.name }))
    countries.value = c.data.map((x) => ({ value: x.id, label: x.translated.name }))
    if (isNew.value) {
      customer.value = null
      orders.value = []
      return
    }
    customer.value = await customers.get(props.id, {
      associations: { addresses: { associations: { country: {} } }, group: {} },
    })
    Object.assign(form, {
      firstName: customer.value.firstName,
      lastName: customer.value.lastName,
      email: customer.value.email,
      company: customer.value.company ?? '',
      groupId: customer.value.groupId,
      active: customer.value.active ?? true,
    })
    if (can('order')) {
      orders.value = (
        await repository<Order>('order').search({
          filter: [{ type: 'equals', field: 'customerId', value: props.id }],
          sort: [{ field: 'orderDateTime', order: 'DESC' }],
        })
      ).data
    }
  } catch (e) {
    notice.value = { variant: 'critical', text: e instanceof ApiError && e.status === 404 ? t('common.notFound') : t('common.error') }
  } finally {
    loading.value = false
  }
}

function showErrors(e: unknown) {
  if (e instanceof ApiError) {
    errors.value = Object.fromEntries(
      Object.entries(e.byPointer()).map(([p, err]) => [p, { ...err, detail: te(`errors.${err.code}`) ? t(`errors.${err.code}`) : err.detail }]),
    )
    notice.value = { variant: 'critical', text: e.status === 400 ? t('errors.checkInput') : e.message }
  } else {
    notice.value = { variant: 'critical', text: t('common.error') }
  }
}

async function save() {
  saving.value = true
  errors.value = {}
  notice.value = null
  const data = { ...form, company: form.company || null }
  try {
    if (isNew.value) {
      const created = await customers.create({
        ...data,
        salesChannelId: DEFAULT_CHANNEL,
        languageId: SYSTEM_LANGUAGE,
      } as Partial<Customer>)
      await router.replace({ name: 'customer.detail', params: { id: created.id } })
      return
    }
    await customers.update(props.id, data as Partial<Customer>)
    await load()
    notice.value = { variant: 'positive', text: t('common.saved') }
  } catch (e) {
    showErrors(e)
  } finally {
    saving.value = false
  }
}

async function addAddress() {
  errors.value = {}
  try {
    const created = await addresses.create({ ...newAddress, customerId: props.id } as Partial<CustomerAddress>)
    if (!customer.value?.defaultBillingAddressId) {
      await customers.update(props.id, { defaultBillingAddressId: created.id, defaultShippingAddressId: created.id })
    }
    Object.assign(newAddress, { street: '', zipcode: '', city: '' })
    showAddressForm.value = false
    await load()
  } catch (e) {
    showErrors(e)
  }
}

async function makeDefault(addressId: string, kind: 'defaultBillingAddressId' | 'defaultShippingAddressId') {
  await customers.update(props.id, { [kind]: addressId })
  await load()
}

async function removeAddress(addressId: string) {
  if (!confirm(t('common.confirmDelete', { name: t('customer.address') }))) return
  await addresses.delete(addressId)
  await load()
}

const canEdit = computed(() => can('customer', isNew.value ? 'create' : 'update'))
watch(() => props.id, load, { immediate: true })
</script>

<template>
  <section class="fs-page">
    <div class="fs-page-header">
      <div>
        <RouterLink :to="{ name: 'entity.list', params: { module: 'customers' } }">← {{ t('modules.customers') }}</RouterLink>
        <h1>{{ isNew ? t('customer.new') : `${customer?.firstName ?? ''} ${customer?.lastName ?? ''}` }}</h1>
        <p v-if="customer" class="fs-muted">{{ t('fields.customerNumber') }} {{ customer.customerNumber }}</p>
      </div>
      <MtButton v-if="canEdit" variant="primary" :is-loading="saving" :disabled="loading" @click="save">{{ t('common.save') }}</MtButton>
    </div>
    <MtBanner v-if="notice" :variant="notice.variant" closable @close="notice = null">{{ notice.text }}</MtBanner>
    <MtLoader v-if="loading" />
    <template v-else>
      <MtCard :title="t('customer.account')">
        <div class="fs-grid">
          <MtTextField v-model="form.firstName" :label="t('fields.firstName')" required :disabled="!canEdit" :error="errors['/firstName']" />
          <MtTextField v-model="form.lastName" :label="t('fields.lastName')" required :disabled="!canEdit" :error="errors['/lastName']" />
          <MtTextField v-model="form.email" :label="t('fields.email')" required :disabled="!canEdit" :error="errors['/email']" />
          <MtTextField v-model="form.company" :label="t('fields.company')" :disabled="!canEdit" />
          <MtSelect
            v-model="form.groupId"
            :label="t('fields.customerGroup')"
            :options="groups"
            label-property="label"
            value-property="value"
            hide-clearable-button
            :disabled="!canEdit"
          />
        </div>
        <MtSwitch v-model="form.active" :label="t('fields.active')" :disabled="!canEdit" />
      </MtCard>

      <MtCard v-if="customer" :title="t('customer.addresses')">
        <div class="addresses">
          <div v-for="address in customer.addresses" :key="address.id" class="address">
            <p class="line"><strong>{{ address.firstName }} {{ address.lastName }}</strong></p>
            <p v-if="address.company" class="line">{{ address.company }}</p>
            <p class="line">{{ address.street }}</p>
            <p class="line">{{ address.zipcode }} {{ address.city }}</p>
            <p class="line fs-muted">{{ address.country?.translated?.name }}</p>
            <p class="tags">
              <span v-if="address.id === customer.defaultBillingAddressId" class="tag">{{ t('customer.defaultBilling') }}</span>
              <span v-if="address.id === customer.defaultShippingAddressId" class="tag">{{ t('customer.defaultShipping') }}</span>
            </p>
            <div v-if="canEdit" class="address-actions">
              <MtButton v-if="address.id !== customer.defaultBillingAddressId" size="x-small" variant="secondary" @click="makeDefault(address.id!, 'defaultBillingAddressId')">
                {{ t('customer.setBilling') }}
              </MtButton>
              <MtButton v-if="address.id !== customer.defaultShippingAddressId" size="x-small" variant="secondary" @click="makeDefault(address.id!, 'defaultShippingAddressId')">
                {{ t('customer.setShipping') }}
              </MtButton>
              <MtButton size="x-small" variant="critical" ghost @click="removeAddress(address.id!)">{{ t('common.delete') }}</MtButton>
            </div>
          </div>
        </div>
        <template v-if="canEdit">
          <MtButton v-if="!showAddressForm" variant="secondary" @click="showAddressForm = true">{{ t('customer.addAddress') }}</MtButton>
          <form v-else class="fs-grid new-address" @submit.prevent="addAddress">
            <MtTextField v-model="newAddress.firstName" :label="t('fields.firstName')" required :error="errors['/firstName']" />
            <MtTextField v-model="newAddress.lastName" :label="t('fields.lastName')" required :error="errors['/lastName']" />
            <MtTextField v-model="newAddress.street" :label="t('fields.street')" required :error="errors['/street']" />
            <MtTextField v-model="newAddress.zipcode" :label="t('fields.zipcode')" />
            <MtTextField v-model="newAddress.city" :label="t('fields.city')" required :error="errors['/city']" />
            <MtSelect v-model="newAddress.countryId" :label="t('fields.country')" :options="countries" label-property="label" value-property="value" hide-clearable-button />
            <div class="form-actions">
              <MtButton variant="primary" type="submit">{{ t('common.save') }}</MtButton>
              <MtButton variant="secondary" @click="showAddressForm = false">{{ t('common.cancel') }}</MtButton>
            </div>
          </form>
        </template>
      </MtCard>

      <MtCard v-if="customer && can('order')" :title="`${t('modules.orders')} (${orders.length})`">
        <p v-if="!orders.length" class="fs-muted">{{ t('common.empty') }}</p>
        <div v-else class="fs-table-scroll">
          <table class="fs-table">
            <thead>
              <tr>
                <th>{{ t('fields.orderNumber') }}</th>
                <th>{{ t('fields.orderDate') }}</th>
                <th class="num">{{ t('fields.amountTotal') }}</th>
                <th>{{ t('fields.state') }}</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="order in orders" :key="order.id">
                <td><RouterLink :to="{ name: 'order.detail', params: { id: order.id } }">{{ order.orderNumber }}</RouterLink></td>
                <td>{{ fmt.date(order.orderDateTime) }}</td>
                <td class="num">{{ fmt.currency(order.amountTotal) }}</td>
                <td><StateBadge :state="order.state!" /></td>
              </tr>
            </tbody>
          </table>
        </div>
      </MtCard>
    </template>
  </section>
</template>

<style scoped>
.addresses { display: grid; grid-template-columns: repeat(auto-fill, minmax(220px, 1fr)); gap: 12px; margin-bottom: 16px; }
.address { border: 1px solid var(--fs-border); border-radius: 8px; padding: 12px; font-size: 14px; }
.line { margin: 0 0 2px; }
.tags { display: flex; gap: 6px; flex-wrap: wrap; margin: 8px 0 0; }
.tag { font-size: 12px; background: var(--fs-bg); border-radius: 999px; padding: 2px 8px; }
.address-actions { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 10px; }
.new-address { margin-top: 8px; }
.form-actions { display: flex; gap: 8px; align-items: end; padding-bottom: 20px; }
</style>
