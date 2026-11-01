import { useEffect, useMemo, useRef, useState } from 'react'
import { fetchIceConfig, fetchMediaReadToken } from '../lib/api'

type PlayerStatus = 'idle' | 'connecting' | 'live' | 'error'

type LiveMediaPlayerProps = {
  streamId: string
  mediaServerUrl?: string
  iceServers?: RTCIceServer[]
}

const STREAM_ID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i
const ICE_GATHERING_TIMEOUT_MS = 15_000

function waitForIceGatheringComplete(peer: RTCPeerConnection, signal: AbortSignal): Promise<void> {
  if (signal.aborted) return Promise.reject(new DOMException('Playback connection was cancelled.', 'AbortError'))
  if (peer.iceGatheringState === 'complete') return Promise.resolve()

  return new Promise((resolve, reject) => {
    const cleanup = () => {
      window.clearTimeout(timeout)
      peer.removeEventListener('icegatheringstatechange', onStateChange)
      signal.removeEventListener('abort', onAbort)
    }
    const timeout = window.setTimeout(() => {
      cleanup()
      reject(new Error('ICE gathering timed out. Check the media server and network configuration.'))
    }, ICE_GATHERING_TIMEOUT_MS)
    const onStateChange = () => {
      if (peer.iceGatheringState !== 'complete') return
      cleanup()
      resolve()
    }
    const onAbort = () => {
      cleanup()
      reject(new DOMException('Playback connection was cancelled.', 'AbortError'))
    }

    peer.addEventListener('icegatheringstatechange', onStateChange)
    signal.addEventListener('abort', onAbort, { once: true })
  })
}

