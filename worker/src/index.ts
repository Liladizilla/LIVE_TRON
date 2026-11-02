import { Hono } from 'hono'
import { cors } from 'hono/cors'
import { createRemoteJWKSet, jwtVerify } from 'jose'
import postgres from 'postgres'

interface Env {
  DB: Hyperdrive
  MEDIA: R2Bucket
  CHAT_ROOMS: DurableObjectNamespace<ChatRoom>
  CLERK_JWKS_URL: string
  CLERK_ISSUER: string
  APP_ORIGIN: string
}
type Variables = { clerkUserId: string; userId: string }
const app = new Hono<{ Bindings: Env; Variables: Variables }>()
const jwksCache = new Map<string, ReturnType<typeof createRemoteJWKSet>>()
const sqlFor = (env: Env) => postgres(env.DB.connectionString, { max: 5, idle_timeout: 20, connect_timeout: 10 })

app.use('/api/*', async (c, next) => {
  const origin = c.req.header('Origin')
  if (origin && origin !== c.env.APP_ORIGIN) return c.json({ error: 'Origin not allowed' }, 403)
  await next()
})
app.use('/api/*', cors({ origin: (origin, c) => origin === c.env.APP_ORIGIN ? origin : '', allowMethods: ['GET','POST','PUT','DELETE','OPTIONS'], allowHeaders: ['Authorization','Content-Type'], credentials: true, maxAge: 86400 }))

app.use('/api/*', async (c, next) => {
  if (c.req.path === '/api/health') return next()
  const token = c.req.header('Authorization')?.replace(/^Bearer\s+/i, '')
  if (!token) return c.json({ error: 'Authentication required' }, 401)
  try {
    const jwksUrl = new URL(c.env.CLERK_JWKS_URL)
    let jwks = jwksCache.get(jwksUrl.href)
    if (!jwks) { jwks = createRemoteJWKSet(jwksUrl); jwksCache.set(jwksUrl.href, jwks) }
    const { payload } = await jwtVerify(token, jwks, { issuer: c.env.CLERK_ISSUER })
    if (typeof payload.sub !== 'string') return c.json({ error: 'Invalid identity token' }, 401)
    const sql = sqlFor(c.env)
    try {
      const [user] = await sql`
        INSERT INTO users (clerk_user_id, email, display_name)
        VALUES (${payload.sub}, ${typeof payload.email === 'string' ? payload.email : null}, ${typeof payload.name === 'string' ? payload.name : 'New creator'})
        ON CONFLICT (clerk_user_id) DO UPDATE SET updated_at = now()
        RETURNING id
      `
      c.set('clerkUserId', payload.sub)
      c.set('userId', user.id as string)
    } finally { await sql.end() }
    await next()
  } catch {
    return c.json({ error: 'Invalid or expired session' }, 401)
  }
})

app.get('/api/health', (c) => c.json({ status: 'ok', service: 'live-tron-api', timestamp: new Date().toISOString() }))

app.get('/api/me', async (c) => {
  const sql = sqlFor(c.env)
  try {
    const [user] = await sql`SELECT id, clerk_user_id, email, display_name, username, avatar_url, bio, created_at FROM users WHERE id = ${c.get('userId')}`
    return c.json({ user })
  } finally { await sql.end() }
})

app.get('/api/streams', async (c) => {
  const sql = sqlFor(c.env)
  const category = c.req.query('category')
  try {
    const rows = category
      ? await sql`SELECT s.id, s.title, s.description, s.category, s.status, s.thumbnail_key, s.started_at, ch.slug AS channel_slug, ch.title AS channel_title, u.username, u.display_name, u.avatar_url FROM streams s JOIN channels ch ON ch.id=s.channel_id JOIN users u ON u.id=ch.owner_id WHERE s.category=${category} ORDER BY s.created_at DESC LIMIT 60`
      : await sql`SELECT s.id, s.title, s.description, s.category, s.status, s.thumbnail_key, s.started_at, ch.slug AS channel_slug, ch.title AS channel_title, u.username, u.display_name, u.avatar_url FROM streams s JOIN channels ch ON ch.id=s.channel_id JOIN users u ON u.id=ch.owner_id ORDER BY s.created_at DESC LIMIT 60`
    return c.json({ streams: rows })
  } finally { await sql.end() }
})

