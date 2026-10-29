import { Fragment, useEffect, useRef, useState } from 'react'
import { useInfiniteQuery, useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'
import { api, type Account, type Label, type ThreadFilter, type ThreadSummary } from '../../lib/api'
import { ACCOUNT_COLORS, dayGroup, initials, shortDate } from '../../lib/mail'
import { Alert } from '../ui'
import { Icon } from './Icons'

/** Los hilos de un filtro, página a página. La lista y los atajos de teclado comparten la consulta. */
export function useThreadPages(filter: ThreadFilter, enabled = true) {
  return useInfiniteQuery({
    enabled,
    queryKey: ['threads', filter],
    queryFn: ({ pageParam }) => api.threads(filter, pageParam),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (p) => p.next_cursor ?? undefined,
  })
}

export function ThreadList({
  title, filter, accounts, selected, grouped, showAccount, onSelect, onMenu, onCompose, before,
  hidden, checked, onCheck, toolbar, search, onSearch, pinned, actions, emptyText,
}: {
  title: string
  filter: ThreadFilter
  accounts: Account[]
  selected: string | null
  /** Agrupar por día (modo Enfoque). */
  grouped: boolean
  /** Marcar de qué cuenta es cada hilo (vista unificada). */
  showAccount: boolean
  onSelect: (id: string) => void
  onMenu: () => void
  onCompose?: () => void
  /** Algo que mostrar antes de los hilos (los borradores de Buzón). */
  before?: React.ReactNode
  /** Hilos que se han movido y aún no han llegado al servidor. */
  hidden: ReadonlySet<string>
  /** Hilos seleccionados para las acciones en lote. */
  checked: ReadonlySet<string>
  onCheck: (ids: string[], on: boolean) => void
  /** Acciones para la selección; sustituye a la cabecera mientras hay algo marcado. */
  toolbar: React.ReactNode
  search: string
  onSearch: (q: string) => void
  /** Hilos fijos arriba, con su título (los que reaparecen, en Principal). */
  pinned?: { title: string; filter: ThreadFilter }
  /** Botones junto al título. */
  actions?: React.ReactNode
  /** Qué decir cuando no hay nada (en vez del texto de carpeta vacía). */
  emptyText?: React.ReactNode
}) {
  const { t, i18n } = useTranslation()
  const q = useThreadPages(filter)
  const labels = useQuery({ queryKey: ['labels'], queryFn: api.labels })
  const top = useThreadPages(pinned?.filter ?? {}, !!pinned)
  const tops = pinned ? (top.data?.pages[0]?.threads ?? []).filter((x) => !hidden.has(x.id)) : []
  const threads = (q.data?.pages.flatMap((p) => p.threads) ?? []).filter((x) => !hidden.has(x.id))
  const byId = new Map((labels.data ?? []).map((l) => [l.id, l]))
  const sentinel = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const el = sentinel.current
    if (!el || !q.hasNextPage) return
    const io = new IntersectionObserver((e) => {
      if (e[0]?.isIntersecting && !q.isFetchingNextPage) q.fetchNextPage()
    }, { rootMargin: '400px' })
    io.observe(el)
    return () => io.disconnect()
  }, [q.hasNextPage, q.isFetchingNextPage, q.fetchNextPage, threads.length])

  const color = (id: string) => ACCOUNT_COLORS[accounts.findIndex((a) => a.id === id) % ACCOUNT_COLORS.length]
  const syncing = accounts.some((a) => a.state === 'connecting' || a.state === 'syncing')

  let empty: React.ReactNode = null
  if (accounts.length === 0) {
    empty = (
      <>
        <b className="block text-fg">{t('home.noAccounts')}</b>
        <Link to="/settings/accounts" className="btn btn-primary mt-3">{t('home.addAccount')}</Link>
      </>
    )
  } else if (emptyText) {
    empty = emptyText
  } else if (search) {
    empty = <><b className="block text-fg">{t('search.empty')}</b>{t('search.emptyBody')}</>
  } else if (syncing) {
    empty = <><b className="block text-fg">{t('mail.downloading')}</b>{t('mail.downloadingBody')}</>
  } else {
    empty = <><b className="block text-fg">{t('mail.empty')}</b>{t('mail.emptyBody')}</>
  }

  return (
    <>
      {checked.size > 0 ? (
        <header className="flex min-h-[3.75rem] flex-wrap items-center gap-1 border-b border-line bg-surface-2 px-[var(--pad)] py-2">
          <button className="btn btn-ghost -ml-2 px-2" onClick={() => onCheck([...checked], false)} aria-label={t('actions.clear')} title={t('actions.clear')}>
            <Icon name="close" />
          </button>
          <span className="mr-auto font-semibold tabular-nums" aria-live="polite">{t('actions.selected', { count: checked.size })}</span>
          {threads.some((x) => !checked.has(x.id)) && (
            <button className="btn btn-ghost px-2.5 text-sm" onClick={() => onCheck(threads.map((x) => x.id), true)}>{t('actions.selectAll')}</button>
          )}
          <div className="flex w-full flex-wrap gap-1">{toolbar}</div>
        </header>
      ) : (
        <header className="flex flex-col gap-2 border-b border-line px-[var(--pad)] py-3">
          <div className="flex items-center gap-2">
            <button className="btn btn-ghost -ml-2 px-2 lg:hidden" onClick={onMenu} aria-label={t('home.menu')}>
              <Icon name="menu" />
            </button>
            <h1 className="min-w-0 flex-1 truncate text-xl font-bold">{title}</h1>
            {actions}
            {onCompose && (
              <button className="btn btn-primary size-10 rounded-full px-0 lg:hidden" onClick={onCompose}
                title={t('compose.new')} aria-label={t('compose.new')}>
                <Icon name="compose" />
              </button>
            )}
          </div>
          <SearchBox value={search} onChange={onSearch} />
        </header>
      )}
      <div className="min-h-0 flex-1 overflow-y-auto pb-4" role="list" aria-label={title}>
        {q.isError && <div className="p-4"><Alert>{t('common.error')}</Alert></div>}
        {q.isLoading && <p className="p-6 text-muted">{t('common.loading')}</p>}
        {before}
        {tops.length > 0 && (
          <div className="border-b-4 border-line">
            <p className="eyebrow px-[var(--pad)] pb-1 pt-4">{pinned?.title}</p>
            {tops.map((th) => (
              <ThreadItem key={th.id} thread={th} lang={i18n.language} selected={selected === th.id} checked={checked.has(th.id)}
                labels={th.labels.flatMap((id) => byId.get(id) ?? [])}
                dot={showAccount ? color(th.account_id) : undefined} onClick={() => onSelect(th.id)}
                onCheck={(on) => onCheck([th.id], on)} />
            ))}
          </div>
        )}
        {q.isSuccess && threads.length === 0 && tops.length === 0 && !before && <div className="px-6 py-12 text-center text-muted">{empty}</div>}
        {threads.map((th, i) => {
          const group = grouped ? dayGroup(th.last_date) : null
          const newGroup = grouped && (i === 0 || dayGroup(threads[i - 1].last_date) !== group)
          return (
            <Fragment key={th.id}>
              {newGroup && <p className="eyebrow px-[var(--pad)] pb-1 pt-4">{t(`mail.groups.${group}`)}</p>}
              <ThreadItem thread={th} lang={i18n.language} selected={selected === th.id} checked={checked.has(th.id)}
                labels={th.labels.flatMap((id) => byId.get(id) ?? [])}
                dot={showAccount ? color(th.account_id) : undefined} onClick={() => onSelect(th.id)}
                onCheck={(on) => onCheck([th.id], on)} />
            </Fragment>
          )
        })}
        <div ref={sentinel} />
        {q.isFetchingNextPage && <p className="p-4 text-center text-sm text-muted">{t('common.loading')}</p>}
      </div>
    </>
  )
}

