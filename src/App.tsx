import { useEffect, useMemo, useRef, useState } from 'react'
import {
  ArrowLeftIcon as ArrowLeft, BellIcon as Bell, BookmarkSimpleIcon as Bookmark,
  BookmarkSimpleIcon as BookmarkCheck, CalendarBlankIcon as CalendarClock, CheckIcon as Check,
  CaretDownIcon as ChevronDown, CaretLeftIcon as ChevronLeft, CaretRightIcon as ChevronRight,
  CompassIcon as Compass, GameControllerIcon as Gamepad2, GraduationCapIcon as GraduationCap,
  HeartIcon as Heart, HouseIcon as Home, MapPinIcon as MapPin, ListIcon as Menu,
  ChatCircleIcon as MessageCircle, MoonIcon as Moon, MusicNotesIcon as Music2,
  RadioIcon as Radio, MagnifyingGlassIcon as Search, PaperPlaneTiltIcon as Send,
  SealCheckIcon as ShieldCheck, SlidersHorizontalIcon as SlidersHorizontal, SunIcon as Sun,
  UsersIcon as Users, VideoCameraIcon as Video, XIcon as X, ShoppingBagIcon as ShoppingBag,
  SparkleIcon as Sparkles, QuestionIcon as CircleHelp,
} from '@phosphor-icons/react'
import { categories, creators, formatViewers, streams, type Category, type Stream } from './data'

type View = 'home' | 'discover' | 'following' | 'saved'
type InstallPromptEvent = Event & { prompt: () => Promise<void>; userChoice: Promise<{ outcome: 'accepted' | 'dismissed' }> }
type Theme = 'dark' | 'light'

const categoryIcons: Record<Category, typeof Home> = {
  'For you': Sparkles, Gaming: Gamepad2, Music: Music2, Talk: MessageCircle,
  Education: GraduationCap, Sports: Radio, IRL: MapPin, Commerce: ShoppingBag,
}

function CreatorAvatar({ src, name, size = 'medium', live = false }: { src: string; name: string; size?: 'small' | 'medium' | 'large'; live?: boolean }) {
  return <span className={'creator-avatar creator-avatar-' + size + (live ? ' is-live' : '')}><img src={src} alt={name} loading="lazy" /></span>
}

function StreamCard({ stream, saved, onOpen, onSave, onNotify, index }: {
  stream: Stream; saved: boolean; onOpen: (stream: Stream) => void; onSave: (stream: Stream) => void; onNotify: (stream: Stream) => void; index: number
}) {
  const isLive = stream.status === 'live'
  return (
    <article className={'stream-card' + (index % 7 === 2 ? ' stream-card-wide' : '')}>
      <button className="stream-media" onClick={() => onOpen(stream)} aria-label={(stream.status === 'ended' ? 'Watch replay: ' : 'Open stream: ') + stream.title}>
        <img src={stream.image} alt="" loading="lazy" />
        <span className="media-shade" />
        {isLive && <span className="live-label"><i /> Live</span>}
        {stream.status === 'scheduled' && <span className="scheduled-label"><CalendarClock weight="regular" size={14} /> Later today</span>}
        {stream.status === 'ended' && <span className="replay-label">Replay</span>}
        {isLive && <span className="stream-time">{stream.duration}</span>}
        <span className="media-bottom">{isLive ? <><Users weight="regular" size={15} /> {formatViewers(stream.viewers)} watching</> : stream.status === 'scheduled' ? stream.startsAt : 'Watch replay'}</span>
        <span className="media-play"><span>{isLive ? 'Watch live' : stream.status === 'scheduled' ? 'Set reminder' : 'Watch replay'}</span></span>
      </button>
      <div className="stream-details">
        <CreatorAvatar src={stream.avatarUrl} name={stream.creator} size="medium" live={isLive} />
        <div className="stream-copy">
          <button className="stream-title" onClick={() => onOpen(stream)}>{stream.title}</button>
          <div className="stream-creator">{stream.creator}{stream.verified && <ShieldCheck weight="regular" size={14} aria-label="Verified creator" />}</div>
          <div className="stream-meta">{stream.category} <span>·</span> {isLive ? formatViewers(stream.viewers) + ' watching' : stream.status === 'scheduled' ? stream.startsAt : 'Replay available'}</div>
        </div>
        {stream.status === 'scheduled' && <button className="notify-me-button" onClick={() => onNotify(stream)}>Notify me</button>}
        <button className={'save-control' + (saved ? ' is-saved' : '')} onClick={() => onSave(stream)} aria-label={saved ? 'Remove saved stream' : 'Save stream'} title={saved ? 'Saved' : 'Save stream'}>
          {saved ? <BookmarkCheck weight="regular" size={19} /> : <Bookmark weight="regular" size={19} />}
        </button>
      </div>
    </article>
  )
}

