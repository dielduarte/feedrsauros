import type { ArticleRef, Scope } from './routes'

export interface SidebarFeed {
  slug: string
  title: string
  url: string
  site_url: string | null
  unread: number
  last_error: string | null
  /** Client-only: shown while the feed is still being added. */
  pending?: boolean
}

export interface SidebarFolder {
  slug: string
  name: string
  unread: number
  feeds: SidebarFeed[]
}

export interface Sidebar {
  total_unread: number
  total_starred: number
  folders: SidebarFolder[]
  uncategorized: SidebarFeed[]
}

export interface ItemSummary {
  slug: string
  feed_slug: string
  feed_title: string
  url: string | null
  title: string | null
  author: string | null
  summary: string | null
  published_at: string
  /** When feedrsauros stored it; "mark all read" spares anything stored later. */
  fetched_at: string
  read_at: string | null
  starred_at: string | null
}

export interface Item extends ItemSummary {
  content_html: string | null
}

export interface Page {
  items: ItemSummary[]
  next_cursor: string | null
}

export interface Added {
  slug: string
  title: string
  new_items: number
  /** Slug of the folder Jev filed the feed under, if it picked one. */
  ai_folder: string | null
}

/** The API key itself never reaches the browser, only its last characters. */
export interface Settings {
  ai_enabled: boolean
  api_key: { hint: string } | null
}

/** A filter Jev applies to a feed's new articles. */
export interface Rule {
  /** The kind of article, in the reader's words, e.g. "soccer news". */
  condition: string
  action: 'hide' | 'keep_only'
}

export interface ImportReport {
  added: number
  skipped: number
  invalid: string[]
}

export type PollerEvent =
  | { type: 'batch_started'; feeds: number }
  | { type: 'feed_refreshed'; feed: string; new_items: number }
  | { type: 'feed_failed'; feed: string; error: string }
  | { type: 'batch_finished'; health: 'online' | 'offline' }
  | { type: 'resync' }

export interface Renamed {
  slug: string
}

function articleUrl(article: ArticleRef): string {
  return `/api/feeds/${encodeURIComponent(article.feed)}/items/${encodeURIComponent(article.slug)}`
}

export class ApiError extends Error {
  readonly status: number

  constructor(status: number, message: string) {
    super(message)
    this.status = status
  }
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const response = await fetch(path, {
    method,
    headers: body === undefined ? undefined : { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  if (!response.ok) {
    const error = await response.json().catch(() => null)
    throw new ApiError(response.status, error?.error ?? `Request failed (${response.status})`)
  }
  return response.status === 204 ? (undefined as T) : response.json()
}

type ScopeFilter = { feed?: string; folder?: string; starred?: boolean }

function scopeFilter(scope: Scope): ScopeFilter {
  switch (scope.kind) {
    case 'folder':
      return { folder: scope.slug }
    case 'feed':
      return { feed: scope.slug }
    case 'starred':
      return { starred: true }
    case 'all':
    case 'unread':
      return {}
  }
}

export function listsUnreadOnly(scope: Scope, unreadOnly: boolean): boolean {
  return scope.kind === 'unread' || (unreadOnly && scope.kind !== 'starred')
}

export const api = {
  sidebar: () => request<Sidebar>('GET', '/api/sidebar'),
  settings: () => request<Settings>('GET', '/api/settings'),
  saveApiKey: (key: string) => request<void>('PUT', '/api/settings/api-key', { key }),
  removeApiKey: () => request<void>('DELETE', '/api/settings/api-key'),
  setAi: (enabled: boolean) => request<void>('PUT', '/api/settings/ai', { enabled }),
  rules: (feed: string) => request<Rule[]>('GET', `/api/feeds/${encodeURIComponent(feed)}/rules`),
  saveRules: (feed: string, rules: Rule[]) => request<void>('PUT', `/api/feeds/${encodeURIComponent(feed)}/rules`, rules),

  items(scope: Scope, unreadOnly: boolean, cursor: string | null) {
    const query = new URLSearchParams()
    for (const [key, value] of Object.entries(scopeFilter(scope))) query.set(key, String(value))
    if (listsUnreadOnly(scope, unreadOnly)) query.set('unread', 'true')
    if (cursor) query.set('cursor', cursor)
    return request<Page>('GET', `/api/items?${query}`)
  },

  item: (article: ArticleRef) => request<Item>('GET', articleUrl(article)),
  updateItem: (article: ArticleRef, patch: { read?: boolean; starred?: boolean }) =>
    request<void>('PATCH', articleUrl(article), patch),
  markRead: (scope: Scope, seenUntil: string) =>
    request<{ marked: number }>('POST', '/api/items/mark-read', {
      ...scopeFilter(scope),
      seen_until: seenUntil,
    }),

  subscribe: (url: string, folder: string | null) => request<Added>('POST', '/api/feeds', { url, folder }),
  unsubscribe: (slug: string) => request<void>('DELETE', `/api/feeds/${encodeURIComponent(slug)}`),
  moveFeed: (slug: string, folder: string | null, index: number) =>
    request<void>('PUT', `/api/feeds/${encodeURIComponent(slug)}/position`, { folder, index }),
  /** Resolves to the feed's new slug: its URL follows its name. */
  renameFeed: (slug: string, title: string | null) =>
    request<Renamed>('PUT', `/api/feeds/${encodeURIComponent(slug)}/title`, { title }),

  createFolder: (name: string) => request<{ slug: string; name: string }>('POST', '/api/folders', { name }),
  /** Resolves to the folder's new slug: its URL follows its name. */
  renameFolder: (slug: string, name: string) =>
    request<Renamed>('PATCH', `/api/folders/${encodeURIComponent(slug)}`, { name }),
  moveFolder: (slug: string, index: number) =>
    request<void>('PUT', `/api/folders/${encodeURIComponent(slug)}/position`, { index }),
  deleteFolder: (slug: string) => request<void>('DELETE', `/api/folders/${encodeURIComponent(slug)}`),

  refresh(scope: Scope) {
    const body = scope.kind === 'feed' ? { feed: scope.slug } : scope.kind === 'folder' ? { folder: scope.slug } : {}
    return request<{ scheduled: number }>('POST', '/api/refresh', body)
  },

  async importOpml(file: File): Promise<ImportReport> {
    const response = await fetch('/api/opml', {
      method: 'POST',
      headers: { 'content-type': 'text/x-opml' },
      body: await file.text(),
    })
    const body = await response.json()
    if (!response.ok) throw new ApiError(response.status, body.error)
    return body
  },
}
