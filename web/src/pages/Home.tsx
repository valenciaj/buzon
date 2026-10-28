import { useEffect, useRef, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { useLocation, useNavigate, useSearchParams } from 'react-router'
import { api, type From, type Me, type Role, type ThreadFilter } from '../lib/api'
import { actionsFor, boxKey, parseBox, type Box, type MailAction } from '../lib/mail'
import { useLiveUpdates } from '../lib/live'
import { useSetMe } from '../lib/session'
import { Sidebar } from '../components/mail/Sidebar'
import { ThreadList, useThreadPages } from '../components/mail/ThreadList'
import { Reader, REPLY_EVENT, type ReplyKind } from '../components/mail/Reader'
import { Icon } from '../components/mail/Icons'
import { LabelMenu, MoveMenu } from '../components/mail/Menus'
import { useMailActions, type MoveTarget } from '../components/mail/useMailActions'
import { Shortcuts } from '../components/mail/Shortcuts'
import { DraftList } from '../components/compose/DraftList'
import { ComposerProvider, useCompose } from '../components/compose/ComposerProvider'
import { emptyCompose, pickIdentity } from '../lib/compose'

const ACTION_ICONS: Record<MailAction, string> = { archive: 'archive', inbox: 'inbox', trash: 'trash', junk: 'junk', delete: 'trash' }

/**
 * El correo: barra lateral, lista de hilos y lector. Lo que se ve va en la URL
 * (`a` cuenta, `b` carpeta, `q` búsqueda, `t` hilo) para que el botón Atrás funcione, también en el móvil.
 */
export default function Home({ me }: { me: Me }) {
  return (
    <ComposerProvider>
      <Mail me={me} />
    </ComposerProvider>
  )
}

function Mail({ me }: { me: Me }) {
  const { t } = useTranslation()
  const compose = useCompose()
  const identities = useQuery({ queryKey: ['identities'], queryFn: api.identities })
  const drafts = useQuery({ queryKey: ['drafts'], queryFn: api.drafts })
  const labels = useQuery({ queryKey: ['labels'], queryFn: api.labels })
  const navigate = useNavigate()
  const location = useLocation()
  const setMe = useSetMe()
  const [params, setParams] = useSearchParams()
  const [drawer, setDrawer] = useState(false)
  const [help, setHelp] = useState(false)
  useLiveUpdates()

  const accounts = useQuery({ queryKey: ['accounts'], queryFn: api.accounts })
  const mailboxes = useQuery({ queryKey: ['mailboxes'], queryFn: () => api.mailboxes() })
  const list = accounts.data ?? []
  const boxes = mailboxes.data ?? []

  const unified = me.preferences.view === 'unified' && list.length > 1
  const account = unified ? undefined : (list.find((a) => a.id === params.get('a')) ?? list[0])?.id
  const box = parseBox(params.get('b'))
  const search = params.get('q') ?? ''
  const thread = params.get('t')
  const filter: ThreadFilter = search
    ? { q: search, account }
    : 'role' in box ? { role: box.role, account } : 'mailbox' in box ? { mailbox: box.mailbox } : { label: box.label, account }
  const focus = me.preferences.mode === 'focus'

  const folder = 'mailbox' in box ? boxes.find((m) => m.id === box.mailbox) : undefined
  const label = 'label' in box ? labels.data?.find((l) => l.id === box.label) : undefined
  const title = search
    ? t('search.title', { q: search })
    : folder?.name ?? label?.name ?? ('role' in box
      ? t(`mail.roles.${focus && box.role === 'inbox' ? 'principal' : box.role}`)
      : t('mail.roles.inbox'))

  // Desde dónde se mueve: la carpeta que se ve. En una etiqueta o una búsqueda, de donde estén.
  const role: Role | null | undefined = search ? undefined : 'role' in box ? box.role : folder?.role
  const from: From = search ? {} : 'role' in box ? { from_role: box.role } : 'mailbox' in box ? { from_mailbox: box.mailbox } : {}
  const actions = actionsFor(role)
  const mail = useMailActions(from)

  // La selección vale para lo que se ve: se vacía al cambiar de carpeta o de búsqueda.
  const [checked, setChecked] = useState<ReadonlySet<string>>(new Set())
  const view = JSON.stringify(filter)
  useEffect(() => setChecked(new Set()), [view])
  const check = (ids: string[], on: boolean) =>
    setChecked((c) => (on ? new Set([...c, ...ids]) : new Set([...c].filter((x) => !ids.includes(x)))))

  const pages = useThreadPages(filter)
  const visible = (pages.data?.pages.flatMap((p) => p.threads) ?? []).filter((x) => !mail.hidden.has(x.id))

  function go(change: { a?: string | null; b?: Box; t?: string | null; q?: string }) {
    const next = new URLSearchParams(params)
    if (change.a !== undefined) {
      if (change.a) next.set('a', change.a)
      else next.delete('a')
    }
    if (change.b) {
      next.set('b', boxKey(change.b))
      next.delete('t')
      next.delete('q')
    }
    if (change.q !== undefined) {
      if (change.q) next.set('q', change.q)
      else next.delete('q')
    }
    if (change.t !== undefined) {
      if (change.t) next.set('t', change.t)
      else next.delete('t')
    }
    // Al abrir un hilo se recuerda que se vino de la lista, para que «Atrás» vuelva a ella.
    setParams(next, { state: change.t ? { fromList: true } : undefined, replace: change.q !== undefined && !change.t })
  }

  function back() {
    if ((location.state as { fromList?: boolean } | null)?.fromList) navigate(-1)
    else go({ t: null })
  }

  async function chooseAccount(id: string | null) {
    const view = id ? 'per_account' : 'unified'
    go({ a: id, b: { role: 'inbox' } })
    setDrawer(false)
    if (view !== me.preferences.view) {
      setMe({ ...me, preferences: { ...me.preferences, view } })
      setMe(await api.updatePreferences({ view }).catch(() => me))
    }
  }

  function newMessage() {
    setDrawer(false)
    compose(emptyCompose(pickIdentity(identities.data ?? [], null, account)))
  }

  // «Escribir» desde Contactos llega como ?to=…
  const wrote = useRef(false)
  useEffect(() => {
    const to = params.get('to')
    if (!to || wrote.current || !identities.data) return
    wrote.current = true
    compose({ ...emptyCompose(pickIdentity(identities.data, null, account)), to: [{ name: params.get('name'), email: to }] })
    const next = new URLSearchParams(params)
    next.delete('to')
    next.delete('name')
    setParams(next, { replace: true })
  }, [params, identities.data]) // eslint-disable-line react-hooks/exhaustive-deps

  async function logout() {
    await api.logout().catch(() => undefined)
    setMe(null)
    navigate('/login', { replace: true })
  }

  /** Tras mover el hilo abierto, se pasa al siguiente (o a la lista en el móvil). */
  function afterMove(ids: string[]) {
    if (!thread || !ids.includes(thread)) return
    const wide = window.matchMedia('(min-width: 768px)').matches
    const rest = visible.filter((x) => !ids.includes(x.id) || x.id === thread)
    const i = rest.findIndex((x) => x.id === thread)
    const next = wide ? (rest[i + 1] ?? rest[i - 1]) : undefined
    go({ t: next && next.id !== thread ? next.id : null })
  }

  function moveIds(target: MoveTarget, ids: string[]) {
    const open = thread
    mail.move(target, ids, { onUndo: () => open && ids.includes(open) && go({ t: open }) })
    afterMove(ids)
    check(ids, false)
  }

  /** A lo que se aplican los atajos: la selección o, si no hay, el hilo abierto. */
  const targets = () => (checked.size ? [...checked] : thread ? [thread] : [])

  function key(k: string) {
    const ids = targets()
    const at = visible.findIndex((x) => x.id === thread)
    switch (k) {
      case 'j': case 'ArrowDown': {
        const next = visible[at + 1] ?? (at < 0 ? visible[0] : undefined)
        if (next) go({ t: next.id })
        break
      }
      case 'k': case 'ArrowUp': {
        const prev = at > 0 ? visible[at - 1] : undefined
        if (prev) go({ t: prev.id })
        break
      }
      case 'o': case 'Enter':
        if (!thread && visible[0]) go({ t: visible[0].id })
        break
      case 'u': case 'Escape':
        if (help) setHelp(false)
        else if (checked.size) setChecked(new Set())
        else if (thread) back()
        break
      case 'e':
        if (actions.includes('archive')) moveIds('archive', ids)
        else if (actions.includes('inbox')) moveIds('inbox', ids)
        break
      case '#':
        moveIds(actions.includes('trash') ? 'trash' : 'delete', ids)
        break
      case '!':
        if (actions.includes('junk')) moveIds('junk', ids)
        break
      case 's': {
        const starred = visible.filter((x) => ids.includes(x.id)).every((x) => x.flagged)
        mail.flags(ids, { flagged: !starred })
        break
      }
      case 'I':
        mail.flags(ids, { seen: true })
        break
      case 'U':
        mail.flags(ids, { seen: false })
        if (thread && ids.includes(thread)) back()
        break
      case 'x':
        if (thread) check([thread], !checked.has(thread))
        break
      case 'r': case 'a': case 'f':
        if (thread) window.dispatchEvent(new CustomEvent<ReplyKind>(REPLY_EVENT, { detail: k === 'r' ? 'reply' : k === 'a' ? 'all' : 'forward' }))
        break
      case 'c':
        if (list.length) newMessage()
        break
      case '/':
        document.getElementById('mail-search')?.focus()
        break
      case '?':
        setHelp(!help)
        break
      case 'gi': go({ b: { role: 'inbox' } }); break
      case 'gs': go({ b: { role: 'sent' } }); break
      case 'gd': go({ b: { role: 'drafts' } }); break
      case 'ga': go({ b: { role: 'archive' } }); break
    }
  }

  const keyRef = useRef(key)
  keyRef.current = key
  useEffect(() => {
    let prefix = ''
    let timer: ReturnType<typeof setTimeout> | undefined
    const on = (e: KeyboardEvent) => {
      if (e.ctrlKey || e.metaKey || e.altKey || e.isComposing) return
      const el = e.target as HTMLElement | null
      if (el && (el.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName))) return
      // Con un menú o el redactor abiertos, las teclas son suyas.
      if (document.querySelector('[role="menu"]') || el?.closest('[role="dialog"]:not([data-shortcuts])')) return
      const k = e.key
      if (prefix === 'g') {
        prefix = ''
        clearTimeout(timer)
        if ('isda'.includes(k)) {
          e.preventDefault()
          keyRef.current(`g${k}`)
        }
        return
      }
      if (k === 'g') {
        prefix = 'g'
        timer = setTimeout(() => { prefix = '' }, 1200)
        return
      }
      // Enter y flechas solo cuando nada tiene el foco, para no robar el de los botones.
      if ((k === 'Enter' || k.startsWith('Arrow')) && el && el !== document.body) return
      if (!/^([jkoue#!sIUxrafc/?]|Escape|Enter|ArrowUp|ArrowDown)$/.test(k)) return
      e.preventDefault()
      keyRef.current(k)
    }
    window.addEventListener('keydown', on)
    return () => {
      window.removeEventListener('keydown', on)
      clearTimeout(timer)
    }
  }, [])

  const side = (
    <Sidebar me={me} accounts={list} mailboxes={boxes} account={unified ? undefined : account} box={box}
      onAccount={chooseAccount} onBox={(b) => { go({ b }); setDrawer(false) }} onLogout={logout}
      onCompose={list.length ? newMessage : undefined} />
  )

  const ids = [...checked]
  const picked = visible.filter((x) => checked.has(x.id))
  const allRead = picked.every((x) => x.unread === 0)
  const allStarred = picked.length > 0 && picked.every((x) => x.flagged)
  const common = picked.length ? picked[0].labels.filter((l) => picked.every((x) => x.labels.includes(l))) : []
  const accountsPicked = new Set(picked.map((x) => x.account_id))
  const toolbar = (
    <>
      {actions.map((a) => (
        <button key={a} className={`btn btn-ghost px-2.5 ${a === 'delete' ? 'text-crit' : ''}`} title={t(`actions.${a}`)} onClick={() => moveIds(a, ids)}>
          <Icon name={ACTION_ICONS[a]} /> <span className="sr-only">{t(`actions.${a}`)}</span>
        </button>
      ))}
      <button className="btn btn-ghost px-2.5" title={allRead ? t('actions.unread') : t('actions.read')}
        onClick={() => mail.flags(ids, { seen: !allRead })}>
        <Icon name="unread" /> <span className="sr-only">{allRead ? t('actions.unread') : t('actions.read')}</span>
      </button>
      <button className="btn btn-ghost px-2.5" title={allStarred ? t('actions.unstar') : t('actions.star')}
        onClick={() => mail.flags(ids, { flagged: !allStarred })}>
        <Icon name="star" filled={allStarred} className={allStarred ? 'text-warn' : ''} />
        <span className="sr-only">{allStarred ? t('actions.unstar') : t('actions.star')}</span>
      </button>
      <LabelMenu current={common} onChange={(add, remove) => mail.label(ids, add, remove)} compact />
      {accountsPicked.size === 1 && (
        <MoveMenu mailboxes={boxes} account={[...accountsPicked][0]} compact
          onMove={(m) => moveIds({ mailbox: m.id, name: m.role ? t(`mail.roles.${m.role}`) : m.name }, ids)} />
      )}
    </>
  )

  return (
    <div className="flex h-full">
      <aside className="hidden w-64 shrink-0 border-r border-line bg-surface-2 lg:block">{side}</aside>
      {drawer && (
        <div className="fixed inset-0 z-20 bg-black/40 lg:hidden" onClick={() => setDrawer(false)}>
          <aside className="h-full w-72 max-w-[85vw] bg-surface-2 pt-[env(safe-area-inset-top)]" onClick={(e) => e.stopPropagation()}>
            {side}
          </aside>
        </div>
      )}
      <section className={`${thread ? 'hidden md:flex' : 'flex'} min-h-0 w-full shrink-0 flex-col border-r border-line bg-surface md:w-[22rem] lg:w-[26rem]`}>
        <ThreadList title={title} filter={filter} accounts={list} selected={thread} grouped={focus && !search}
          showAccount={unified} onSelect={(id) => go({ t: id })} onMenu={() => setDrawer(true)}
          onCompose={list.length ? newMessage : undefined}
          hidden={mail.hidden} checked={checked} onCheck={check} toolbar={toolbar}
          search={search} onSearch={(q) => go({ q, t: null })}
          before={!search && 'role' in box && box.role === 'drafts' && drafts.data?.length ? <DraftList drafts={drafts.data} /> : null} />
      </section>
      <main className={`${thread ? 'flex' : 'hidden md:flex'} min-h-0 min-w-0 flex-1 flex-col overflow-y-auto`}>
        <Reader id={thread} font={me.preferences.font} onBack={back} actions={actions} mailboxes={boxes}
          onMove={(target) => thread && moveIds(target, [thread])}
          onLabel={(add, remove) => thread && mail.label([thread], add, remove)} />
      </main>
      <button className="fixed bottom-4 right-4 z-10 hidden size-9 place-items-center rounded-full border border-line bg-surface text-muted shadow hover:text-fg lg:grid"
        title={t('shortcuts.open')} aria-label={t('shortcuts.open')} onClick={() => setHelp(true)}>
        <Icon name="keyboard" />
      </button>
      {help && <Shortcuts onClose={() => setHelp(false)} />}
    </div>
  )
}
