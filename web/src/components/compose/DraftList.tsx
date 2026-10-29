import { useTranslation } from 'react-i18next'
import type { Draft } from '../../lib/api'
import { fromDraft } from '../../lib/compose'
import { shortDate } from '../../lib/mail'
import { Icon } from '../mail/Icons'
import { useCompose } from './ComposerProvider'

/** Borradores guardados en Buzón, encima de los de la carpeta del servidor. */
export function DraftList({ drafts }: { drafts: Draft[] }) {
  const { t, i18n } = useTranslation()
  const compose = useCompose()
  return (
    <div className="border-b border-line">
      <p className="eyebrow px-[var(--pad)] pb-1 pt-4">{t('compose.localDrafts')}</p>
      {drafts.map((d) => {
        const to = [...d.to, ...d.cc, ...d.bcc].map((a) => a.name || a.email)
        return (
          <button key={d.id} role="listitem" onClick={() => compose(fromDraft(d))}
            className="grid w-full grid-cols-[2.5rem_minmax(0,1fr)_auto] items-start gap-3 px-[var(--pad)] py-3 text-left hover:bg-surface-2">
            <span className="grid size-10 place-items-center rounded-full bg-surface-2 text-muted" aria-hidden="true">
              <Icon name="drafts" />
            </span>
            <span className="flex min-w-0 flex-col gap-0.5">
              <span className="truncate font-semibold">
                <span className="text-crit">{t('compose.draft')}</span>
                {to.length > 0 && <span className="font-normal"> · {to.join(', ')}</span>}
              </span>
              <span className="truncate">{d.subject || t('mail.noSubject')}</span>
              {d.send_error
                ? <span className="line-clamp-2 text-sm text-crit">{t('compose.sendFailed', { error: d.send_error })}</span>
                : <span className="line-clamp-2 text-sm text-muted">{d.text.trim().split('\n')[0] || '…'}</span>}
            </span>
            <span className="flex flex-col items-end gap-1 text-sm text-muted tabular-nums">
              <span className="whitespace-nowrap">{shortDate(d.updated_at, i18n.language)}</span>
              {d.attachments.length > 0 && <Icon name="clip" className="size-4" />}
            </span>
          </button>
        )
      })}
    </div>
  )
}
