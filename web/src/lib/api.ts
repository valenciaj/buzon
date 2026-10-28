// Cliente mínimo de la API. Usa la cookie de sesión del navegador.

export type Theme = 'system' | 'light' | 'dark'
export type Mode = 'focus' | 'classic'
export type AccountView = 'unified' | 'per_account'
export type Font = 'atkinson' | 'inter' | 'source-sans' | 'plex' | 'lexend' | 'system'
export type Density = 'comfortable' | 'compact'
export type Language = 'es' | 'en'

export interface Preferences {
  theme: Theme
  mode: Mode
  view: AccountView
  font: Font
  font_size: number
  density: Density
}

export interface Me {
  id: string
  username: string
  email: string | null
  is_admin: boolean
  totp_enabled: boolean
  language: Language
  preferences: Preferences
}

export interface AccountInput {
  name: string
  email: string
  imap_host: string
  imap_port: number
  smtp_host: string
  smtp_port: number
  login: string
  password: string
}

export interface Account extends Omit<AccountInput, 'password'> {
  id: string
}

export interface InvitationOut {
  id: string
  note: string | null
  created_at: number
  expires_at: number
  status: 'pending' | 'used' | 'expired'
}

export class ApiError extends Error {
  constructor(
    public status: number,
    public code: string,
    message: string,
  ) {
    super(message)
  }
}

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(`/api/v1${path}`, {
    method,
    credentials: 'same-origin',
    headers: method === 'GET' ? {} : { 'Content-Type': 'application/json' },
    body: method === 'GET' ? undefined : JSON.stringify(body ?? {}),
  })
  if (res.status === 204) return undefined as T
  const data = await res.json().catch(() => null)
  if (!res.ok) throw new ApiError(res.status, data?.error ?? 'unknown', data?.message ?? res.statusText)
  return data as T
}

export const api = {
  setupStatus: () => call<{ needs_setup: boolean }>('GET', '/setup'),
  setup: (body: unknown) => call<Me>('POST', '/setup', body),
  login: (body: { username: string; password: string; totp?: string }) => call<Me>('POST', '/auth/login', body),
  logout: () => call<void>('POST', '/auth/logout'),
  me: () => call<Me>('GET', '/me'),
  updatePreferences: (body: Partial<Preferences> & { language?: Language }) =>
    call<Me>('PUT', '/me/preferences', body),
  totpSetup: () => call<{ secret: string; otpauth_url: string }>('POST', '/me/totp/setup'),
  totpEnable: (code: string) => call<Me>('POST', '/me/totp/enable', { code }),
  totpDisable: (password: string) => call<Me>('POST', '/me/totp/disable', { password }),
  accounts: () => call<Account[]>('GET', '/accounts'),
  addAccount: (a: AccountInput) => call<Account>('POST', '/accounts', a),
  invitations: () => call<InvitationOut[]>('GET', '/invitations'),
  createInvitation: (note?: string) =>
    call<{ id: string; url: string; expires_at: number }>('POST', '/invitations', { note }),
  revokeInvitation: (id: string) => call<void>('DELETE', `/invitations/${id}`),
  checkInvite: (token: string) => call<{ valid: boolean; expires_at: number | null }>('GET', `/invite/${token}`),
  acceptInvite: (token: string, body: { username: string; password: string; email?: string; language?: Language }) =>
    call<Me>('POST', `/invite/${token}`, body),
}