function App() {
  const [view, setView] = useState<View>('home')
  const [activeCategory, setActiveCategory] = useState<Category>('For you')
  const [query, setQuery] = useState('')
  const [selectedStream, setSelectedStream] = useState<Stream | null>(null)
  const [following, setFollowing] = useState<string[]>(['nia', 'mika', 'maya'])
  const [saved, setSaved] = useState<string[]>(['s2', 's8'])
  const [collapsed, setCollapsed] = useState(false)
  const [mobileNavOpen, setMobileNavOpen] = useState(false)
  const [theme, setTheme] = useState<Theme>(() => {
    try {
      const stored = localStorage.getItem('live-tron-theme')
      if (stored === 'light' || stored === 'dark') return stored
    } catch { /* Storage may be unavailable. */ }
    return window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark'
  })
  const [heroIndex, setHeroIndex] = useState(0)
  const [heroPaused, setHeroPaused] = useState(false)
  const [sort, setSort] = useState('recommended')
  const [paletteOpen, setPaletteOpen] = useState(false)
  const [notice, setNotice] = useState('')
  const [chatMessage, setChatMessage] = useState('')
  const [chatMessages, setChatMessages] = useState([
    { name: 'Lena', message: 'Just got here. This sounds lovely.', role: 'member' },
    { name: 'Jayden', message: 'That last transition was so clean.', role: 'member' },
    { name: 'Mika', message: 'Thanks for hanging out with us tonight.', role: 'creator' },
    { name: 'Nuru', message: 'Please do the slower version next.', role: 'member' },
    { name: 'Asha', message: 'Quick note: keep spoilers out of chat, please.', role: 'moderator' },
  ])
  const [chatOpen, setChatOpen] = useState(false)
  const [chatExpanded, setChatExpanded] = useState(false)
  const touchStartY = useRef<number | null>(null)
  const [reactionSent, setReactionSent] = useState(false)
  const [installPrompt, setInstallPrompt] = useState<InstallPromptEvent | null>(null)
  const [installState, setInstallState] = useState<'hidden' | 'available' | 'installed'>('hidden')

  const liveStreams = useMemo(() => streams.filter((stream) => stream.status === 'live'), [])
  const featuredStreams = liveStreams.filter((stream) => stream.featured || ['s2', 's5', 's8'].includes(stream.id))
  const featured = featuredStreams[heroIndex % featuredStreams.length] ?? liveStreams[0]
  const hour = new Date().getHours()
  const greeting = hour < 12 ? 'Good morning' : hour < 18 ? 'Good afternoon' : 'Good evening'
  const followedLive = liveStreams.filter((stream) => following.includes(stream.creatorId))
  const followedCreators = creators.filter((creator) => following.includes(creator.id))

  const filteredStreams = useMemo(() => {
    const normalized = query.trim().toLowerCase()
    let result = streams.filter((stream) => {
      const categoryMatch = activeCategory === 'For you' || stream.category === activeCategory
      const queryMatch = !normalized || [stream.title, stream.creator, stream.handle, stream.category].some((value) => value.toLowerCase().includes(normalized))
      const viewMatch = view === 'following' ? following.includes(stream.creatorId) : view === 'saved' ? saved.includes(stream.id) : true
      return categoryMatch && queryMatch && viewMatch
    })
    if (sort === 'viewers') result = [...result].sort((a, b) => b.viewers - a.viewers)
    if (sort === 'newest') result = [...result].reverse()
    return result
  }, [activeCategory, query, view, following, saved, sort])

  useEffect(() => {
    document.documentElement.dataset.theme = theme
    try { localStorage.setItem('live-tron-theme', theme) } catch { /* Session theme remains active. */ }
  }, [theme])

  useEffect(() => {
    if (heroPaused || window.matchMedia('(prefers-reduced-motion: reduce)').matches) return
    const timer = window.setInterval(() => setHeroIndex((current) => (current + 1) % featuredStreams.length), 8000)
    return () => window.clearInterval(timer)
  }, [heroPaused, featuredStreams.length])

  useEffect(() => {
    if (!notice) return
    const timer = window.setTimeout(() => setNotice(''), 5000)
    return () => window.clearTimeout(timer)
  }, [notice])

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault()
        setPaletteOpen(true)
      }
      if (event.key === 'Escape') {
        setPaletteOpen(false)
        setMobileNavOpen(false)
        setChatOpen(false)
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [])

  useEffect(() => {
    const onBeforeInstall = (event: Event) => {
      event.preventDefault()
      setInstallPrompt(event as InstallPromptEvent)
      setInstallState('available')
    }
    const onInstalled = () => { setInstallState('installed'); setInstallPrompt(null) }
    window.addEventListener('beforeinstallprompt', onBeforeInstall)
    window.addEventListener('appinstalled', onInstalled)
    return () => {
      window.removeEventListener('beforeinstallprompt', onBeforeInstall)
      window.removeEventListener('appinstalled', onInstalled)
    }
  }, [])

  const openStream = (stream: Stream) => {
    setSelectedStream(stream)
    setChatOpen(false)
    setChatExpanded(false)
    setChatMessages([
      { name: 'Lena', message: 'Just got here. This sounds lovely.', role: 'member' },
      { name: 'Jayden', message: 'That last transition was so clean.', role: 'member' },
      { name: stream.creator.split(' ')[0], message: 'Thanks for hanging out with us tonight.', role: 'creator' },
      { name: 'Nuru', message: 'Please do the slower version next.', role: 'member' },
      { name: 'Asha', message: 'Quick note: keep spoilers out of chat, please.', role: 'moderator' },
    ])
    setReactionSent(false)
  }
  const toggleFollow = (creatorId: string) => {
    const isFollowing = following.includes(creatorId)
    setFollowing((current) => isFollowing ? current.filter((id) => id !== creatorId) : [...current, creatorId])
    const creator = creators.find((item) => item.id === creatorId)
    setNotice(isFollowing ? 'You unfollowed ' + (creator?.name ?? 'this creator') + '.' : 'You are now following ' + (creator?.name ?? 'this creator') + '.')
  }
  const toggleSave = (stream: Stream) => {
    const exists = saved.includes(stream.id)
    setSaved((current) => exists ? current.filter((id) => id !== stream.id) : [...current, stream.id])
    setNotice(exists ? 'Removed from your saved streams.' : 'Saved for later.')
  }
  const sendChat = () => {
    const message = chatMessage.trim()
    if (!message) return
    setChatMessages((current) => [...current, { name: 'You', message, role: 'member' }])
    setChatMessage('')
  }
  const installApp = async () => {
    if (!installPrompt) return
    await installPrompt.prompt()
    const choice = await installPrompt.userChoice
    if (choice.outcome === 'accepted') setInstallState('installed')
    setInstallPrompt(null)
  }
  const selectView = (next: View) => {
    setView(next)
    setSelectedStream(null)
    setMobileNavOpen(false)
    setActiveCategory('For you')
    setQuery('')
  }
  const currentTitle = view === 'following' ? 'Following' : view === 'saved' ? 'Saved streams' : view === 'discover' ? 'Discover' : 'Home'

  return (
    <div className={'app-shell' + (collapsed ? ' sidebar-collapsed' : '')}>
      <aside className={'sidebar' + (mobileNavOpen ? ' sidebar-open' : '')} aria-label="Main navigation">
        <div className="sidebar-top">
          <button className="brand" onClick={() => selectView('home')} aria-label="LIVE_TRON home">
            <span className="brand-symbol"><span /></span><span className="brand-word">live<span>_</span>tron</span>
          </button>
          <button className="collapse-button" onClick={() => setCollapsed((value) => !value)} aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'} title={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}><ChevronLeft weight="regular" size={17} /></button>
          <button className="mobile-close" onClick={() => setMobileNavOpen(false)} aria-label="Close navigation"><X weight="regular" size={19} /></button>
        </div>
        <nav className="primary-nav" aria-label="Main">
          <button className={view === 'home' && !selectedStream ? 'nav-link active' : 'nav-link'} onClick={() => selectView('home')} title="Home"><Home weight="regular" size={19} /><span>Home</span></button>
          <button className={view === 'discover' ? 'nav-link active' : 'nav-link'} onClick={() => selectView('discover')} title="Discover"><Compass weight="regular" size={19} /><span>Discover</span></button>
          <button className={view === 'following' ? 'nav-link active' : 'nav-link'} onClick={() => selectView('following')} title="Following"><Users weight="regular" size={19} /><span>Following</span><span className="nav-count">{following.length}</span></button>
          <button className={view === 'saved' ? 'nav-link active' : 'nav-link'} onClick={() => selectView('saved')} title="Saved"><Bookmark weight="regular" size={19} /><span>Saved</span><span className="nav-count">{saved.length}</span></button>
        </nav>
        <div className="sidebar-rule" />
        <div className="following-heading"><span>People you follow</span><span>{followedCreators.length}</span></div>
        <div className="following-list">
          {followedCreators.length ? followedCreators.map((creator) => {
            const current = liveStreams.find((stream) => stream.creatorId === creator.id)
            return <button className="following-person" key={creator.id} onClick={() => current ? openStream(current) : selectView('following')} title={creator.name + (current ? ', live now' : '')}>
              <CreatorAvatar src={creator.avatar} name={creator.name} size="small" live={Boolean(current)} />
              <span className="following-person-copy"><strong>{creator.name}</strong><small>{current ? current.title : 'Not live right now'}</small></span>
            </button>
          }) : <p className="sidebar-empty">Creators you follow will show up here.</p>}
        </div>
        <div className="sidebar-bottom">
          <button className="go-live-button" onClick={() => setNotice('Creator Studio is not connected yet. Your stream setup will live here.')}><Video weight="regular" size={18} /><span>Go live</span></button>
          <div className="sidebar-profile">
            <CreatorAvatar src="https://images.unsplash.com/photo-1500648767791-00dcc994a43e?w=160&h=160&fit=crop&crop=faces&auto=format&q=85" name="Guest account" />
            <span className="profile-label"><strong>Guest account</strong><small>Preview profile</small></span>
            <button onClick={() => setNotice('Account settings will be available when sign-in is connected.')} aria-label="Profile settings"><ChevronDown weight="regular" size={16} /></button>
          </div>
        </div>
      </aside>
      {mobileNavOpen && <button className="mobile-scrim" onClick={() => setMobileNavOpen(false)} aria-label="Close menu" />}

      <div className="main-column">
        <header className="topbar">
          <div className="topbar-page">
            <button className="mobile-menu-button" onClick={() => setMobileNavOpen(true)} aria-label="Open navigation"><Menu weight="regular" size={21} /></button>
            {selectedStream ? <><button className="topbar-back" onClick={() => setSelectedStream(null)}><ArrowLeft weight="regular" size={16} /> Back</button><span className="topbar-divider" /><span className="topbar-title">Watching</span></> : view !== 'home' ? <span className="topbar-title">{currentTitle}</span> : null}
          </div>
          <label className="search-box"><Search weight="regular" size={18} /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search people, streams, topics" aria-label="Search people, streams, topics" /><button type="button" className="search-shortcut" onClick={() => setPaletteOpen(true)}><kbd>Ctrl K</kbd></button></label>
          <div className="topbar-tools">
            <button className="tool-button" aria-label="Notifications" onClick={() => setNotice('You are all caught up.')}><Bell weight="regular" size={19} /><span className="notification-count">2</span></button>
            <button className="tool-button theme-toggle" aria-label={theme === 'dark' ? 'Switch to light theme' : 'Switch to dark theme'} onClick={() => setTheme((value) => value === 'dark' ? 'light' : 'dark')}>{theme === 'dark' ? <Sun weight="regular" size={19} /> : <Moon weight="regular" size={19} />}</button>
            <button className="profile-menu" onClick={() => setNotice('Your account menu will be available after sign-in is connected.')} aria-label="Open profile menu"><img src="https://images.unsplash.com/photo-1500648767791-00dcc994a43e?w=80&h=80&fit=crop&crop=faces&auto=format&q=85" alt="" /></button>
          </div>
        </header>

        {selectedStream ? (
          <main className="watch-page">
            <div className="watch-layout">
              <section className="watch-main">
                <div className="watch-poster">
                  <img src={selectedStream.image} alt={'Preview image for ' + selectedStream.title} />
                  <div className="watch-poster-shade" />
                  <div className="watch-poster-top">{selectedStream.status === 'live' ? <span className="live-label"><i /> Live</span> : <span className="replay-label">{selectedStream.status === 'ended' ? 'Replay' : 'Scheduled'}</span>}{selectedStream.status === 'live' && <span className="watch-viewers"><Users weight="regular" size={15} /> {formatViewers(selectedStream.viewers)} watching</span>}</div>
                  <div className="poster-message"><span className="poster-mark"><Radio weight="regular" size={22} /></span><strong>Stream preview</strong><span>Live playback will appear here when the media service is connected.</span></div>
                  <div className="watch-poster-bottom"><span>LIVE_TRON PLAYER</span><span>Preview image · not a live feed</span></div>
                </div>
                <div className="watch-heading-row"><div><p className="watch-category">{selectedStream.category} <span>·</span> {selectedStream.status === 'live' ? 'Live now' : selectedStream.status === 'scheduled' ? selectedStream.startsAt : 'Replay'}</p><h1>{selectedStream.title}</h1></div><button className={'save-control' + (saved.includes(selectedStream.id) ? ' is-saved' : '')} onClick={() => toggleSave(selectedStream)} aria-label={saved.includes(selectedStream.id) ? 'Remove from saved' : 'Save stream'}>{saved.includes(selectedStream.id) ? <BookmarkCheck weight="regular" size={21} /> : <Bookmark weight="regular" size={21} />}</button></div>
                <p className="watch-description">{selectedStream.description}</p>
                <div className="channel-row"><CreatorAvatar src={selectedStream.avatarUrl} name={selectedStream.creator} size="large" live={selectedStream.status === 'live'} /><div className="channel-copy"><strong>{selectedStream.creator} {selectedStream.verified && <ShieldCheck weight="regular" size={16} />}</strong><span>{selectedStream.handle}</span></div><button className={following.includes(selectedStream.creatorId) ? 'follow-button is-following' : 'follow-button'} onClick={() => toggleFollow(selectedStream.creatorId)}>{following.includes(selectedStream.creatorId) ? <><Check weight="regular" size={16} /> Following</> : 'Follow'}</button></div>
                <div className="about-stream"><strong>About this stream</strong><p>{selectedStream.description}</p></div>
              </section>
              <aside className={'chat-panel' + (chatOpen ? ' chat-panel-open' : '') + (chatExpanded ? ' chat-expanded' : '')}>
                <div className="chat-header" onTouchStart={(event) => { touchStartY.current = event.touches[0]?.clientY ?? null }} onTouchEnd={(event) => { if (touchStartY.current === null) return; const delta = touchStartY.current - (event.changedTouches[0]?.clientY ?? touchStartY.current); if (delta > 35) setChatExpanded(true); if (delta < -35) setChatExpanded(false); touchStartY.current = null }}><span className="chat-handle" /><div><MessageCircle weight="regular" size={18} /><strong>Live chat</strong></div><span>Community room</span><button className="chat-close" onClick={() => setChatOpen(false)} aria-label="Close chat"><X weight="regular" size={18} /></button></div>
                <div className="pinned-message"><span>Pinned by the creator</span><p>Be kind to each other. Make room for new people joining the conversation.</p></div>
                <div className="chat-feed" aria-live="polite">{chatMessages.map((item, index) => <div className={'chat-message' + (item.role === 'creator' ? ' from-creator' : '')} key={item.name + '-' + index}><strong>{item.name}{item.role === 'creator' ? <span className="creator-badge">Creator</span> : item.role === 'moderator' ? <span className="moderator-badge">Moderator</span> : null}</strong><p>{item.message}</p></div>)}</div>
                <button className="jump-button" onClick={() => document.querySelector('.chat-feed')?.scrollTo({ top: 99999, behavior: 'smooth' })}>New messages <ChevronDown weight="regular" size={14} /></button>
                <button className={reactionSent ? 'reaction-button reacted' : 'reaction-button'} onClick={() => setReactionSent((value) => !value)}><Heart weight="regular" size={17} fill={reactionSent ? 'currentColor' : 'none'} />{reactionSent ? 'Reaction sent' : 'Send a reaction'}</button>
                <form className="chat-compose" onSubmit={(event) => { event.preventDefault(); sendChat() }}><input value={chatMessage} onChange={(event) => setChatMessage(event.target.value)} placeholder="Write a message" aria-label="Write a chat message" /><button type="submit" disabled={!chatMessage.trim()} aria-label="Send message"><Send weight="regular" size={17} /></button></form>
                <p className="chat-note">Guest preview · messages stay on this page</p>
              </aside>
              <button className="mobile-chat-toggle" onClick={() => setChatOpen((value) => !value)}><MessageCircle weight="regular" size={17} />{chatOpen ? 'Hide chat' : 'Open live chat'}</button>
            </div>
          </main>
        ) : (
          <main className="content-wrap">
            <section className="greeting-row"><div><h1>{greeting}{view === 'home' ? '.' : ', ' + currentTitle.toLowerCase() + '.'}</h1><p>{view === 'home' ? followedLive.length + ' people you follow are live right now.' : view === 'following' ? 'The creators you follow, all in one place.' : view === 'saved' ? 'Streams you kept for another time.' : 'Find a room that feels like your kind of place.'}</p></div></section>

            {view === 'home' && <>
              <section className="featured-section">
                <div className="section-title-row"><div><h2>Come on in</h2><p>There’s something happening right now.</p></div><span className="live-summary"><i /> {liveStreams.length} live rooms</span></div>
                <article className="featured-live" onMouseEnter={() => setHeroPaused(true)} onMouseLeave={() => setHeroPaused(false)}>
                  <img className="featured-image" src={featured.image} alt="" />
                  <div className="featured-scrim" />
                  <div className="featured-caption">
                    <div className="featured-creator-line"><CreatorAvatar src={featured.avatarUrl} name={featured.creator} size="large" live /><strong>{featured.creator}</strong>{featured.verified && <ShieldCheck weight="regular" size={17} />}</div>
                    <h2>{featured.title}</h2><p>{featured.description}</p>
                    <div className="featured-foot"><span className="featured-viewers"><Users weight="regular" size={16} /> {formatViewers(featured.viewers)} watching</span><div><button className="primary-button" onClick={() => openStream(featured)}>Watch live</button><button className="outline-button" onClick={() => toggleSave(featured)}>{saved.includes(featured.id) ? <BookmarkCheck weight="regular" size={16} /> : <Bookmark weight="regular" size={16} />} Remind me later</button></div></div>
                  </div>
                  <div className="hero-arrows"><button onClick={() => setHeroIndex((current) => (current - 1 + featuredStreams.length) % featuredStreams.length)} aria-label="Previous featured stream"><ChevronLeft weight="regular" size={18} /></button><button onClick={() => setHeroIndex((current) => (current + 1) % featuredStreams.length)} aria-label="Next featured stream"><ChevronRight weight="regular" size={18} /></button></div>
                </article>
                <div className="featured-switcher">{featuredStreams.slice(0, 4).map((stream, index) => <button key={stream.id} className={heroIndex % featuredStreams.length === index ? 'featured-thumb selected' : 'featured-thumb'} onClick={() => setHeroIndex(index)}><img src={stream.image} alt="" /><span><strong>{stream.creator}</strong><small>{stream.title}</small></span></button>)}</div>
              </section>

              <section className="live-people-section">
                <div className="section-title-row"><div><h2>Live now</h2><p>People you follow are around.</p></div><button className="text-link" onClick={() => selectView('following')}>See everyone <ChevronRight weight="regular" size={16} /></button></div>
                {followedLive.length ? <div className="live-people-row">{followedLive.map((stream) => <button className="live-person" key={stream.creatorId} onClick={() => openStream(stream)}><CreatorAvatar src={stream.avatarUrl} name={stream.creator} size="large" live /><strong>{stream.creator.split(' ')[0]}</strong><span>{stream.category}</span></button>)}</div> : <div className="inline-empty"><span>Follow a creator and you’ll see them here when they go live.</span><button onClick={() => selectView('discover')}>Find people to follow</button></div>}
              </section>
            </>}

            <section className="explore-section">
              <div className="section-title-row"><div><h2>{view === 'following' ? 'Your people' : view === 'saved' ? 'Saved for later' : view === 'discover' ? 'Find your people' : 'Explore'}</h2><p>{view === 'following' ? 'Pick up where your favourite creators left off.' : view === 'saved' ? 'Your list is ready whenever you are.' : 'Choose a topic or see what catches your eye.'}</p></div>
                <label className="sort-select"><SlidersHorizontal weight="regular" size={16} /><select value={sort} onChange={(event) => setSort(event.target.value)} aria-label="Sort streams"><option value="recommended">Recommended</option><option value="viewers">Most viewers</option><option value="newest">Recently added</option></select><ChevronDown weight="regular" size={15} /></label>
              </div>
              {view !== 'following' && view !== 'saved' && <div className="category-chips" role="tablist" aria-label="Stream topics">{categories.map((category) => { const Icon = categoryIcons[category]; return <button key={category} role="tab" aria-selected={activeCategory === category} className={activeCategory === category ? 'category-chip active' : 'category-chip'} onClick={() => setActiveCategory(category)}><Icon weight="regular" size={16} />{category}</button> })}</div>}
              {query && <p className="search-result-count">{filteredStreams.length} results for “{query}”</p>}
              {filteredStreams.length ? <div className="stream-grid">{filteredStreams.map((stream, index) => <StreamCard key={stream.id} stream={stream} index={index} saved={saved.includes(stream.id)} onOpen={openStream} onSave={toggleSave} onNotify={(item) => setNotice('Reminder set for ' + item.creator + ' at ' + item.startsAt + '.')} />)}</div> : <div className="empty-state"><span className="empty-icon">{view === 'saved' ? <Bookmark weight="regular" size={23} /> : view === 'following' ? <Users weight="regular" size={23} /> : <Search weight="regular" size={23} />}</span><h3>{view === 'saved' ? 'Nothing saved yet' : view === 'following' ? 'Your people will show up here' : 'No streams found'}</h3><p>{view === 'saved' ? 'Save a stream and it’ll be waiting here when you come back.' : view === 'following' ? 'Follow a few creators and their live rooms will be easy to find.' : 'Try another search or choose a different topic.'}</p><button className="secondary-button" onClick={() => { setQuery(''); setActiveCategory('For you'); if (view === 'following' || view === 'saved') selectView('discover') }}>{view === 'following' || view === 'saved' ? 'Discover creators' : 'Clear filters'}</button></div>}
            </section>
            <footer className="page-footer"><span>live_tron</span><div><button onClick={() => setNotice('Community guidelines are being prepared.')}>Community guidelines</button><button onClick={() => setNotice('Help centre is being prepared.')}>Help</button><button onClick={() => setNotice('Language settings will be available with account preferences.')}>English</button></div><span>Preview build · 0.2.0</span></footer>
          </main>
        )}
      </div>

      <nav className="mobile-tabbar" aria-label="Mobile navigation">
        <button className={view === 'home' ? 'mobile-tab active' : 'mobile-tab'} onClick={() => selectView('home')}><Home weight="regular" size={20} /><span>Home</span></button>
        <button className={view === 'discover' ? 'mobile-tab active' : 'mobile-tab'} onClick={() => selectView('discover')}><Compass weight="regular" size={20} /><span>Discover</span></button>
        <button className="mobile-go-live" onClick={() => setNotice('Creator Studio is not connected yet. Your stream setup will live here.')}><Video weight="regular" size={20} /><span>Go live</span></button>
        <button className={view === 'following' ? 'mobile-tab active' : 'mobile-tab'} onClick={() => selectView('following')}><Users weight="regular" size={20} /><span>Following</span></button>
        <button className="mobile-tab" onClick={() => setNotice('Your guest profile is a preview until sign-in is connected.')}><CircleHelp weight="regular" size={20} /><span>You</span></button>
      </nav>

      {paletteOpen && <div className="palette-backdrop" onMouseDown={(event) => { if (event.target === event.currentTarget) setPaletteOpen(false) }}><section className="command-palette" role="dialog" aria-modal="true" aria-label="Search LIVE_TRON">
        <div className="palette-input"><Search weight="regular" size={19} /><input autoFocus value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search people, streams, topics" /><kbd>ESC</kbd><button onClick={() => setPaletteOpen(false)} aria-label="Close search"><X weight="regular" size={18} /></button></div>
        <p className="palette-heading">Jump to</p>
        <button className="palette-option" onClick={() => { selectView('home'); setPaletteOpen(false) }}><Home weight="regular" size={17} />Home<small>Open page</small></button>
        <button className="palette-option" onClick={() => { selectView('discover'); setPaletteOpen(false) }}><Compass weight="regular" size={17} />Discover<small>Open page</small></button>
        <button className="palette-option" onClick={() => { selectView('following'); setPaletteOpen(false) }}><Users weight="regular" size={17} />Following<small>Open page</small></button>
        <button className="palette-option" onClick={() => { selectView('saved'); setPaletteOpen(false) }}><Bookmark weight="regular" size={17} />Saved<small>Open page</small></button>
        <p className="palette-heading">Live right now</p>
        {liveStreams.slice(0, 3).map((stream) => <button className="palette-option" key={stream.id} onClick={() => { openStream(stream); setPaletteOpen(false) }}><CreatorAvatar src={stream.avatarUrl} name={stream.creator} size="small" live /><span>{stream.creator}</span><small>{stream.title}</small></button>)}
      </section></div>}

      {installState === 'available' && <div className="install-banner"><div><strong>Keep LIVE_TRON close</strong><span>Install the app on this device.</span></div><button onClick={installApp}>Install</button><button onClick={() => setInstallState('hidden')} aria-label="Dismiss install prompt"><X weight="regular" size={17} /></button></div>}
      {notice && <div className="toast" role="status"><span>{notice}</span><button onClick={() => setNotice('')} aria-label="Dismiss notification"><X weight="regular" size={16} /></button></div>}
    </div>
  )
}

export default App
