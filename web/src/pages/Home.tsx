import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { useLocation, useNavigate, useSearchParams } from 'react-router'
import { api, type Me, type ThreadFilter } from '../lib/api'
import { boxKey, parseBox, type Box } from '../lib/mail'
import { useLiveUpdates } from '../lib/live'
import { useSetMe } from '../lib/session'
import { Sidebar } from '../components/mail/Sidebar'
import { ThreadList } from '../components/mail/ThreadList'
import { Reader } from '../components/mail/Reader'
import { DraftList } from '../components/compose/DraftList'
import { ComposerProvider, useCompose } from '../components/compose/ComposerProvider'
import { emptyCompose, pickIdentity } from '../lib/compose'

/**
 * El correo: barra lateral, lista de hilos y lector. Lo que se ve va en la URL
 * (`a` cuenta, `b` carpeta, `t` hilo) para que el botón Atrás funcione, también en el móvil.
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
  const navigate = useNavigate()
  const location = useLocation()
  const setMe = useSetMe()
  const [params, setParams] = useSearchParams()
  const [drawer, setDrawer] = useState(false)
  useLiveUpdates()

  const accounts = useQuery({ queryKey: ['accounts'], queryFn: api.accounts })
  const mailboxes = useQuery({ queryKey: ['mailboxes'], queryFn: () => api.mailboxes() })
  const list = accounts.data ?? []

  const unified = me.preferences.view === 'unified' && list.length > 1
  const account = unified ? undefined : (list.find((a) => a.id === params.get('a')) ?? list[0])?.id
  const box = parseBox(params.get('b'))
  const thread = params.get('t')
  const filter: ThreadFilter = 'role' in box ? { role: box.role, account } : { mailbox: box.mailbox }
  const focus = me.preferences.mode === 'focus'

  const folder = 'mailbox' in box ? mailboxes.data?.find((m) => m.id === box.mailbox) : undefined
  const title = folder?.name ?? ('role' in box
    ? t(`mail.roles.${focus && box.role === 'inbox' ? 'principal' : box.role}`)
    : t('mail.roles.inbox'))

  function go(change: { a?: string | null; b?: Box; t?: string | null }) {
    const next = new URLSearchParams(params)
    if (change.a !== undefined) {
      if (change.a) next.set('a', change.a)
      else next.delete('a')
    }
    if (change.b) {
      next.set('b', boxKey(change.b))
      next.delete('t')
    }
    if (change.t !== undefined) {
      if (change.t) next.set('t', change.t)
      else next.delete('t')
    }
    // Al abrir un hilo se recuerda que se vino de la lista, para que «Atrás» vuelva a ella.
    setParams(next, { state: change.t ? { fromList: true } : undefined })
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

  async function logout() {
    await api.logout().catch(() => undefined)
    setMe(null)
    navigate('/login', { replace: true })
  }

  const side = (
    <Sidebar me={me} accounts={list} mailboxes={mailboxes.data ?? []} account={unified ? undefined : account} box={box}
      onAccount={chooseAccount} onBox={(b) => { go({ b }); setDrawer(false) }} onLogout={logout}
      onCompose={list.length ? newMessage : undefined} />
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
        <ThreadList title={title} filter={filter} accounts={list} selected={thread} grouped={focus}
          showAccount={unified} onSelect={(id) => go({ t: id })} onMenu={() => setDrawer(true)}
          onCompose={list.length ? newMessage : undefined}
          before={'role' in box && box.role === 'drafts' && drafts.data?.length ? <DraftList drafts={drafts.data} /> : null} />
      </section>
      <main className={`${thread ? 'flex' : 'hidden md:flex'} min-h-0 min-w-0 flex-1 flex-col overflow-y-auto`}>
        <Reader id={thread} font={me.preferences.font} onBack={back} />
      </main>
    </div>
  )
}
