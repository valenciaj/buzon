import { createContext, lazy, Suspense, useContext, useState, type ReactNode } from 'react'
import { useToast } from '../Toaster'
import { useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { api, ApiError } from '../../lib/api'
import { finalHtml, fromDraft, toDraftInput, type ComposeState } from '../../lib/compose'
import { useMe } from '../../lib/session'
import type { Outgoing } from './Composer'

// El editor pesa: se carga la primera vez que se redacta.
const Composer = lazy(() => import('./Composer').then((m) => ({ default: m.Composer })))

const Ctx = createContext<(s: ComposeState) => void>(() => undefined)

/** Abre la ventana de redactar con lo indicado (nuevo, respuesta, reenvío o borrador). */
export function useCompose() {
  return useContext(Ctx)
}

export function ComposerProvider({ children }: { children: ReactNode }) {
  const { t, i18n } = useTranslation()
  const qc = useQueryClient()
  const [open, setOpen] = useState<{ key: number; state: ComposeState; error?: string } | null>(null)
  const show = useToast()
  const me = useMe()

  function compose(state: ComposeState, error?: string) {
    setOpen({ key: Date.now(), state, error })
  }

  /**
   * Envía a través del servidor. Con margen para deshacer (o programado), el correo
   * espera allí y sale aunque se cierre la pestaña; «Deshacer» lo devuelve a la ventana.
   */
  async function queue(o: Outgoing) {
    setOpen(null)
    const undoSeconds = me.data?.preferences.undo_send ?? 10
    const sendAt = o.sendAt ?? (undoSeconds > 0 ? Math.floor(Date.now() / 1000) + undoSeconds : undefined)
    show({ text: t('compose.sendingNow') })
    try {
      const r = await api.send({
        ...toDraftInput({ ...o.state, html: finalHtml(o.html) }, o.text),
        draft_id: o.state.draftId,
        send_at: sendAt,
        follow_up_days: o.followUp || undefined,
      })
      qc.invalidateQueries({ queryKey: ['drafts'] })
      qc.invalidateQueries({ queryKey: ['scheduled'] })
      qc.invalidateQueries({ queryKey: ['followups'] })
      const id = r.scheduled_id
      if (!id) {
        show({ text: t('compose.sent') })
        qc.invalidateQueries({ queryKey: ['threads'] })
        qc.invalidateQueries({ queryKey: ['thread'] })
        return
      }
      const undo = async () => {
        try {
          const d = await api.cancelScheduled(id)
          qc.invalidateQueries({ queryKey: ['scheduled'] })
          compose(fromDraft(d))
        } catch (e) {
          show({ text: e instanceof ApiError ? e.message : t('common.error') })
        }
      }
      if (o.sendAt) {
        const when = new Date(o.sendAt * 1000).toLocaleString(i18n.language, { dateStyle: 'medium', timeStyle: 'short' })
        show({ text: t('compose.scheduledFor', { when }), undo, ms: 8000 })
      } else {
        show({ text: t('compose.sending', { count: undoSeconds }), undo, ms: undoSeconds * 1000 })
        // Pasado el margen, se confirma si salió (o se avisa si volvió a Borradores con error).
        setTimeout(() => confirmSent(id), (undoSeconds + 2) * 1000)
      }
    } catch (e) {
      show(null)
      compose(o.state, e instanceof ApiError ? e.message : t('common.error'))
    }
  }

  async function confirmSent(id: string, tries = 5) {
    const [scheduled, drafts] = await Promise.all([api.scheduled(), api.drafts()]).catch(() => [[], []])
    qc.invalidateQueries({ queryKey: ['scheduled'] })
    qc.invalidateQueries({ queryKey: ['drafts'] })
    const failed = drafts.find((d) => d.id === id && d.send_error)
    if (failed) {
      show({ text: t('compose.sendFailed', { error: failed.send_error }) })
    } else if (scheduled.some((d) => d.id === id)) {
      // Cancelado y vuelto a programar, o aún saliendo: se mira otra vez en un momento.
      if (tries > 0) setTimeout(() => confirmSent(id, tries - 1), 2000)
    } else if (!drafts.some((d) => d.id === id)) {
      show({ text: t('compose.sent') })
      qc.invalidateQueries({ queryKey: ['threads'] })
      qc.invalidateQueries({ queryKey: ['thread'] })
    }
  }

  return (
    <Ctx.Provider value={compose}>
      {children}
      {open && (
        <Suspense fallback={null}>
          <Composer key={open.key} initial={open.state} error={open.error} onSend={queue}
            onClose={(saved) => { setOpen(null); if (saved) show({ text: t('compose.draftSaved') }) }} />
        </Suspense>
      )}
    </Ctx.Provider>
  )
}
