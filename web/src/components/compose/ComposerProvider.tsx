import { createContext, lazy, Suspense, useContext, useEffect, useRef, useState, type ReactNode } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { api, ApiError } from '../../lib/api'
import { finalHtml, toDraftInput, type ComposeState } from '../../lib/compose'
import type { Outgoing } from './Composer'

// El editor pesa: se carga la primera vez que se redacta.
const Composer = lazy(() => import('./Composer').then((m) => ({ default: m.Composer })))

/** Segundos para arrepentirse antes de que el correo salga de verdad. */
const UNDO_SECONDS = 5

type Toast = { text: string; undo?: () => void }

const Ctx = createContext<(s: ComposeState) => void>(() => undefined)

/** Abre la ventana de redactar con lo indicado (nuevo, respuesta, reenvío o borrador). */
export function useCompose() {
  return useContext(Ctx)
}

export function ComposerProvider({ children }: { children: ReactNode }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const [open, setOpen] = useState<{ key: number; state: ComposeState; error?: string } | null>(null)
  const [toast, setToast] = useState<Toast | null>(null)
  const toastTimer = useRef<ReturnType<typeof setTimeout>>(undefined)
  const sending = useRef(0)

  function show(next: Toast | null, ms = 4000) {
    clearTimeout(toastTimer.current)
    setToast(next)
    if (next && !next.undo) toastTimer.current = setTimeout(() => setToast(null), ms)
  }

  function compose(state: ComposeState, error?: string) {
    setOpen({ key: Date.now(), state, error })
  }

  // Avisa si se cierra la pestaña con un envío en la cuenta atrás.
  useEffect(() => {
    const warn = (e: BeforeUnloadEvent) => {
      if (sending.current > 0) e.preventDefault()
    }
    window.addEventListener('beforeunload', warn)
    return () => window.removeEventListener('beforeunload', warn)
  }, [])

  function queue(o: Outgoing) {
    setOpen(null)
    sending.current++
    let timer: ReturnType<typeof setTimeout> | undefined = undefined
    const undo = () => {
      clearTimeout(timer)
      sending.current--
      show(null)
      compose(o.state)
    }
    show({ text: t('compose.sending', { count: UNDO_SECONDS }), undo })
    timer = setTimeout(async () => {
      show({ text: t('compose.sendingNow') })
      try {
        await api.send({ ...toDraftInput({ ...o.state, html: finalHtml(o.html) }, o.text), draft_id: o.state.draftId })
        show({ text: t('compose.sent') })
        qc.invalidateQueries({ queryKey: ['drafts'] })
        qc.invalidateQueries({ queryKey: ['threads'] })
        qc.invalidateQueries({ queryKey: ['thread'] })
      } catch (e) {
        show(null)
        compose(o.state, e instanceof ApiError ? e.message : t('common.error'))
      } finally {
        sending.current--
      }
    }, UNDO_SECONDS * 1000)
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
      {toast && (
        <div role="status" className="fixed inset-x-4 bottom-[calc(1rem+env(safe-area-inset-bottom))] z-40 mx-auto flex max-w-md md:left-6 md:right-auto md:mx-0 md:w-[24rem] items-center gap-3 rounded-xl bg-fg px-4 py-3 text-bg shadow-xl">
          <span className="min-w-0 flex-1">{toast.text}</span>
          {toast.undo && (
            <button className="rounded-md px-2 py-1 font-bold underline underline-offset-2" onClick={toast.undo}>
              {t('compose.undo')}
            </button>
          )}
        </div>
      )}
    </Ctx.Provider>
  )
}
