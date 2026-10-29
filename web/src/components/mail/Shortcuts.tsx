import { useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { Icon } from './Icons'
import { manualUrl } from '../../lib/manual'

/** Las teclas, en el orden del manual. Cada entrada: alternativas; «g i» son dos pulsaciones seguidas. */
const GROUPS: { title: string; keys: [string[], string][] }[] = [
  {
    title: 'move',
    keys: [
      [['j', '↓'], 'next'], [['k', '↑'], 'prev'], [['o', 'Enter'], 'openThread'], [['u', 'Esc'], 'close'],
      [['/'], 'search'], [['Ctrl K'], 'palette'],
      [['g i'], 'goInbox'], [['g t'], 'goToday'], [['g s'], 'goSent'], [['g d'], 'goDrafts'], [['g a'], 'goArchive'],
    ],
  },
  {
    title: 'act',
    keys: [
      [['c'], 'compose'], [['r'], 'reply'], [['a'], 'replyAll'], [['f'], 'forward'],
      [['e'], 'archive'], [['#'], 'trash'], [['!'], 'junk'], [['s'], 'star'],
      [['⇧ I'], 'read'], [['⇧ U'], 'unread'], [['x'], 'select'],
      [['l'], 'replyLater'], [['p'], 'aside'], [['b'], 'snooze'], [['?'], 'help'],
    ],
  },
]

function Keys({ seq }: { seq: string }) {
  const { t } = useTranslation()
  const parts = seq.split(' ')
  // «g i» es una secuencia; «Ctrl K» y «⇧ I», una combinación.
  const sequence = parts[0] === 'g'
  return parts.map((p, i) => (
    <span key={i} className="flex items-center gap-1">
      {i > 0 && sequence && <span className="text-sm text-muted">{t('shortcuts.then')}</span>}
      <kbd className="min-w-7 rounded-md border border-line bg-surface-2 px-1.5 py-0.5 text-center font-mono text-sm">{p}</kbd>
    </span>
  ))
}

export function Shortcuts({ onClose }: { onClose: () => void }) {
  const { t, i18n } = useTranslation()
  const dialog = useRef<HTMLDialogElement>(null)
  useEffect(() => {
    const d = dialog.current
    if (d && !d.open) d.showModal()
  }, [])
  return (
    <dialog ref={dialog} data-shortcuts aria-labelledby="shortcuts-title"
      className="card m-auto max-h-[85vh] w-[calc(100%-2rem)] max-w-lg overflow-hidden p-0 text-fg backdrop:bg-black/40"
      onCancel={(e) => { e.preventDefault(); onClose() }}
      onClick={(e) => { if (e.target === e.currentTarget) onClose() }}>
      <div className="flex max-h-[85vh] flex-col">
        <div className="flex items-center gap-2 border-b border-line px-4 py-3">
          <h2 id="shortcuts-title" className="flex-1 text-lg font-bold">{t('shortcuts.title')}</h2>
          <button autoFocus className="btn btn-ghost px-2" onClick={onClose} aria-label={t('common.cancel')}>
            <Icon name="close" />
          </button>
        </div>
        <div className="flex flex-col gap-5 overflow-y-auto p-4">
          {GROUPS.map((g) => (
            <section key={g.title}>
              <h3 className="eyebrow mb-2">{t(`shortcuts.groups.${g.title}`)}</h3>
              <dl className="flex flex-col gap-2">
                {g.keys.map(([alts, what]) => (
                  <div key={what} className="flex items-center justify-between gap-3">
                    <dt className="text-muted">{t(`shortcuts.${what}`)}</dt>
                    <dd className="flex shrink-0 flex-wrap items-center justify-end gap-1">
                      {alts.map((k, i) => (
                        <span key={k} className="flex items-center gap-1">
                          {i > 0 && <span className="text-sm text-muted">{t('shortcuts.or')}</span>}
                          <Keys seq={k} />
                        </span>
                      ))}
                    </dd>
                  </div>
                ))}
              </dl>
            </section>
          ))}
        </div>
        <a href={manualUrl(i18n.language, 'shortcuts')} target="_blank" rel="noopener"
          className="border-t border-line px-4 py-3 text-sm text-accent hover:underline">{t('home.manualMore')}</a>
      </div>
    </dialog>
  )
}
