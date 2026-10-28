// Utilidades del correo: carpetas, fechas, nombres y el documento del lector.

import type { Mailbox, Role } from './api'

export const ROLE_ORDER: Role[] = ['inbox', 'sent', 'drafts', 'archive', 'junk', 'trash']
export const ACCOUNT_COLORS = ['#2f9a72', '#c4683a', '#7a3fa0', '#3d7fb5', '#a8862a']

/** Lo que se está viendo: un tipo de carpeta (en una cuenta o en todas) o una carpeta concreta. */
export type Box = { role: Role } | { mailbox: string }

export function parseBox(s: string | null): Box {
  if (s?.startsWith('m:')) return { mailbox: s.slice(2) }
  if (s && (ROLE_ORDER as string[]).includes(s)) return { role: s as Role }
  return { role: 'inbox' }
}

export function boxKey(b: Box): string {
  return 'role' in b ? b.role : `m:${b.mailbox}`
}

/** Carpetas sin tipo especial, ordenadas por nombre. */
export function customFolders(mailboxes: Mailbox[], account?: string): Mailbox[] {
  return mailboxes
    .filter((m) => !m.role && (!account || m.account_id === account))
    .sort((a, b) => a.name.localeCompare(b.name))
}

/** No leídos por tipo de carpeta, sumando las cuentas que se ven. */
export function unreadByRole(mailboxes: Mailbox[], account?: string): Partial<Record<Role, number>> {
  const out: Partial<Record<Role, number>> = {}
  for (const m of mailboxes) {
    if (!m.role || (account && m.account_id !== account)) continue
    out[m.role] = (out[m.role] ?? 0) + m.unread
  }
  return out
}

export function initials(name: string): string {
  const words = name.replace(/[<>"@].*$/, '').trim().split(/\s+/).filter(Boolean)
  const letters = words.length > 1 ? [words[0], words[words.length - 1]] : words
  return letters.map((w) => [...w][0]?.toUpperCase() ?? '').join('') || '?'
}

function startOfDay(d: Date): number {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime()
}

/** Fecha corta para la lista: hora si es de hoy, día de la semana si es de esta semana… */
export function shortDate(ts: number, lang: string, now = new Date()): string {
  const d = new Date(ts * 1000)
  const days = Math.round((startOfDay(now) - startOfDay(d)) / 86_400_000)
  if (days === 0) return d.toLocaleTimeString(lang, { hour: '2-digit', minute: '2-digit' })
  if (days === 1) return new Intl.RelativeTimeFormat(lang, { numeric: 'auto' }).format(-1, 'day')
  if (days > 1 && days < 7) return d.toLocaleDateString(lang, { weekday: 'short' })
  if (d.getFullYear() === now.getFullYear()) return d.toLocaleDateString(lang, { day: 'numeric', month: 'short' })
  return d.toLocaleDateString(lang, { day: '2-digit', month: '2-digit', year: '2-digit' })
}

export function longDate(ts: number, lang: string): string {
  return new Date(ts * 1000).toLocaleString(lang, { dateStyle: 'medium', timeStyle: 'short' })
}

export type DayGroup = 'today' | 'yesterday' | 'week' | 'older'

export function dayGroup(ts: number, now = new Date()): DayGroup {
  const days = Math.round((startOfDay(now) - startOfDay(new Date(ts * 1000))) / 86_400_000)
  if (days <= 0) return 'today'
  if (days === 1) return 'yesterday'
  if (days < 7) return 'week'
  return 'older'
}

export function fileSize(bytes: number, lang: string): string {
  const units = ['B', 'KB', 'MB', 'GB']
  let n = bytes
  let i = 0
  while (n >= 1024 && i < units.length - 1) {
    n /= 1024
    i++
  }
  return `${n.toLocaleString(lang, { maximumFractionDigits: i ? 1 : 0 })} ${units[i]}`
}

/** Trozos de texto plano con los enlaces separados, para pintarlos como <a>. */
export function linkify(text: string): { text: string; href?: string }[] {
  const out: { text: string; href?: string }[] = []
  const re = /\b(https?:\/\/[^\s<>"']+[^\s<>"'.,;:!?)\]])|\b([\w.+-]+@[\w-]+(?:\.[\w-]+)+)/g
  let last = 0
  for (const m of text.matchAll(re)) {
    if (m.index > last) out.push({ text: text.slice(last, m.index) })
    out.push({ text: m[0], href: m[1] ? m[1] : `mailto:${m[2]}` })
    last = m.index + m[0].length
  }
  if (last < text.length) out.push({ text: text.slice(last) })
  return out
}

/**
 * Documento del lector para un correo HTML (ya saneado en el servidor).
 * La CSP bloquea todo lo remoto salvo que la persona pida ver las imágenes;
 * el iframe va además con sandbox y sin scripts.
 */
export function emailDocument(html: string, opts: { images: boolean; font: string }): string {
  const img = opts.images ? "'self' data: https: http:" : "'self' data:"
  const csp = `default-src 'none'; img-src ${img}; style-src 'unsafe-inline'; font-src data:`
  return `<!doctype html><html><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="${csp}">
<meta name="referrer" content="no-referrer">
<base target="_blank">
<style>
html,body{margin:0;padding:0;background:#fff;color:#1a1a1a}
body{font-family:${opts.font};font-size:16px;line-height:1.5;overflow-wrap:anywhere;padding:16px}
img{max-width:100%;height:auto}
table{max-width:100%}
pre{white-space:pre-wrap}
a{color:#2f4fc4}
</style></head><body>${html}</body></html>`
}
