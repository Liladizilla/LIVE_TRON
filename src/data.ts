export type Category = 'For you' | 'Gaming' | 'Music' | 'Talk' | 'Education' | 'Sports' | 'IRL' | 'Commerce'

export type Stream = {
  id: string
  title: string
  creator: string
  handle: string
  category: Exclude<Category, 'For you'>
  viewers: number
  accent: string
  image: string
  avatar: string
  duration: string
  verified?: boolean
  featured?: boolean
  description: string
}

export const categories: Category[] = ['For you', 'Gaming', 'Music', 'Talk', 'Education', 'Sports', 'IRL', 'Commerce']

export const streams: Stream[] = [
  { id: 's1', title: 'Building a world from scratch | Unreal Engine 5', creator: 'Mika Studios', handle: '@mikastudios', category: 'Gaming', viewers: 18420, accent: '#a78bfa', image: 'https://images.unsplash.com/photo-1542751371-adc38448a05e?w=1200&auto=format&fit=crop&q=85', avatar: 'MS', duration: '02:41:08', verified: true, featured: true, description: 'A live creative session exploring environments, lighting, and world building.' },
  { id: 's2', title: 'Midnight jazz set · live from the room', creator: 'Nia Sol', handle: '@niasounds', category: 'Music', viewers: 8290, accent: '#e8a87c', image: 'https://images.unsplash.com/photo-1516280440614-37939bbacd81?w=1000&auto=format&fit=crop&q=85', avatar: 'NS', duration: '01:18:32', verified: true, description: 'An intimate late-night set with original arrangements and listener requests.' },
  { id: 's3', title: 'Can we make sense of the AI boom?', creator: 'The Signal Room', handle: '@signalroom', category: 'Talk', viewers: 6124, accent: '#74b9b0', image: 'https://images.unsplash.com/photo-1475721027785-f74eccf877e2?w=1000&auto=format&fit=crop&q=85', avatar: 'SR', duration: '00:54:17', description: 'A thoughtful conversation about technology, work, and what comes next.' },
  { id: 's4', title: 'Learn to draw expressive characters', creator: 'Ari Draws', handle: '@aridraws', category: 'Education', viewers: 2910, accent: '#e5b85c', image: 'https://images.unsplash.com/photo-1513364776144-60967b0f800f?w=1000&auto=format&fit=crop&q=85', avatar: 'AD', duration: '00:36:44', description: 'Practical drawing lessons, from gesture to expressive character design.' },
  { id: 's5', title: 'Match day watchalong · final minutes', creator: 'Touchline', handle: '@touchline', category: 'Sports', viewers: 12750, accent: '#91c77b', image: 'https://images.unsplash.com/photo-1522778119026-d647f0596c20?w=1000&auto=format&fit=crop&q=85', avatar: 'TL', duration: '01:02:19', description: 'Community match discussion and live tactical breakdown.' },
  { id: 's6', title: 'Sunday market finds and small brands', creator: 'Maya On The Move', handle: '@mayaonthemove', category: 'IRL', viewers: 1460, accent: '#d99aab', image: 'https://images.unsplash.com/photo-1483985988355-763728e1935b?w=1000&auto=format&fit=crop&q=85', avatar: 'MM', duration: '00:22:06', description: 'Exploring independent makers and local finds around the city.' },
  { id: 's7', title: 'Studio desk tour + gear Q&A', creator: 'Frame by Frame', handle: '@framebyframe', category: 'Talk', viewers: 987, accent: '#7aa8d8', image: 'https://images.unsplash.com/photo-1498050108023-c5249f4df085?w=1000&auto=format&fit=crop&q=85', avatar: 'FF', duration: '00:48:51', description: 'A practical tour of a compact creator setup and live questions.' },
  { id: 's8', title: 'Sneaker restoration: bringing these back', creator: 'Sole Lab', handle: '@solelab', category: 'Commerce', viewers: 3240, accent: '#c9a77b', image: 'https://images.unsplash.com/photo-1542291026-7eec264c27ff?w=1000&auto=format&fit=crop&q=85', avatar: 'SL', duration: '00:19:40', description: 'Restoration process, product details, and a live workshop.' },
]

export const formatViewers = (count: number) =>
  count >= 1000 ? `${(count / 1000).toFixed(count >= 10000 ? 0 : 1)}K` : String(count)
