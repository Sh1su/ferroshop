import { useI18n } from 'vue-i18n'

/** Shared value formatting for tables and detail pages. */
export function useFormat() {
  const { n, d, t } = useI18n()
  return {
    currency: (value: unknown, currency = 'EUR') =>
      typeof value === 'number' ? n(value, { style: 'currency', currency }) : '–',
    date: (value: unknown) =>
      typeof value === 'string' ? d(new Date(value), { dateStyle: 'medium', timeStyle: 'short' }) : '–',
    bool: (value: unknown) => (value ? t('common.yes') : t('common.no')),
    percent: (value: unknown) => (typeof value === 'number' ? `${n(value)} %` : '–'),
  }
}
