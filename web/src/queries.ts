import {
  type InfiniteData,
  type QueryClient,
  useInfiniteQuery,
  useMutation,
  useMutationState,
  useQuery,
  useQueryClient,
} from '@tanstack/react-query'
import { useMemo } from 'react'
import { toast } from 'sonner'
import { api, type Item, type ItemSummary, type Filters, type Page, type Sidebar } from './api'
import { sentence } from './format'
import { buildLookup } from './lookup'
import { withPendingFeed } from './pending'
import { type ArticleRef, articleKey, scopePath, type Scope } from './routes'

export const keys = {
  sidebar: ['sidebar'] as const,
  settings: ['settings'] as const,
  filters: (feed: string) => ['filters', feed] as const,
  poller: ['poller'] as const,
  allItems: ['items'] as const,
  items: (scope: Scope, unreadOnly: boolean) => ['items', scopePath(scope), unreadOnly] as const,
  allItem: ['item'] as const,
  item: (article: ArticleRef) => ['item', articleKey(article)] as const,
}

/** The sidebar as the server knows it, plus a placeholder for every feed still being added. */
export function useSidebarData() {
  const query = useQuery({ queryKey: keys.sidebar, queryFn: api.sidebar })
  const adding = useMutationState({
    filters: { mutationKey: addFeedKey, status: 'pending' },
    select: (mutation) => mutation.state.variables as NewSubscription,
  })
  const data = useMemo(
    () => query.data && adding.reduce((sidebar, { pending, folder }) => withPendingFeed(sidebar, pending, folder), query.data),
    [query.data, adding],
  )
  return { ...query, data }
}

export function useSettings() {
  return useQuery({ queryKey: keys.settings, queryFn: api.settings })
}

/** A settings change; settings reload once the server has it. */
function useSettingsMutation<Args>(run: (args: Args) => Promise<void>) {
  const client = useQueryClient()
  return useMutation({
    mutationFn: run,
    onSuccess: () => client.invalidateQueries({ queryKey: keys.settings }),
    onError: (error) => toast.error(sentence(error.message)),
  })
}

export function useSettingsActions() {
  return {
    saveApiKey: useSettingsMutation(api.saveApiKey),
    removeApiKey: useSettingsMutation(() => api.removeApiKey()),
    setAi: useSettingsMutation(api.setAi),
  }
}

export function useFilters(feed: string) {
  return useQuery({ queryKey: keys.filters(feed), queryFn: () => api.filters(feed) })
}

export function useSaveFilters(feed: string) {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (filters: Filters) => api.saveFilters(feed, filters),
    onSuccess: () => client.invalidateQueries({ queryKey: keys.filters(feed) }),
    onError: (error) => toast.error(sentence(error.message)),
  })
}

export function useLookup(sidebar: Sidebar | undefined) {
  return useMemo(() => buildLookup(sidebar), [sidebar])
}

export function useItems(scope: Scope, unreadOnly: boolean, { enabled = true } = {}) {
  const query = useInfiniteQuery({
    enabled,
    queryKey: keys.items(scope, unreadOnly),
    queryFn: ({ pageParam }) => api.items(scope, unreadOnly, pageParam),
    initialPageParam: null as string | null,
    getNextPageParam: (page) => page.next_cursor,
  })
  const items = useMemo(() => query.data?.pages.flatMap((page) => page.items) ?? [], [query.data])
  return { ...query, items }
}

export function useItem(article: ArticleRef) {
  return useQuery({ queryKey: keys.item(article), queryFn: () => api.item(article) })
}

/** Where an article from a list lives. */
export function refOf(item: ItemSummary): ArticleRef {
  return { feed: item.feed_slug, slug: item.slug }
}

type Snapshot = [readonly unknown[], unknown][]

function snapshot(client: QueryClient): Snapshot {
  return [
    ...client.getQueriesData({ queryKey: keys.allItems }),
    ...client.getQueriesData({ queryKey: keys.allItem }),
    ...client.getQueriesData({ queryKey: keys.sidebar }),
  ]
}

function restore(client: QueryClient, saved: Snapshot) {
  for (const [key, data] of saved) client.setQueryData(key, data)
}

function patchItem(client: QueryClient, article: ArticleRef, patch: Partial<ItemSummary>) {
  const key = articleKey(article)
  client.setQueriesData<InfiniteData<Page>>({ queryKey: keys.allItems }, (data) =>
    data && {
      ...data,
      pages: data.pages.map((page) => ({
        ...page,
        items: page.items.map((item) => (articleKey(refOf(item)) === key ? { ...item, ...patch } : item)),
      })),
    },
  )
  client.setQueryData<Item>(keys.item(article), (item) => item && { ...item, ...patch })
}

/** Drops an article from every cached page of one list, whichever unread filter it was loaded with. */
export function removeFromList(client: QueryClient, scope: Scope, article: ArticleRef) {
  const key = articleKey(article)
  client.setQueriesData<InfiniteData<Page>>({ queryKey: ['items', scopePath(scope)] }, (data) =>
    data && {
      ...data,
      pages: data.pages.map((page) => ({ ...page, items: page.items.filter((item) => articleKey(refOf(item)) !== key) })),
    },
  )
}

