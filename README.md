# LIVE_TRON

A live streaming platform for people and communities across gaming, music, talk, education, sports, IRL, and live commerce.

> Current release: **0.2.0 · Human-first discovery preview**
>
> The interface uses seeded sample streams and remote editorial photography. No live media is being ingested or delivered yet. Chat, follows, saves, reminders, and account controls shown in this build are local preview interactions.

## Product direction

LIVE_TRON should feel like walking into a lively room, not browsing a catalogue. The interface puts creators and their activity ahead of thumbnails, with calm typography, warm surfaces, restrained motion, and a single ember accent.

- Responsive web application with an installable Progressive Web App (PWA).
- A human-first home feed, featured live room, live creator row, topic filters, and varied stream cards.
- A dedicated watch layout with player state, creator information, and a local chat preview.
- Dark and light themes, following and saved views, command search, and keyboard-accessible navigation.
- No simulated live playback. The player clearly identifies itself as a preview until the media plane is connected.

## Visual system

| Token | Dark theme | Light theme |
| --- | --- | --- |
| Background | `#12100F` | `#FAF6F1` |
| Surface | `#1B1816` | `#FFFFFF` |
| Raised surface | `#25211E` | `#F3EEE8` |
| Border | `#322D29` | `#E4DCD3` |
| Primary text | `#F4EFE9` | `#1F1A17` |
| Secondary text | `#B3A99F` | `#625950` |
| Accent | `#FF6A45` | `#D94E2D` |

Display type uses Bricolage Grotesque; interface text uses DM Sans. Cards and media use a 14px radius, controls use 10px, and the layout follows an 8px spacing rhythm.

## Current implementation

- React 19, TypeScript, and Vite 6.
- Warm, responsive application shell with a collapsible desktop sidebar, tablet icon rail, and mobile bottom navigation.
- Human-first home page with greeting, featured live story, manual and timed story switching, and followed creator avatars.
- Twenty-four seeded streams across eight categories and twelve creator profiles with photo avatars.
- Mixed-width discovery cards, category filters, search, sorting, save/follow interactions, and empty states.
- Watch preview layout with creator details, follow action, pinned chat note, local messages, and a mobile chat sheet.
- Dark/light theme toggle, system-preference default, command palette shortcut, PWA install prompt, and offline shell.
- Sample photos are requested from Unsplash at runtime and therefore require a network connection.

## Run locally

Requirements: Node.js 22+ and npm.

```bash
npm install
npm run dev
```

Open the URL printed by Vite (normally http://localhost:5173).

Production validation:

```bash
npm run build
npm run preview
```

## Production architecture plan

1. **Web client / PWA** — React, TypeScript, accessible responsive UI, service worker, install/update lifecycle.
2. **Application API** — authentication, profiles, channels, follows, discovery metadata, moderation, and creator settings.
3. **Media plane** — WebRTC ingest and playback through WHIP/WHEP-compatible endpoints and a real SFU router.
4. **Realtime plane** — authenticated chat, presence, reactions, polls, and event delivery.
5. **Persistence** — PostgreSQL for durable application data; Redis for ephemeral presence, rate limits, and fanout where justified.
6. **Operations** — metrics, structured logs, abuse controls, TURN capacity, regional routing, and media quality telemetry.

### Media engineering guardrails

- Do not advertise latency or quality targets until measured on real devices and networks.
- Do not expose stream secrets in browser bundles or public channel metadata.
- Authenticate publish permissions separately from viewer access.
- Validate origin, ownership, room membership, rate limits, and moderation permissions server-side.
- A working WHIP/WHEP negotiation path must be connected to an actual SFU track router before calling the platform live.
- Plan TURN from the beginning; direct peer connectivity is not reliable on every mobile or carrier network.

## Roadmap

- [x] Product identity and responsive visual foundation
- [x] Human-first discovery preview and 24-stream seed dataset
- [x] Watch page preview, theme toggle, command search, and responsive navigation
- [x] PWA manifest, icon, install prompt, offline shell
- [ ] Account system and creator/channel profiles
- [ ] API contract, database schema, migrations, and seed data
- [ ] Creator Studio and secure stream-key lifecycle
- [ ] WebRTC ingest, SFU routing, and viewer playback
- [ ] Authenticated realtime chat, moderation, and presence
- [ ] Creator analytics, reporting, and production deployment

Features marked preview/local are not backed by a production API. The next engineering work should connect the discovery surface to persistent account and stream data, then connect the watch page to the real media and chat services.
