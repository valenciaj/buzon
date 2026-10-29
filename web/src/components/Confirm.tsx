import { createContext, useContext, useEffect, useRef, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

export interface ConfirmOptions {
  text: string
  /** Texto del botón de aceptar; por defecto «Aceptar». */
  ok?: string
  /** Acción destructiva: el botón sale en rojo. */
  danger?: boolean
}

type Ask = (o: ConfirmOptions | string) => Promise<boolean>

const Ctx = createContext<Ask>(async () => false)

/** Pregunta con un diálogo propio en lugar de `window.confirm`. Devuelve si se aceptó. */
export function useConfirm() {
  return useContext(Ctx)
}

export function ConfirmProvider({ children }: { children: ReactNode }) {
  const { t } = useTranslation()
  const [open, setOpen] = useState<ConfirmOptions | null>(null)
  const resolve = useRef<(v: boolean) => void>(undefined)
  const dialog = useRef<HTMLDialogElement>(null)

  const ask: Ask = (o) => {
    resolve.current?.(false)
    setOpen(typeof o === 'string' ? { text: o } : o)
    return new Promise((r) => { resolve.current = r })
  }

  function done(v: boolean) {
    resolve.current?.(v)
    resolve.current = undefined
    setOpen(null)
  }

  useEffect(() => {
    const d = dialog.current
    if (open && d && !d.open) d.showModal()
  }, [open])

  return (
    <Ctx.Provider value={ask}>
      {children}
      {open && (
        <dialog ref={dialog} aria-labelledby="confirm-text"
          className="card m-auto w-[calc(100%-2rem)] max-w-md p-0 text-fg backdrop:bg-black/40"
          onCancel={(e) => { e.preventDefault(); done(false) }}
          onClick={(e) => { if (e.target === e.currentTarget) done(false) }}>
          <form method="dialog" className="flex flex-col gap-5 p-5" onSubmit={(e) => { e.preventDefault(); done(true) }}>
            <p id="confirm-text" className="text-lg">{open.text}</p>
            <div className="flex justify-end gap-2">
              <button type="button" className="btn btn-ghost" onClick={() => done(false)}>{t('common.cancel')}</button>
              <button type="submit" autoFocus className={`btn ${open.danger ? 'btn-danger' : 'btn-primary'}`}>
                {open.ok ?? t('common.ok')}
              </button>
            </div>
          </form>
        </dialog>
      )}
    </Ctx.Provider>
  )
}
