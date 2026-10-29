/** Capítulos del manual (docs/manual), en cada idioma. */
const PAGES = {
  es: { home: '', settings: 'ajustes', shortcuts: 'atajos', focus: 'enfoque', api: 'api' },
  en: { home: '', settings: 'settings', shortcuts: 'shortcuts', focus: 'focus', api: 'api' },
} as const

export type ManualPage = keyof (typeof PAGES)['es']

/** Dirección del manual que sirve la propia instalación, en el idioma de la interfaz. */
export function manualUrl(language: string, page: ManualPage = 'home', anchor?: string): string {
  const lang = language.startsWith('es') ? 'es' : 'en'
  const base = lang === 'es' ? '/manual/' : '/manual/en/'
  const slug = PAGES[lang][page]
  return base + (slug ? `${slug}.html` : '') + (anchor ? `#${anchor}` : '')
}
