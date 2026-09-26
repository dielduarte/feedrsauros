import { QueryClient } from '@tanstack/react-query'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { applyEvent, nextBatchDone, serverPoller } from './poller'
import { keys } from './queries'

let client: QueryClient

beforeEach(() => {
  client = new QueryClient()
  vi.useFakeTimers()
})

afterEach(() => vi.useRealTimers())

function settled(promise: Promise<void>) {
  let done = false
  promise.then(() => (done = true))
  return async () => {
    await vi.advanceTimersByTimeAsync(0)
    return done
  }
}

describe('server poller status', () => {
  it('is idle and online before the server says anything', () => {
    expect(serverPoller(client)).toEqual({ running: false, offline: false })
  })

  it('follows batches as the server reports them', () => {
    applyEvent(client, { type: 'batch_started', feeds: 3 })
    expect(serverPoller(client).running).toBe(true)

    applyEvent(client, { type: 'batch_finished', health: 'offline' })
    expect(serverPoller(client)).toEqual({ running: false, offline: true })
  })
})

describe('articles filtered by new rules', () => {
  it('reload the sidebar and every list', () => {
    client.setQueryData(keys.sidebar, { total_unread: 4, total_starred: 0, folders: [], uncategorized: [] })
    client.setQueryData(keys.items({ kind: 'all' }, false), { pages: [], pageParams: [] })

    applyEvent(client, { type: 'feed_filtered', feed: 'cloudflare-blog', hidden: 2 })

    expect(client.getQueryState(keys.sidebar)?.isInvalidated).toBe(true)
    expect(client.getQueryState(keys.items({ kind: 'all' }, false))?.isInvalidated).toBe(true)
  })
})

describe('waiting for a requested refresh', () => {
  it('ends when the batch that starts afterwards finishes', async () => {
    const isDone = settled(nextBatchDone(client))

    applyEvent(client, { type: 'batch_started', feeds: 1 })
    expect(await isDone()).toBe(false)

    applyEvent(client, { type: 'batch_finished', health: 'online' })
    expect(await isDone()).toBe(true)
  })

  it('catches a batch that starts and ends before the server answers the request', async () => {
    const done = nextBatchDone(client)
    applyEvent(client, { type: 'batch_started', feeds: 1 })
    applyEvent(client, { type: 'batch_finished', health: 'online' })

    await expect(done).resolves.toBeUndefined()
  })

  it('gives up when no batch starts in time', async () => {
    const isDone = settled(nextBatchDone(client, 10_000))

    await vi.advanceTimersByTimeAsync(9_999)
    expect(await isDone()).toBe(false)
    await vi.advanceTimersByTimeAsync(1)
    expect(await isDone()).toBe(true)
  })

  it('keeps waiting past the timeout once a batch is running', async () => {
    const isDone = settled(nextBatchDone(client, 10_000))
    applyEvent(client, { type: 'batch_started', feeds: 200 })

    await vi.advanceTimersByTimeAsync(60_000)
    expect(await isDone()).toBe(false)
  })
})
