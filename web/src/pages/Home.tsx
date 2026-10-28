import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Link, useNavigate } from 'react-router'
import { api, type Me } from '../lib/api'
import { useSetMe } from '../lib/session'
import { Logo } from '../components/ui'

const FOCUS_BOXES = ['principal', 'novedades', 'recibos', 'later', 'aside', 'screener', 'sent'] as const
const CLASSIC_BOXES = ['inbox', 'sent', 'drafts', 'archive', 'spam'] as const
const ACCOUNT_COLORS = ['#2f9a72', '#c4683a', '#7a3fa0', '#3d7fb5', '#a8862a']

/** Estructura de la app. El correo llega en la fase 2; aquí ya se ven modos, cuentas y ajustes. */
export default function Home({ me }: { me: Me }) {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const setMe = useSetMe()
  const accounts = useQuery({ queryKey: ['accounts'], queryFn: api.accounts })
  const boxes = me.preferences.mode === 'focus' ? FOCUS_BOXES : CLASSIC_BOXES
  const [box, setBox] = useState<string>(boxes[0])
  const [account, setAccount] = useState<string | null>(null)
  const [drawer, setDrawer] = useState(false)
  const current = (boxes as readonly string[]).includes(box) ? box : boxes[0]

  async function logout() {
    await api.logout().catch(() => undefined)
    setMe(null)
    navigate('/login', { replace: true })
  }

  const side = (
    <nav className="flex h-full flex-col gap-5 overflow-y-auto p-4" aria-label={t('home.menu')}>
      <Logo />
      <div className="flex flex-col gap-2">
        <p className="eyebrow">{t('home.accounts')}</p>
        <div className="flex flex-wrap gap-1.5">
          <Chip active={account === null} onClick={() => setAccount(null)}>{t('home.all')}</Chip>
          {accounts.data?.map((a, i) => (
            <Chip key={a.id} active={account === a.id} onClick={() => setAccount(a.id)}>
              <span className="size-2 rounded-full" style={{ background: ACCOUNT_COLORS[i % ACCOUNT_COLORS.length] }} />
              {a.name}
            </Chip>
          ))}
        </div>
      </div>
      <ul className="flex flex-col gap-0.5">
        {boxes.map((b) => (
          <li key={b}>
            <button aria-current={current === b} onClick={() => { setBox(b); setDrawer(false) }}
              className={`w-full rounded-lg px-3 py-2 text-left hover:bg-surface ${current === b ? 'bg-surface font-semibold shadow-[inset_3px_0_0_var(--accent)]' : ''}`}>
              {t(`home.boxes.${b}`)}
            </button>
          </li>
        ))}
      </ul>
      <div className="mt-auto flex flex-col gap-1">
        <p className="px-3 text-sm text-muted">
          {t(`look.modes.${me.preferences.mode}`)} · {me.username}
        </p>
        <Link to="/settings" className="rounded-lg px-3 py-2 hover:bg-surface">{t('home.settings')}</Link>
        <button onClick={logout} className="rounded-lg px-3 py-2 text-left hover:bg-surface">{t('home.logout')}</button>
      </div>
    </nav>
  )

  return (
    <div className="grid h-full lg:grid-cols-[16rem_minmax(0,1fr)]">
      <aside className="hidden border-r border-line bg-surface-2 lg:block">{side}</aside>
      {drawer && (
        <div className="fixed inset-0 z-20 bg-black/40 lg:hidden" onClick={() => setDrawer(false)}>
          <aside className="h-full w-72 max-w-[85vw] bg-surface-2 pt-[env(safe-area-inset-top)]" onClick={(e) => e.stopPropagation()}>
            {side}
          </aside>
        </div>
      )}
      <main className="flex min-w-0 flex-col">
        <header className="flex items-center gap-2 border-b border-line bg-surface px-4 py-3">
          <button className="btn btn-ghost lg:hidden" onClick={() => setDrawer(true)} aria-label={t('home.menu')}>
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
              <path d="M4 7h16M4 12h16M4 17h16" />
            </svg>
          </button>
          <h1 className="text-xl font-bold">{t(`home.boxes.${current}`)}</h1>
        </header>
        <section className="mx-auto flex max-w-lg flex-col items-center gap-3 px-4 py-20 text-center">
          <h2 className="text-lg font-bold">{t('home.empty')}</h2>
          <p className="text-muted">{t('home.emptyBody')}</p>
          {accounts.data?.length === 0 && (
            <>
              <p className="text-muted">{t('home.noAccounts')}</p>
              <Link to="/settings/accounts" className="btn btn-primary">{t('home.addAccount')}</Link>
            </>
          )}
        </section>
      </main>
    </div>
  )
}

function Chip({ active, onClick, children }: { active: boolean; onClick: () => void; children: React.ReactNode }) {
  return (
    <button aria-pressed={active} onClick={onClick}
      className={`inline-flex items-center gap-1.5 rounded-full border px-3 py-0.5 text-sm ${active ? 'border-accent font-semibold text-accent' : 'border-line bg-surface'}`}>
      {children}
    </button>
  )
}
