import { useEffect, useRef, useState, type ReactNode } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { api, ApiError, type Mailbox } from '../../lib/api'
import { customFolders, LABEL_COLORS, ROLE_ORDER } from '../../lib/mail'
import { Icon } from './Icons'

/** Botón con un menú desplegable. Se cierra al pulsar fuera o con Escape. */
export function Popover({ label, icon, children, align = 'left', compact }: {
  label: string
  icon: string
  children: (close: () => void) => ReactNode
  align?: 'left' | 'right'
  /** Solo el icono (el nombre queda como título y para lectores de pantalla). */
  compact?: boolean
}) {
  const [open, setOpen] = useState(false)
  const ref = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (!open) return
    const out = (e: MouseEvent) => { if (!ref.current?.contains(e.target as Node)) setOpen(false) }
    const esc = (e: KeyboardEvent) => { if (e.key === 'Escape') { e.stopPropagation(); setOpen(false) } }
    document.addEventListener('mousedown', out)
    document.addEventListener('keydown', esc, true)
    return () => {
      document.removeEventListener('mousedown', out)
      document.removeEventListener('keydown', esc, true)
    }
  }, [open])
  return (
    <div ref={ref} className="relative">
      <button className="btn btn-ghost px-2.5" aria-expanded={open} aria-haspopup="menu" title={label}
        onClick={() => setOpen(!open)}>
        <Icon name={icon} /> <span className={compact ? 'sr-only' : 'max-sm:sr-only'}>{label}</span>
      </button>
      {open && (
        <div role="menu" className={`absolute top-full z-20 mt-1 max-h-[60vh] w-64 overflow-y-auto rounded-lg border border-line bg-surface py-1 shadow-lg ${align === 'right' ? 'right-0' : 'left-0'}`}>
          {children(() => setOpen(false))}
        </div>
      )}
    </div>
  )
}

/** Poner y quitar etiquetas. `current` son las que ya tienen todos los hilos. */
export function LabelMenu({ current, onChange, compact }: {
  current: string[]
  onChange: (add: string[], remove: string[]) => void
  compact?: boolean
}) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const labels = useQuery({ queryKey: ['labels'], queryFn: api.labels })
  const [name, setName] = useState('')
  const [error, setError] = useState<string | null>(null)

  async function create() {
    if (!name.trim()) return
    try {
      const color = LABEL_COLORS[(labels.data?.length ?? 0) % LABEL_COLORS.length]
      const l = await api.createLabel(name.trim(), color)
      setName('')
      setError(null)
      qc.invalidateQueries({ queryKey: ['labels'] })
      onChange([l.id], [])
    } catch (e) {
      setError(e instanceof ApiError ? e.message : t('common.error'))
    }
  }

  return (
    <Popover label={t('actions.labels')} icon="tag" compact={compact}>
      {() => (
        <>
          {(labels.data ?? []).map((l) => {
            const on = current.includes(l.id)
            return (
              <button key={l.id} role="menuitemcheckbox" aria-checked={on}
                className="flex w-full items-center gap-2.5 px-3 py-2 text-left hover:bg-surface-2"
                onClick={() => (on ? onChange([], [l.id]) : onChange([l.id], []))}>
                <span className={`grid size-4 shrink-0 place-items-center rounded border ${on ? 'border-accent bg-accent text-on-accent' : 'border-line'}`}>
                  {on && <Icon name="check" className="size-3" />}
                </span>
                <span className="size-2.5 shrink-0 rounded-full" style={{ background: l.color }} />
                <span className="min-w-0 truncate">{l.name}</span>
              </button>
            )
          })}
          <form className="flex gap-1 border-t border-line p-2" onSubmit={(e) => { e.preventDefault(); create() }}>
            <input className="input min-h-9 py-1 text-sm" placeholder={t('actions.newLabel')} aria-label={t('actions.newLabel')}
              value={name} onChange={(e) => setName(e.target.value)} />
            <button className="btn min-h-9 px-2.5 py-1 text-sm" disabled={!name.trim()}>{t('actions.create')}</button>
          </form>
          {error && <p className="px-3 pb-2 text-sm text-crit">{error}</p>}
        </>
      )}
    </Popover>
  )
}

/** Mover a una carpeta concreta (las propias de la cuenta y las especiales). */
export function MoveMenu({ mailboxes, account, onMove, compact }: {
  mailboxes: Mailbox[]
  /** Cuenta de los hilos; sin ella, carpetas de todas. */
  account?: string
  onMove: (m: Mailbox) => void
  compact?: boolean
}) {
  const { t } = useTranslation()
  const mine = mailboxes.filter((m) => !account || m.account_id === account)
  const special = ROLE_ORDER.flatMap((r) => (r === 'sent' || r === 'drafts' ? [] : mine.filter((m) => m.role === r)))
  const list = [...special, ...customFolders(mailboxes, account)]
  return (
    <Popover label={t('actions.moveTo')} icon="folder" compact={compact}>
      {(close) => list.map((m) => (
        <button key={m.id} role="menuitem" className="flex w-full items-center gap-2.5 px-3 py-2 text-left hover:bg-surface-2"
          onClick={() => { close(); onMove(m) }}>
          <Icon name={m.role ?? 'folder'} className="shrink-0 text-muted" />
          <span className="min-w-0 truncate">{m.role ? t(`mail.roles.${m.role}`) : m.name}</span>
        </button>
      ))}
    </Popover>
  )
}
