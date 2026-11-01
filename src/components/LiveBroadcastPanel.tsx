import { useEffect, useRef, useState } from 'react'
import './LiveBroadcastPanel.css'

type BroadcastStatus = 'idle' | 'requesting' | 'connecting' | 'live' | 'stopping' | 'error'

type LiveBroadcastPanelProps = {
  streamId: string
  publishingKey: string
  mediaServerUrl?: string
  iceServers?: RTCIceServer[]
  onStopped?: () => void
}

const STREAM_ID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i
const ICE_GATHERING_TIMEOUT_MS = 15_000

function waitForIceGatheringComplete(peer: RTCPeerConnection): Promise<void> {
  if (peer.iceGatheringState === 'complete') return Promise.resolve()

  return new Promise((resolve, reject) => {
    const timeout = window.setTimeout(() => {
      peer.removeEventListener('icegatheringstatechange', onStateChange)
      reject(new Error('ICE gathering timed out. Check STUN/TURN and firewall configuration.'))
    }, ICE_GATHERING_TIMEOUT_MS)

    const onStateChange = () => {
      if (peer.iceGatheringState !== 'complete') return
      window.clearTimeout(timeout)
      peer.removeEventListener('icegatheringstatechange', onStateChange)
      resolve()
    }

    peer.addEventListener('icegatheringstatechange', onStateChange)
  })
}

function resolveWhipEndpoint(baseUrl: string, streamId: string): URL {
  const origin = new URL(baseUrl.trim())
  const localHost = ['localhost', '127.0.0.1', '[::1]'].includes(origin.hostname)
  if (origin.protocol !== 'https:' && !(origin.protocol === 'http:' && localHost)) {
    throw new Error('Broadcasting requires HTTPS outside localhost development.')
  }
  if (origin.username || origin.password || origin.search || origin.hash) {
    throw new Error('The MediaMTX URL must be a clean origin without credentials or query parameters.')
  }
  origin.pathname = `/${encodeURIComponent(streamId)}/whip`
  return origin
}

