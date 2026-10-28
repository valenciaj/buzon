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

export type Security = 'tls' | 'starttls' | 'none'

export interface AccountInput {
  name: string
  email: string
  imap_host: string
  imap_port: number
  imap_security: Security
  smtp_host: string
  smtp_port: number
  smtp_security: Security
  login: string
  password: string
  /** Comprobar usuario y contraseña con el servidor antes de guardar (por defecto, sí). */
  verify?: boolean
}

export type SyncState = 'off' | 'connecting' | 'syncing' | 'idle' | 'error'

export interface Account extends Omit<AccountInput, 'password' | 'verify'> {
  id: string
  state: SyncState
  synced_at: number | null
  sync_error: string | null
}

export type Role = 'inbox' | 'sent' | 'drafts' | 'archive' | 'junk' | 'trash'

export interface Mailbox {
  id: string
  account_id: string
  name: string
  delimiter: string | null
  role: Role | null
  total: number
  unread: number
}

export interface Addr {
  name: string | null
  email: string
}

export interface ThreadSummary {
  id: string
  account_id: string
  subject: string
  last_date: number
  participants: string[]
  snippet: string
  messages: number
  unread: number
  flagged: boolean
  has_attachments: boolean
}

export interface ThreadPage {
  threads: ThreadSummary[]
  next_cursor: string | null
}

export interface AttachmentInfo {
  index: number
  name: string
  content_type: string
  size: number
  inline: boolean
  url: string
}

export interface Message {
  id: string
  mailbox_id: string
  subject: string
  from: Addr | null
  to: Addr[]
  cc: Addr[]
  date: number
  seen: boolean
  flagged: boolean
  answered: boolean
  text: string | null
  html: string | null
  remote_images: number
  attachments: AttachmentInfo[]
}

export interface Thread {
  id: string
  account_id: string
  subject: string
  messages: Message[]
}

export interface ThreadFilter {
  account?: string
  mailbox?: string
  role?: Role
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

function query(params: Record<string, string | undefined>): string {
  const q = new URLSearchParams()
  for (const [k, v] of Object.entries(params)) if (v) q.set(k, v)
  const s = q.toString()
  return s ? `?${s}` : ''
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
  deleteAccount: (id: string) => call<void>('DELETE', `/accounts/${id}`),
  syncAccount: (id: string) => call<void>('POST', `/accounts/${id}/sync`),
  mailboxes: (account?: string) => call<Mailbox[]>('GET', `/mailboxes${query({ account })}`),
  threads: (f: ThreadFilter, cursor?: string) => call<ThreadPage>('GET', `/threads${query({ ...f, cursor })}`),
  thread: (id: string) => call<Thread>('GET', `/threads/${id}`),
  updateThread: (id: string, change: { seen?: boolean; flagged?: boolean }) =>
    call<void>('PATCH', `/threads/${id}`, change),
  invitations: () => call<InvitationOut[]>('GET', '/invitations'),
  createInvitation: (note?: string) =>
    call<{ id: string; url: string; expires_at: number }>('POST', '/invitations', { note }),
  revokeInvitation: (id: string) => call<void>('DELETE', `/invitations/${id}`),
  checkInvite: (token: string) => call<{ valid: boolean; expires_at: number | null }>('GET', `/invite/${token}`),
  acceptInvite: (token: string, body: { username: string; password: string; email?: string; language?: Language }) =>
    call<Me>('POST', `/invite/${token}`, body),
}
