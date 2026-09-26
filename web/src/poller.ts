import { hashKey, type QueryClient, skipToken, useIsMutating, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useMemo } from 'react'
import { toast } from 'sonner'
import { api, type PollerEvent } from './api'
import { sentence } from './format'
import { keys } from './queries'
import type { Scope } from './routes'

export type PollerStatus = { refreshing: boolean; offline: boolean }

/** What the server last said about its poller, kept in the query cache. */
type ServerPoller = { running: boolean; offline: boolean }

const idle: ServerPoller = { running: false, offline: false }
const refreshKey = ['refresh']

/** How long a requested refresh may wait for the server to start before we stop showing it. */
const REFRESH_START_TIMEOUT = 10_000

export function serverPoller(client: QueryClient): ServerPoller {
  return client.getQueryData<ServerPoller>(keys.poller) ?? idle
}

/** Mirrors one server event into the cache. */
export function applyEvent(client: QueryClient, event: PollerEvent) {
  switch (event.type) {
    case 'batch_started':
      client.setQueryData<ServerPoller>(keys.poller, { ...serverPoller(client), running: true })
      break
    case 'feed_refreshed':
    case 'feed_failed':
      client.invalidateQueries({ queryKey: keys.sidebar })
      break
    case 'batch_finished':
      client.setQueryData<ServerPoller>(keys.poller, { running: false, offline: event.health === 'offline' })
      client.invalidateQueries({ queryKey: keys.sidebar })
      client.invalidateQueries({ queryKey: keys.allItems })
      break
    case 'feed_filtered':
      client.invalidateQueries({ queryKey: keys.sidebar })
      client.invalidateQueries({ queryKey: keys.allItems })
      break
    case 'resync':
      client.invalidateQueries()
      break
  }
}

/**
 * Resolves when the next batch the server runs is over, or when none starts in time, e.g. because
 * the event stream is down.
 */
export function nextBatchDone(client: QueryClient, startTimeout = REFRESH_START_TIMEOUT): Promise<void> {
  const pollerHash = hashKey(keys.poller)
  return new Promise((resolve) => {
    let started = serverPoller(client).running
    const finish = () => {
      clearTimeout(timer)
      unsubscribe()
      resolve()
    }
    const timer = setTimeout(finish, startTimeout)
    const unsubscribe = client.getQueryCache().subscribe((event) => {
      if (event.type !== 'updated' || event.query.queryHash !== pollerHash) return
      if (serverPoller(client).running) {
        started = true
        clearTimeout(timer)
      } else if (started) {
        finish()
      }
    })
  })
}

/** Follows the server's poller over SSE and keeps what's on screen current as feeds update. */
export function useServerEvents() {
  const client = useQueryClient()
  useEffect(() => {
    const source = new EventSource('/api/events')
    source.onmessage = (message) => applyEvent(client, JSON.parse(message.data))
    return () => source.close()
  }, [client])
}

/** Stays pending until the batch it asked for has finished, so the spinner covers the whole refresh. */
export function useRefresh() {
  const client = useQueryClient()
  return useMutation({
    mutationKey: refreshKey,
    mutationFn: async (scope: Scope) => {
      // Watching starts before the request, so a batch that ends before the response isn't missed.
      const done = nextBatchDone(client)
      const { scheduled } = await api.refresh(scope)
      if (scheduled > 0) await done
    },
    onError: (error) => toast.error(sentence(error.message)),
  }).mutate
}

export function usePollerStatus(): PollerStatus {
  const { data = idle } = useQuery<ServerPoller>({ queryKey: keys.poller, queryFn: skipToken })
  const requested = useIsMutating({ mutationKey: refreshKey }) > 0
  const refreshing = data.running || requested
  return useMemo(() => ({ refreshing, offline: data.offline }), [refreshing, data.offline])
}
