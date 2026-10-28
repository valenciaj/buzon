import { useId, type InputHTMLAttributes, type ReactNode } from 'react'

export function Logo({ size = 22 }: { size?: number }) {
  return (
    <span className="flex items-center gap-2 text-lg font-bold tracking-tight">
      <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8"
        strokeLinecap="round" strokeLinejoin="round" className="text-accent" aria-hidden="true">
        <rect x="3" y="5" width="18" height="14" rx="2.5" />
        <path d="M3.5 7l8.5 6 8.5-6" />
      </svg>
      Buzón
    </span>
  )
}

type FieldProps = InputHTMLAttributes<HTMLInputElement> & { label: string; hint?: ReactNode }

export function Field({ label, hint, id, ...rest }: FieldProps) {
  const auto = useId()
  const fid = id ?? auto
  return (
    <div className="flex flex-col gap-1.5">
      <label htmlFor={fid} className="label">{label}</label>
      <input id={fid} className="input" {...rest} />
      {hint && <p className="hint">{hint}</p>}
    </div>
  )
}

export function Segmented<T extends string>({ label, value, options, onChange, hint }: {
  label: string
  value: T
  options: { value: T; label: string }[]
  onChange: (v: T) => void
  hint?: string
}) {
  return (
    <fieldset className="flex flex-col gap-1.5">
      <legend className="label mb-1.5">{label}</legend>
      <div className="flex flex-wrap gap-1.5">
        {options.map((o) => (
          <button key={o.value} type="button" aria-pressed={value === o.value} onClick={() => onChange(o.value)}
            className={`min-h-10 rounded-lg border px-3.5 py-1.5 ${value === o.value
              ? 'border-accent bg-[color-mix(in_srgb,var(--accent)_12%,var(--surface))] font-semibold'
              : 'border-line bg-bg'}`}>
            {o.label}
          </button>
        ))}
      </div>
      {hint && <p className="hint">{hint}</p>}
    </fieldset>
  )
}

export function Alert({ kind = 'error', children }: { kind?: 'error' | 'ok' | 'info'; children: ReactNode }) {
  const tone = {
    error: 'border-[color-mix(in_srgb,var(--crit)_45%,var(--line))] text-crit',
    ok: 'border-[color-mix(in_srgb,var(--good)_45%,var(--line))] text-good',
    info: 'border-line text-muted',
  }[kind]
  return (
    <div role={kind === 'error' ? 'alert' : 'status'} className={`rounded-lg border bg-surface px-3.5 py-2.5 text-sm ${tone}`}>
      {children}
    </div>
  )
}

/** Página centrada para asistente, login e invitación. */
export function Narrow({ children }: { children: ReactNode }) {
  return (
    <main className="mx-auto flex min-h-full w-full max-w-xl flex-col gap-6 px-4 py-8 sm:py-14">
      <Logo />
      {children}
    </main>
  )
}
