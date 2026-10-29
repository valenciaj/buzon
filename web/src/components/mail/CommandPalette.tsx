import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Icon } from './Icons'

export interface Command {
  id: string
  label: string
  /** Grupo en el que aparece (Ir a, Acciones, Ajustes…). */
  group: string
  icon: string
  /** Otras palabras con las que encontrarlo. */
  keywords?: string
  /** Atajo de teclado, para recordarlo. */
  hint?: string
  run: () => void
}

/** Sin mayúsculas ni tildes, para buscar «papelera» escribiendo «Papelera» o «papeléra». */
export function normalize(s: string): string {
  return s.normalize('NFD').replace(/\p{Diacritic}/gu, '').toLowerCase()
}

/** Las órdenes que contienen todas las palabras escritas, primero las que empiezan por ellas. */
export function filterCommands(list: Command[], query: string): Command[] {
  const words = normalize(query).split(/\s+/).filter(Boolean)
  if (!words.length) return list
  const scored: { c: Command; score: number }[] = []
  for (const c of list) {
    const label = normalize(c.label)
    const hay = `${label} ${normalize(c.keywords ?? '')} ${normalize(c.group)}`
    if (!words.every((w) => hay.includes(w))) continue
    scored.push({ c, score: label.startsWith(words[0]) ? 0 : label.includes(words[0]) ? 1 : 2 })
  }
  return scored.sort((a, b) => a.score - b.score).map((x) => x.c)
}

/** Paleta de órdenes (Ctrl+K): todo Buzón a un par de teclas. */
export function CommandPalette({ commands, onClose, onSearch }: {
  commands: Command[]
  onClose: () => void
  /** «Buscar …» con lo escrito, al final de la lista. */
  onSearch: (q: string) => void
}) {
  const { t } = useTranslation()
  const [query, setQuery] = useState('')
  const [at, setAt] = useState(0)
  const listRef = useRef<HTMLUListElement>(null)
  const found = useMemo(() => {
    const out = filterCommands(commands, query)
    const q = query.trim()
    return q
      ? [...out, { id: 'search', label: t('palette.search', { q }), group: t('palette.groups.search'), icon: 'search', run: () => onSearch(q) }]
      : out
  }, [commands, query, onSearch, t])
  useEffect(() => setAt(0), [query])
  useEffect(() => {
    listRef.current?.querySelector(`[data-index="${at}"]`)?.scrollIntoView({ block: 'nearest' })
  }, [at])

  function run(c: Command | undefined) {
    if (!c) return
    onClose()
    c.run()
  }

  function onKey(e: React.KeyboardEvent) {
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      setAt((i) => Math.min(i + 1, found.length - 1))
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      setAt((i) => Math.max(i - 1, 0))
    } else if (e.key === 'Enter') {
      e.preventDefault()
      run(found[at])
    } else if (e.key === 'Escape') {
      e.preventDefault()
      onClose()
    }
  }

  let lastGroup = ''
  return (
    <div className="fixed inset-0 z-40 flex items-start justify-center bg-black/40 p-4 pt-[12vh]" onClick={onClose}>
      <div role="dialog" aria-modal="true" aria-label={t('palette.title')} onClick={(e) => e.stopPropagation()}
        className="card flex max-h-[70vh] w-full max-w-xl flex-col overflow-hidden shadow-2xl">
        <div className="flex items-center gap-2 border-b border-line px-4">
          <Icon name="search" className="shrink-0 text-muted" />
          <input autoFocus value={query} onChange={(e) => setQuery(e.target.value)} onKeyDown={onKey}
            placeholder={t('palette.placeholder')} aria-label={t('palette.title')}
            role="combobox" aria-expanded="true" aria-controls="palette-list" aria-activedescendant={found[at] ? `cmd-${found[at].id}` : undefined}
            className="min-h-12 min-w-0 flex-1 bg-transparent outline-none placeholder:text-muted" />
          <kbd className="hidden rounded-md border border-line bg-surface-2 px-1.5 py-0.5 font-mono text-xs text-muted sm:block">Esc</kbd>
        </div>
        <ul ref={listRef} id="palette-list" role="listbox" className="min-h-0 flex-1 overflow-y-auto py-1">
          {found.length === 0 && <li className="px-4 py-6 text-center text-muted">{t('palette.none')}</li>}
          {found.map((c, i) => {
            const header = !query.trim() && c.group !== lastGroup ? c.group : null
            lastGroup = c.group
            return (
              <li key={c.id} role="presentation">
                {header && <p className="eyebrow px-4 pb-1 pt-3">{header}</p>}
                <button id={`cmd-${c.id}`} role="option" aria-selected={i === at} data-index={i}
                  onMouseMove={() => setAt(i)} onClick={() => run(c)}
                  className={`flex w-full items-center gap-3 px-4 py-2 text-left ${i === at ? 'bg-surface-2' : ''}`}>
                  <Icon name={c.icon} className="shrink-0 text-muted" />
                  <span className="min-w-0 flex-1 truncate">{c.label}</span>
                  {c.hint && <kbd className="shrink-0 rounded-md border border-line px-1.5 py-0.5 font-mono text-xs text-muted">{c.hint}</kbd>}
                </button>
              </li>
            )
          })}
        </ul>
      </div>
    </div>
  )
}
