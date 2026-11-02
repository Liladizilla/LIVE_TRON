export type Category = 'For you' | 'Gaming' | 'Music' | 'Talk' | 'Education' | 'Sports' | 'IRL' | 'Commerce'
export type StreamStatus = 'live' | 'scheduled' | 'ended'

export type Stream = {
  id: string
  creatorId: string
  title: string
  creator: string
  handle: string
  category: Exclude<Category, 'For you'>
  viewers: number
  image: string
  avatarUrl: string
  duration: string
  verified?: boolean
  featured?: boolean
  description: string
  status: StreamStatus
  startsAt?: string
}

export const categories: Category[] = ['For you', 'Gaming', 'Music', 'Talk', 'Education', 'Sports', 'IRL', 'Commerce']

export const creators = [
  { id: 'mika', name: 'Mika Okafor', handle: '@mika.builds', avatar: 'https://images.unsplash.com/photo-1500648767791-00dcc994a43e?w=160&h=160&fit=crop&crop=faces&auto=format&q=85', verified: true },
  { id: 'nia', name: 'Nia Sol', handle: '@niasounds', avatar: 'https://images.unsplash.com/photo-1534528741775-53994a69daeb?w=160&h=160&fit=crop&crop=faces&auto=format&q=85', verified: true },
  { id: 'leo', name: 'Leo Martin', handle: '@leomakes', avatar: 'https://images.unsplash.com/photo-1506794778202-cad84cf45f1d?w=160&h=160&fit=crop&crop=faces&auto=format&q=85', verified: false },
  { id: 'amara', name: 'Amara Mensah', handle: '@amarainmotion', avatar: 'https://images.unsplash.com/photo-1531123897727-8f129e1688ce?w=160&h=160&fit=crop&crop=faces&auto=format&q=85', verified: true },
  { id: 'jay', name: 'Jay Park', handle: '@jayafterhours', avatar: 'https://images.unsplash.com/photo-1507003211169-0a1dd7228f2d?w=160&h=160&fit=crop&crop=faces&auto=format&q=85', verified: false },
  { id: 'zuri', name: 'Zuri Wanjiku', handle: '@zuricooks', avatar: 'https://images.unsplash.com/photo-1524504388940-b1c1722653e1?w=160&h=160&fit=crop&crop=faces&auto=format&q=85', verified: true },
  { id: 'sam', name: 'Sam Rivera', handle: '@samontheline', avatar: 'https://images.unsplash.com/photo-1535713875002-d1d0cf377fde?w=160&h=160&fit=crop&crop=faces&auto=format&q=85', verified: false },
  { id: 'imani', name: 'Imani Cole', handle: '@imanilearns', avatar: 'https://images.unsplash.com/photo-1544005313-94ddf0286df2?w=160&h=160&fit=crop&crop=faces&auto=format&q=85', verified: true },
  { id: 'theo', name: 'Theo Brooks', handle: '@theobrooks', avatar: 'https://images.unsplash.com/photo-1527980965255-d3b416303d12?w=160&h=160&fit=crop&crop=faces&auto=format&q=85', verified: false },
  { id: 'maya', name: 'Maya Njeri', handle: '@mayaoutside', avatar: 'https://images.unsplash.com/photo-1548142813-c348350df52b?w=160&h=160&fit=crop&crop=faces&auto=format&q=85', verified: true },
  { id: 'eli', name: 'Eli Brooks', handle: '@eliplays', avatar: 'https://images.unsplash.com/photo-1529139574466-a303027c1d8b?w=160&h=160&fit=crop&crop=faces&auto=format&q=85', verified: false },
  { id: 'sana', name: 'Sana Ali', handle: '@sana.studio', avatar: 'https://images.unsplash.com/photo-1521119989659-a83eee488004?w=160&h=160&fit=crop&crop=faces&auto=format&q=85', verified: false },
]

const photo = (id: string, width = 900) => `https://images.unsplash.com/${id}?auto=format&fit=crop&w=${width}&q=85`