/** Buscador: espera a que se deje de teclear; Escape lo vacía. Se enfoca con «/». */
function SearchBox({ value, onChange }: { value: string; onChange: (q: string) => void }) {
  const { t } = useTranslation()
  const [text, setText] = useState(value)
  const last = useRef(value)
  useEffect(() => {
    if (value !== last.current) setText(value)
    last.current = value
  }, [value])
  useEffect(() => {
    if (text.trim() === last.current) return
    const h = setTimeout(() => { last.current = text.trim(); onChange(text.trim()) }, 300)
    return () => clearTimeout(h)
  }, [text]) // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <form role="search" className="relative" onSubmit={(e) => { e.preventDefault(); last.current = text.trim(); onChange(text.trim()) }}>
      <Icon name="search" className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-muted" />
      <input id="mail-search" type="search" className="input min-h-10 py-1.5 pl-10 pr-10" value={text} enterKeyHint="search"
        placeholder={t('search.placeholder')} aria-label={t('search.label')} autoComplete="off" spellCheck={false}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Escape') {
            e.stopPropagation()
            setText('')
            last.current = ''
            onChange('')
            e.currentTarget.blur()
          }
        }} />
      {text && (
        <button type="button" className="absolute right-1 top-1/2 grid size-8 -translate-y-1/2 place-items-center rounded-md text-muted hover:bg-surface-2"
          aria-label={t('search.clear')} onClick={() => { setText(''); last.current = ''; onChange('') }}>
          <Icon name="close" className="size-4" />
        </button>
      )}
    </form>
  )
}

