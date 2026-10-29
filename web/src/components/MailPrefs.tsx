import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { Preferences } from '../lib/api'
import { notifyEnabled, notifySupported, setNotify } from '../lib/notify'
import { Alert, Segmented } from './ui'

const UNDO = [0, 5, 10, 20, 30] as const

/** Ajustes del envío y de los avisos (en Apariencia, para no crear otra pestaña). */
export function MailPrefs({ value, onChange }: { value: Preferences; onChange: (c: Partial<Preferences>) => void }) {
  const { t } = useTranslation()
  const [on, setOn] = useState(notifyEnabled())
  const [denied, setDenied] = useState(false)
  async function toggle() {
    const next = await setNotify(!on)
    setDenied(!on && !next)
    setOn(next)
  }
  return (
    <section className="flex flex-col gap-6 border-t border-line pt-6">
      <h2 className="text-lg font-bold">{t('prefs.title')}</h2>
      <Segmented label={t('prefs.undo')} hint={t('prefs.undoHint')} value={String(value.undo_send)}
        onChange={(v) => onChange({ undo_send: Number(v) })}
        options={UNDO.map((s) => ({ value: String(s), label: s ? t('prefs.seconds', { count: s }) : t('prefs.noUndo') }))} />
      <div className="flex flex-col gap-1.5">
        <span className="label">{t('prefs.notify')}</span>
        {notifySupported() ? (
          <label className="flex items-start gap-2">
            <input type="checkbox" className="mt-1 size-4 shrink-0 accent-[var(--accent)]" checked={on} onChange={toggle} />
            <span>{t('prefs.notifyBody')}</span>
          </label>
        ) : (
          <p className="text-muted">{t('prefs.notifyUnsupported')}</p>
        )}
        {denied && <Alert>{t('prefs.notifyDenied')}</Alert>}
        <p className="hint">{t('prefs.installHint')}</p>
      </div>
    </section>
  )
}
