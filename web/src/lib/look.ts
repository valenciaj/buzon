import '@fontsource/atkinson-hyperlegible-next/400.css'
import '@fontsource/atkinson-hyperlegible-next/700.css'
import '@fontsource/inter/400.css'
import '@fontsource/inter/600.css'
import '@fontsource/source-sans-3/400.css'
import '@fontsource/source-sans-3/600.css'
import '@fontsource/ibm-plex-sans/400.css'
import '@fontsource/ibm-plex-sans/600.css'
import '@fontsource/lexend/400.css'
import '@fontsource/lexend/600.css'
import type { Font, Preferences } from './api'

// Todas las fuentes van empaquetadas con la app: no dependen de servicios externos.
export const FONTS: Record<Font, { label: string; stack: string }> = {
  atkinson: { label: 'Atkinson Hyperlegible Next', stack: '"Atkinson Hyperlegible Next", system-ui, sans-serif' },
  inter: { label: 'Inter', stack: '"Inter", system-ui, sans-serif' },
  'source-sans': { label: 'Source Sans 3', stack: '"Source Sans 3", system-ui, sans-serif' },
  plex: { label: 'IBM Plex Sans', stack: '"IBM Plex Sans", system-ui, sans-serif' },
  lexend: { label: 'Lexend', stack: '"Lexend", system-ui, sans-serif' },
  system: { label: 'Sistema', stack: 'system-ui, -apple-system, "Segoe UI", Roboto, sans-serif' },
}

export const FONT_SIZES = [87.5, 100, 112.5, 125, 137.5, 150]

export const DEFAULT_PREFERENCES: Preferences = {
  theme: 'system',
  mode: 'focus',
  view: 'unified',
  font: 'atkinson',
  font_size: 100,
  density: 'comfortable',
}

/** Aplica tema, fuente, tamaño y densidad al documento. */
export function applyLook(p: Preferences, root: HTMLElement = document.documentElement) {
  if (p.theme === 'system') delete root.dataset.theme
  else root.dataset.theme = p.theme
  root.dataset.density = p.density
  root.style.setProperty('--font', FONTS[p.font]?.stack ?? FONTS.atkinson.stack)
  root.style.fontSize = `${p.font_size}%`
}
