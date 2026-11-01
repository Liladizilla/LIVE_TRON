import { useEffect, useState, type FormEvent } from 'react'
import { Check, Copy, LogIn, Radio, Shield, Video, X } from 'lucide-react'
import LiveBroadcastPanel from './LiveBroadcastPanel'
import {
  ApiError,
  createChannel,
  createCreatorStream,
  fetchMyChannels,
  isApiConfigured,
  loginAccount,
  logoutAccount,
  registerAccount,
  type ApiChannel,
  type ApiUser,
  type CreatedStream,
} from '../lib/api'
import './CreatorStudio.css'

type Props = {
  user: ApiUser | null
  onUserChange: (user: ApiUser | null) => void
  onClose: () => void
  open: boolean
}

const categories = ['gaming', 'music', 'talk', 'education', 'sports', 'irl', 'commerce']

function messageFor(error: unknown): string {
  if (error instanceof ApiError) {
    const messages: Record<string, string> = {
      invalid_credentials: 'Email or password is incorrect.',
      email_already_registered: 'That email already has an account. Sign in instead.',
      invalid_email: 'Enter a valid email address.',
      password_length_invalid: 'Password must contain 12 to 128 characters.',
      invalid_display_name: 'Display name must contain 2 to 80 characters.',
      invalid_channel_slug: 'Use 3–48 lowercase letters, numbers, and single hyphens.',
      channel_slug_taken: 'That channel handle is already taken.',
      invalid_channel_title: 'Channel title must contain 2 to 100 characters.',
      invalid_stream_title: 'Stream title must contain 2 to 120 characters.',
    }
    return messages[error.message] ?? `Request failed (HTTP ${error.status}): ${error.message}`
  }
  return error instanceof Error ? error.message : 'Something went wrong. Try again.'
}

