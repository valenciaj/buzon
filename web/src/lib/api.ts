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
  /** Segundos para deshacer un envío: 0, 5, 10, 20 o 30. */
  undo_send: number
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

export type OAuthProvider = 'google' | 'microsoft'

export interface Account extends Omit<AccountInput, 'password' | 'verify'> {
  id: string
  /** Cómo se entra: con contraseña o con OAuth2 (Gmail, Outlook). */
  auth: 'password' | OAuthProvider
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
  labels: string[]
  reply_later: boolean
  aside: boolean
  /** Pospuesto hasta esta fecha (segundos). Si ya pasó, ha reaparecido. */
  snooze_until: number | null
  has_note: boolean
  /** Traía píxeles espía (ya quitados). */
  tracked: boolean
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
  /** Dominios de los píxeles espía quitados. */
  trackers: string[]
  attachments: AttachmentInfo[]
}

export interface Thread {
  id: string
  account_id: string
  subject: string
  labels: string[]
  messages: Message[]
  note: string
  reply_later: boolean
  aside: boolean
  snooze_until: number | null
  sender: Sender | null
}

/** Bandejas y pilas del modo Enfoque. */
export type View = 'principal' | 'news' | 'receipts' | 'screener' | 'blocked' | 'reply_later' | 'aside' | 'snoozed' | 'bubbled'
export type Bucket = 'principal' | 'news' | 'receipts'
export type SenderStatus = 'pending' | 'approved' | 'blocked'

export interface Sender {
  email: string
  name: string
  status: SenderStatus
  bucket: Bucket
}

export interface SenderSummary extends Sender {
  threads: number
  last_thread: string | null
  last_subject: string
  last_date: number
}

export interface FlowCounts {
  screener: number
  reply_later: number
  aside: number
  snoozed: number
  bubbled: number
  principal_unread: number
  news_unread: number
  receipts_unread: number
}

/** Cambios en las pilas. `snooze_until: 0` quita lo pospuesto. */
export interface FlowChange {
  reply_later?: boolean
  aside?: boolean
  snooze_until?: number
}

export interface ThreadFilter {
  account?: string
  mailbox?: string
  role?: Role
  label?: string
  /** Búsqueda (texto libre y `de:`, `tiene:adjunto`…). */
  q?: string
  view?: View
}

export interface InvitationOut {
  id: string
  note: string | null
  created_at: number
  expires_at: number
  status: 'pending' | 'used' | 'expired'
}

export interface Identity {
  id: string
  account_id: string
  name: string
  email: string
  signature: string
  is_default: boolean
}

export interface IdentityInput {
  account_id?: string
  name: string
  email: string
  signature: string
  is_default: boolean
}

export interface Upload {
  id: string
  name: string
  content_type: string
  size: number
}

export interface DraftInput {
  identity_id: string | null
  to: Addr[]
  cc: Addr[]
  bcc: Addr[]
  subject: string
  html: string
  text: string
  reply_to: string | null
  forward_of: string | null
  /** Ids de ficheros subidos. */
  attachments: string[]
}

export interface Draft extends Omit<DraftInput, 'attachments'> {
  id: string
  attachments: Upload[]
  updated_at: number
  /** Cuándo sale, si está programado (negativo: está saliendo). */
  send_at: number | null
  send_error: string | null
  follow_up_days: number | null
}

export interface SendOptions {
  draft_id?: string | null
  /** Segundos Unix: sale entonces (y hasta entonces se puede cancelar). */
  send_at?: number
  /** Avisar si nadie responde en estos días. */
  follow_up_days?: number
}

export interface FollowUp {
  id: string
  account_id: string
  subject: string
  to: Addr[]
  sent_at: number
  due_at: number
  thread: string | null
}

export interface Label {
  id: string
  name: string
  color: string
  threads: number
  unread: number
}

export type MoveRole = 'inbox' | 'archive' | 'trash' | 'junk'

export interface RuleInput {
  name: string
  enabled: boolean
  account_id: string | null
  from_has: string
  to_has: string
  subject_has: string
  body_has: string
  label_id: string | null
  mark_read: boolean
  star: boolean
  move_to: '' | 'archive' | 'trash' | 'junk'
}

export interface Rule extends RuleInput {
  id: string
}

export interface ContactInput {
  name: string
  email: string
  phone: string
  notes: string
}

export interface Contact extends ContactInput {
  id: string
  source: 'manual' | 'sent' | 'vcard'
}

