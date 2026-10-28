import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { useNavigate, useParams } from 'react-router'
import { api, ApiError, type Language, type Me } from '../lib/api'
import { useApplyUser, useSetMe } from '../lib/session'
import { setLanguage } from '../i18n'
import { Alert, Field, Narrow, Segmented } from '../components/ui'
import { TotpPanel } from '../components/TotpPanel'

export default function Invite() {
  const { token = '' } = useParams()
  const { t, i18n } = useTranslation()
  const navigate = useNavigate()
  const setMe = useSetMe()
  const check = useQuery({ queryKey: ['invite', token], queryFn: () => api.checkInvite(token) })
  const [form, setForm] = useState({ username: '', password: '', repeat: '', email: '' })
  const [language, setLang] = useState<Language>((i18n.language as Language) ?? 'es')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [created, setCreated] = useState<Me | null>(null)
  const [totp, setTotp] = useState(false)
  useApplyUser(created)

  async function submit(e: React.FormEvent) {
    e.preventDefault()
    setError(null)
    if (form.password !== form.repeat) return setError(t('fields.passwordMismatch'))
    setBusy(true)
    try {
      setCreated(await api.acceptInvite(token, {
        username: form.username, password: form.password, email: form.email || undefined, language,
      }))
    } catch (e) {
      setError(e instanceof ApiError ? e.message : t('common.error'))
    } finally {
      setBusy(false)
    }
  }

  const done = (me: Me) => {
    setMe(me)
    navigate('/', { replace: true })
  }

  if (check.isLoading) return <Narrow><p className="text-muted">{t('common.loading')}</p></Narrow>
  if (!created && !check.data?.valid) return <Narrow><Alert>{t('invite.invalid')}</Alert></Narrow>

  if (created) {
    return (
      <Narrow>
        <div className="card flex flex-col gap-4 p-5 sm:p-7">
          {!totp ? (
            <>
              <h1 className="text-2xl font-bold">{t('invite.totpOffer')}</h1>
              <p className="text-muted">{t('invite.totpOfferBody')}</p>
              <div className="flex flex-wrap gap-2">
                <button className="btn btn-primary" onClick={() => setTotp(true)}>{t('invite.totpYes')}</button>
                <button className="btn btn-ghost" onClick={() => done(created)}>{t('invite.totpLater')}</button>
              </div>
            </>
          ) : (
            <>
              <TotpPanel me={created} onChange={setCreated} autoStart />
              <button className="btn self-start" onClick={() => done(created)}>
                {created.totp_enabled ? t('common.next') : t('invite.totpLater')}
              </button>
            </>
          )}
        </div>
      </Narrow>
    )
  }

  const expires = check.data?.expires_at ? new Date(check.data.expires_at * 1000).toLocaleString(i18n.language) : ''
  return (
    <Narrow>
      <form onSubmit={submit} className="card flex flex-col gap-5 p-5 sm:p-7">
        <h1 className="text-2xl font-bold">{t('invite.title')}</h1>
        <p className="text-muted">{t('invite.body')}</p>
        <p className="hint">{t('invite.expires', { date: expires })}</p>
        {error && <Alert>{error}</Alert>}
        <Segmented label={t('fields.language')} value={language}
          onChange={(l) => { setLang(l); setLanguage(l) }}
          options={(['es', 'en'] as const).map((l) => ({ value: l, label: t(`languages.${l}`) }))} />
        <Field label={t('fields.username')} hint={t('fields.usernameHint')} autoComplete="username" required
          minLength={3} maxLength={32} pattern="[A-Za-z0-9._\-]+" value={form.username}
          onChange={(e) => setForm({ ...form, username: e.target.value })} />
        <Field label={t('fields.password')} hint={t('fields.passwordHint')} type="password" autoComplete="new-password"
          required minLength={10} value={form.password} onChange={(e) => setForm({ ...form, password: e.target.value })} />
        <Field label={t('fields.passwordRepeat')} type="password" autoComplete="new-password" required minLength={10}
          value={form.repeat} onChange={(e) => setForm({ ...form, repeat: e.target.value })} />
        <Field label={`${t('fields.email')} (${t('common.optional')})`} type="email" autoComplete="email"
          value={form.email} onChange={(e) => setForm({ ...form, email: e.target.value })} />
        <button className="btn btn-primary" disabled={busy}>{t('invite.submit')}</button>
      </form>
    </Narrow>
  )
}