function adjustUnread(client: QueryClient, feedSlug: string, delta: number) {
  client.setQueryData<Sidebar>(keys.sidebar, (sidebar) => {
    if (!sidebar) return sidebar
    const adjust = (feeds: Sidebar['uncategorized']) =>
      feeds.map((feed) => (feed.slug === feedSlug ? { ...feed, unread: Math.max(0, feed.unread + delta) } : feed))
    return {
      ...sidebar,
      total_unread: Math.max(0, sidebar.total_unread + delta),
      uncategorized: adjust(sidebar.uncategorized),
      folders: sidebar.folders.map((folder) =>
        folder.feeds.some((feed) => feed.slug === feedSlug)
          ? { ...folder, unread: Math.max(0, folder.unread + delta), feeds: adjust(folder.feeds) }
          : folder,
      ),
    }
  })
}

export type ItemPatch = { read?: boolean; starred?: boolean }

/** Read and star changes apply everywhere at once, and roll back if the server refuses. */
export function useUpdateItem() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({ item, patch }: { item: ItemSummary; patch: ItemPatch }) => api.updateItem(refOf(item), patch),
    onMutate: async ({ item, patch }) => {
      await client.cancelQueries({ queryKey: keys.allItems })
      const saved = snapshot(client)
      const now = new Date().toISOString()
      if (patch.read !== undefined && patch.read !== (item.read_at !== null)) {
        patchItem(client, refOf(item), { read_at: patch.read ? now : null })
        adjustUnread(client, item.feed_slug, patch.read ? -1 : 1)
      }
      if (patch.starred !== undefined && patch.starred !== (item.starred_at !== null)) {
        patchItem(client, refOf(item), { starred_at: patch.starred ? now : null })
        if (!patch.starred) removeFromList(client, { kind: 'starred' }, refOf(item))
        client.setQueryData<Sidebar>(keys.sidebar, (sidebar) =>
          sidebar && { ...sidebar, total_starred: Math.max(0, sidebar.total_starred + (patch.starred ? 1 : -1)) },
        )
      }
      return saved
    },
    onError: (error, _variables, saved) => {
      if (saved) restore(client, saved)
      toast.error(sentence(error.message))
    },
    onSettled: (_data, _error, { patch }) => {
      client.invalidateQueries({ queryKey: keys.sidebar })
      // A list refetch that started before the server saved this change would bring the old state
      // back. Starred is refetched right away, cancelling any such fetch; Unread only goes stale so
      // J/K keeps walking the list you're reading through.
      if (patch.starred !== undefined) client.invalidateQueries({ queryKey: ['items', scopePath({ kind: 'starred' })] })
      client.invalidateQueries({ queryKey: keys.allItems, refetchType: 'none' })
    },
  }).mutate
}

export function useMarkAllRead() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({ scope, seenUntil }: { scope: Scope; seenUntil: string }) => api.markRead(scope, seenUntil),
    onSuccess: () => {
      client.invalidateQueries({ queryKey: keys.sidebar })
      client.invalidateQueries({ queryKey: keys.allItems })
      client.invalidateQueries({ queryKey: keys.allItem })
    },
    onError: (error) => toast.error(sentence(error.message)),
  }).mutate
}

/**
 * For sidebar edits: subscribe, move, rename, delete. Failures show as a toast unless the caller
 * displays them itself.
 */
export function useSidebarMutation<Args, Result>(run: (args: Args) => Promise<Result>, { inlineErrors = false } = {}) {
  const client = useQueryClient()
  return useMutation({
    mutationFn: run,
    onError: (error) => !inlineErrors && toast.error(sentence(error.message)),
    onSuccess: () => {
      client.invalidateQueries({ queryKey: keys.sidebar })
      client.invalidateQueries({ queryKey: keys.allItems })
    },
  })
}

/** Every sidebar edit, as stable functions safe to pass to memoized components. */
export function useSubscriptionActions() {
  const moveFeed = useSidebarMutation((a: { slug: string; folder: string | null; index: number }) =>
    api.moveFeed(a.slug, a.folder, a.index),
  ).mutate
  const moveFolder = useSidebarMutation((a: { slug: string; index: number }) => api.moveFolder(a.slug, a.index)).mutate
  const renameFeed = useSidebarMutation((a: { slug: string; title: string | null }) => api.renameFeed(a.slug, a.title)).mutate
  const renameFolder = useSidebarMutation((a: { slug: string; name: string }) => api.renameFolder(a.slug, a.name)).mutate
  const createFolder = useSidebarMutation((name: string) => api.createFolder(name)).mutate
  const unsubscribe = useSidebarMutation((slug: string) => api.unsubscribe(slug)).mutate
  const deleteFolder = useSidebarMutation((slug: string) => api.deleteFolder(slug)).mutate
  return useMemo(
    () => ({ moveFeed, moveFolder, renameFeed, renameFolder, createFolder, unsubscribe, deleteFolder }),
    [moveFeed, moveFolder, renameFeed, renameFolder, createFolder, unsubscribe, deleteFolder],
  )
}

export type NewSubscription = {
  url: string
  folder: string | null
  /** The placeholder shown in the sidebar until the server answers. */
  pending: { slug: string; title: string }
}

const addFeedKey = ['addFeed']

/**
 * Subscribes without waiting. The placeholder comes from the mutation itself rather than the cache,
 * so several adds can run at once without one's refetch or rollback wiping out another's row.
 */
export function useAddFeed() {
  const client = useQueryClient()
  return useMutation({
    mutationKey: addFeedKey,
    mutationFn: ({ url, folder }: NewSubscription) => api.subscribe(url, folder),
    // Awaited so the placeholder stays until the real feed has arrived in the sidebar.
    onSettled: () =>
      Promise.all([
        client.invalidateQueries({ queryKey: keys.sidebar }),
        client.invalidateQueries({ queryKey: keys.allItems }),
      ]),
  }).mutateAsync
}
