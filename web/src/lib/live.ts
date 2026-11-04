import { useEffect, useRef } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import type { Me } from './api'
import { notifyEnabled, shouldNotify, showNewMail, type NewMail } from './notify'

const RETRY_MIN = 2_000
const RETRY_MAX = 60_000

/** Escucha los avisos del servidor y refresca lo que haya cambiado. */
export function useLiveUpdates(me?: Me) {
  const qc = useQueryClient()
  const mode = useRef(me?.preferences.mode ?? 'focus')
  mode.current = me?.preferences.mode ?? 'focus'
  useEffect(() => {
    if (typeof EventSource === 'undefined') return
    const mail = () => {
      qc.invalidateQueries({ queryKey: ['threads'] })
      qc.invalidateQueries({ queryKey: ['mailboxes'] })
      qc.invalidateQueries({ queryKey: ['thread'] })
      qc.invalidateQueries({ queryKey: ['flow'] })
      qc.invalidateQueries({ queryKey: ['senders'] })
      qc.invalidateQueries({ queryKey: ['drafts'] })
      qc.invalidateQueries({ queryKey: ['scheduled'] })
      qc.invalidateQueries({ queryKey: ['followups'] })
    }
    // Correo nuevo: aviso del navegador si la pestaña no está a la vista.
    const message = (e: MessageEvent) => {
      if (!document.hidden || !notifyEnabled()) return
      try {
        const m = JSON.parse(e.data) as NewMail
        if (shouldNotify(m, mode.current)) showNewMail(m).catch(() => undefined)
      } catch {
        // Aviso mal formado: se ignora.
      }
    }
    const status = () => qc.invalidateQueries({ queryKey: ['accounts'] })
    let es: EventSource | undefined
    let retry: ReturnType<typeof setTimeout> | undefined
    let wait = RETRY_MIN
    const connect = () => {
      es = new EventSource('/api/v1/events')
      es.addEventListener('message', message)
      es.addEventListener('mail', mail)
      es.addEventListener('status', status)
      // Al reconectar (por ejemplo, tras suspender el portátil) puede haberse perdido algo.
      es.addEventListener('open', () => {
        wait = RETRY_MIN
        mail()
        status()
      })
      // Si el servidor se corta, el navegador reintenta solo; pero si al reintentar recibe un
      // error (un 502 mientras Buzón se reinicia), deja de intentarlo: se vuelve a abrir aquí.
      es.addEventListener('error', () => {
        if (es?.readyState !== EventSource.CLOSED) return
        es.close()
        retry = setTimeout(connect, wait)
        wait = Math.min(wait * 2, RETRY_MAX)
      })
    }
    connect()
    return () => {
      clearTimeout(retry)
      es?.close()
    }
  }, [qc])
}
