import { useEffect, useMemo, useState } from 'react'
import LiveMediaPlayer from './components/LiveMediaPlayer'
import CreatorStudio from './components/CreatorStudio'
import type { ApiChatMessage, ApiUser } from './lib/api'
import { connectStreamChatSocket, fetchLiveStreams, fetchStreamChat, isApiConfigured, sendStreamChatMessage, toDiscoveryStream } from './lib/api'
import {
  ArrowLeft, Bell, Bookmark, Check, ChevronDown, Compass, Flame, Headphones,
  Home, Menu, MessageCircle, MonitorPlay, MoreHorizontal, Play, Plus, Radio,
  Search, Send, Settings2, ShieldCheck, Sparkles, Users, Video, X,
} from 'lucide-react'
import { categories, formatViewers, streams, type Category, type Stream } from './data'

type InstallPromptEvent = Event & { prompt: () => Promise<void>; userChoice: Promise<{ outcome: 'accepted' | 'dismissed' }> }

function App() {
  const [activeCategory, setActiveCategory] = useState<Category>('For you')
  const [query, setQuery] = useState('')
  const [selectedStream, setSelectedStream] = useState<Stream | null>(null)
  const [following, setFollowing] = useState<string[]>([])
  const [saved, setSaved] = useState<string[]>([])
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false)
  const [chatMessage, setChatMessage] = useState('')
  const [chatMessages, setChatMessages] = useState<ApiChatMessage[]>([])
  const [chatStatus, setChatStatus] = useState('Chat loads when a live broadcast is connected.')
  const [chatSending, setChatSending] = useState(false)
  const [installPrompt, setInstallPrompt] = useState<InstallPromptEvent | null>(null)
  const [installState, setInstallState] = useState<'hidden' | 'available' | 'installed'>('hidden')
  const [notice, setNotice] = useState('')
  const [apiStreams, setApiStreams] = useState<Stream[] | null>(null)
  const [apiLoading, setApiLoading] = useState(false)
  const [apiError, setApiError] = useState('')
  const [creatorStudioOpen, setCreatorStudioOpen] = useState(false)
  const [creatorUser, setCreatorUser] = useState<ApiUser | null>(null)

  useEffect(() => {
    const onBeforeInstall = (event: Event) => {
      event.preventDefault()
      setInstallPrompt(event as InstallPromptEvent)
      setInstallState('available')
    }
    const onInstalled = () => {
      setInstallState('installed')
      setInstallPrompt(null)
    }
    window.addEventListener('beforeinstallprompt', onBeforeInstall)
    window.addEventListener('appinstalled', onInstalled)
    return () => {
      window.removeEventListener('beforeinstallprompt', onBeforeInstall)
      window.removeEventListener('appinstalled', onInstalled)
    }
  }, [])

  useEffect(() => {
    if (!isApiConfigured()) return
    const controller = new AbortController()
    setApiLoading(true)
    setApiError('')
    fetchLiveStreams({
      query,
      category: activeCategory === 'For you' ? undefined : activeCategory,
      signal: controller.signal,
    }).then((items) => {
      if (!controller.signal.aborted) setApiStreams(items.map(toDiscoveryStream))
    }).catch((cause: unknown) => {
      if (!controller.signal.aborted) {
        setApiError(cause instanceof Error ? cause.message : 'Unable to load live streams.')
        setApiStreams([])
      }
    }).finally(() => {
      if (!controller.signal.aborted) setApiLoading(false)
    })
    return () => controller.abort()
  }, [activeCategory, query])

  useEffect(() => {
    if (!selectedStream) return
    const streamId = selectedStream.id
    const isStreamId = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(streamId)
    if (!isStreamId || !isApiConfigured()) {
      setChatMessages([])
      setChatStatus('Live chat is available on connected LIVE_TRON broadcasts.')
      return
    }
    let active = true
    let socket: WebSocket | null = null
    let reconnectTimer = 0
    let reconnectDelay = 1500
    const controller = new AbortController()
    const mergeMessages = (incoming: ApiChatMessage[]) => {
      setChatMessages((current) => {
        const byId = new Map(current.map((item) => [item.id, item]))
        incoming.forEach((item) => byId.set(item.id, item))
        return [...byId.values()]
          .sort((a, b) => a.created_at.localeCompare(b.created_at))
          .slice(-100)
      })
    }
    const loadHistory = async () => {
      const history = await fetchStreamChat(streamId, controller.signal)
      if (active) mergeMessages(history)
    }
    const connect = async () => {
      try {
        socket = await connectStreamChatSocket(streamId)
        if (!active) {
          socket.close()
          return
        }
        reconnectDelay = 1500
        void loadHistory().catch(() => {
          if (active) setChatStatus('Connected · history refresh failed')
        })
        socket.onmessage = (event) => {
          if (!active || typeof event.data !== 'string') return
          try {
            const payload = JSON.parse(event.data) as { type?: string; message?: ApiChatMessage }
            if (payload.type === 'message' && payload.message) {
              mergeMessages([payload.message])
              setChatStatus('Connected · messages arrive live')
            } else if (payload.type === 'message_deleted' && typeof (payload as { message_id?: unknown }).message_id === 'string') {
              const messageId = (payload as { message_id: string }).message_id
              setChatMessages((current) => current.filter((item) => item.id !== messageId))
            } else if (payload.type === 'presence' && typeof (payload as { participants?: unknown }).participants === 'number') {
              const participants = (payload as { participants: number }).participants
              setChatStatus(`Live chat · ${participants} connected`)
            } else if (payload.type === 'resync_required') {
              void loadHistory().catch(() => setChatStatus('Chat history needs to be refreshed.'))
            } else if (payload.type === 'ready') {
              setChatStatus('Connected · messages arrive live')
            }
          } catch {
            setChatStatus('Received an invalid chat event.')
          }
        }
        socket.onclose = () => {
          if (!active) return
          setChatStatus('Chat disconnected. Reconnecting…')
          reconnectTimer = window.setTimeout(() => {
            reconnectDelay = Math.min(reconnectDelay * 2, 15000)
            void connect()
          }, reconnectDelay)
        }
        socket.onerror = () => setChatStatus('Chat connection interrupted.')
      } catch (cause) {
        if (!active) return
        setChatStatus(cause instanceof Error ? cause.message : 'Unable to connect to live chat.')
        reconnectTimer = window.setTimeout(() => {
          reconnectDelay = Math.min(reconnectDelay * 2, 15000)
          void connect()
        }, reconnectDelay)
      }
    }
    setChatMessages([])
    setChatStatus('Loading chat history…')
    void loadHistory().then(() => {
      if (active) void connect()
    }).catch((cause) => {
      if (active && !controller.signal.aborted) {
        setChatStatus(cause instanceof Error ? cause.message : 'Unable to load live chat.')
      }
    })
    return () => {
      active = false
      controller.abort()
      window.clearTimeout(reconnectTimer)
      socket?.close(1000, 'Stream changed')
    }
  }, [selectedStream?.id])

  const catalogue = isApiConfigured() ? (apiStreams ?? []) : streams
  const filteredStreams = useMemo(() => {
    const normalized = query.trim().toLowerCase()
    return catalogue.filter((stream) => {
      const matchesCategory = activeCategory === 'For you' || stream.category === activeCategory
      const matchesQuery = !normalized || [stream.title, stream.creator, stream.category, stream.handle].some((value) => value.toLowerCase().includes(normalized))
      return matchesCategory && matchesQuery
    })
  }, [activeCategory, catalogue, query])

  const featured = catalogue.find((stream) => stream.featured) ?? catalogue[0]
  const visibleStreams = filteredStreams.filter((stream) => stream.id !== featured?.id)
  const isFollowing = (stream: Stream) => following.includes(stream.id)
  const canUseChat = !!selectedStream && isApiConfigured() && /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(selectedStream.id)

  const toggleFollow = (stream: Stream) => {
    setFollowing((current) => current.includes(stream.id) ? current.filter((id) => id !== stream.id) : [...current, stream.id])
  }
  const toggleSave = (stream: Stream) => {
    setSaved((current) => current.includes(stream.id) ? current.filter((id) => id !== stream.id) : [...current, stream.id])
    setNotice(saved.includes(stream.id) ? 'Removed from your saved list' : 'Added to your saved list')
    window.setTimeout(() => setNotice(''), 2200)
  }
  const openStream = (stream: Stream) => {
    setSelectedStream(stream)
    setChatMessages([])
    setChatStatus('Connecting to live chat…')
  }
  const sendChat = async () => {
    const body = chatMessage.trim()
    if (!body || !selectedStream || chatSending) return
    setChatSending(true)
    setChatStatus('')
    try {
      const message = await sendStreamChatMessage(selectedStream.id, body)
      setChatMessages((current) => [...current.filter((item) => item.id !== message.id), message])
      setChatMessage('')
    } catch (cause) {
      setChatStatus(cause instanceof Error ? cause.message : 'Unable to send chat message.')
    } finally {
      setChatSending(false)
    }
  }
  const installApp = async () => {
    if (!installPrompt) return
    await installPrompt.prompt()
    const choice = await installPrompt.userChoice
    if (choice.outcome === 'accepted') setInstallState('installed')
    setInstallPrompt(null)
  }

  return (
    <div className="app-shell">
      <aside className={`sidebar ${mobileMenuOpen ? 'sidebar-open' : ''}`}>
        <a className="brand" href="#" onClick={(event) => { event.preventDefault(); setSelectedStream(null) }}>
          <span className="brand-mark"><span /></span>
          <span className="brand-word">LIVE<span>_</span>TRON</span>
        </a>
        <button className="mobile-close icon-button" onClick={() => setMobileMenuOpen(false)} aria-label="Close navigation"><X size={19} /></button>
        <div className="sidebar-label">MENU</div>
        <nav className="primary-nav" aria-label="Main navigation">
          <button className={!selectedStream ? 'nav-item active' : 'nav-item'} onClick={() => { setSelectedStream(null); setMobileMenuOpen(false) }}><Home size={19} /> <span>Home</span></button>
          <button className="nav-item" onClick={() => { setActiveCategory('For you'); setSelectedStream(null); setMobileMenuOpen(false) }}><Compass size={19} /> <span>Discover</span></button>
          <button className="nav-item" onClick={() => { setNotice('Following channels will appear here as you follow creators.'); setMobileMenuOpen(false) }}><Users size={19} /> <span>Following</span><span className="nav-count">{following.length || ''}</span></button>
          <button className="nav-item" onClick={() => { setNotice(saved.length ? `${saved.length} saved stream(s) in your list.` : 'Your saved streams will appear here.'); setMobileMenuOpen(false) }}><Bookmark size={19} /> <span>Saved</span></button>
        </nav>
        <div className="sidebar-divider" />
        <div className="sidebar-label">YOUR INTERESTS</div>
        <nav className="interest-nav" aria-label="Categories">
          {categories.slice(1, 6).map((category) => (
            <button key={category} className={activeCategory === category ? 'nav-item active' : 'nav-item'} onClick={() => { setActiveCategory(category); setSelectedStream(null); setMobileMenuOpen(false) }}>
              <span className={`interest-dot dot-${category.toLowerCase()}`} /> <span>{category}</span>
            </button>
          ))}
          <button className="nav-item muted-nav" onClick={() => setNotice('More categories are being added to LIVE_TRON.') }><Plus size={17} /><span>More topics</span></button>
        </nav>
        <div className="sidebar-bottom">
          <div className="creator-card">
            <div className="creator-icon"><Video size={18} /></div>
            <div><strong>Go live on LIVE_TRON</strong><span>Your audience is waiting.</span></div>
            <button aria-label="Creator studio" onClick={() => { setCreatorStudioOpen(true); setMobileMenuOpen(false) }}><ArrowLeft size={16} /></button>
          </div>
          <button className="nav-item muted-nav" onClick={() => setNotice('Settings will be available when accounts are implemented.')}><Settings2 size={18} /><span>Settings</span></button>
          <div className="profile-row"><div className="profile-avatar">{creatorUser?.display_name.slice(0,1).toUpperCase() ?? 'A'}</div><div className="profile-copy"><strong>{creatorUser?.display_name ?? 'Guest account'}</strong><span>{creatorUser?.email ?? 'Explore LIVE_TRON'}</span></div><MoreHorizontal size={19} /></div>
        </div>
      </aside>

      {mobileMenuOpen && <button className="mobile-scrim" aria-label="Close navigation overlay" onClick={() => setMobileMenuOpen(false)} />}

      <main className="main-area">
        <header className="topbar">
          <div className="topbar-left">
            <button className="icon-button mobile-menu" aria-label="Open navigation" onClick={() => setMobileMenuOpen(true)}><Menu size={21} /></button>
            <div className="breadcrumb"><span>LIVE_TRON</span><span className="breadcrumb-slash">/</span><strong>{selectedStream ? 'Watch' : 'Discover'}</strong></div>
          </div>
          <div className="topbar-actions">
            <label className="search-box"><Search size={18} /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search streams, creators..." aria-label="Search streams and creators" /><kbd>⌘ K</kbd></label>
            <button className="icon-button notification-button" aria-label="Notifications" onClick={() => setNotice('You are all caught up.') }><Bell size={19} /><i /></button>
            <button className="top-avatar" aria-label="Open account and creator studio" onClick={() => setCreatorStudioOpen(true)}>{creatorUser?.display_name.slice(0,1).toUpperCase() ?? 'A'}</button>
          </div>
        </header>

        {selectedStream ? (
          <section className="watch-page">
            <button className="back-link" onClick={() => setSelectedStream(null)}><ArrowLeft size={16} /> Back to discovery</button>
            <div className="watch-layout">
              <div className="watch-main">
                {/^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(selectedStream.id) && import.meta.env.VITE_MEDIAMTX_WHEP_BASE_URL ? (
                  <LiveMediaPlayer streamId={selectedStream.id} />
                ) : (
                  <div className="watch-player" style={{ backgroundImage: `linear-gradient(180deg, transparent 55%, rgba(0,0,0,.72)), url("${selectedStream.image}")` }}>
                    <div className="player-topline"><span className="live-pill"><i /> LIVE</span><span className="player-viewers"><Users size={14} /> {formatViewers(selectedStream.viewers)} watching</span></div>
                    <div className="player-center"><button className="play-button" aria-label="Play stream preview"><Play size={25} fill="currentColor" /></button><span>Stream preview</span></div>
                    <div className="player-bottomline"><span><Radio size={15} /> LIVE_TRON PLAYER</span><span>Preview experience · playback integration pending</span></div>
                  </div>
                )}
                <div className="watch-title-row"><div><div className="eyebrow">{selectedStream.category.toUpperCase()} <span>·</span> LIVE NOW</div><h1>{selectedStream.title}</h1></div><button className={saved.includes(selectedStream.id) ? 'icon-button saved' : 'icon-button'} onClick={() => toggleSave(selectedStream)} aria-label="Save stream"><Bookmark size={19} /></button></div>
                <p className="watch-description">{selectedStream.description}</p>
                <div className="channel-strip"><div className="channel-avatar" style={{ background: selectedStream.accent }}>{selectedStream.avatar}</div><div className="channel-details"><strong>{selectedStream.creator} {selectedStream.verified && <ShieldCheck size={15} />}</strong><span>{selectedStream.handle}</span></div><button className={isFollowing(selectedStream) ? 'follow-button following' : 'follow-button'} onClick={() => toggleFollow(selectedStream)}>{isFollowing(selectedStream) ? <><Check size={15} /> Following</> : <><Plus size={15} /> Follow</>}</button></div>
              </div>
              <aside className="chat-panel">
                <div className="chat-header"><div><MessageCircle size={17} /><strong>Live chat</strong></div><span>COMMUNITY</span></div>
                <div className="chat-notice">Be respectful. Keep the conversation welcoming.</div>
                <div className="chat-feed">{chatMessages.map((item) => <div className="chat-line" key={item.id}><strong>{item.display_name}</strong><span>{item.body}</span></div>)}</div>
                <form className="chat-compose" onSubmit={(event) => { event.preventDefault(); void sendChat() }}><input value={chatMessage} onChange={(event) => setChatMessage(event.target.value)} maxLength={500} disabled={!canUseChat} placeholder={canUseChat ? "Sign in to join the conversation…" : "Chat unavailable in preview mode"} aria-label="Chat message" /><button type="submit" aria-label="Send chat message" disabled={!canUseChat || !chatMessage.trim() || chatSending}><Send size={16} /></button></form>
                <div className="chat-footer" role="status">{chatStatus || 'Live connection · 500 character limit'}</div>
              </aside>
            </div>
          </section>
        ) : (
          <div className="content-wrap">
            <div className="page-heading"><div><div className="eyebrow"><span className="eyebrow-line" /> THE LIVE NETWORK</div><h1>Find your <span>live.</span></h1><p>Real-time moments. Communities worth staying for.</p></div><button className="go-live-button" onClick={() => setCreatorStudioOpen(true)}><Radio size={17} /> Go live <span className="go-live-arrow">↗</span></button></div>

            {featured && <section className="featured-section">
              <div className="section-heading"><div><span className="section-kicker">EDITOR'S PICK</span><h2>In the spotlight</h2></div><span className="updated-label"><span /> LIVE RIGHT NOW</span></div>
              <article className="featured-card" style={{ backgroundImage: `linear-gradient(90deg, rgba(8,10,14,.96) 0%, rgba(8,10,14,.78) 39%, rgba(8,10,14,.08) 100%), linear-gradient(0deg, rgba(8,10,14,.42), transparent 55%), url("${featured.image}")` }}>
                <div className="featured-content"><div className="featured-meta"><span className="live-pill"><i /> LIVE</span><span>{featured.category}</span><span className="meta-separator">/</span><span>{formatViewers(featured.viewers)} watching</span></div><h2>{featured.title}</h2><p>{featured.description}</p><div className="featured-creator"><div className="mini-avatar" style={{ background: featured.accent }}>{featured.avatar}</div><strong>{featured.creator}</strong>{featured.verified && <ShieldCheck size={15} />}</div><div className="featured-actions"><button className="watch-button" onClick={() => openStream(featured)}><Play size={15} fill="currentColor" /> Watch live</button><button className="save-button" onClick={() => toggleSave(featured)}><Bookmark size={16} /> {saved.includes(featured.id) ? 'Saved' : 'Save for later'}</button></div></div>
                <div className="featured-index"><span>01</span><i /><span>08</span></div>
              </article>
            </section>}

            <section className="discovery-section">
              <div className="section-heading discovery-heading"><div><span className="section-kicker">YOUR NEXT OBSESSION</span><h2>Explore live channels</h2></div><button className="sort-button" onClick={() => setNotice('Streams are currently sorted by a curated discovery order.')} >Recommended <ChevronDown size={15} /></button></div>
              <div className="category-tabs" role="tablist" aria-label="Stream categories">{categories.map((category) => <button key={category} role="tab" aria-selected={activeCategory === category} className={activeCategory === category ? 'category-tab selected' : 'category-tab'} onClick={() => setActiveCategory(category)}>{category === 'For you' && <Sparkles size={14} />}{category === 'Gaming' && <MonitorPlay size={14} />}{category === 'Music' && <Headphones size={14} />}{category === 'Talk' && <MessageCircle size={14} />}{category === 'Sports' && <Flame size={14} />}{category}</button>)}</div>
              {apiLoading && <div className="results-label" role="status">Loading live channels…</div>}
              {apiError && <div className="results-label" role="alert">Live discovery unavailable: {apiError}</div>}
              {isApiConfigured() && !apiLoading && !apiError && filteredStreams.length === 0 && <div className="results-label">No public live broadcasts are available right now.</div>}
              {query && <div className="results-label">{filteredStreams.length} result(s) for <strong>“{query}”</strong></div>}
              {apiLoading && !apiStreams && <div className="empty-state"><strong>Connecting to LIVE_TRON</strong><span>Loading public broadcasts from the API.</span></div>}
              {visibleStreams.length ? <div className="stream-grid">{visibleStreams.map((stream) => <article className="stream-card" key={stream.id}>
                <button className="stream-thumbnail" style={{ backgroundImage: `linear-gradient(0deg, rgba(0,0,0,.58), transparent 55%), url("${stream.image}")` }} onClick={() => openStream(stream)} aria-label={`Watch ${stream.title}`}><span className="live-pill"><i /> LIVE</span><span className="thumbnail-duration">{stream.duration}</span><span className="thumbnail-play"><Play size={18} fill="currentColor" /></span><span className="thumbnail-viewers"><span /> {formatViewers(stream.viewers)} watching</span></button>
                <div className="stream-info"><div className="stream-avatar" style={{ background: stream.accent }}>{stream.avatar}</div><div className="stream-copy"><button className="stream-title" onClick={() => openStream(stream)}>{stream.title}</button><div className="stream-creator">{stream.creator} {stream.verified && <ShieldCheck size={13} />}</div><div className="stream-category">{stream.category} <span>·</span> {formatViewers(stream.viewers)} watching</div></div><button className={saved.includes(stream.id) ? 'card-more saved' : 'card-more'} onClick={() => toggleSave(stream)} aria-label="Save stream"><Bookmark size={17} /></button></div>
              </article>)}</div> : <div className="empty-state"><Search size={25} /><strong>No streams found</strong><span>Try another search or choose a different category.</span><button onClick={() => { setQuery(''); setActiveCategory('For you') }}>Clear filters</button></div>}
            </section>
            <footer className="page-footer"><span>LIVE<span className="footer-accent">_</span>TRON</span><span>Built for the moments that bring us together.</span><span>PREVIEW BUILD · 0.1.0</span></footer>
          </div>
        )}
      </main>
      <CreatorStudio open={creatorStudioOpen} user={creatorUser} onUserChange={setCreatorUser} onClose={() => setCreatorStudioOpen(false)} />
      {installState === 'available' && <div className="install-banner"><div><strong>Take LIVE_TRON with you</strong><span>Install the app for a focused, full-screen experience.</span></div><button onClick={installApp}>Install app</button><button className="dismiss-install" onClick={() => setInstallState('hidden')} aria-label="Dismiss install prompt"><X size={16} /></button></div>}
      {notice && <div className="toast" role="status">{notice}<button onClick={() => setNotice('')} aria-label="Dismiss notification"><X size={15} /></button></div>}
    </div>
  )
}

export default App
