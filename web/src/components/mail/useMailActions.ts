import { useEffect, useRef, useState } from 'react'
import { useQueryClient, type InfiniteData } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { api, ApiError, type From, type Thread, type ThreadPage } from '../../lib/api'
import type { MailAction } from '../../lib/mail'
import { useToast } from '../Toaster'

/** Segundos para deshacer antes de que el cambio llegue al servidor. */
const UNDO_MS = 5000

export type MoveTarget = MailAction | { mailbox: string; name: string }

/**
 * Archivar, borrar, mover, marcar y etiquetar hilos. Mover y borrar se ven al momento,
 * pero esperan unos segundos por si se deshace; al cerrar la pestaña se envían ya.
 */
export function useMailActions(from: From) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const toast = useToast()
  const pending = useRef(new Map<number, () => void>())
  const seq = useRef(0)
  // Lo que está a la espera de enviarse no debe reaparecer si la lista se recarga antes.
  const [hidden, setHidden] = useState<ReadonlySet<string>>(new Set())
  const unhide = (ids: string[]) => setHidden((h) => new Set([...h].filter((x) => !ids.includes(x))))

  useEffect(() => {
    const flushAll = () => {
      for (const f of [...pending.current.values()]) f()
    }
    window.addEventListener('pagehide', flushAll)
    return () => {
      window.removeEventListener('pagehide', flushAll)
      flushAll()
    }
  }, [])

  function refresh() {
    qc.invalidateQueries({ queryKey: ['threads'] })
    qc.invalidateQueries({ queryKey: ['mailboxes'] })
    qc.invalidateQueries({ queryKey: ['thread'] })
    qc.invalidateQueries({ queryKey: ['labels'] })
  }

  function editPages(edit: (p: ThreadPage) => ThreadPage) {
    qc.setQueriesData<InfiniteData<ThreadPage>>({ queryKey: ['threads'] }, (old) =>
      old && { ...old, pages: old.pages.map(edit) })
  }

  function hide(ids: string[]) {
    editPages((p) => ({ ...p, threads: p.threads.filter((x) => !ids.includes(x.id)) }))
  }

  function fail(e: unknown) {
    toast({ text: e instanceof ApiError ? e.message : t('common.error') })
    refresh()
  }

  function send(target: MoveTarget, ids: string[], keepalive: boolean): Promise<unknown> {
    if (target === 'delete') return api.deleteThreads(ids, from, keepalive)
    const to = typeof target === 'string' ? { to_role: target } : { to_mailbox: target.mailbox }
    return api.moveThreads(ids, to, from, keepalive)
  }

  /** Mueve (o borra para siempre) con unos segundos para deshacer. */
  function move(target: MoveTarget, ids: string[], opts: { onUndo?: () => void } = {}) {
    if (!ids.length) return
    if (target === 'delete' && !window.confirm(t('actions.deleteConfirm', { count: ids.length }))) return
    hide(ids)
    setHidden((h) => new Set([...h, ...ids]))
    const key = ++seq.current
    let done = false
    const flush = (keepalive: boolean) => {
      if (done) return
      done = true
      clearTimeout(timer)
      pending.current.delete(key)
      send(target, ids, keepalive)
        .then(refresh, fail)
        .finally(() => unhide(ids))
    }
    const timer = setTimeout(() => flush(false), target === 'delete' ? 0 : UNDO_MS)
    pending.current.set(key, () => flush(true))
    const kind = typeof target === 'string' ? target : 'move'
    const text = t(`actions.done.${kind}`, { count: ids.length, name: typeof target === 'string' ? '' : target.name })
    if (target === 'delete') {
      toast({ text })
      return
    }
    toast({
      text,
      ms: UNDO_MS,
      undo: () => {
        if (done) return
        done = true
        clearTimeout(timer)
        pending.current.delete(key)
        unhide(ids)
        refresh()
        opts.onUndo?.()
      },
    })
  }

  function flags(ids: string[], change: { seen?: boolean; flagged?: boolean }) {
    if (!ids.length) return
    editPages((p) => ({
      ...p,
      threads: p.threads.map((x) => (ids.includes(x.id)
        ? {
            ...x,
            unread: change.seen === undefined ? x.unread : change.seen ? 0 : Math.max(1, x.unread),
            flagged: change.flagged ?? x.flagged,
          }
        : x)),
    }))
    for (const id of ids) {
      qc.setQueryData<Thread>(['thread', id], (old) => old && {
        ...old,
        messages: old.messages.map((m, i) => ({
          ...m,
          seen: change.seen ?? m.seen,
          flagged: change.flagged === undefined ? m.flagged : change.flagged ? m.flagged || i === old.messages.length - 1 : false,
        })),
      })
    }
    api.flagThreads(ids, change).then(refresh, fail)
  }

  function label(ids: string[], add: string[], remove: string[] = []) {
    if (!ids.length) return
    const apply = (list: string[]) => [...list.filter((l) => !remove.includes(l)), ...add.filter((l) => !list.includes(l))]
    editPages((p) => ({ ...p, threads: p.threads.map((x) => (ids.includes(x.id) ? { ...x, labels: apply(x.labels) } : x)) }))
    for (const id of ids) qc.setQueryData<Thread>(['thread', id], (old) => old && { ...old, labels: apply(old.labels) })
    api.labelThreads(ids, add, remove).then(refresh, fail)
  }

  return { move, flags, label, hidden }
}