export const streams: Stream[] = [
  { id: 's1', creatorId: 'mika', title: 'Building a city that feels lived in', creator: 'Mika Okafor', handle: '@mika.builds', category: 'Gaming', viewers: 18420, image: photo('photo-1542751371-adc38448a05e'), avatarUrl: creators[0].avatar, duration: '02:41:08', verified: true, featured: true, description: 'Working through the tiny details that make a virtual city feel like a real place. Bring a question and stay for the build.', status: 'live' },
  { id: 's2', creatorId: 'nia', title: 'A slow jazz set for the end of the day', creator: 'Nia Sol', handle: '@niasounds', category: 'Music', viewers: 8290, image: photo('photo-1501386761578-eac5c94b800a'), avatarUrl: creators[1].avatar, duration: '01:18:32', verified: true, description: 'Original arrangements, a few old favourites, and room for listener requests.', status: 'live' },
  { id: 's3', creatorId: 'leo', title: 'Small studio, big ideas', creator: 'Leo Martin', handle: '@leomakes', category: 'Talk', viewers: 6124, image: photo('photo-1475721027785-f74eccf877e2'), avatarUrl: creators[2].avatar, duration: '00:54:17', description: 'A relaxed conversation about building things, creative work, and finding a pace that lasts.', status: 'live' },
  { id: 's4', creatorId: 'amara', title: 'Drawing faces that actually feel alive', creator: 'Amara Mensah', handle: '@amarainmotion', category: 'Education', viewers: 2910, image: photo('photo-1513364776144-60967b0f800f'), avatarUrl: creators[3].avatar, duration: '00:36:44', verified: true, description: 'A practical drawing session on gesture, expression, and getting out of your own way.', status: 'live' },
  { id: 's5', creatorId: 'jay', title: 'Match day, last twenty minutes', creator: 'Jay Park', handle: '@jayafterhours', category: 'Sports', viewers: 12750, image: photo('photo-1522778119026-d647f0596c20'), avatarUrl: creators[4].avatar, duration: '01:02:19', description: 'Watching the match together and talking through the decisions as they happen.', status: 'live' },
  { id: 's6', creatorId: 'maya', title: 'Walking the old town before sunset', creator: 'Maya Njeri', handle: '@mayaoutside', category: 'IRL', viewers: 1460, image: photo('photo-1480714378408-67cf0d13bc1b'), avatarUrl: creators[9].avatar, duration: '00:22:06', verified: true, description: 'A slow walk through the market streets, with a few stops to meet the people behind the stalls.', status: 'live' },
  { id: 's7', creatorId: 'sam', title: 'My desk setup, without the expensive stuff', creator: 'Sam Rivera', handle: '@samontheline', category: 'Talk', viewers: 987, image: photo('photo-1498050108023-c5249f4df085'), avatarUrl: creators[6].avatar, duration: '00:48:51', description: 'A practical tour of a small creator setup and an open Q&A about making it work on a budget.', status: 'live' },
  { id: 's8', creatorId: 'zuri', title: 'Dinner from the market, one pan only', creator: 'Zuri Wanjiku', handle: '@zuricooks', category: 'Commerce', viewers: 3240, image: photo('photo-1556911220-bff31c812dba'), avatarUrl: creators[5].avatar, duration: '00:19:40', verified: true, description: 'Cooking with what is in season, sharing the process, and answering kitchen questions.', status: 'live' },
  { id: 's9', creatorId: 'imani', title: 'Learning the basics of motion design', creator: 'Imani Cole', handle: '@imanilearns', category: 'Education', viewers: 1840, image: photo('photo-1516321318423-f06f85e504b3'), avatarUrl: creators[7].avatar, duration: '00:31:14', verified: true, description: 'A beginner-friendly walkthrough of timing, easing, and motion that has a reason to exist.', status: 'live' },
  { id: 's10', creatorId: 'theo', title: 'A quiet night in the rehearsal room', creator: 'Theo Brooks', handle: '@theobrooks', category: 'Music', viewers: 760, image: photo('photo-1516280440614-37939bbacd81'), avatarUrl: creators[8].avatar, duration: '00:44:21', description: 'Trying a few arrangements before the weekend show. Suggestions are welcome.', status: 'live' },
  { id: 's11', creatorId: 'eli', title: 'First run through the new map', creator: 'Eli Brooks', handle: '@eliplays', category: 'Gaming', viewers: 5210, image: photo('photo-1593305841991-05c297ba4575'), avatarUrl: creators[10].avatar, duration: '01:12:03', description: 'Exploring the new map and taking the scenic route, as usual.', status: 'live' },
  { id: 's12', creatorId: 'sana', title: 'Making a small space feel like home', creator: 'Sana Ali', handle: '@sana.studio', category: 'IRL', viewers: 1130, image: photo('photo-1616486338812-3dadae4b4ace'), avatarUrl: creators[11].avatar, duration: '00:28:39', description: 'A calm studio reset with practical ideas for a more comfortable creative space.', status: 'live' },
  { id: 's13', creatorId: 'mika', title: 'Lighting a rainy street in real time', creator: 'Mika Okafor', handle: '@mika.builds', category: 'Gaming', viewers: 2210, image: photo('photo-1519608487953-e999c86e7455'), avatarUrl: creators[0].avatar, duration: '00:42:12', verified: true, description: 'Testing reflections, street lamps, and small environmental details.', status: 'live' },
  { id: 's14', creatorId: 'nia', title: 'Taking requests at the piano', creator: 'Nia Sol', handle: '@niasounds', category: 'Music', viewers: 4020, image: photo('photo-1524368535928-5b5e00ddc76b'), avatarUrl: creators[1].avatar, duration: '00:57:10', verified: true, description: 'An open piano session. Leave a song suggestion in the chat.', status: 'live' },
  { id: 's15', creatorId: 'leo', title: 'What makes a good conversation?', creator: 'Leo Martin', handle: '@leomakes', category: 'Talk', viewers: 643, image: photo('photo-1528605248644-14dd04022da1'), avatarUrl: creators[2].avatar, duration: '00:35:52', description: 'A thoughtful open room about listening, attention, and making better things together.', status: 'live' },
  { id: 's16', creatorId: 'amara', title: 'Watercolour studies from the garden', creator: 'Amara Mensah', handle: '@amarainmotion', category: 'Education', viewers: 922, image: photo('photo-1455390582262-044cdead277a'), avatarUrl: creators[3].avatar, duration: '00:25:09', verified: true, description: 'A gentle study session focused on colour, texture, and observation.', status: 'live' },
  { id: 's17', creatorId: 'jay', title: 'Breaking down the weekend fixtures', creator: 'Jay Park', handle: '@jayafterhours', category: 'Sports', viewers: 1870, image: photo('photo-1508098682722-e99c43a406b2'), avatarUrl: creators[4].avatar, duration: '00:49:50', description: 'Looking at the fixtures, form, and what to keep an eye on this weekend.', status: 'live' },
  { id: 's18', creatorId: 'maya', title: 'Street food and stories from the night market', creator: 'Maya Njeri', handle: '@mayaoutside', category: 'IRL', viewers: 2840, image: photo('photo-1555939594-58d7cb561ad1'), avatarUrl: creators[9].avatar, duration: '00:38:07', verified: true, description: 'Meeting food vendors and finding out how their favourite dishes are made.', status: 'live' },
  { id: 's19', creatorId: 'sam', title: 'How I plan a week of independent work', creator: 'Sam Rivera', handle: '@samontheline', category: 'Talk', viewers: 412, image: photo('photo-1497366754035-f200968a6e72'), avatarUrl: creators[6].avatar, duration: '00:21:33', description: 'A real look at planning work, protecting focus, and leaving room for life.', status: 'live' },
  { id: 's20', creatorId: 'zuri', title: 'A better lunchbox, made together', creator: 'Zuri Wanjiku', handle: '@zuricooks', category: 'Commerce', viewers: 1320, image: photo('photo-1547592180-85f173990554'), avatarUrl: creators[5].avatar, duration: '00:17:48', verified: true, description: 'Simple lunch ideas, useful kitchen tools, and a live cooking Q&A.', status: 'live' },
  { id: 's21', creatorId: 'imani', title: 'A study room for the next 45 minutes', creator: 'Imani Cole', handle: '@imanilearns', category: 'Education', viewers: 2150, image: photo('photo-1434030216411-0b793f4b4173'), avatarUrl: creators[7].avatar, duration: '00:09:34', verified: true, description: 'Quiet company while we work through a short study session together.', status: 'live' },
  { id: 's22', creatorId: 'theo', title: 'Soundcheck before the Friday set', creator: 'Theo Brooks', handle: '@theobrooks', category: 'Music', viewers: 0, image: photo('photo-1506157786151-b8491531f063'), avatarUrl: creators[8].avatar, duration: 'Starts 8:30 pm', description: 'A short soundcheck before tonight’s full set.', status: 'scheduled', startsAt: 'Today, 8:30 pm' },
  { id: 's23', creatorId: 'eli', title: 'Community lobby opens later tonight', creator: 'Eli Brooks', handle: '@eliplays', category: 'Gaming', viewers: 0, image: photo('photo-1598550476439-6847785fcea6'), avatarUrl: creators[10].avatar, duration: 'Starts 9:00 pm', description: 'We will open the lobby for community games after dinner.', status: 'scheduled', startsAt: 'Today, 9:00 pm' },
  { id: 's24', creatorId: 'sana', title: 'A studio tour from last week', creator: 'Sana Ali', handle: '@sana.studio', category: 'IRL', viewers: 0, image: photo('photo-1497366811353-6870744d04b2'), avatarUrl: creators[11].avatar, duration: 'Replay', description: 'A replay from the studio refresh, with notes and answers from the chat.', status: 'ended', verified: false },
]

export const formatViewers = (count: number) =>
  count >= 1000 ? `${(count / 1000).toFixed(count >= 10000 ? 0 : 1)}K` : String(count)
