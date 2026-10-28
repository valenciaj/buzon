import { useEffect } from 'react'
import { useQueryClient } from '@tanstack/react-query'

/** Escucha los avisos del servidor y refresca lo que haya cambiado. */
export function useLiveUpdates() {
  const qc = useQueryClient()
  useEffect(() => {
    if (typeof EventSource === 'undefined') return
    const es = new EventSource('/api/v1/events')
    const mail = () => {
      qc.invalidateQueries({ queryKey: ['threads'] })
      qc.invalidateQueries({ queryKey: ['mailboxes'] })
      qc.invalidateQueries({ queryKey: ['thread'] })
    }
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
