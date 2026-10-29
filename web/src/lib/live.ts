import { useEffect, useRef } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import type { Me } from './api'
import { notifyEnabled, shouldNotify, showNewMail, type NewMail } from './notify'

/** Escucha los avisos del servidor y refresca lo que haya cambiado. */
export function useLiveUpdates(me?: Me) {
  const qc = useQueryClient()
  const mode = useRef(me?.preferences.mode ?? 'focus')
  mode.current = me?.preferences.mode ?? 'focus'
  useEffect(() => {
    if (typeof EventSource === 'undefined') return
    const es = new EventSource('/api/v1/events')
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
    es.addEventListener('message', message)
    const status = () => qc.invalidateQueries({ queryKey: ['accounts'] })
    es.addEventListener('mail', mail)
    es.addEventListener('status', status)
    // Al reconectar (por ejemplo, tras suspender el portátil) puede haberse perdido algo.
    es.addEventListener('open', () => {
      mail()
      status()
    })
    return () => es.close()
  }, [qc])
}