export function LabelChip({ label, onRemove }: { label: Label; onRemove?: () => void }) {
  const { t } = useTranslation()
  return (
    <span className="inline-flex max-w-[10rem] items-center gap-1 rounded-full px-2 py-px text-xs font-semibold"
      style={{ background: `color-mix(in srgb, ${label.color} 16%, transparent)`, color: `color-mix(in srgb, ${label.color} 75%, var(--fg))` }}>
      <span className="truncate">{label.name}</span>
      {onRemove && (
        <button className="-mr-1 grid size-5 place-items-center rounded-full hover:bg-surface" aria-label={t('actions.removeLabel', { name: label.name })}
          onClick={onRemove}>
          <Icon name="close" className="size-3" />
        </button>
      )}
    </span>
  )
}

function ThreadItem({ thread: th, lang, selected, checked, labels, dot, onClick, onCheck }: {
  thread: ThreadSummary
  lang: string
  selected: boolean
  checked: boolean
  labels: Label[]
  dot?: string
  onClick: () => void
  onCheck: (on: boolean) => void
}) {
  const { t } = useTranslation()
  const unread = th.unread > 0
  const who = th.participants.length ? th.participants : ['?']
  const last = who[who.length - 1]
  const tone = checked
    ? 'bg-[color-mix(in_srgb,var(--accent)_16%,var(--surface))]'
    : selected ? 'bg-[color-mix(in_srgb,var(--accent)_10%,var(--surface))]' : ''
  return (
    <div role="listitem" data-thread={th.id}
      className={`relative grid grid-cols-[2.5rem_minmax(0,1fr)_auto] items-start gap-3 border-b border-line px-[var(--pad)] py-3 hover:bg-surface-2 ${tone}`}>
      {/* El avatar sirve de casilla para seleccionar, como en el móvil. */}
      <button role="checkbox" aria-checked={checked} aria-label={t('actions.select', { subject: th.subject || t('mail.noSubject') })}
        onClick={() => onCheck(!checked)}
        className={`relative z-[1] grid size-10 place-items-center rounded-full text-sm font-bold ${checked ? 'bg-accent text-on-accent' : 'bg-surface-2 hover:ring-2 hover:ring-accent'}`}>
        {checked ? <Icon name="check" /> : <span aria-hidden="true">{initials(last)}</span>}
        {dot && <span className="absolute -bottom-px -right-px size-3 rounded-full border-2 border-surface" style={{ background: dot }} />}
      </button>
      <button aria-current={selected} onClick={onClick} className="flex min-w-0 flex-col gap-0.5 text-left after:absolute after:inset-0 after:content-['']">
        <span className={`flex min-w-0 items-center gap-1.5 ${unread ? 'font-bold' : 'font-semibold'}`}>
          {unread && <span className="size-2 shrink-0 rounded-full bg-accent" aria-label={t('mail.unread')} />}
          <span className="truncate">{who.slice(-3).join(', ')}</span>
          {th.messages > 1 && <span className="shrink-0 text-sm font-normal text-muted tabular-nums">{th.messages}</span>}
        </span>
        <span className={`truncate ${unread ? 'font-semibold' : ''}`}>{th.subject || t('mail.noSubject')}</span>
        <span className="line-clamp-2 text-sm text-muted">{th.snippet}</span>
        {labels.length > 0 && (
          <span className="mt-1 flex flex-wrap gap-1">{labels.map((l) => <LabelChip key={l.id} label={l} />)}</span>
        )}
      </button>
      <span className="flex flex-col items-end gap-1 text-sm text-muted tabular-nums">
        <span className="whitespace-nowrap">{shortDate(th.last_date, lang)}</span>
        <span className="flex gap-1">
          {th.has_note && <Icon name="note" className="size-4" />}
          {th.snooze_until !== null && th.snooze_until * 1000 > Date.now() && <Icon name="clock" className="size-4" />}
          {th.has_attachments && <Icon name="clip" className="size-4" />}
          {th.flagged && <Icon name="star" filled className="size-4 text-warn" />}
        </span>
      </span>
    </div>
  )
}
