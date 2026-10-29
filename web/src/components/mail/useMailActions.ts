import { useEffect, useRef, useState } from 'react'
import { useQueryClient, type InfiniteData } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { api, ApiError, type FlowChange, type From, type Thread, type ThreadPage } from '../../lib/api'
import type { MailAction } from '../../lib/mail'
import { useToast } from '../Toaster'
import { useConfirm } from '../Confirm'

/** Segundos para deshacer antes de que el cambio llegue al servidor. */
const UNDO_MS = 5000

export type MoveTarget = MailAction | { mailbox: string; name: string }

/**
 * Archivar, borrar, mover, marcar y etiquetar hilos. Mover y borrar se ven al momento,
 * pero esperan unos segundos por si se deshace; al cerrar la pestaña se envían ya.
 */
export function useMailActions(from: From) {
  const { t, i18n } = useTranslation()
  const confirm = useConfirm()
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
    qc.invalidateQueries({ queryKey: ['flow'] })
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
  function move(target: MoveTarget, ids: string[], opts: { onUndo?: () => void; confirmed?: boolean } = {}) {
    if (!ids.length) return
    if (target === 'delete' && !opts.confirmed) {
      void confirm({ text: t('actions.deleteConfirm', { count: ids.length }), ok: t('common.delete'), danger: true })
        .then((ok) => { if (ok) move(target, ids, { ...opts, confirmed: true }) })
      return
    }
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

  /**
   * Pilas y pospuestos. Con `leaves`, los hilos salen de la lista que se ve
   * (por ejemplo, al apartar algo de Principal). Se puede deshacer.
   */
  function flow(ids: string[], change: FlowChange, opts: { leaves?: boolean; onUndo?: () => void } = {}) {
    if (!ids.length) return
    if (opts.leaves) hide(ids)
    const patch = (x: { reply_later: boolean; aside: boolean; snooze_until: number | null }) => ({
      reply_later: change.reply_later ?? x.reply_later,
      aside: change.aside ?? x.aside,
      snooze_until: change.snooze_until === undefined ? x.snooze_until : change.snooze_until || null,
    })
    editPages((p) => ({ ...p, threads: p.threads.map((x) => (ids.includes(x.id) ? { ...x, ...patch(x) } : x)) }))
    for (const id of ids) qc.setQueryData<Thread>(['thread', id], (old) => old && { ...old, ...patch(old) })
    const undo: FlowChange = {}
    let kind = ''
    if (change.reply_later !== undefined) {
      undo.reply_later = !change.reply_later
      kind = change.reply_later ? 'replyLater' : 'unReplyLater'
    }
    if (change.aside !== undefined) {
      undo.aside = !change.aside
      kind = change.aside ? 'aside' : 'unAside'
    }
    if (change.snooze_until !== undefined) {
      undo.snooze_until = 0
      kind = change.snooze_until ? 'snooze' : 'unsnooze'
    }
    const date = change.snooze_until
      ? new Date(change.snooze_until * 1000).toLocaleString(i18n.language, { weekday: 'long', day: 'numeric', month: 'long', hour: '2-digit', minute: '2-digit' })
      : ''
    api.setFlow(ids, change).then(refresh, fail)
    toast({
      text: t(`flow.toast.${kind}`, { count: ids.length, date }),
      ms: UNDO_MS,
      // Deshacer un pospuesto lo quita; deshacer quitarlo no sabe la fecha que tenía.
      undo: change.snooze_until === 0 ? undefined : () => {
        api.setFlow(ids, undo).then(refresh, fail)
        opts.onUndo?.()
      },
    })
  }

  return { move, flags, label, flow, hidden }
}
