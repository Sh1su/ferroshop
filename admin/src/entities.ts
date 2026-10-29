/**
 * Configuration for the generic list/detail pages. Each module names its API entity,
 * the list columns and the form fields; labels are i18n keys under `fields.*`.
 */

import {
  COUNTRY_DE,
  CURRENCY_DEFAULT,
  CUSTOMER_GROUP_DEFAULT,
  LANGUAGE_DE,
  PAYMENT_PREPAYMENT,
  ROOT_CATEGORY,
  SHIPPING_STANDARD,
} from './api/defaults'

export type ColumnFormat = 'text' | 'currency' | 'date' | 'bool' | 'state' | 'percent'

export interface Column {
  /** Dotted path into the row, e.g. `translated.name` or `group.translated.name`. */
  path: string
  label: string
  /** Sort field for the API; omitted = not sortable. */
  sort?: string
  format?: ColumnFormat
  hideSmall?: boolean
}

export type FieldType = 'text' | 'textarea' | 'int' | 'float' | 'bool' | 'password' | 'select' | 'multi' | 'price'

export interface FormField {
  name: string
  label: string
  type: FieldType
  required?: boolean
  /** For select/multi: entity to load options from and the property to show. */
  entity?: string
  optionLabel?: string
  /** Static options for select. */
  options?: { value: string; label: string }[]
  /** Initial value for new entities (switches default to on). */
  default?: unknown
  /** Only show when editing (e.g. computed values) or only when creating. */
  only?: 'create' | 'edit'
  /** Only administrators may change it (admin flag, roles). */
  adminOnly?: boolean
}

export interface EntityModule {
  /** Route segment and i18n key (`modules.<key>`). */
  key: string
  entity: string
  group: 'catalog' | 'sales' | 'settings'
  columns: Column[]
  fields: FormField[]
  defaultSort: string
  associations?: Record<string, object>
  /** Custom detail route name; otherwise the generic detail page is used. */
  detailRoute?: string
  /** Hide the "create" button (e.g. orders come from the shop). */
  noCreate?: boolean
  titleField: string
}

const translatedName: Column = { path: 'translated.name', label: 'fields.name', sort: 'name' }

