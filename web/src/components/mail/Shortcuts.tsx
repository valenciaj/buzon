import { useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { Icon } from './Icons'

/** Las teclas, para la ayuda. «g i» son dos pulsaciones seguidas. */
const KEYS: [string, string][] = [
  ['j', 'next'], ['k', 'prev'], ['o', 'openThread'], ['u', 'close'],
  ['e', 'archive'], ['#', 'trash'], ['!', 'junk'], ['s', 'star'],
  ['⇧ I', 'read'], ['⇧ U', 'unread'], ['x', 'select'],
  ['r', 'reply'], ['a', 'replyAll'], ['f', 'forward'], ['c', 'compose'], ['/', 'search'],
  ['g i', 'goInbox'], ['g s', 'goSent'], ['g d', 'goDrafts'], ['g a', 'goArchive'], ['?', 'help'],
]

export function Shortcuts({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation()
  const close = useRef<HTMLButtonElement>(null)
  useEffect(() => close.current?.focus(), [])
  return (
    <div className="fixed inset-0 z-30 grid place-items-center bg-black/40 p-4" onClick={onClose}>
      <div role="dialog" aria-modal="true" data-shortcuts aria-labelledby="shortcuts-title"
        className="card flex max-h-[85vh] w-full max-w-lg flex-col overflow-hidden" onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => { if (e.key === 'Escape') onClose() }}>
        <div className="flex items-center gap-2 border-b border-line px-4 py-3">
          <h2 id="shortcuts-title" className="flex-1 text-lg font-bold">{t('shortcuts.title')}</h2>
          <button ref={close} className="btn btn-ghost px-2" onClick={onClose} aria-label={t('common.cancel')}>
            <Icon name="close" />
          </button>
        </div>
        <dl className="grid grid-cols-1 gap-x-6 gap-y-2 overflow-y-auto p-4 sm:grid-cols-2">
          {KEYS.map(([k, what]) => (
            <div key={k} className="flex items-center justify-between gap-3">
              <dt className="text-muted">{t(`shortcuts.${what}`)}</dt>
              <dd className="flex shrink-0 gap-1">
                {k.split(' ').map((p, i) => (
                  <kbd key={i} className="min-w-7 rounded-md border border-line bg-surface-2 px-1.5 py-0.5 text-center font-mono text-sm">{p}</kbd>
                ))}
              </dd>
            </div>
          ))}
        </dl>
      </div>
    </div>
  )
}
