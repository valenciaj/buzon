import { createContext, useContext, useRef, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

export interface Toast {
  text: string
  /** Botón «Deshacer». */
  undo?: () => void
  /** Cuánto se ve. Sin `ms`, un aviso con «Deshacer» se queda hasta que se quite. */
  ms?: number
}

const Ctx = createContext<(t: Toast | null) => void>(() => undefined)

/** Muestra un aviso abajo (uno cada vez; el nuevo sustituye al anterior). */
export function useToast() {
  return useContext(Ctx)
}

export function ToastProvider({ children }: { children: ReactNode }) {
  const { t } = useTranslation()
  const [toast, setToast] = useState<Toast | null>(null)
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined)

  function show(next: Toast | null) {
    clearTimeout(timer.current)
    setToast(next)
    const ms = next?.ms ?? (next?.undo ? undefined : 4000)
    if (next && ms) timer.current = setTimeout(() => setToast(null), ms)
  }

  return (
    <Ctx.Provider value={show}>
      {children}
      {toast && (
        <div role="status" className="fixed inset-x-4 bottom-[calc(1rem+env(safe-area-inset-bottom))] z-40 mx-auto flex max-w-md items-center gap-3 rounded-xl bg-fg px-4 py-3 text-bg shadow-xl md:left-6 md:right-auto md:mx-0 md:w-[24rem]">
          <span className="min-w-0 flex-1">{toast.text}</span>
          {toast.undo && (
            <button className="rounded-md px-2 py-1 font-bold underline underline-offset-2"
              onClick={() => { const u = toast.undo; show(null); u?.() }}>
              {t('compose.undo')}
            </button>
          )}
        </div>
      )}
    </Ctx.Provider>
  )
}
