import { useTranslation } from 'react-i18next'
import type { Preferences } from '../lib/api'
import { FONTS, FONT_SIZES } from '../lib/look'
import { Segmented } from './ui'

/** Tema, modo, vista, fuente, tamaño y densidad. Se usa en el asistente y en Ajustes. */
export function LookPicker({ value, onChange, showView = true }: {
  value: Preferences
  onChange: (p: Partial<Preferences>) => void
  showView?: boolean
}) {
  const { t } = useTranslation()
  const sizeIdx = Math.max(0, FONT_SIZES.indexOf(value.font_size))
  return (
    <div className="flex flex-col gap-6">
      <Segmented label={t('look.theme')} value={value.theme} onChange={(theme) => onChange({ theme })}
        options={(['system', 'light', 'dark'] as const).map((v) => ({ value: v, label: t(`look.themes.${v}`) }))} />
      <Segmented label={t('look.mode')} value={value.mode} onChange={(mode) => onChange({ mode })} hint={t('look.modeHint')}
        options={(['focus', 'classic'] as const).map((v) => ({ value: v, label: t(`look.modes.${v}`) }))} />
      {showView && (
        <Segmented label={t('look.view')} value={value.view} onChange={(view) => onChange({ view })}
          options={(['unified', 'per_account'] as const).map((v) => ({ value: v, label: t(`look.views.${v}`) }))} />
      )}
      <div className="flex flex-col gap-1.5">
        <label htmlFor="font" className="label">{t('look.font')}</label>
        <select id="font" className="input" value={value.font}
          onChange={(e) => onChange({ font: e.target.value as Preferences['font'] })}>
          {Object.entries(FONTS).map(([k, f]) => (
            <option key={k} value={k}>{f.label}{k === 'atkinson' ? ` · ${t('look.fontDefault')}` : ''}</option>
          ))}
        </select>
      </div>
      <div className="flex flex-col gap-1.5">
        <label htmlFor="font-size" className="label">{t('look.fontSize')}</label>
        <div className="flex items-center gap-3">
          <input id="font-size" type="range" min={0} max={FONT_SIZES.length - 1} step={1} value={sizeIdx}
            className="flex-1 accent-[var(--accent)]"
            onChange={(e) => onChange({ font_size: FONT_SIZES[Number(e.target.value)] })} />
          <output htmlFor="font-size" className="w-16 text-right font-semibold tabular-nums">
            {value.font_size.toLocaleString(undefined, { maximumFractionDigits: 1 })} %
          </output>
        </div>
        <p className="rounded-lg border border-line bg-bg px-3.5 py-2.5">{t('look.preview')}</p>
      </div>
      <Segmented label={t('look.density')} value={value.density} onChange={(density) => onChange({ density })}
        hint={t('look.densityHint')}
        options={(['comfortable', 'compact'] as const).map((v) => ({ value: v, label: t(`look.densities.${v}`) }))} />
    </div>
  )
}
