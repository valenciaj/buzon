import type { CSSProperties } from 'react'
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
  const setSize = (i: number) => onChange({ font_size: FONT_SIZES[Math.min(FONT_SIZES.length - 1, Math.max(0, i))] })
  const pct = (n: number) => n.toLocaleString(undefined, { maximumFractionDigits: 1 })
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
        <div className="flex items-start gap-3">
          {/* --thumb: ancho del tirador del control, para alinear las guías con él. */}
          <div className="flex-1" style={{ '--thumb': '16px' } as CSSProperties}>
            <input id="font-size" type="range" min={0} max={FONT_SIZES.length - 1} step={1} value={sizeIdx}
              list="font-size-marks" className="mt-2.5 w-full accent-[var(--accent)]"
              onChange={(e) => setSize(Number(e.target.value))} />
            <datalist id="font-size-marks">
              {FONT_SIZES.map((_, i) => <option key={i} value={i} />)}
            </datalist>
            {/* Guías con cada tamaño, bajo la posición del control. */}
            <div className="relative mt-1 h-5 text-xs text-muted" aria-hidden="true">
              {FONT_SIZES.map((s, i) => (
                <button key={s} type="button" tabIndex={-1} onClick={() => setSize(i)}
                  className={`absolute -translate-x-1/2 tabular-nums ${i === sizeIdx ? 'font-semibold text-fg' : ''}`}
                  style={{ left: `calc(var(--thumb) / 2 + (100% - var(--thumb)) * ${i / (FONT_SIZES.length - 1)})` }}>
                  {pct(s)}
                </button>
              ))}
            </div>
          </div>
          <div className="flex shrink-0 items-center gap-1">
            <button type="button" className="btn btn-ghost size-10 px-0 text-lg" aria-label={t('look.smaller')} title={t('look.smaller')}
              disabled={sizeIdx === 0} onClick={() => setSize(sizeIdx - 1)}>−</button>
            <output htmlFor="font-size" className="w-16 text-center font-semibold tabular-nums">
              {pct(value.font_size)} %
            </output>
            <button type="button" className="btn btn-ghost size-10 px-0 text-lg" aria-label={t('look.larger')} title={t('look.larger')}
              disabled={sizeIdx === FONT_SIZES.length - 1} onClick={() => setSize(sizeIdx + 1)}>+</button>
          </div>
        </div>
        <p className="rounded-lg border border-line bg-bg px-3.5 py-2.5">{t('look.preview')}</p>
      </div>
      <Segmented label={t('look.density')} value={value.density} onChange={(density) => onChange({ density })}
        hint={t('look.densityHint')}
        options={(['comfortable', 'compact'] as const).map((v) => ({ value: v, label: t(`look.densities.${v}`) }))} />
    </div>
  )
}
