import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'
import { api, ApiError } from '../lib/api'
import { useSetMe } from '../lib/session'
import { Alert, Field, Narrow } from '../components/ui'

export default function Login() {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const setMe = useSetMe()
  const [form, setForm] = useState({ username: '', password: '', totp: '' })
  const [needTotp, setNeedTotp] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  async function submit(e: React.FormEvent) {
    e.preventDefault()
    setBusy(true)
    setError(null)
    try {
      setMe(await api.login({ ...form, totp: needTotp ? form.totp : undefined }))
      navigate('/', { replace: true })
    } catch (e) {
      if (e instanceof ApiError && e.code === 'totp_required') setNeedTotp(true)
      else setError(e instanceof ApiError ? e.message : t('common.error'))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Narrow>
      <form onSubmit={submit} className="card flex flex-col gap-5 p-5 sm:p-7">
        <h1 className="text-2xl font-bold">{t('login.title')}</h1>
        {error && <Alert>{error}</Alert>}
        <Field label={t('fields.username')} autoComplete="username" required value={form.username}
          readOnly={needTotp} onChange={(e) => setForm({ ...form, username: e.target.value })} />
        <Field label={t('fields.password')} type="password" autoComplete="current-password" required
          readOnly={needTotp} value={form.password} onChange={(e) => setForm({ ...form, password: e.target.value })} />
        {needTotp && (
          <Field label={t('login.totp')} hint={t('login.totpHint')} inputMode="numeric" autoComplete="one-time-code"
            pattern="[0-9]{6}" maxLength={6} required autoFocus value={form.totp}
            onChange={(e) => setForm({ ...form, totp: e.target.value })} />
        )}
        <button className="btn btn-primary" disabled={busy}>{t('login.submit')}</button>
      </form>
    </Narrow>
  )
}
