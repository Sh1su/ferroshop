import type { components } from './schema'

export type Schemas = components['schemas']
export type Product = Schemas['Product']
export type Category = Schemas['Category']
export type Tax = Schemas['Tax']
export type ProductManufacturer = Schemas['ProductManufacturer']
export type Language = Schemas['Language']
export type Currency = Schemas['Currency']
export type User = Schemas['User']
export type Price = Schemas['Price']
export type Criteria = Schemas['Criteria']
export type Customer = Schemas['Customer']
export type CustomerAddress = Schemas['CustomerAddress']
export type Order = Schemas['Order']
export type OrderLineItem = Schemas['OrderLineItem']
export type OrderAddress = Schemas['OrderAddress']
export type Media = Schemas['Media']
export type ProductMedia = Schemas['ProductMedia']
export type AclRole = Schemas['AclRole']
export type PropertyGroupOption = Schemas['PropertyGroupOption']

export interface SearchResult<T> {
  total: number
  data: T[]
  aggregations: Record<string, unknown>
}

export interface ApiErrorEntry {
  status: string
  code: string
  title: string
  detail: string
  source?: { pointer?: string }
}

export class ApiError extends Error {
  status: number
  errors: ApiErrorEntry[]

  constructor(status: number, errors: ApiErrorEntry[]) {
    super(errors[0]?.detail ?? `HTTP ${status}`)
    this.status = status
    this.errors = errors
  }

  /** Errors keyed by JSON pointer (`/name` -> error), for field-level display. */
  byPointer(): Record<string, { code: string; detail: string }> {
    return Object.fromEntries(
      this.errors.filter((e) => e.source?.pointer).map((e) => [e.source!.pointer!, { code: e.code, detail: e.detail }]),
    )
  }
}

const TOKEN_KEY = 'ferroshop.auth'

interface Tokens {
  access: string
  refresh: string
  expiresAt: number
}

function loadTokens(): Tokens | null {
  try {
    return JSON.parse(localStorage.getItem(TOKEN_KEY) ?? 'null')
  } catch {
    return null
  }
}

let tokens: Tokens | null = loadTokens()
let refreshing: Promise<boolean> | null = null
let onLogout: () => void = () => {}

/** Content language sent as `sw-language-id`; set by the language switcher. */
export const contentLanguage = { id: '2fbb5fe2e29a4d70aa5854ce7ce3e20b' }

function storeTokens(body: { access_token: string; refresh_token: string; expires_in: number }) {
  tokens = { access: body.access_token, refresh: body.refresh_token, expiresAt: Date.now() + body.expires_in * 1000 }
  localStorage.setItem(TOKEN_KEY, JSON.stringify(tokens))
}

export function isLoggedIn(): boolean {
  return tokens !== null
}

export function setLogoutHandler(handler: () => void) {
  onLogout = handler
}

export function logout() {
  tokens = null
  localStorage.removeItem(TOKEN_KEY)
  onLogout()
}

async function tokenRequest(body: Record<string, string>): Promise<Response> {
  return fetch('/api/oauth/token', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ client_id: 'administration', scopes: 'write', ...body }),
  })
}

export async function login(username: string, password: string): Promise<void> {
  const res = await tokenRequest({ grant_type: 'password', username, password })
  if (!res.ok) {
    throw new ApiError(res.status, (await res.json()).errors ?? [])
  }
  storeTokens(await res.json())
}

/** Exchanges the refresh token once, even if several requests fail concurrently. */
async function refresh(): Promise<boolean> {
  if (!tokens) return false
  refreshing ??= tokenRequest({ grant_type: 'refresh_token', refresh_token: tokens.refresh })
    .then(async (res) => {
      if (!res.ok) return false
      storeTokens(await res.json())
      return true
    })
    .finally(() => {
      refreshing = null
    })
  return refreshing
}