export const modules: EntityModule[] = [
  {
    key: 'manufacturers',
    entity: 'product_manufacturer',
    group: 'catalog',
    columns: [translatedName, { path: 'link', label: 'fields.link', hideSmall: true }],
    fields: [
      { name: 'name', label: 'fields.name', type: 'text', required: true },
      { name: 'link', label: 'fields.link', type: 'text' },
      { name: 'description', label: 'fields.description', type: 'textarea' },
    ],
    defaultSort: 'name',
    titleField: 'translated.name',
  },
  {
    key: 'properties',
    entity: 'property_group',
    group: 'catalog',
    columns: [
      translatedName,
      { path: 'displayType', label: 'fields.displayType', hideSmall: true },
      { path: 'filterable', label: 'fields.filterable', format: 'bool' },
      { path: 'position', label: 'fields.position', sort: 'position', hideSmall: true },
    ],
    fields: [
      { name: 'name', label: 'fields.name', type: 'text', required: true },
      { name: 'description', label: 'fields.description', type: 'textarea' },
      {
        name: 'displayType',
        label: 'fields.displayType',
        type: 'select',
        default: 'text',
        options: [
          { value: 'text', label: 'Text' },
          { value: 'color', label: 'Color' },
          { value: 'media', label: 'Media' },
        ],
      },
      {
        name: 'sortingType',
        label: 'fields.sortingType',
        type: 'select',
        default: 'alphanumeric',
        options: [
          { value: 'alphanumeric', label: 'A–Z' },
          { value: 'position', label: 'Position' },
        ],
      },
      { name: 'position', label: 'fields.position', type: 'int', default: 0 },
      { name: 'filterable', label: 'fields.filterable', type: 'bool', default: true },
      { name: 'visibleOnProductDetailPage', label: 'fields.visibleOnProductDetailPage', type: 'bool', default: true },
    ],
    defaultSort: 'name',
    titleField: 'translated.name',
  },
  {
    key: 'propertyOptions',
    entity: 'property_group_option',
    group: 'catalog',
    columns: [
      translatedName,
      { path: 'group.translated.name', label: 'fields.propertyGroup' },
      { path: 'colorHexCode', label: 'fields.colorHexCode', hideSmall: true },
      { path: 'position', label: 'fields.position', sort: 'position', hideSmall: true },
    ],
    fields: [
      { name: 'name', label: 'fields.name', type: 'text', required: true },
      {
        name: 'groupId',
        label: 'fields.propertyGroup',
        type: 'select',
        entity: 'property_group',
        optionLabel: 'translated.name',
        required: true,
      },
      { name: 'position', label: 'fields.position', type: 'int', default: 0 },
      { name: 'colorHexCode', label: 'fields.colorHexCode', type: 'text' },
    ],
    associations: { group: {} },
    defaultSort: 'name',
    titleField: 'translated.name',
  },
  {
    key: 'cmsPages',
    entity: 'cms_page',
    group: 'catalog',
    columns: [translatedName, { path: 'type', label: 'fields.pageType' }],
    fields: [
      { name: 'name', label: 'fields.name', type: 'text', required: true },
      {
        name: 'type',
        label: 'fields.pageType',
        type: 'select',
        default: 'landingpage',
        options: [
          { value: 'landingpage', label: 'Landing page' },
          { value: 'product_list', label: 'Listing page' },
          { value: 'page', label: 'Shop page' },
        ],
      },
    ],
    defaultSort: 'name',
    titleField: 'translated.name',
  },
  {
    key: 'categories',
    entity: 'category',
    group: 'catalog',
    columns: [
      translatedName,
      { path: 'parent.translated.name', label: 'fields.parent' },
      { path: 'childCount', label: 'fields.childCount', sort: 'childCount' },
      { path: 'active', label: 'fields.active', format: 'bool' },
    ],
    fields: [
      { name: 'name', label: 'fields.name', type: 'text', required: true },
      { name: 'parentId', label: 'fields.parent', type: 'select', entity: 'category', optionLabel: 'translated.name' },
      { name: 'cmsPageId', label: 'fields.cmsPage', type: 'select', entity: 'cms_page', optionLabel: 'translated.name' },
      { name: 'active', label: 'fields.active', type: 'bool' },
      { name: 'visible', label: 'fields.visible', type: 'bool' },
      { name: 'position', label: 'fields.position', type: 'int' },
      { name: 'description', label: 'fields.description', type: 'textarea' },
      { name: 'metaTitle', label: 'fields.metaTitle', type: 'text' },
      { name: 'metaDescription', label: 'fields.metaDescription', type: 'textarea' },
    ],
    associations: { parent: {} },
    defaultSort: 'name',
    titleField: 'translated.name',
  },
  {
    key: 'orders',
    entity: 'order',
    group: 'sales',
    columns: [
      { path: 'orderNumber', label: 'fields.orderNumber', sort: 'orderNumber' },
      { path: 'orderDateTime', label: 'fields.orderDate', sort: 'orderDateTime', format: 'date' },
      { path: 'customerLastName', label: 'fields.customer', sort: 'customerLastName' },
      { path: 'amountTotal', label: 'fields.amountTotal', sort: 'amountTotal', format: 'currency' },
      { path: 'state', label: 'fields.state', format: 'state' },
      { path: 'paymentState', label: 'fields.paymentState', format: 'state', hideSmall: true },
      { path: 'deliveryState', label: 'fields.deliveryState', format: 'state', hideSmall: true },
    ],
    fields: [],
    defaultSort: '-orderDateTime',
    detailRoute: 'order.detail',
    noCreate: true,
    titleField: 'orderNumber',
  },
  {
    key: 'customers',
    entity: 'customer',
    group: 'sales',
    columns: [
      { path: 'customerNumber', label: 'fields.customerNumber', sort: 'customerNumber' },
      { path: 'lastName', label: 'fields.lastName', sort: 'lastName' },
      { path: 'firstName', label: 'fields.firstName', sort: 'firstName' },
      { path: 'email', label: 'fields.email', sort: 'email', hideSmall: true },
      { path: 'orderCount', label: 'fields.orderCount', sort: 'orderCount' },
      { path: 'active', label: 'fields.active', format: 'bool', hideSmall: true },
    ],
    fields: [],
    defaultSort: 'lastName',
    detailRoute: 'customer.detail',
    titleField: 'lastName',
  },
  {
    key: 'taxes',
    entity: 'tax',
    group: 'settings',
    columns: [
      { path: 'name', label: 'fields.name', sort: 'name' },
      { path: 'taxRate', label: 'fields.taxRate', sort: 'taxRate', format: 'percent' },
    ],
    fields: [
      { name: 'name', label: 'fields.name', type: 'text', required: true },
      { name: 'taxRate', label: 'fields.taxRate', type: 'float', required: true, default: 19 },
      { name: 'position', label: 'fields.position', type: 'int', default: 0 },
    ],
    defaultSort: 'position',
    titleField: 'name',
  },
  {
    key: 'taxRules', entity: 'tax_rule', group: 'settings', columns: [{ path: 'tax.name', label: 'fields.tax' }, { path: 'country.translated.name', label: 'fields.country' }, { path: 'taxRate', label: 'fields.taxRate', sort: 'taxRate', format: 'percent' }, { path: 'priority', label: 'fields.priority', sort: 'priority' }], fields: [{ name: 'taxId', label: 'fields.taxClass', type: 'select', entity: 'tax', optionLabel: 'name', required: true }, { name: 'countryId', label: 'fields.country', type: 'select', entity: 'country', optionLabel: 'translated.name', required: true, default: COUNTRY_DE }, { name: 'taxRate', label: 'fields.taxRate', type: 'float', required: true, default: 19 }, { name: 'priority', label: 'fields.priority', type: 'int', default: 1 }], associations: { tax: {}, country: {} }, defaultSort: 'priority', titleField: 'taxRate' }, { key: 'currencies',
    entity: 'currency',
    group: 'settings',
    columns: [
      translatedName,
      { path: 'isoCode', label: 'fields.isoCode', sort: 'isoCode' },
      { path: 'symbol', label: 'fields.symbol' },
      { path: 'factor', label: 'fields.factor', sort: 'factor' },
    ],
    fields: [
      { name: 'name', label: 'fields.name', type: 'text', required: true },
      { name: 'shortName', label: 'fields.shortName', type: 'text', required: true },
      { name: 'isoCode', label: 'fields.isoCode', type: 'text', required: true },
      { name: 'symbol', label: 'fields.symbol', type: 'text', required: true },
      { name: 'factor', label: 'fields.factor', type: 'float', required: true, default: 1 },
      { name: 'decimalPrecision', label: 'fields.decimalPrecision', type: 'int', default: 2 },
      { name: 'position', label: 'fields.position', type: 'int', default: 0 },
    ],
    defaultSort: 'position',
    titleField: 'translated.name',
  },
  {
    key: 'languages',
    entity: 'language',
    group: 'settings',
    columns: [
      { path: 'name', label: 'fields.name', sort: 'name' },
      { path: 'localeCode', label: 'fields.localeCode', sort: 'localeCode' },
    ],
    fields: [
      { name: 'name', label: 'fields.name', type: 'text', required: true },
      { name: 'localeCode', label: 'fields.localeCode', type: 'text', required: true },
      { name: 'parentId', label: 'fields.parentLanguage', type: 'select', entity: 'language', optionLabel: 'name' },
    ],
    defaultSort: 'name',
    titleField: 'name',
  },
  {
    key: 'countries',
    entity: 'country',
    group: 'settings',
    columns: [
      translatedName,
      { path: 'iso', label: 'fields.iso', sort: 'iso' },
      { path: 'active', label: 'fields.active', format: 'bool' },
    ],
    fields: [
      { name: 'name', label: 'fields.name', type: 'text', required: true },
      { name: 'iso', label: 'fields.iso', type: 'text', required: true },
      { name: 'iso3', label: 'fields.iso3', type: 'text' },
      { name: 'active', label: 'fields.active', type: 'bool' },
      { name: 'position', label: 'fields.position', type: 'int' },
    ],
    defaultSort: 'position',
    titleField: 'translated.name',
  },
  {
    key: 'paymentMethods',
    entity: 'payment_method',
    group: 'settings',
    columns: [
      translatedName,
      { path: 'technicalName', label: 'fields.technicalName' },
      { path: 'active', label: 'fields.active', format: 'bool' },
    ],
    fields: [
      { name: 'name', label: 'fields.name', type: 'text', required: true },
      { name: 'technicalName', label: 'fields.technicalName', type: 'text', required: true },
      { name: 'description', label: 'fields.description', type: 'textarea' },
      { name: 'active', label: 'fields.active', type: 'bool' },
      { name: 'position', label: 'fields.position', type: 'int' },
    ],
    defaultSort: 'position',
    titleField: 'translated.name',
  },
  {
    key: 'shippingMethods',
    entity: 'shipping_method',
    group: 'settings',
    columns: [
      translatedName,
      { path: 'technicalName', label: 'fields.technicalName' },
      { path: 'active', label: 'fields.active', format: 'bool' },
    ],
    fields: [
      { name: 'name', label: 'fields.name', type: 'text', required: true },
      { name: 'technicalName', label: 'fields.technicalName', type: 'text', required: true },
      { name: 'price', label: 'fields.price', type: 'price' },
      { name: 'description', label: 'fields.description', type: 'textarea' },
      { name: 'active', label: 'fields.active', type: 'bool' },
      { name: 'position', label: 'fields.position', type: 'int' },
    ],
    defaultSort: 'position',
    titleField: 'translated.name',
  },
  {
    key: 'customerGroups',
    entity: 'customer_group',
    group: 'settings',
    columns: [translatedName, { path: 'displayGross', label: 'fields.displayGross', format: 'bool' }],
    fields: [
      { name: 'name', label: 'fields.name', type: 'text', required: true },
      { name: 'displayGross', label: 'fields.displayGross', type: 'bool' },
    ],
    defaultSort: 'name',
    titleField: 'translated.name',
  },
  {
    key: 'salesChannelDomains',
    entity: 'sales_channel_domain',
    group: 'settings',
    columns: [
      { path: 'url', label: 'fields.domainUrl', sort: 'url' },
      { path: 'salesChannel.translated.name', label: 'fields.salesChannel' },
      { path: 'language.name', label: 'fields.language' },
      { path: 'currency.isoCode', label: 'fields.currency' },
    ],
    fields: [
      { name: 'url', label: 'fields.domainUrl', type: 'text', required: true },
      { name: 'salesChannelId', label: 'fields.salesChannel', type: 'select', entity: 'sales_channel', optionLabel: 'translated.name', required: true },
      { name: 'languageId', label: 'fields.language', type: 'select', entity: 'language', optionLabel: 'name', required: true, default: LANGUAGE_DE },
      { name: 'currencyId', label: 'fields.currency', type: 'select', entity: 'currency', optionLabel: 'translated.name', required: true, default: CURRENCY_DEFAULT },
      { name: 'browserLanguage', label: 'fields.browserLanguage', type: 'bool', default: false },
    ],
    associations: { salesChannel: {}, language: {}, currency: {} },
    defaultSort: 'url',
    titleField: 'url',
  },
  {
    key: 'salesChannels',
    entity: 'sales_channel',
    group: 'settings',
    columns: [
      translatedName,
      { path: 'accessKey', label: 'fields.accessKey', hideSmall: true },
      { path: 'active', label: 'fields.active', format: 'bool' },
    ],
    fields: [
      { name: 'name', label: 'fields.name', type: 'text', required: true },
      { name: 'accessKey', label: 'fields.accessKey', type: 'text', required: true },
      { name: 'languageId', label: 'fields.language', type: 'select', entity: 'language', optionLabel: 'name', required: true, default: LANGUAGE_DE },
      { name: 'currencyId', label: 'fields.currency', type: 'select', entity: 'currency', optionLabel: 'translated.name', required: true, default: CURRENCY_DEFAULT },
      {
        name: 'navigationCategoryId',
        label: 'fields.navigationCategory',
        type: 'select',
        entity: 'category',
        optionLabel: 'translated.name',
        default: ROOT_CATEGORY,
      },
      { name: 'customerGroupId', label: 'fields.customerGroup', type: 'select', entity: 'customer_group', optionLabel: 'translated.name', default: CUSTOMER_GROUP_DEFAULT },
      { name: 'countryId', label: 'fields.country', type: 'select', entity: 'country', optionLabel: 'translated.name', default: COUNTRY_DE },
      { name: 'paymentMethodId', label: 'fields.paymentMethod', type: 'select', entity: 'payment_method', optionLabel: 'translated.name', default: PAYMENT_PREPAYMENT },
      { name: 'shippingMethodId', label: 'fields.shippingMethod', type: 'select', entity: 'shipping_method', optionLabel: 'translated.name', default: SHIPPING_STANDARD },
      { name: 'active', label: 'fields.active', type: 'bool', default: false },
    ],
    defaultSort: 'name',
    titleField: 'translated.name',
  },
  {
    key: 'users',
    entity: 'user',
    group: 'settings',
    columns: [
      { path: 'username', label: 'fields.username', sort: 'username' },
      { path: 'firstName', label: 'fields.firstName', sort: 'firstName', hideSmall: true },
      { path: 'lastName', label: 'fields.lastName', sort: 'lastName', hideSmall: true },
      { path: 'email', label: 'fields.email', sort: 'email' },
      { path: 'admin', label: 'fields.admin', format: 'bool' },
      { path: 'active', label: 'fields.active', format: 'bool' },
    ],
    fields: [
      { name: 'username', label: 'fields.username', type: 'text', required: true },
      { name: 'email', label: 'fields.email', type: 'text', required: true },
      { name: 'firstName', label: 'fields.firstName', type: 'text' },
      { name: 'lastName', label: 'fields.lastName', type: 'text' },
      { name: 'password', label: 'fields.password', type: 'password' },
      {
        name: 'localeCode',
        label: 'fields.localeCode',
        type: 'select',
        options: [
          { value: 'de-DE', label: 'Deutsch' },
          { value: 'en-GB', label: 'English' },
        ],
      },
      { name: 'active', label: 'fields.active', type: 'bool' },
      { name: 'admin', label: 'fields.admin', type: 'bool', default: false, adminOnly: true },
      { name: 'aclRoles', label: 'fields.roles', type: 'multi', entity: 'acl_role', optionLabel: 'name', adminOnly: true },
    ],
    associations: { aclRoles: {} },
    defaultSort: 'username',
    titleField: 'username',
  },
  {
    key: 'roles',
    entity: 'acl_role',
    group: 'settings',
    columns: [
      { path: 'name', label: 'fields.name', sort: 'name' },
      { path: 'description', label: 'fields.description', hideSmall: true },
    ],
    fields: [],
    defaultSort: 'name',
    detailRoute: 'role.detail',
    titleField: 'name',
  },
]

export const moduleByKey = (key: string) => modules.find((m) => m.key === key)

/** Reads a dotted path from an object. */
export function at(obj: unknown, path: string): unknown {
  return path.split('.').reduce<unknown>((value, key) => (value as Record<string, unknown> | null | undefined)?.[key], obj)
}

/** `-field` = descending. */
export function parseSort(sort: string) {
  return sort.startsWith('-') ? { field: sort.slice(1), order: 'DESC' as const } : { field: sort, order: 'ASC' as const }
}
