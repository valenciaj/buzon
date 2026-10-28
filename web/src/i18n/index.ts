import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'
import es from './es.json'
import en from './en.json'

export const LANGUAGES = ['es', 'en'] as const

function initial(): string {
  try {
    const saved = localStorage.getItem('buzon-lang')
    if (saved && (LANGUAGES as readonly string[]).includes(saved)) return saved
  } catch {
    /* sin almacenamiento */
  }
  return navigator.language?.toLowerCase().startsWith('es') ? 'es' : 'en'
}

i18n.use(initReactI18next).init({
  resources: { es: { translation: es }, en: { translation: en } },
  lng: typeof navigator === 'undefined' ? 'es' : initial(),
  fallbackLng: 'es',
  interpolation: { escapeValue: false },
})

export function setLanguage(lng: string) {
  i18n.changeLanguage(lng)
  document.documentElement.lang = lng
  try {
    localStorage.setItem('buzon-lang', lng)
  } catch {
    /* sin almacenamiento */
  }
}

export default i18n