export default function CreatorStudio({ user, onUserChange, onClose, open }: Props) {
  const [registerMode, setRegisterMode] = useState(false)
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [displayName, setDisplayName] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [channel, setChannel] = useState<ApiChannel | null>(null)
  const [slug, setSlug] = useState('')
  const [channelTitle, setChannelTitle] = useState('')
  const [channelDescription, setChannelDescription] = useState('')
  const [category, setCategory] = useState('talk')
  const [streamTitle, setStreamTitle] = useState('')
  const [visibility, setVisibility] = useState<'public' | 'unlisted' | 'private'>('public')
  const [created, setCreated] = useState<CreatedStream | null>(null)
  const [keyCopied, setKeyCopied] = useState(false)

  useEffect(() => {
    if (!user || !isApiConfigured()) return
    let active = true
    fetchMyChannels().then((items) => {
      if (active && items.length > 0) setChannel(items[0])
    }).catch((cause: unknown) => {
      if (active) setError(messageFor(cause))
    })
    return () => { active = false }
  }, [user])

  if (!open) return null

  if (!isApiConfigured()) {
    return <div className="studio-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose() }}>
      <section className="studio-modal" role="dialog" aria-modal="true" aria-labelledby="studio-title">
        <button className="studio-close" onClick={onClose} aria-label="Close"><X size={19} /></button>
        <div className="studio-kicker">CREATOR WORKSPACE</div><h2 id="studio-title">Creator Studio</h2>
        <p className="studio-muted">Set <code>VITE_LIVE_TRON_API_URL</code> and connect the backend before creating a channel or stream.</p>
      </section>
    </div>
  }

  const submitAuth = async (event: FormEvent) => {
    event.preventDefault()
    setBusy(true)
    setError('')
    try {
      const nextUser = registerMode
        ? await registerAccount({ email, password, display_name: displayName })
        : await loginAccount({ email, password })
      onUserChange(nextUser)
    } catch (cause) {
      setError(messageFor(cause))
    } finally {
      setBusy(false)
    }
  }

  const submitChannel = async (event: React.FormEvent) => {
    event.preventDefault()
    setBusy(true)
    setError('')
    try {
      const next = await createChannel({
        slug: slug.trim().toLowerCase(),
        title: channelTitle.trim(),
        description: channelDescription.trim(),
        category,
        is_mature: false,
      })
      setChannel(next)
    } catch (cause) {
      setError(messageFor(cause))
    } finally {
      setBusy(false)
    }
  }

  const submitStream = async (event: React.FormEvent) => {
    event.preventDefault()
    if (!channel) return
    setBusy(true)
    setError('')
    try {
      const result = await createCreatorStream(channel.slug, {
        title: streamTitle.trim(),
        category,
        visibility,
      })
      setCreated(result)
    } catch (cause) {
      setError(messageFor(cause))
    } finally {
      setBusy(false)
    }
  }

  const copyKey = async () => {
    if (!created) return
    try {
      await navigator.clipboard.writeText(created.publishing_key)
      setKeyCopied(true)
    } catch {
      setError('Clipboard access was blocked. Select and copy the key manually.')
    }
  }

  const signOut = async () => {
    setBusy(true)
    setError('')
    try {
      await logoutAccount()
      onUserChange(null)
      setChannel(null)
      setCreated(null)
    } catch (cause) {
      setError(messageFor(cause))
    } finally {
      setBusy(false)
    }
  }

  return <div className="studio-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose() }}>
    <section className="studio-modal" role="dialog" aria-modal="true" aria-labelledby="studio-title">
      <header className="studio-header">
        <div><div className="studio-kicker">LIVE_TRON / CREATOR WORKSPACE</div><h2 id="studio-title">Creator Studio</h2></div>
        <button className="studio-close" onClick={onClose} aria-label="Close"><X size={19} /></button>
      </header>

      {!user ? <form className="studio-form" onSubmit={submitAuth}>
        <div className="studio-step"><LogIn size={17} /> ACCOUNT ACCESS</div>
        <p className="studio-muted">Sign in to manage a channel, or create an account to get started.</p>
        {registerMode && <label>Display name<input value={displayName} onChange={(event) => setDisplayName(event.target.value)} minLength={2} maxLength={80} required autoComplete="name" /></label>}
        <label>Email<input type="email" value={email} onChange={(event) => setEmail(event.target.value)} required autoComplete="email" /></label>
        <label>Password<input type="password" value={password} onChange={(event) => setPassword(event.target.value)} minLength={registerMode ? 12 : undefined} maxLength={128} required autoComplete={registerMode ? 'new-password' : 'current-password'} /></label>
        {error && <p className="studio-error" role="alert">{error}</p>}
        <button className="studio-primary" type="submit" disabled={busy}>{busy ? 'Please wait…' : registerMode ? 'Create account' : 'Sign in'}</button>
        <button className="studio-text-button" type="button" onClick={() => { setRegisterMode(!registerMode); setError('') }}>{registerMode ? 'Already registered? Sign in' : 'New to LIVE_TRON? Create an account'}</button>
      </form> : <div className="studio-workspace">
        <div className="studio-account"><div className="studio-user-mark">{user.display_name.slice(0,1).toUpperCase()}</div><div><strong>{user.display_name}</strong><span>{user.email}</span></div><button className="studio-text-button" onClick={signOut} disabled={busy}>Sign out</button></div>
        {!channel ? <form className="studio-form" onSubmit={submitChannel}>
          <div className="studio-step"><Video size={17} /> STEP 1 / CHANNEL</div>
          <p className="studio-muted">Create your creator channel. Its handle becomes your public channel address.</p>
          <label>Channel handle<input value={slug} onChange={(event) => setSlug(event.target.value.toLowerCase().replace(/[^a-z0-9-]/g, '-'))} minLength={3} maxLength={48} pattern="[a-z0-9]+(?:-[a-z0-9]+)*" placeholder="your-channel" required /></label>
          <label>Channel title<input value={channelTitle} onChange={(event) => setChannelTitle(event.target.value)} minLength={2} maxLength={100} required /></label>
          <label>Description<textarea value={channelDescription} onChange={(event) => setChannelDescription(event.target.value)} maxLength={2000} rows={3} /></label>
          <label>Primary category<select value={category} onChange={(event) => setCategory(event.target.value)}>{categories.map((item) => <option key={item} value={item}>{item[0].toUpperCase()+item.slice(1)}</option>)}</select></label>
          {error && <p className="studio-error" role="alert">{error}</p>}
          <button className="studio-primary" disabled={busy}>{busy ? 'Creating channel…' : 'Create channel'}</button>
        </form> : <div className="studio-form">
          <div className="studio-step"><Check size={17} /> CHANNEL READY</div>
          <div className="studio-channel-summary"><strong>{channel.title}</strong><span>@{channel.slug} · {channel.category}</span></div>
          {!created ? <form className="studio-form studio-nested" onSubmit={submitStream}>
            <div className="studio-step"><Radio size={17} /> STEP 2 / STREAM</div>
            <p className="studio-muted">Create a stream record and issue its one-time publishing key.</p>
            <label>Stream title<input value={streamTitle} onChange={(event) => setStreamTitle(event.target.value)} minLength={2} maxLength={120} required /></label>
            <label>Category<select value={category} onChange={(event) => setCategory(event.target.value)}>{categories.map((item) => <option key={item} value={item}>{item[0].toUpperCase()+item.slice(1)}</option>)}</select></label>
            <label>Visibility<select value={visibility} onChange={(event) => setVisibility(event.target.value as typeof visibility)}><option value="public">Public</option><option value="unlisted">Unlisted</option><option value="private">Private</option></select></label>
            {error && <p className="studio-error" role="alert">{error}</p>}
            <button className="studio-primary" disabled={busy}>{busy ? 'Creating stream…' : 'Create stream & publishing key'}</button>
          </form> : <div className="studio-key-section">
            <div className="studio-step"><Shield size={17} /> PUBLISHING CREDENTIAL</div>
            <p className="studio-warning">This key is shown once. Copy it now and keep it private. Closing this window loses the plaintext key.</p>
            <div className="studio-key-value"><code>{created.publishing_key}</code><button onClick={copyKey} aria-label="Copy publishing key"><Copy size={16} /></button></div>
            {keyCopied && <p className="studio-success"><Check size={14} /> Copied to clipboard</p>}
            <p className="studio-muted">{created.publishing_key_notice}</p>
            <div className="studio-stream-state"><span>STREAM CREATED</span><strong>{created.stream.title}</strong><small>Waiting for a publisher. This is not live yet.</small></div>
            <LiveBroadcastPanel streamId={created.stream.id} publishingKey={created.publishing_key} />
          </div>}
        </div>}
        {error && channel && created && <p className="studio-error" role="alert">{error}</p>}
      </div>}
      <footer className="studio-footer">Publishing keys are credentials. Never share them in chat, screenshots, or public repositories.</footer>
    </section>
  </div>
}
