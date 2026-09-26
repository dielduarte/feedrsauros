import { describe, expect, it } from 'vitest'
import { SETTINGS_PATH, articlePath, parseLocation, rulesPath, scopePath, type Scope } from './routes'

const scopes: Scope[] = [
  { kind: 'all' },
  { kind: 'unread' },
  { kind: 'starred' },
  { kind: 'folder', slug: 'engineering' },
  { kind: 'feed', slug: 'cloudflare-blog' },
]

describe('routes', () => {
  it('round-trips every scope through its path', () => {
    for (const scope of scopes) {
      expect(parseLocation(scopePath(scope))).toEqual({ scope, article: null })
    }
  })

  it('uses readable slug paths', () => {
    expect(scopePath({ kind: 'all' })).toBe('/')
    expect(scopePath({ kind: 'folder', slug: 'engineering' })).toBe('/folders/engineering')
    expect(articlePath({ feed: 'cloudflare-blog', slug: 'saving-ram' })).toBe('/feeds/cloudflare-blog/saving-ram')
  })

  it('gives each article one address, inside its feed', () => {
    expect(parseLocation('/feeds/cloudflare-blog/saving-ram')).toEqual({
      scope: { kind: 'feed', slug: 'cloudflare-blog' },
      article: { feed: 'cloudflare-blog', slug: 'saving-ram' },
    })
  })

  it('decodes percent-encoded slugs', () => {
    expect(parseLocation('/folders/caf%C3%A9')).toEqual({ scope: { kind: 'folder', slug: 'café' }, article: null })
  })

  it('has a page for settings', () => {
    expect(parseLocation(SETTINGS_PATH)).toEqual({ page: 'settings' })
    expect(parseLocation('/settings/nope')).toEqual({ scope: { kind: 'all' }, article: null })
  })

  it('has a rules page per feed, apart from its articles', () => {
    expect(rulesPath('cloudflare-blog')).toBe('/rules/cloudflare-blog')
    expect(parseLocation(rulesPath('cloudflare-blog'))).toEqual({ page: 'rules', feed: 'cloudflare-blog' })
    expect(parseLocation('/feeds/cloudflare-blog/rules')).toEqual({
      scope: { kind: 'feed', slug: 'cloudflare-blog' },
      article: { feed: 'cloudflare-blog', slug: 'rules' },
    })
  })

  it('falls back to all articles for unknown paths', () => {
    expect(parseLocation('/nope')).toEqual({ scope: { kind: 'all' }, article: null })
    expect(parseLocation('/feeds')).toEqual({ scope: { kind: 'all' }, article: null })
    expect(parseLocation('/feeds/a/b/c')).toEqual({ scope: { kind: 'all' }, article: null })
  })
})