app.post('/api/channels/:channelId/follow', async (c) => {
  const sql = sqlFor(c.env)
  try {
    await sql`INSERT INTO follows (follower_id, channel_id) VALUES (${c.get('userId')}, ${c.req.param('channelId')}) ON CONFLICT DO NOTHING`
    return c.json({ following: true })
  } catch { return c.json({ error: 'Channel not found' }, 404) }
  finally { await sql.end() }
})
app.delete('/api/channels/:channelId/follow', async (c) => {
  const sql = sqlFor(c.env)
  try {
    await sql`DELETE FROM follows WHERE follower_id=${c.get('userId')} AND channel_id=${c.req.param('channelId')}`
    return c.json({ following: false })
  } finally { await sql.end() }
})

app.post('/api/streams/:streamId/save', async (c) => {
  const sql = sqlFor(c.env)
  try {
    await sql`INSERT INTO saved_streams (user_id, stream_id) VALUES (${c.get('userId')}, ${c.req.param('streamId')}) ON CONFLICT DO NOTHING`
    return c.json({ saved: true })
  } catch { return c.json({ error: 'Stream not found' }, 404) }
  finally { await sql.end() }
})
app.delete('/api/streams/:streamId/save', async (c) => {
  const sql = sqlFor(c.env)
  try {
    await sql`DELETE FROM saved_streams WHERE user_id=${c.get('userId')} AND stream_id=${c.req.param('streamId')}`
    return c.json({ saved: false })
  } finally { await sql.end() }
})

app.post('/api/media/upload/:key', async (c) => {
  const key = c.req.param('key')
  if (!/^[a-zA-Z0-9/_-]{1,180}$/.test(key) || key.includes('..')) return c.json({ error: 'Invalid object key' }, 400)
  const contentType = c.req.header('Content-Type') ?? ''
  if (!['image/jpeg','image/png','image/webp','image/avif'].includes(contentType)) return c.json({ error: 'Unsupported media type' }, 415)
  const body = await c.req.arrayBuffer()
  if (body.byteLength < 1 || body.byteLength > 8 * 1024 * 1024) return c.json({ error: 'Image must be 1 byte to 8 MB' }, 413)
  await c.env.MEDIA.put(key, body, { httpMetadata: { contentType } })
  return c.json({ key, size: body.byteLength }, 201)
})

app.get('/api/streams/:streamId/chat', async (c) => {
  const id = c.env.CHAT_ROOMS.idFromName(c.req.param('streamId'))
  return c.env.CHAT_ROOMS.get(id).fetch(new Request('https://chat.internal/history', { headers: { Authorization: c.req.header('Authorization') ?? '' } }))
})
app.get('/ws/streams/:streamId', async (c) => {
  const id = c.env.CHAT_ROOMS.idFromName(c.req.param('streamId'))
  const url = new URL(c.req.url)
  url.pathname = '/connect'
  return c.env.CHAT_ROOMS.get(id).fetch(new Request(url, c.req.raw))
})

export class ChatRoom {
  private state: DurableObjectState
  private env: Env
  constructor(state: DurableObjectState, env: Env) { this.state = state; this.env = env }
  async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url)
    if (url.pathname === '/history') {
      const token = request.headers.get('Authorization')?.replace(/^Bearer\s+/i, '')
      if (!token) return Response.json({ error: 'Authentication required' }, { status: 401 })
      return Response.json({ messages: await this.state.storage.get('recentMessages') ?? [] })
    }
    if (url.pathname !== '/connect' || request.headers.get('Upgrade') !== 'websocket') return new Response('WebSocket upgrade required', { status: 426 })
    const pair = new WebSocketPair()
    const client = pair[0], server = pair[1]
    this.state.acceptWebSocket(server)
    server.send(JSON.stringify({ type: 'ready', connected: this.state.getWebSockets().length }))
    return new Response(null, { status: 101, webSocket: client })
  }
  async webSocketMessage(ws: WebSocket, message: string | ArrayBuffer): Promise<void> {
    if (typeof message !== 'string' || message.length > 1200) return
    let event: { type?: string; body?: string; user?: string }
    try { event = JSON.parse(message) as typeof event } catch { ws.send(JSON.stringify({ type: 'error', message: 'Invalid message' })); return }
    if (event.type !== 'chat' || typeof event.body !== 'string' || !event.body.trim() || event.body.length > 1000) return
    const entry = { type: 'chat', id: crypto.randomUUID(), body: event.body.trim(), user: 'Member', createdAt: new Date().toISOString() }
    const recent = (await this.state.storage.get<typeof entry[]>('recentMessages') ?? []).slice(-99)
    recent.push(entry)
    await this.state.storage.put('recentMessages', recent)
    for (const socket of this.state.getWebSockets()) socket.send(JSON.stringify(entry))
  }
  async webSocketClose(ws: WebSocket): Promise<void> { ws.close() }
  async webSocketError(ws: WebSocket): Promise<void> { ws.close(1011, 'WebSocket error') }
}

export default app