/** Dónde está lo que se mueve: la carpeta que se ve (si se ve una). */
export interface From {
  from_role?: Role
  from_mailbox?: string
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

async function call<T>(method: string, path: string, body?: unknown, keepalive = false): Promise<T> {
  const res = await fetch(`/api/v1${path}`, {
    method,
    credentials: 'same-origin',
    // Para que una acción pendiente llegue al servidor aunque se cierre la pestaña.
    keepalive,
    headers: method === 'GET' ? {} : { 'Content-Type': 'application/json' },
    body: method === 'GET' ? undefined : JSON.stringify(body ?? {}),
  })
  if (res.status === 204) return undefined as T
  const data = await res.json().catch(() => null)
  if (!res.ok) throw new ApiError(res.status, data?.error ?? 'unknown', data?.message ?? res.statusText)
  return data as T
}

/** Sube un fichero tal cual; el nombre va en una cabecera propia. */
async function uploadFile(file: File): Promise<Upload> {
  const res = await fetch('/api/v1/uploads', {
    method: 'POST',
    credentials: 'same-origin',
    headers: {
      'Content-Type': file.type || 'application/octet-stream',
      'X-Buzon-Filename': encodeURIComponent(file.name),
    },
    body: file,
  })
  const data = await res.json().catch(() => null)
  if (!res.ok) throw new ApiError(res.status, data?.error ?? 'unknown', data?.message ?? res.statusText)
  return data as Upload
}

export const SCOPES = ['mail:read', 'mail:write', 'mail:send', 'contacts', 'webhooks'] as const
export type Scope = (typeof SCOPES)[number]

export const HOOK_EVENTS = [
  'message.received', 'message.sent', 'message.read', 'message.moved',
  'thread.labeled', 'contact.screened', 'account.sync_failed',
] as const
export type HookEvent = (typeof HOOK_EVENTS)[number]

export interface ApiToken {
  id: string
  name: string
  prefix: string
  scopes: Scope[]
  created_at: number
  expires_at: number | null
  last_used_at: number | null
}

export interface Webhook {
  id: string
  url: string
  description: string
  events: HookEvent[]
  active: boolean
  created_at: number
  last_status: 'ok' | 'failed' | null
}

export interface WebhookInput {
  url: string
  description: string
  events: HookEvent[]
}

export interface Delivery {
  id: string
  event: HookEvent | 'ping'
  status: 'pending' | 'ok' | 'failed'
  attempts: number
  next_at: number | null
  response: number | null
  error: string | null
  created_at: number
  finished_at: number | null
  payload: string
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
  oauthProviders: () => call<{ providers: OAuthProvider[] }>('GET', '/oauth/providers'),
  oauthStart: (provider: OAuthProvider, login_hint?: string) =>
    call<{ url: string }>('POST', `/oauth/${provider}/start`, { login_hint }),
  syncAccount: (id: string) => call<void>('POST', `/accounts/${id}/sync`),
  mailboxes: (account?: string) => call<Mailbox[]>('GET', `/mailboxes${query({ account })}`),
  threads: (f: ThreadFilter, cursor?: string) => call<ThreadPage>('GET', `/threads${query({ ...f, cursor })}`),
  thread: (id: string) => call<Thread>('GET', `/threads/${id}`),
  updateThread: (id: string, change: { seen?: boolean; flagged?: boolean }) =>
    call<void>('PATCH', `/threads/${id}`, change),
  identities: () => call<Identity[]>('GET', '/identities'),
  createIdentity: (i: IdentityInput) => call<Identity>('POST', '/identities', i),
  updateIdentity: (id: string, i: IdentityInput) => call<Identity>('PUT', `/identities/${id}`, i),
  deleteIdentity: (id: string) => call<void>('DELETE', `/identities/${id}`),
  upload: uploadFile,
  deleteUpload: (id: string) => call<void>('DELETE', `/uploads/${id}`),
  drafts: () => call<Draft[]>('GET', '/drafts'),
  createDraft: (d: DraftInput) => call<Draft>('POST', '/drafts', d),
  updateDraft: (id: string, d: DraftInput) => call<Draft>('PUT', `/drafts/${id}`, d),
  deleteDraft: (id: string) => call<void>('DELETE', `/drafts/${id}`),
  send: (d: DraftInput & SendOptions) =>
    call<{ message_id: string | null; scheduled_id: string | null; send_at: number | null }>('POST', '/send', d),
  scheduled: () => call<Draft[]>('GET', '/scheduled'),
  cancelScheduled: (id: string) => call<Draft>('POST', `/scheduled/${id}/cancel`),
  followups: (due = false) => call<FollowUp[]>('GET', `/followups${due ? '?due=true' : ''}`),
  dismissFollowup: (id: string) => call<void>('DELETE', `/followups/${id}`),
  suggest: (q: string) => call<Addr[]>('GET', `/contacts/suggest${query({ q })}`),
  contacts: (q?: string) => call<Contact[]>('GET', `/contacts${query({ q })}`),
  createContact: (c: ContactInput) => call<Contact>('POST', '/contacts', c),
  updateContact: (id: string, c: ContactInput) => call<Contact>('PUT', `/contacts/${id}`, c),
  deleteContact: (id: string) => call<void>('DELETE', `/contacts/${id}`),
  importContacts: (vcard: string) => call<{ added: number; existing: number }>('POST', '/contacts/import', { vcard }),
  moveThreads: (threads: string[], to: { to_role?: MoveRole; to_mailbox?: string }, from: From = {}, keepalive = false) =>
    call<{ moved: number }>('POST', '/threads/move', { threads, ...to, ...from }, keepalive),
  deleteThreads: (threads: string[], from: From = {}, keepalive = false) =>
    call<{ deleted: number }>('POST', '/threads/delete', { threads, ...from }, keepalive),
  flagThreads: (threads: string[], change: { seen?: boolean; flagged?: boolean }) =>
    call<void>('POST', '/threads/flags', { threads, ...change }),
  labelThreads: (threads: string[], add: string[], remove: string[] = []) =>
    call<void>('POST', '/threads/labels', { threads, add, remove }),
  labels: () => call<Label[]>('GET', '/labels'),
  createLabel: (name: string, color: string) => call<Label>('POST', '/labels', { name, color }),
  updateLabel: (id: string, name: string, color: string) => call<Label>('PUT', `/labels/${id}`, { name, color }),
  deleteLabel: (id: string) => call<void>('DELETE', `/labels/${id}`),
  rules: () => call<Rule[]>('GET', '/rules'),
  createRule: (r: RuleInput) => call<Rule>('POST', '/rules', r),
  updateRule: (id: string, r: RuleInput) => call<Rule>('PUT', `/rules/${id}`, r),
  deleteRule: (id: string) => call<void>('DELETE', `/rules/${id}`),
  flowCounts: (account?: string) => call<FlowCounts>('GET', `/flow${query({ account })}`),
  senders: (status?: SenderStatus) => call<SenderSummary[]>('GET', `/senders${query({ status })}`),
  decideSender: (email: string, status: SenderStatus, bucket?: Bucket) =>
    call<Sender>('PUT', '/senders', { email, status, bucket }),
  setFlow: (threads: string[], change: FlowChange) => call<void>('POST', '/threads/flow', { threads, ...change }),
  setNote: (id: string, note: string) => call<void>('PUT', `/threads/${id}/note`, { note }),
  tokens: () => call<ApiToken[]>('GET', '/tokens'),
  createToken: (name: string, scopes: Scope[], expires_days?: number) =>
    call<ApiToken & { token: string }>('POST', '/tokens', { name, scopes, expires_days }),
  revokeToken: (id: string) => call<void>('DELETE', `/tokens/${id}`),
  webhooks: () => call<Webhook[]>('GET', '/webhooks'),
  createWebhook: (w: WebhookInput) => call<Webhook & { secret: string }>('POST', '/webhooks', w),
  updateWebhook: (id: string, w: Partial<WebhookInput> & { active?: boolean }) => call<Webhook>('PUT', `/webhooks/${id}`, w),
  deleteWebhook: (id: string) => call<void>('DELETE', `/webhooks/${id}`),
  testWebhook: (id: string) => call<void>('POST', `/webhooks/${id}/test`),
  deliveries: (id: string) => call<Delivery[]>('GET', `/webhooks/${id}/deliveries`),
  redeliver: (id: string) => call<Delivery>('POST', `/webhooks/deliveries/${id}/redeliver`),
  invitations: () => call<InvitationOut[]>('GET', '/invitations'),
  createInvitation: (note?: string) =>
    call<{ id: string; url: string; expires_at: number }>('POST', '/invitations', { note }),
  revokeInvitation: (id: string) => call<void>('DELETE', `/invitations/${id}`),
  checkInvite: (token: string) => call<{ valid: boolean; expires_at: number | null }>('GET', `/invite/${token}`),
  acceptInvite: (token: string, body: { username: string; password: string; email?: string; language?: Language }) =>
    call<Me>('POST', `/invite/${token}`, body),
}
