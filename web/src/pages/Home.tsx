import { useEffect, useRef, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { useLocation, useNavigate, useSearchParams } from 'react-router'
import { api, type FlowChange, type From, type Me, type Role, type ThreadFilter, type View } from '../lib/api'
import { actionsFor, boxKey, parseBox, snoozeOptions, type Box, type MailAction } from '../lib/mail'
import { useLiveUpdates } from '../lib/live'
import { useSetMe } from '../lib/session'
import { Sidebar, VIEW_ICONS } from '../components/mail/Sidebar'
import { CommandPalette, type Command } from '../components/mail/CommandPalette'
import { ThreadList, useThreadPages } from '../components/mail/ThreadList'
import { Reader, REPLY_EVENT, type ReplyKind } from '../components/mail/Reader'
import { Icon } from '../components/mail/Icons'
import { LabelMenu, MoveMenu, SnoozeMenu } from '../components/mail/Menus'
import { Screener } from '../components/mail/Screener'
import { PageView } from '../components/mail/Pages'
import { useMailActions, type MoveTarget } from '../components/mail/useMailActions'
import { Shortcuts } from '../components/mail/Shortcuts'
import { DraftList } from '../components/compose/DraftList'
import { ComposerProvider, useCompose } from '../components/compose/ComposerProvider'
import { emptyCompose, pickIdentity } from '../lib/compose'
import { manualUrl } from '../lib/manual'

const ACTION_ICONS: Record<MailAction, string> = { archive: 'archive', inbox: 'inbox', trash: 'trash', junk: 'junk', delete: 'trash' }

/** Vistas del modo Enfoque que son parte de la entrada (el resto son pilas). */
const INBOX_VIEWS: View[] = ['principal', 'news', 'receipts', 'screener', 'blocked', 'bubbled']

/** Si un cambio en las pilas saca los hilos de la vista que se está viendo. */
function leaves(view: View | undefined, c: FlowChange): boolean {
  if (!view) return false
  const parked = !!(c.reply_later || c.aside || c.snooze_until)
  if (INBOX_VIEWS.includes(view)) return parked
  if (view === 'reply_later') return c.reply_later === false || !!c.snooze_until
  if (view === 'aside') return c.aside === false || !!c.snooze_until
  return view === 'snoozed' && c.snooze_until === 0
}

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
  const { t, i18n } = useTranslation()
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
  const [palette, setPalette] = useState(false)
  useLiveUpdates(me)

  const accounts = useQuery({ queryKey: ['accounts'], queryFn: api.accounts })
  const mailboxes = useQuery({ queryKey: ['mailboxes'], queryFn: () => api.mailboxes() })
  const list = accounts.data ?? []
  const boxes = mailboxes.data ?? []

  const unified = me.preferences.view === 'unified' && list.length > 1
  const account = unified ? undefined : (list.find((a) => a.id === params.get('a')) ?? list[0])?.id
  const focus = me.preferences.mode === 'focus'
  // En Enfoque se empieza en Principal; la entrada completa sigue en «Más carpetas».
  const box: Box = focus && !params.get('b') ? { view: 'principal' } : parseBox(params.get('b'))
  const view = 'view' in box ? box.view : undefined
  const search = params.get('q') ?? ''
  const thread = params.get('t')
  const filter: ThreadFilter = search
    ? { q: search, account }
    : 'role' in box ? { role: box.role, account }
      : 'mailbox' in box ? { mailbox: box.mailbox }
        : 'label' in box ? { label: box.label, account }
          : 'view' in box ? { view: box.view, account } : { view: 'principal', account }
  const page = 'page' in box && !search ? box.page : undefined
  const home: Box = focus ? { view: 'principal' } : { role: 'inbox' }

  const folder = 'mailbox' in box ? boxes.find((m) => m.id === box.mailbox) : undefined
  const label = 'label' in box ? labels.data?.find((l) => l.id === box.label) : undefined
  const title = search
    ? t('search.title', { q: search })
    : folder?.name ?? label?.name ?? ('role' in box ? t(`mail.roles.${box.role}`) : 'page' in box ? t(`pages.${box.page}`)
      : view ? t(`flow.views.${view}`) : t('mail.roles.inbox'))

  // Desde dónde se mueve: la carpeta que se ve. En una etiqueta, una pila o una búsqueda, de donde estén.
  const inboxView = !!view && INBOX_VIEWS.includes(view)
  const role: Role | null | undefined = search ? undefined : 'role' in box ? box.role : inboxView ? 'inbox' : folder?.role
  const from: From = search ? {}
    : 'role' in box ? { from_role: box.role }
      : 'mailbox' in box ? { from_mailbox: box.mailbox } : inboxView ? { from_role: 'inbox' } : {}
  const actions = actionsFor(role)
  const mail = useMailActions(from)

  // La selección vale para lo que se ve: se vacía al cambiar de carpeta o de búsqueda.
  const [checked, setChecked] = useState<ReadonlySet<string>>(new Set())
  const viewKey = JSON.stringify(filter)
  useEffect(() => setChecked(new Set()), [viewKey])
  const check = (ids: string[], on: boolean) =>
    setChecked((c) => (on ? new Set([...c, ...ids]) : new Set([...c].filter((x) => !ids.includes(x)))))

  const pages = useThreadPages(filter, !page)
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
    const pref = id ? 'per_account' : 'unified'
    go({ a: id, b: home })
    setDrawer(false)
    if (pref !== me.preferences.view) {
      setMe({ ...me, preferences: { ...me.preferences, view: pref } })
      setMe(await api.updatePreferences({ view: pref }).catch(() => me))
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

  function flowIds(change: FlowChange, ids: string[]) {
    const out = leaves(view, change)
    const open = thread
    mail.flow(ids, change, { leaves: out, onUndo: () => out && open && ids.includes(open) && go({ t: open }) })
    if (out) {
      afterMove(ids)
      check(ids, false)
    }
  }

  /** «Responder en bloque»: abre el primero de la pila y empieza la respuesta. */
  function replyInBlock() {
    const first = visible[0]
    if (!first) return
    go({ t: first.id })
    setTimeout(() => window.dispatchEvent(new CustomEvent<ReplyKind>(REPLY_EVENT, { detail: 'reply' })), 600)
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
      case 'l': {
        const on = visible.filter((x) => ids.includes(x.id)).every((x) => x.reply_later)
        flowIds({ reply_later: !on }, ids)
        break
      }
      case 'p': {
        const on = visible.filter((x) => ids.includes(x.id)).every((x) => x.aside)
        flowIds({ aside: !on }, ids)
        break
      }
      case 'b': {
        const tomorrow = snoozeOptions().find((o) => o.key === 'tomorrow')!
        flowIds({ snooze_until: Math.floor(tomorrow.at.getTime() / 1000) }, ids)
        break
      }
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
      case 'gi': go({ b: home }); break
      case 'gt': go({ b: { page: 'today' } }); break
      case 'gs': go({ b: { role: 'sent' } }); break
      case 'gd': go({ b: { role: 'drafts' } }); break
      case 'ga': go({ b: { role: 'archive' } }); break
    }
  }

  const keyRef = useRef(key)
  keyRef.current = key

  // Ctrl+K (⌘K en Mac) abre la paleta de órdenes, también desde un campo de texto.
  useEffect(() => {
    const on = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === 'k') {
        e.preventDefault()
        setPalette((p) => !p)
      }
    }
    window.addEventListener('keydown', on)
    return () => window.removeEventListener('keydown', on)
  }, [])

  async function setPref(change: Partial<Me['preferences']>) {
    setMe({ ...me, preferences: { ...me.preferences, ...change } })
    setMe(await api.updatePreferences(change).catch(() => me))
  }

  const commands = (): Command[] => {
    const goTo = t('palette.groups.go')
    const act = t('palette.groups.actions')
    const prefs = t('palette.groups.settings')
    const nav = (id: string, label: string, icon: string, b: Box, hint?: string): Command =>
      ({ id, label, icon, group: goTo, hint, run: () => go({ b }) })
    const out: Command[] = [nav('p:today', t('pages.today'), 'sun', { page: 'today' }, 'g t')]
    if (focus) {
      for (const v of ['principal', 'news', 'receipts', 'screener', 'reply_later', 'aside', 'snoozed', 'blocked'] as View[]) {
        out.push(nav(`v:${v}`, t(`flow.views.${v}`), VIEW_ICONS[v], { view: v }))
      }
    }
    for (const r of ['inbox', 'sent', 'drafts', 'archive', 'junk', 'trash'] as Role[]) {
      out.push(nav(r, t(`mail.roles.${r}`), r, { role: r }, ({ inbox: 'g i', sent: 'g s', drafts: 'g d', archive: 'g a' } as Record<string, string>)[r]))
    }
    out.push(nav('p:scheduled', t('pages.scheduled'), 'clock', { page: 'scheduled' }))
    out.push(nav('p:followups', t('pages.followups'), 'bell', { page: 'followups' }))
    for (const l of labels.data ?? []) out.push({ ...nav(`l:${l.id}`, l.name, 'tag', { label: l.id }), keywords: t('labels.title') })
    for (const f of boxes.filter((m) => !m.role)) out.push({ ...nav(`m:${f.id}`, f.name, 'folder', { mailbox: f.id }), keywords: t('mail.folders') })
    out.push({ id: 'contacts', label: t('home.contacts'), icon: 'contacts', group: goTo, run: () => navigate('/contacts') })

    if (list.length) out.push({ id: 'compose', label: t('compose.new'), icon: 'compose', group: act, hint: 'c', run: newMessage })
    if (thread) {
      out.push({ id: 'reply', label: t('compose.replyAction'), icon: 'reply', group: act, hint: 'r', run: () => key('r') })
      out.push({ id: 'replyAll', label: t('compose.replyAll'), icon: 'reply', group: act, hint: 'a', run: () => key('a') })
      out.push({ id: 'forward', label: t('compose.forwardAction'), icon: 'forward', group: act, hint: 'f', run: () => key('f') })
      for (const a of actions) {
        out.push({ id: `move-${a}`, label: t(`actions.${a}`), icon: ACTION_ICONS[a], group: act, run: () => moveIds(a, [thread]) })
      }
      if (focus) {
        out.push({ id: 'later', label: t('flow.replyLater'), icon: 'later', group: act, hint: 'l', run: () => key('l') })
        out.push({ id: 'aside', label: t('flow.aside'), icon: 'pin', group: act, hint: 'p', run: () => key('p') })
      }
    }
    out.push({ id: 'shortcuts', label: t('shortcuts.title'), icon: 'keyboard', group: act, hint: '?', run: () => setHelp(true) })
    out.push({ id: 'manual', label: t('home.manual'), icon: 'help', group: act, keywords: 'ayuda help manual', run: () => window.open(manualUrl(i18n.language), '_blank', 'noopener') })

    const other = focus ? 'classic' : 'focus'
    out.push({ id: 'mode', label: t('palette.mode', { mode: t(`look.modes.${other}`) }), icon: 'sliders', group: prefs, run: () => setPref({ mode: other }) })
    for (const theme of ['light', 'dark', 'system'] as const) {
      if (theme !== me.preferences.theme) {
        out.push({ id: `theme-${theme}`, label: t('palette.theme', { theme: t(`look.themes.${theme}`) }), icon: 'sun', group: prefs, keywords: 'tema theme', run: () => setPref({ theme }) })
      }
    }
    for (const tab of ['look', 'security', 'accounts', 'identities', 'labels', 'api']) {
      out.push({ id: `settings-${tab}`, label: `${t('home.settings')}: ${t(`settings.tabs.${tab}`)}`, icon: 'sliders', group: prefs, run: () => navigate(`/settings/${tab}`) })
    }
    out.push({ id: 'logout', label: t('home.logout'), icon: 'back', group: prefs, run: logout })
    return out
  }
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
        if ('isdat'.includes(k)) {
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
      if (!/^([jkoue#!sIUxrafclpb/?]|Escape|Enter|ArrowUp|ArrowDown)$/.test(k)) return
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
  const allLater = picked.length > 0 && picked.every((x) => x.reply_later)
  const allAside = picked.length > 0 && picked.every((x) => x.aside)
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
      <button className="btn btn-ghost px-2.5" title={allLater ? t('flow.unReplyLater') : t('flow.replyLater')}
        onClick={() => flowIds({ reply_later: !allLater }, ids)}>
        <Icon name="later" className={allLater ? 'text-accent' : ''} />
        <span className="sr-only">{allLater ? t('flow.unReplyLater') : t('flow.replyLater')}</span>
      </button>
      <button className="btn btn-ghost px-2.5" title={allAside ? t('flow.unAside') : t('flow.aside')}
        onClick={() => flowIds({ aside: !allAside }, ids)}>
        <Icon name="pin" className={allAside ? 'text-accent' : ''} />
        <span className="sr-only">{allAside ? t('flow.unAside') : t('flow.aside')}</span>
      </button>
      <SnoozeMenu snoozed={picked.some((x) => x.snooze_until !== null)} onSnooze={(until) => flowIds({ snooze_until: until }, ids)} compact />
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
        {page ? (
          <PageView page={page} me={me} account={account} onMenu={() => setDrawer(true)}
            onBox={(b) => go({ b })} onOpen={(id) => go({ t: id })} />
        ) : view === 'screener' && !search ? (
          <Screener selected={thread} onOpen={(id) => go({ t: id })} onMenu={() => setDrawer(true)}
            onBlocked={() => go({ b: { view: 'blocked' } })} />
        ) : (
          <ThreadList title={title} filter={filter} accounts={list} selected={thread} grouped={focus && !search && !view?.startsWith('reply')}
            showAccount={unified} onSelect={(id) => go({ t: id })} onMenu={() => setDrawer(true)}
            onCompose={list.length ? newMessage : undefined}
            hidden={mail.hidden} checked={checked} onCheck={check} toolbar={toolbar}
            search={search} onSearch={(q) => go({ q, t: null })}
            pinned={view === 'principal' && !search ? { title: t('flow.views.bubbled'), filter: { view: 'bubbled', account } } : undefined}
            emptyText={view && !inboxView && !search ? <b className="block text-fg">{t('flow.emptyPile')}</b> : undefined}
            actions={view === 'reply_later' && visible.length > 0 && !search ? (
              <button className="btn px-3 text-sm" onClick={replyInBlock}><Icon name="reply" /> <span className="max-sm:sr-only">{t('flow.replyAllInBlock')}</span></button>
            ) : undefined}
            before={!search && 'role' in box && box.role === 'drafts' && drafts.data?.length ? <DraftList drafts={drafts.data} /> : null} />
        )}
      </section>
      <main className={`${thread ? 'flex' : 'hidden md:flex'} min-h-0 min-w-0 flex-1 flex-col overflow-y-auto`}>
        <Reader id={thread} font={me.preferences.font} onBack={back} actions={actions} mailboxes={boxes}
          onMove={(target) => thread && moveIds(target, [thread])}
          onLabel={(add, remove) => thread && mail.label([thread], add, remove)}
          onFlow={(change) => thread && flowIds(change, [thread])} focus={focus} />
      </main>
      {list.length > 0 && (
        <button className="btn btn-primary fixed bottom-4 right-4 z-10 hidden size-11 rounded-full px-0 shadow-lg lg:inline-flex"
          title={t('compose.new')} aria-label={t('compose.new')} onClick={newMessage}>
          <Icon name="compose" />
        </button>
      )}
      <button className={`fixed bottom-5 ${list.length > 0 ? 'right-18' : 'right-4'} z-10 hidden size-9 place-items-center rounded-full border border-line bg-surface text-muted shadow hover:text-fg lg:grid`}
        title={t('shortcuts.open')} aria-label={t('shortcuts.open')} onClick={() => setHelp(true)}>
        <Icon name="keyboard" />
      </button>
      <button className={`fixed bottom-5 ${list.length > 0 ? 'right-30' : 'right-16'} z-10 hidden size-9 place-items-center rounded-full border border-line bg-surface text-muted shadow hover:text-fg lg:grid`}
        title={`${t('palette.title')} (Ctrl+K)`} aria-label={t('palette.title')} onClick={() => setPalette(true)}>
        <Icon name="command" />
      </button>
      {help && <Shortcuts onClose={() => setHelp(false)} />}
      {palette && <CommandPalette commands={commands()} onClose={() => setPalette(false)} onSearch={(q) => go({ q, t: null })} />}
    </div>
  )
}
