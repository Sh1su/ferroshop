import { createI18n } from 'vue-i18n'
import de from './de.json'
import en from './en.json'

const STORAGE_KEY = 'ferroshop.locale'

function initialLocale(): 'de' | 'en' {
  try {
    const stored = localStorage.getItem(STORAGE_KEY)
    if (stored === 'de' || stored === 'en') return stored
  } catch {
    /* storage unavailable */
  }
  return navigator.language.startsWith('de') ? 'de' : 'en'
}

export const i18n = createI18n({
  legacy: false,
  locale: initialLocale(),
  fallbackLocale: 'en',
  messages: { de, en },
})

export function setLocale(locale: 'de' | 'en') {
  i18n.global.locale.value = locale
  document.documentElement.lang = locale
  try {
    localStorage.setItem(STORAGE_KEY, locale)
  } catch {
    /* storage unavailable */
  }
}
