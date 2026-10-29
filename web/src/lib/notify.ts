// Notificaciones del navegador para el correo nuevo. Se activan en cada navegador
// (Ajustes → Apariencia), porque el permiso es de cada dispositivo.

import type { Addr, Me } from './api'

const KEY = 'buzon.notify'

/** Lo que manda el servidor cuando llega correo nuevo. */
export interface NewMail {
  thread: string
  account: string
  from: Addr | null
  subject: string
  snippet: string
  status: 'pending' | 'approved' | 'blocked' | null
  bucket: 'principal' | 'news' | 'receipts' | null
}

export function notifySupported(): boolean {
  return typeof window !== 'undefined' && 'Notification' in window
}

export function notifyEnabled(): boolean {
  if (!notifySupported() || Notification.permission !== 'granted') return false
  try {
    return localStorage.getItem(KEY) === 'on'
  } catch {
    return false
  }
}

/** Pide permiso (si hace falta) y lo recuerda. Devuelve si quedan activadas. */
export async function setNotify(on: boolean): Promise<boolean> {
  if (!notifySupported()) return false
  if (on && Notification.permission !== 'granted') {
    if ((await Notification.requestPermission()) !== 'granted') return false
  }
  try {
    localStorage.setItem(KEY, on ? 'on' : 'off')
  } catch {
    // Sin almacenamiento (modo privado): se queda para esta visita.
  }
  return on
}

/** En Enfoque solo avisa de Principal (remitentes aprobados); en Clásico, de todo menos lo rechazado. */
export function shouldNotify(m: NewMail, mode: Me['preferences']['mode']): boolean {
  if (m.status === 'blocked') return false
  if (mode === 'classic') return true
  return m.status === 'approved' && m.bucket === 'principal'
}

export async function showNewMail(m: NewMail) {
  const title = m.from?.name || m.from?.email || 'Buzón'
  const options: NotificationOptions = {
    body: m.subject ? `${m.subject}\n${m.snippet}` : m.snippet,
    icon: '/icon-192.png',
    tag: m.thread,
    data: { url: `/?t=${m.thread}` },
  }
  // En el móvil solo funcionan las del service worker.
  const reg = await navigator.serviceWorker?.getRegistration().catch(() => undefined)
  if (reg) {
    await reg.showNotification(title, options)
    return
  }
  const n = new Notification(title, options)
  n.onclick = () => {
    window.focus()
    window.location.assign(`/?t=${m.thread}`)
  }
}
