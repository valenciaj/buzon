import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import QRCode from 'qrcode'
import { api, ApiError, type Me } from '../lib/api'
import { Alert, Field } from './ui'

/** Activa o desactiva la verificación en dos pasos. */
export function TotpPanel({ me, onChange, autoStart = false }: { me: Me; onChange: (me: Me) => void; autoStart?: boolean }) {
  const { t } = useTranslation()
  const [pending, setPending] = useState<{ secret: string; qr: string } | null>(null)
  const [code, setCode] = useState('')
  const [password, setPassword] = useState('')
  const [disabling, setDisabling] = useState(false)
  const [msg, setMsg] = useState<{ kind: 'ok' | 'error'; text: string } | null>(null)

  const started = useRef(false)
  useEffect(() => {
    if (autoStart && !me.totp_enabled && !started.current) {
      started.current = true
      start()
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [autoStart])

  const fail = (e: unknown) => setMsg({ kind: 'error', text: e instanceof ApiError ? e.message : t('common.error') })

  async function start() {
    setMsg(null)
    try {
      const s = await api.totpSetup()
      setPending({ secret: s.secret, qr: await QRCode.toDataURL(s.otpauth_url, { margin: 1, width: 200 }) })
    } catch (e) {
      fail(e)
    }
  }

  async function confirm(e: React.FormEvent) {
    e.preventDefault()
    try {
      onChange(await api.totpEnable(code))
      setPending(null)
      setCode('')
      setMsg({ kind: 'ok', text: t('totp.enabled') })
    } catch (e) {
      fail(e)
    }
  }

  async function disable(e: React.FormEvent) {
    e.preventDefault()
    try {
      onChange(await api.totpDisable(password))
      setDisabling(false)
      setPassword('')
      setMsg({ kind: 'ok', text: t('totp.disabled') })
    } catch (e) {
      fail(e)
    }
  }

  return (
    <section className="flex flex-col gap-4">
      <h2 className="text-lg font-bold">{t('totp.title')}</h2>
      <p className={me.totp_enabled ? 'text-good' : 'text-muted'}>{me.totp_enabled ? t('totp.on') : t('totp.off')}</p>
      {msg && <Alert kind={msg.kind}>{msg.text}</Alert>}

      {!me.totp_enabled && !pending && (
        <button className="btn btn-primary self-start" onClick={start}>{t('totp.start')}</button>
      )}

      {pending && (
        <form onSubmit={confirm} className="card flex flex-col gap-4 p-4">
          <p>{t('totp.scan')}</p>
          <img src={pending.qr} alt="QR" width={200} height={200} className="rounded-lg bg-white p-2" />
          <p className="hint">{t('totp.manual')}</p>
          <code className="break-all rounded-lg bg-bg px-3 py-2 font-mono text-sm tracking-wider">{pending.secret}</code>
          <Field label={t('totp.code')} inputMode="numeric" autoComplete="one-time-code" pattern="[0-9]{6}" maxLength={6}
            value={code} onChange={(e) => setCode(e.target.value)} required />
          <button className="btn btn-primary self-start">{t('totp.confirm')}</button>
        </form>
      )}

      {me.totp_enabled && !disabling && (
        <button className="btn self-start" onClick={() => setDisabling(true)}>{t('totp.disable')}</button>
      )}
      {disabling && (
        <form onSubmit={disable} className="card flex flex-col gap-4 p-4">
          <Field label={t('totp.disablePrompt')} type="password" autoComplete="current-password" value={password}
            onChange={(e) => setPassword(e.target.value)} required />
          <div className="flex gap-2">
            <button className="btn btn-primary">{t('totp.disable')}</button>
            <button type="button" className="btn btn-ghost" onClick={() => setDisabling(false)}>{t('common.cancel')}</button>
          </div>
        </form>
      )}
    </section>
  )
}