export default function LiveBroadcastPanel({
  streamId,
  publishingKey,
  mediaServerUrl,
  iceServers,
  onStopped,
}: LiveBroadcastPanelProps) {
  const previewRef = useRef<HTMLVideoElement>(null)
  const peerRef = useRef<RTCPeerConnection | null>(null)
  const streamRef = useRef<MediaStream | null>(null)
  const sessionRef = useRef<URL | null>(null)
  const requestRef = useRef<AbortController | null>(null)
  const activeRef = useRef(false)
  const [status, setStatus] = useState<BroadcastStatus>('idle')
  const [error, setError] = useState('')
  const [cameraEnabled, setCameraEnabled] = useState(true)
  const [microphoneEnabled, setMicrophoneEnabled] = useState(true)

  const baseUrl = mediaServerUrl ?? import.meta.env.VITE_MEDIAMTX_WHEP_BASE_URL ?? ''
  const stunList = String(import.meta.env.VITE_MEDIAMTX_STUN_URLS || '')
  const configuredIceServers = iceServers ?? stunList.split(',').map((url) => url.trim()).filter(Boolean).map((url) => ({ urls: url }))

  const stopLocalMedia = () => {
    activeRef.current = false
    requestRef.current?.abort()
    requestRef.current = null
    streamRef.current?.getTracks().forEach((track) => track.stop())
    streamRef.current = null
    if (previewRef.current) previewRef.current.srcObject = null
    peerRef.current?.close()
    peerRef.current = null
  }

  const deleteSession = async () => {
    const resource = sessionRef.current
    sessionRef.current = null
    if (!resource) return
    try {
      await fetch(resource, {
        method: 'DELETE',
        headers: { Authorization: `Bearer ${publishingKey}` },
        keepalive: true,
      })
    } catch {
      // MediaMTX expires abandoned sessions; local teardown must still complete.
    }
  }

  const stopBroadcast = async (notify = true) => {
    setStatus('stopping')
    await deleteSession()
    stopLocalMedia()
    setStatus('idle')
    setError('')
    if (notify) onStopped?.()
  }

  const startBroadcast = async () => {
    if (activeRef.current || status === 'requesting' || status === 'connecting') return
    setError('')
    let controller: AbortController | null = null

    try {
      if (!STREAM_ID_PATTERN.test(streamId)) throw new Error('A canonical LIVE_TRON stream ID is required.')
      if (!publishingKey.trim()) throw new Error('A publishing key is required. Create the stream through the authenticated creator API first.')
      if (!baseUrl.trim()) throw new Error('Media ingest is not configured. Set the MediaMTX base URL before broadcasting.')

      const endpoint = resolveWhipEndpoint(baseUrl, streamId)
      controller = new AbortController()
      requestRef.current = controller
      setStatus('requesting')
      const localStream = await navigator.mediaDevices.getUserMedia({
        audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true },
        video: { width: { ideal: 1280 }, height: { ideal: 720 }, frameRate: { ideal: 30, max: 30 } },
      })
      streamRef.current = localStream
      if (previewRef.current) previewRef.current.srcObject = localStream

      const peer = new RTCPeerConnection({ iceServers: configuredIceServers })
      peerRef.current = peer
      localStream.getTracks().forEach((track) => peer.addTrack(track, localStream))
      peer.addEventListener('connectionstatechange', () => {
        if (peer.connectionState === 'connected' && activeRef.current) setStatus('live')
        if (peer.connectionState === 'failed' && activeRef.current) {
          activeRef.current = false
          setError('WebRTC connection failed. Check TURN credentials, UDP reachability, and MediaMTX ICE settings.')
          void deleteSession().finally(() => {
            stopLocalMedia()
            setStatus('error')
          })
        }
      })

      setStatus('connecting')
      const offer = await peer.createOffer()
      await peer.setLocalDescription(offer)
      await waitForIceGatheringComplete(peer)
      if (controller.signal.aborted || peerRef.current !== peer) throw new Error('Broadcast start was cancelled.')
      const localSdp = peer.localDescription?.sdp
      if (!localSdp) throw new Error('The browser did not produce a WebRTC offer.')

      const response = await fetch(endpoint, {
        method: 'POST',
        headers: {
          Accept: 'application/sdp',
          'Content-Type': 'application/sdp',
          Authorization: `Bearer ${publishingKey}`,
        },
        body: localSdp,
        signal: controller.signal,
      })

      if (response.status === 401 || response.status === 403) {
        throw new Error('MediaMTX rejected the publishing key. Rotate the key or verify its authorization callback.')
      }
      if (response.status === 409) throw new Error('This stream already has an active publisher or is not ready to ingest.')
      if (response.status !== 201 && response.status !== 200) {
        throw new Error(`MediaMTX returned HTTP ${response.status} while opening the broadcast session.`)
      }

      const location = response.headers.get('Location')
      if (!location) throw new Error('MediaMTX accepted the offer but did not return a WHIP session Location.')
      const sessionUrl = new URL(location, endpoint)
      if (sessionUrl.origin !== endpoint.origin) throw new Error('MediaMTX returned a cross-origin session URL; refusing to send the publishing key to another host.')
      sessionRef.current = sessionUrl
      if (controller.signal.aborted || peerRef.current !== peer) {
        await deleteSession()
        throw new Error('Broadcast start was cancelled.')
      }

      const answer = await response.text()
      if (!answer.trim()) throw new Error('MediaMTX returned an empty SDP answer.')
      await peer.setRemoteDescription({ type: 'answer', sdp: answer })
      if (controller.signal.aborted || peerRef.current !== peer) throw new Error('Broadcast start was cancelled.')
      requestRef.current = null
      activeRef.current = true
      setStatus(peer.connectionState === 'connected' ? 'live' : 'connecting')
    } catch (cause) {
      if (controller?.signal.aborted) return
      await deleteSession()
      stopLocalMedia()
      setStatus('error')
      if (cause instanceof DOMException && cause.name === 'NotAllowedError') {
        setError('Camera or microphone permission was denied. Allow access in the browser and retry.')
      } else if (cause instanceof DOMException && cause.name === 'NotFoundError') {
        setError('No camera or microphone was found on this device.')
      } else {
        setError(cause instanceof Error ? cause.message : 'Unable to start the live broadcast.')
      }
    }
  }

  useEffect(() => () => {
    void deleteSession()
    stopLocalMedia()
  }, [])

  const setTrackEnabled = (kind: 'audio' | 'video', enabled: boolean) => {
    streamRef.current?.getTracks().filter((track) => track.kind === kind).forEach((track) => { track.enabled = enabled })
    if (kind === 'audio') setMicrophoneEnabled(enabled)
    else setCameraEnabled(enabled)
  }

  const broadcasting = status === 'live' || status === 'connecting'

  return (
    <section className="broadcast-panel" aria-label="Live broadcast controls">
      <div className="broadcast-preview">
        <video ref={previewRef} autoPlay muted playsInline aria-label="Local camera preview" />
        {!streamRef.current && <div className="broadcast-preview-empty"><span /> CAMERA PREVIEW</div>}
        <span className={broadcasting ? 'broadcast-state is-live' : 'broadcast-state'}>{broadcasting ? 'ON AIR' : status === 'connecting' ? 'CONNECTING' : 'STANDBY'}</span>
      </div>
      <div className="broadcast-controls">
        <div className="broadcast-heading">
          <div><span className="broadcast-eyebrow">CREATOR STUDIO</span><h2>Go live</h2></div>
          <span className="broadcast-stream-id">{streamId.slice(0, 8)}</span>
        </div>
        {error && <p className="broadcast-error" role="alert">{error}</p>}
        <div className="broadcast-actions">
          <button type="button" onClick={() => setTrackEnabled('video', !cameraEnabled)} disabled={!streamRef.current}>
            {cameraEnabled ? 'Camera on' : 'Camera off'}
          </button>
          <button type="button" onClick={() => setTrackEnabled('audio', !microphoneEnabled)} disabled={!streamRef.current}>
            {microphoneEnabled ? 'Mic on' : 'Mic off'}
          </button>
          {broadcasting ? (
            <button type="button" className="broadcast-stop" onClick={() => void stopBroadcast()}>End broadcast</button>
          ) : (
            <button type="button" className="broadcast-start" onClick={() => void startBroadcast()} disabled={status === 'requesting' || status === 'stopping'}>
              {status === 'requesting' ? 'Requesting devices…' : status === 'connecting' ? 'Connecting…' : status === 'stopping' ? 'Stopping…' : status === 'error' ? 'Retry broadcast' : 'Start broadcast'}
            </button>
          )}
        </div>
        <p className="broadcast-footnote">Camera and microphone are sent directly to the configured MediaMTX WHIP endpoint. Your publishing key is never placed in the URL.</p>
      </div>
    </section>
  )
}
