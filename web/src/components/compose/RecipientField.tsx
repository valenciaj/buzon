import { useEffect, useId, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { api, type Addr } from '../../lib/api'
import { addrText, parseAddresses } from '../../lib/compose'
import { Icon } from '../mail/Icons'

/** Campo de destinatarios: cada dirección es una pastilla; sugiere contactos al escribir. */
export function RecipientField({ label, value, onChange, autoFocus, children }: {
  label: string
  value: Addr[]
  onChange: (v: Addr[]) => void
  autoFocus?: boolean
  children?: React.ReactNode
}) {
  const { t } = useTranslation()
  const id = useId()
  const [typed, setTyped] = useState('')
  const [bad, setBad] = useState(false)
  const [active, setActive] = useState(0)
  const [term, setTerm] = useState('')

  // Espera a que se deje de teclear para no preguntar en cada letra.
  useEffect(() => {
    const h = setTimeout(() => setTerm(typed.trim()), 150)
    return () => clearTimeout(h)
  }, [typed])
  const taken = new Set(value.map((a) => a.email.toLowerCase()))
  const q = useQuery({ queryKey: ['contacts', term], queryFn: () => api.suggest(term), enabled: term.length > 0, staleTime: 30_000 })
  const suggestions = typed.trim() ? (q.data ?? []).filter((c) => !taken.has(c.email.toLowerCase())) : []

  function add(list: Addr[]) {
    const fresh = list.filter((a) => !taken.has(a.email.toLowerCase()))
    if (fresh.length) onChange([...value, ...fresh])
  }

  /** Convierte lo escrito en pastillas. Devuelve si quedó algo sin entender. */
  function commit(text = typed): boolean {
    const { ok, bad } = parseAddresses(text)
    add(ok)
    setTyped(bad.join(', '))
    setBad(bad.length > 0)
    return bad.length > 0
  }

  function pick(a: Addr) {
    add([a])
    setTyped('')
    setBad(false)
  }

  function onKey(e: React.KeyboardEvent<HTMLInputElement>) {
    if (suggestions.length && (e.key === 'ArrowDown' || e.key === 'ArrowUp')) {
      e.preventDefault()
      setActive((i) => (i + (e.key === 'ArrowDown' ? 1 : suggestions.length - 1)) % suggestions.length)
    } else if (e.key === 'Enter' || e.key === ',' || e.key === ';' || (e.key === 'Tab' && typed.trim())) {
      if (!typed.trim()) return
      e.preventDefault()
      if (suggestions[active] && e.key !== ',' && e.key !== ';') pick(suggestions[active])
      else commit()
    } else if (e.key === 'Backspace' && !typed && value.length) {
      onChange(value.slice(0, -1))
    }
  }

  return (
    <div className="relative flex flex-wrap items-center gap-1.5 border-b border-line px-4 py-1.5">
      <label htmlFor={id} className="w-12 shrink-0 text-sm text-muted">{label}</label>
      {value.map((a) => (
        <span key={a.email} title={a.email}
          className="inline-flex max-w-full items-center gap-1 rounded-full bg-surface-2 py-0.5 pl-2.5 pr-1 text-sm">
          <span className="min-w-0 truncate">{a.name || a.email}</span>
          <button type="button" className="grid size-6 place-items-center rounded-full hover:bg-bg"
            aria-label={t('compose.removeRecipient', { name: addrText(a) })}
            onClick={() => onChange(value.filter((x) => x !== a))}>
            <Icon name="close" className="size-3.5" />
          </button>
        </span>
      ))}
      <input id={id} value={typed} autoFocus={autoFocus} autoComplete="off" spellCheck={false}
        aria-invalid={bad} aria-autocomplete="list" aria-expanded={suggestions.length > 0}
        aria-controls={`${id}-list`} role="combobox"
        className={`min-h-9 min-w-[8rem] flex-1 bg-transparent py-1 outline-none ${bad ? 'text-crit' : ''}`}
        onChange={(e) => { setTyped(e.target.value); setBad(false); setActive(0) }}
        onKeyDown={onKey}
        onBlur={() => typed.trim() && commit()}
        onPaste={(e) => {
          const text = e.clipboardData.getData('text')
          if (/[,;\n]/.test(text)) {
            e.preventDefault()
            commit(`${typed}${text}`)
          }
        }} />
      {children}
      {suggestions.length > 0 && (
        <ul id={`${id}-list`} role="listbox"
          className="absolute left-16 right-4 top-full z-10 mt-1 max-h-64 overflow-y-auto rounded-lg border border-line bg-surface py-1 shadow-lg">
          {suggestions.map((c, i) => (
            <li key={c.email} role="option" aria-selected={i === active}
              className={`cursor-pointer px-3 py-2 ${i === active ? 'bg-surface-2' : ''}`}
              onMouseDown={(e) => { e.preventDefault(); pick(c) }} onMouseEnter={() => setActive(i)}>
              {c.name && <span className="font-semibold">{c.name} </span>}
              <span className="text-sm text-muted">{c.email}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}
