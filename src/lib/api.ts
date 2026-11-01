import type { Stream } from '../data'

type ApiUser = {
  id: string
  email: string
  display_name: string
  avatar_url: string | null
  created_at: string
}

type TokenResponse = {
  access_token: string
  token_type: 'Bearer'
  expires_in: number
  refresh_token: string
  user: ApiUser
}

export type ApiStream = {
  id: string
  channel_id: string
  channel_slug: string
  title: string
  category: string
  status: 'created' | 'live' | 'ended'
  visibility: 'public' | 'unlisted' | 'private'
  started_at: string | null
  ended_at: string | null
  created_at: string
}

type ApiErrorBody = { error?: string }

const API_BASE_URL = (import.meta.env.VITE_LIVE_TRON_API_URL ?? '').trim().replace(/\/+$/, '')
let accessToken: string | null = null
let refreshToken: string | null = null
let refreshInFlight: Promise<boolean> | null = null

export class ApiError extends Error {
  constructor(message: string, readonly status: number) {
    super(message)
    this.name = 'ApiError'
  }
}

export const isApiConfigured = () => API_BASE_URL.length > 0

function endpoint(path: string): string {
  if (!API_BASE_URL) throw new Error('LIVE_TRON API is not configured. Set VITE_LIVE_TRON_API_URL.')
  return `${API_BASE_URL}${path}`
}

async function decodeResponse<T>(response: Response): Promise<T> {
  if (response.status === 204) return undefined as T
  const body = await response.json().catch(() => null) as ApiErrorBody | T | null
  if (!response.ok) {
    const message = body && typeof body === 'object' && 'error' in body && typeof body.error === 'string'
      ? body.error
      : `API request failed with HTTP ${response.status}`
    throw new ApiError(message, response.status)
  }
  return body as T
}

async function rawRequest<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers = new Headers(init.headers)
  if (init.body && !headers.has('Content-Type')) headers.set('Content-Type', 'application/json')
  headers.set('Accept', 'application/json')
  return decodeResponse<T>(await fetch(endpoint(path), { ...init, headers }))
}

async function refreshAccessToken(): Promise<boolean> {
  if (!refreshToken) return false
  if (refreshInFlight) return refreshInFlight

  refreshInFlight = (async () => {
    try {
      const session = await rawRequest<TokenResponse>('/v1/auth/refresh', {
        method: 'POST',
        body: JSON.stringify({ refresh_token: refreshToken }),
      })
      accessToken = session.access_token
      refreshToken = session.refresh_token
      return true
    } catch {
      accessToken = null
      refreshToken = null
      return false
    } finally {
      refreshInFlight = null
    }
  })()

  return refreshInFlight
}

async function authenticatedRequest<T>(path: string, init: RequestInit = {}): Promise<T> {
  const send = () => {
    const headers = new Headers(init.headers)
    if (accessToken) headers.set('Authorization', `Bearer ${accessToken}`)
    return fetch(endpoint(path), { ...init, headers })
  }

  let response = await send()
  if (response.status === 401 && await refreshAccessToken()) response = await send()
  return decodeResponse<T>(response)
}

export async function registerAccount(input: { email: string; password: string; display_name: string }): Promise<ApiUser> {
  const session = await rawRequest<TokenResponse>('/v1/auth/register', {
    method: 'POST',
    body: JSON.stringify(input),
  })
  accessToken = session.access_token
  refreshToken = session.refresh_token
  return session.user
}

export async function loginAccount(input: { email: string; password: string }): Promise<ApiUser> {
  const session = await rawRequest<TokenResponse>('/v1/auth/login', {
    method: 'POST',
    body: JSON.stringify(input),
  })
  accessToken = session.access_token
  refreshToken = session.refresh_token
  return session.user
}

export async function getCurrentUser(): Promise<ApiUser> {
  return authenticatedRequest<ApiUser>('/v1/me')
}

export async function logoutAccount(): Promise<void> {
  try {
    if (refreshToken) {
      await rawRequest<void>('/v1/auth/logout', {
        method: 'POST',
        body: JSON.stringify({ refresh_token: refreshToken }),
      })
    }
  } finally {
    accessToken = null
    refreshToken = null
  }
}

export async function fetchLiveStreams(options: { query?: string; category?: string; signal?: AbortSignal } = {}): Promise<ApiStream[]> {
  const params = new URLSearchParams({ limit: '50' })
  if (options.query?.trim()) params.set('q', options.query.trim())
  if (options.category) params.set('category', options.category.toLowerCase())
  return rawRequest<ApiStream[]>(`/v1/streams?${params.toString()}`, { signal: options.signal })
}

const categoryFallbackImages: Record<string, string> = {
  gaming: 'https://images.unsplash.com/photo-1542751371-adc38448a05e?w=1200&auto=format&fit=crop&q=85',
  music: 'https://images.unsplash.com/photo-1516280440614-37939bbacd81?w=1000&auto=format&fit=crop&q=85',
  talk: 'https://images.unsplash.com/photo-1475721027785-f74eccf877e2?w=1000&auto=format&fit=crop&q=85',
  education: 'https://images.unsplash.com/photo-1513364776144-60967b0f800f?w=1000&auto=format&fit=crop&q=85',
  sports: 'https://images.unsplash.com/photo-1522778119026-d647f0596c20?w=1000&auto=format&fit=crop&q=85',
  irl: 'https://images.unsplash.com/photo-1483985988355-763728e1935b?w=1000&auto=format&fit=crop&q=85',
  commerce: 'https://images.unsplash.com/photo-1542291026-7eec264c27ff?w=1000&auto=format&fit=crop&q=85',
}

export function toDiscoveryStream(stream: ApiStream): Stream {
  const category = stream.category.toLowerCase()
  const supportedCategory = ['gaming', 'music', 'talk', 'education', 'sports', 'irl', 'commerce'].includes(category)
    ? category as Stream['category']
    : 'Talk'
  const handle = `@${stream.channel_slug}`
  const creator = stream.channel_slug.replace(/-/g, ' ').replace(/\b\w/g, (letter) => letter.toUpperCase())

  return {
    id: stream.id,
    title: stream.title,
    creator,
    handle,
    category: (supportedCategory.charAt(0).toUpperCase() + supportedCategory.slice(1)) as Stream['category'],
    viewers: null,
    accent: '#a7a7a7',
    image: categoryFallbackImages[category] ?? categoryFallbackImages.talk,
    avatar: creator.split(/\s+/).map((part) => part[0]).join('').slice(0, 2).toUpperCase(),
    duration: 'LIVE',
    description: `Live broadcast from ${creator}.`,
  }
}
