// Types mirror the JSON of server/src (db.rs, channels/mod.rs, api/*.rs).

export interface User {
  id: number
  username: string
  has_password: boolean
  is_admin: boolean
  active: boolean
  send_token: string
  created_at: string
}

export type ConfigValue = string | boolean

export interface Channel {
  id: number
  user_id: number
  kind: string
  name: string
  key: string
  config: Record<string, ConfigValue>
  active: boolean
  created_at: string
  updated_at: string
}

export interface Field {
  name: string
  label: string
  kind: 'text' | 'secret' | 'textarea' | 'bool' | 'select'
  required: boolean
  default: ConfigValue | null
  options?: string[]
  readonly: boolean
  help: string
}

export interface KindInfo {
  kind: string
  name: string
  fields: Field[]
  user_creatable: boolean
}

export interface Settings {
  proxy: string
  turnstile_enabled: boolean
  turnstile_site_key: string
  turnstile_secret_key: string
  turnstile_secret_key_set: boolean
}

export interface Stats {
  days: { date: string; success: number; failed: number }[]
  kinds: { kind: string; name: string; success: number; failed: number }[]
  users: number
  channels: number
}

const TOKEN_KEY = 'inotify_session'

export function getSession(): string | null {
  try {
    return localStorage.getItem(TOKEN_KEY)
  } catch {
    return null
  }
}

export function setSession(token: string | null) {
  try {
    if (token) localStorage.setItem(TOKEN_KEY, token)
    else localStorage.removeItem(TOKEN_KEY)
  } catch {
    // Storage blocked: the session lasts until reload.
  }
}

export class ApiError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message)
  }
}

let onUnauthorized: () => void = () => {}
export function setUnauthorizedHandler(handler: () => void) {
  onUnauthorized = handler
}

async function request<T>(method: string, path: string, body?: unknown, bearer = getSession()): Promise<T> {
  const headers: Record<string, string> = {}
  if (bearer) headers.Authorization = `Bearer ${bearer}`
  if (body !== undefined) headers['Content-Type'] = 'application/json'
  const resp = await fetch(path, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) })
  const data = await resp.json().catch(() => ({}))
  if (!resp.ok) {
    if (resp.status === 401 && bearer === getSession()) onUnauthorized()
    throw new ApiError(resp.status, data.error ?? `HTTP ${resp.status}`)
  }
  return data as T
}

export const api = {
  login: (username: string, password: string, turnstile_token?: string) =>
    request<{ token: string; user: User }>('POST', '/api/v1/login', { username, password, turnstile_token }),
  authOptions: () => request<{ turnstile_site_key: string | null }>('GET', '/api/v1/auth-options'),
  me: () => request<User>('GET', '/api/v1/me'),
  /** Ends every other session; this one continues with the returned token. */
  changePassword: (old_password: string, new_password: string) =>
    request<{ token: string }>('PUT', '/api/v1/me/password', { old_password, new_password }),
  resetSendToken: () => request<User>('POST', '/api/v1/me/send-token'),

  kinds: () => request<KindInfo[]>('GET', '/api/v1/channel-kinds'),
  channels: () => request<Channel[]>('GET', '/api/v1/channels'),
  createChannel: (c: { kind: string; name: string; config: Channel['config']; active: boolean }) =>
    request<Channel>('POST', '/api/v1/channels', c),
  updateChannel: (id: number, c: Partial<Pick<Channel, 'name' | 'config' | 'active'>>) =>
    request<Channel>('PUT', `/api/v1/channels/${id}`, c),
  deleteChannel: (id: number) => request('DELETE', `/api/v1/channels/${id}`),
  testChannel: (id: number) => request<{ ok: boolean; error?: string }>('POST', `/api/v1/channels/${id}/test`),
  /** The encrypted backup is opaque to the UI: it is saved and uploaded as-is. */
  exportChannels: (password: string) => request<object>('POST', '/api/v1/channels/export', { password }),
  importChannels: (password: string, backup: unknown) =>
    request<{ imported: number; skipped: number; rekeyed: number }>('POST', '/api/v1/channels/import', {
      password,
      backup,
    }),

  /** Sends with a send token or channel key, not the session. */
  send: (token: string, msg: { title?: string; body?: string }) =>
    request<{ queued: boolean }>('POST', '/api/v1/send', msg, token),

  users: () => request<User[]>('GET', '/api/v1/users'),
  createUser: (u: { username: string; password: string; is_admin: boolean }) =>
    request<User>('POST', '/api/v1/users', u),
  updateUser: (id: number, u: { active?: boolean; is_admin?: boolean; password?: string }) =>
    request<User>('PUT', `/api/v1/users/${id}`, u),
  deleteUser: (id: number) => request('DELETE', `/api/v1/users/${id}`),

  settings: () => request<Settings>('GET', '/api/v1/settings'),
  saveSettings: (s: Partial<Settings>) => request<Settings>('PUT', '/api/v1/settings', s),
  stats: (days: number) => request<Stats>('GET', `/api/v1/stats?days=${days}`),
}