export default function LiveMediaPlayer({ streamId, mediaServerUrl, iceServers }: LiveMediaPlayerProps) {
  const videoRef = useRef<HTMLVideoElement>(null)
  const [status, setStatus] = useState<PlayerStatus>('idle')
  const [error, setError] = useState('')
  const [retryCount, setRetryCount] = useState(0)

  const configuredBaseUrl = mediaServerUrl ?? import.meta.env.VITE_MEDIAMTX_WHEP_BASE_URL ?? ''
  const baseUrl = configuredBaseUrl.trim().replace(/\/+$/, '')
  const stunList: string = String(import.meta.env.VITE_MEDIAMTX_STUN_URLS || '')
  const resolvedIceServers = useMemo(
    () => iceServers || stunList.split(',').map((url) => url.trim()).filter(Boolean).map((url) => ({ urls: url })),
    [iceServers, stunList],
  )

  useEffect(() => {
    let disposed = false
    let sessionUrl: string | null = null
    const requestController = new AbortController()
    const peer = new RTCPeerConnection({ iceServers: resolvedIceServers })
    const receivedStream = new MediaStream()

    const removeSession = () => {
      if (!sessionUrl) return
      const resource = sessionUrl
      sessionUrl = null
      void fetch(resource, { method: 'DELETE', keepalive: true }).catch(() => undefined)
    }

    const fail = (message: string) => {
      if (disposed) return
      setStatus('error')
      setError(message)
    }

    const attachTrack = (event: RTCTrackEvent) => {
      if (disposed) return
      const video = videoRef.current
      if (!video) return
      const stream = event.streams[0]
      if (stream) {
        video.srcObject = stream
      } else {
        receivedStream.addTrack(event.track)
        video.srcObject = receivedStream
      }
      setStatus('live')
      setError('')
      void video.play().catch(() => {
        // Native controls remain available if autoplay policy blocks playback.
      })
    }

    peer.addTransceiver('audio', { direction: 'recvonly' })
    peer.addTransceiver('video', { direction: 'recvonly' })
    peer.addEventListener('track', attachTrack)
    peer.addEventListener('connectionstatechange', () => {
      if (peer.connectionState === 'failed') {
        fail('The WebRTC connection failed. Check TURN, firewall, and ICE settings.')
      } else if (peer.connectionState === 'connected' && !disposed) {
        setStatus('live')
      }
    })

    const connect = async () => {
      if (!STREAM_ID_PATTERN.test(streamId)) {
        fail('This stream does not have a valid LIVE_TRON stream ID yet.')
        return
      }
      if (!baseUrl) {
        fail('Media playback is not configured. Set VITE_MEDIAMTX_WHEP_BASE_URL to the public MediaMTX HTTPS origin.')
        return
      }

      setStatus('connecting')
      setError('')

      try {
        const mediaOrigin = new URL(baseUrl)
        const localDevelopmentHost = ['localhost', '127.0.0.1', '[::1]'].includes(mediaOrigin.hostname)
        if (mediaOrigin.protocol !== 'https:' && !(mediaOrigin.protocol === 'http:' && localDevelopmentHost)) {
          throw new Error('Media playback requires HTTPS outside localhost development.')
        }
        if (mediaOrigin.username || mediaOrigin.password || mediaOrigin.search || mediaOrigin.hash) {
          throw new Error('The MediaMTX base URL must be a clean origin without credentials or query parameters.')
        }
        const endpoint = `${mediaOrigin.origin}/${encodeURIComponent(streamId)}/whep`
        let mediaToken: string | null = null
        try {
          const authenticatedIceServers = await fetchIceConfig(requestController.signal)
          if (!disposed) peer.setConfiguration({ iceServers: authenticatedIceServers })
        } catch (iceError) {
          if (requestController.signal.aborted) throw iceError
          // Anonymous viewers can still attempt direct ICE with the configured STUN fallback.
        }
        try {
          const grant = await fetchMediaReadToken(streamId, requestController.signal)
          mediaToken = grant.token
        } catch (tokenError) {
          if (requestController.signal.aborted) throw tokenError
          // Public and unlisted streams remain readable without an authenticated token.
        }
        if (disposed) return
        const offer = await peer.createOffer()
        await peer.setLocalDescription(offer)
        await waitForIceGatheringComplete(peer, requestController.signal)

        if (disposed) return
        const localSdp = peer.localDescription?.sdp
        if (!localSdp) throw new Error('The browser did not produce a WebRTC offer.')

        const response = await fetch(endpoint, {
          method: 'POST',
          headers: {
            Accept: 'application/sdp',
            'Content-Type': 'application/sdp',
            ...(mediaToken ? { Authorization: `Bearer ${mediaToken}` } : {}),
          },
          body: localSdp,
          signal: requestController.signal,
        })

        if (response.status !== 201 && response.status !== 200) {
          if (response.status === 409) {
            throw new Error('This stream is not online yet. Try again when the creator is broadcasting.')
          }
          if (response.status === 401 || response.status === 403) {
            throw new Error('Media server rejected playback authorization for this stream.')
          }
          throw new Error(`Media server returned HTTP ${response.status} while opening playback.`)
        }

        const location = response.headers.get('Location')
        if (!location) throw new Error('Media server did not return a WHEP session location.')
        const session = new URL(location, endpoint)
        if (session.origin !== mediaOrigin.origin || session.username || session.password) {
          throw new Error('Media server returned an unsafe cross-origin WHEP session URL; refusing to manage that session.')
        }
        sessionUrl = session.toString()

        const contentType = response.headers.get('Content-Type') ?? ''
        if (!contentType.toLowerCase().includes('application/sdp')) {
          throw new Error('Media server returned an unexpected WHEP response type instead of SDP.')
        }
        const answer = await response.text()
        if (disposed) {
          removeSession()
          return
        }
        if (!answer.trim()) throw new Error('Media server returned an empty SDP answer.')

        await peer.setRemoteDescription({ type: 'answer', sdp: answer })
      } catch (cause) {
        removeSession()
        if (cause instanceof Error) fail(cause.message)
        else fail('Unable to connect to this live stream.')
      }
    }

    void connect()

    return () => {
      disposed = true
      requestController.abort()
      peer.removeEventListener('track', attachTrack)
      peer.close()
      const video = videoRef.current
      if (video) {
        video.pause()
        video.srcObject = null
      }
      removeSession()
      receivedStream.getTracks().forEach((track) => track.stop())
    }
  }, [baseUrl, resolvedIceServers, retryCount, streamId])

  return (
    <div className="watch-player live-media-player">
      <video ref={videoRef} autoPlay muted playsInline controls aria-label="LIVE_TRON live stream" />
      {status !== 'live' && (
        <div className="media-player-overlay" role="status" aria-live="polite">
          {status === 'connecting' ? <span className="media-player-indicator" /> : null}
          <strong>{status === 'connecting' ? 'Connecting to live stream' : status === 'error' ? 'Playback unavailable' : 'Preparing player'}</strong>
          {error ? <span>{error}</span> : null}
          {status === 'error' ? (
            <button type="button" onClick={() => setRetryCount((value) => value + 1)}>Retry connection</button>
          ) : null}
        </div>
      )}
      <div className="media-player-status" data-status={status}>
        <span />
        {status === 'live' ? 'LIVE TRANSPORT' : status === 'connecting' ? 'CONNECTING' : 'PLAYER STANDBY'}
      </div>
    </div>
  )
}