async function refreshIfNeeded() {
  if (tokens && tokens.expiresAt - Date.now() < 30_000) {
    await refresh()
  }
}

export async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  await refreshIfNeeded()
  const send = () =>
    fetch(`/api${path}`, {
      method,
      headers: {
        'Content-Type': 'application/json',
        Accept: 'application/json',
        'sw-language-id': contentLanguage.id,
        ...(tokens ? { Authorization: `Bearer ${tokens.access}` } : {}),
      },
      body: body === undefined ? undefined : JSON.stringify(body),
    })
  let res = await send()
  if (res.status === 401 && (await refresh())) {
    res = await send()
  }
  if (res.status === 401) {
    logout()
  }
  if (!res.ok) {
    const payload = await res.json().catch(() => ({ errors: [] }))
    throw new ApiError(res.status, payload.errors ?? [])
  }
  return res.status === 204 ? (undefined as T) : res.json()
}

/** Thin repository over the generic Admin-API endpoints of one entity. */
export function repository<T extends { id?: string }>(entity: string) {
  return {
    search: (criteria: Criteria = {}) => request<SearchResult<T>>('POST', `/search/${entity}`, criteria),
    get: async (id: string, criteria: Criteria = {}) => {
      const result = await request<SearchResult<T>>('POST', `/search/${entity}`, { ...criteria, ids: [id] })
      if (!result.data[0]) throw new ApiError(404, [])
      return result.data[0]
    },
    create: async (payload: Partial<T>) => (await request<{ data: T }>('POST', `/${entity}?_response=1`, payload)).data,
    update: async (id: string, payload: Partial<T>) =>
      (await request<{ data: T }>('PATCH', `/${entity}/${id}?_response=1`, payload)).data,
    delete: (id: string) => request<void>('DELETE', `/${entity}/${id}`),
    unlink: (id: string, association: string, referenceId: string) =>
      request<void>('DELETE', `/${entity}/${id}/${association}/${referenceId}`),
  }
}

export type Action = 'read' | 'create' | 'update' | 'delete'
export type Me = User & { admin: boolean; aclPrivileges: string[]; permissions: Record<string, Action[]> }

/** Creates the missing variants for every combination of the options (one per property group). */
export const generateVariants = async (productId: string, options: string[]) =>
  (await request<{ data: string[] }>('POST', `/_action/product/${productId}/generate-variants`, { options })).data

export const me = async () => (await request<{ data: Me }>('GET', '/_info/me')).data
export const updateMe = (payload: Record<string, unknown>) => request<void>('PATCH', '/_info/me', payload)

/** Uploads a file into an existing media entity. */
export async function uploadMedia(mediaId: string, file: File): Promise<void> {
  const dot = file.name.lastIndexOf('.')
  const extension = dot > 0 ? file.name.slice(dot + 1) : ''
  const fileName = dot > 0 ? file.name.slice(0, dot) : file.name
  await refreshIfNeeded()
  const res = await fetch(
    `/api/_action/media/${mediaId}/upload?extension=${encodeURIComponent(extension)}&fileName=${encodeURIComponent(fileName)}`,
    { method: 'POST', headers: { ...(tokens ? { Authorization: `Bearer ${tokens.access}` } : {}), 'Content-Type': file.type }, body: file },
  )
  if (!res.ok) {
    const payload = await res.json().catch(() => ({ errors: [] }))
    throw new ApiError(res.status, payload.errors ?? [])
  }
}

export interface StateInfo {
  current: string
  transitions: string[]
}
export type OrderStates = Record<'state' | 'paymentState' | 'deliveryState', StateInfo>

export const orderTransitions = async (id: string) =>
  (await request<{ data: OrderStates }>('GET', `/_action/order/${id}/transitions`)).data
export const transitionOrder = async (id: string, machine: string, transition: string) =>
  (await request<{ data: OrderStates }>('POST', `/_action/order/${id}/${machine}/${transition}`)).data
