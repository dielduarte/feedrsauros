import { Suspense, useCallback, useMemo, useState } from 'react'
import { toast } from 'sonner'
import { useLocation } from 'wouter'
import { SidebarInset, SidebarProvider, useSidebar } from '@/components/ui/sidebar'
import { useStoredState } from '@/hooks/use-stored-state'
import { listsUnreadOnly } from './api'
import type { Chrome } from './chrome'
import { AddFeedDialog, type DialogName, ShortcutsDialog, TransferDialog, usePreloadDialogs } from './components/dialogs'
import { AppSidebar } from './components/sidebar/AppSidebar'
import { scopeLabel } from './lookup'
import { ListPage } from './pages/ListPage'
import { ReaderPage } from './pages/ReaderPage'
import { FiltersPage } from './pages/FiltersPage'
import { SettingsPage } from './pages/SettingsPage'
import { usePollerStatus, useRefresh, useServerEvents } from './poller'
import { sentence } from './format'
import { hostOf, pendingSlug } from './pending'
import { useAddFeed, useLookup, useSidebarData } from './queries'
import { type ArticleRef, SETTINGS_PATH, articleKey, articlePath, isReading, parseLocation, filtersPath, scopePath, type Scope } from './routes'

export function App() {
  const [open, setOpen] = useStoredState('sidebarOpen', true)
  return (
    <SidebarProvider open={open} onOpenChange={setOpen}>
      <Shell />
    </SidebarProvider>
  )
}

/** What the top bar's refresh and shortcuts act on while settings are open. */
const ALL: Scope = { kind: 'all' }

function Shell() {
  const [path, setPath] = useLocation()
  // Parsed once per URL so `scope` keeps its identity and memoized children don't re-render.
  const location = useMemo(() => parseLocation(path), [path])
  const article = isReading(location) ? location.article : null
  // An article has one URL wherever it was opened from; the list it came from rides along in
  // history state, so the switcher, J/K and Back keep working within that list.
  const scope = useMemo((): Scope => {
    if (!isReading(location)) return location.page === 'filters' ? { kind: 'feed', slug: location.feed } : ALL
    const listPath: unknown = window.history.state?.listPath
    const list = location.article && typeof listPath === 'string' ? parseLocation(listPath) : location
    return isReading(list) ? list.scope : location.scope
  }, [location])
  const { toggleSidebar, isMobile, setOpenMobile } = useSidebar()
  const [unreadPreference, setUnreadPreference] = useStoredState('unreadOnly', false)
  const [dialog, setDialog] = useState<DialogName | null>(null)
  // What was typed in a failed add, so "Try again" reopens the dialog with it.
  const [addDraft, setAddDraft] = useState<{ url: string; folder: string | null } | null>(null)
  const addFeed = useAddFeed()
  const [lastOpenedKey, setLastOpenedKey] = useState<string | null>(null)
  useServerEvents()
  const status = usePollerStatus()
  const refresh = useRefresh()
  const { data: sidebar } = useSidebarData()
  const lookup = useLookup(sidebar)
  usePreloadDialogs()

  const navigate = useCallback(
    (target: Scope) => {
      setPath(scopePath(target))
      if (isMobile) setOpenMobile(false)
    },
    [setPath, isMobile, setOpenMobile],
  )
  const openArticle = useCallback(
    (target: ArticleRef) => {
      setLastOpenedKey(articleKey(target))
      setPath(articlePath(target), { state: { listPath: scopePath(scope) } })
    },
    [setPath, scope],
  )
  // Renaming changes a URL; if you're looking at what was renamed, move with it.
  const followRename = useCallback(
    (from: Scope, to: Scope) => {
      const renamed = (candidate: Scope) => scopePath(candidate) === scopePath(from)
      const listPath = scopePath(renamed(scope) ? to : scope)
      if (article) {
        const feed = from.kind === 'feed' && to.kind === 'feed' && article.feed === from.slug ? to.slug : article.feed
        setPath(articlePath({ feed, slug: article.slug }), { replace: true, state: { listPath } })
      } else if (renamed(scope)) {
        setPath(listPath, { replace: true })
      }
    },
    [article, scope, setPath],
  )
  const backToList = useCallback(() => setPath(scopePath(scope)), [setPath, scope])
  const openSettings = useCallback(() => {
    setPath(SETTINGS_PATH)
    if (isMobile) setOpenMobile(false)
  }, [setPath, isMobile, setOpenMobile])
  const openFilters = useCallback(
    (feed: string) => {
      setPath(filtersPath(feed))
      if (isMobile) setOpenMobile(false)
    },
    [setPath, isMobile, setOpenMobile],
  )
  const closeDialog = useCallback(() => {
    setDialog(null)
    setAddDraft(null)
  }, [])

  // The dialog closes at once; a placeholder feed shows the dinosaur until the server answers.
  const subscribe = (url: string, folder: string | null) => {
    closeDialog()
    const cameFrom = scope
    const pending = { slug: pendingSlug(), title: hostOf(url) }
    const pendingPath = scopePath({ kind: 'feed', slug: pending.slug })
    const stillWaiting = () => window.location.pathname === pendingPath
    // Promises rather than mutate callbacks: those only fire for the latest of several adds.
    addFeed({ url, folder, pending }).then(
      (added) => {
        if (stillWaiting()) setPath(scopePath({ kind: 'feed', slug: added.slug }), { replace: true })
        if (added.ai_folder) {
          toast(`Filed ${added.title} under ${lookup.folder(added.ai_folder)?.name ?? added.ai_folder}`)
        }
      },
      (error: Error) => {
        if (stillWaiting()) setPath(scopePath(cameFrom), { replace: true })
        toast.error(sentence(error.message), {
          action: {
            label: 'Try again',
            onClick: () => {
              setAddDraft({ url, folder })
              setDialog('add')
            },
          },
        })
      },
    )
    navigate({ kind: 'feed', slug: pending.slug })
  }

  const chrome = useMemo<Chrome>(() => {
    const onRefresh = () => refresh(scope.kind === 'feed' || scope.kind === 'folder' ? scope : { kind: 'all' })
    return {
      scope,
      label: scopeLabel(scope, lookup),
      sidebar,
      lookup,
      status,
      onNavigate: navigate,
      onRefresh,
      shortcuts: { r: onRefresh, '[': toggleSidebar, '?': () => setDialog('shortcuts') },
      shortcutsEnabled: dialog === null,
    }
  }, [scope, lookup, sidebar, status, navigate, refresh, toggleSidebar, dialog])

  return (
    <>
      <AppSidebar
        scope={isReading(location) || location.page === 'filters' ? scope : null}
        sidebar={sidebar}
        onNavigate={navigate}
        onOpenSettings={openSettings}
        onOpenFilters={openFilters}
        onOpenDialog={setDialog}
        onRenamed={followRename}
      />

      <SidebarInset className="h-dvh min-w-0 overflow-hidden md:h-[calc(100dvh-1rem)] md:border md:shadow-[0_1px_2px_rgb(0_0_0/0.03),0_18px_40px_-20px_rgb(0_0_0/0.12)]">
        {!isReading(location) ? (
          location.page === 'settings' ? (
            <SettingsPage chrome={chrome} />
          ) : (
            <FiltersPage chrome={chrome} feed={location.feed} />
          )
        ) : article === null ? (
          <ListPage
            // A new scope starts with a fresh selection.
            key={scopePath(scope)}
            chrome={chrome}
            unreadPreference={unreadPreference}
            onUnreadPreferenceChange={setUnreadPreference}
            initialSelectedKey={lastOpenedKey}
            onOpen={openArticle}
            onAddFeed={() => setDialog('add')}
            onImport={() => setDialog('transfer')}
          />
        ) : (
          <ReaderPage
            chrome={chrome}
            article={article}
            unreadOnly={listsUnreadOnly(scope, unreadPreference)}
            onBack={backToList}
            onOpen={openArticle}
          />
        )}
      </SidebarInset>

      <Suspense fallback={null}>
        {dialog === 'add' ? (
          <AddFeedDialog
            sidebar={sidebar}
            initialUrl={addDraft?.url ?? ''}
            defaultFolder={addDraft ? addDraft.folder : scope.kind === 'folder' ? scope.slug : null}
            onClose={closeDialog}
            onSubmit={subscribe}
          />
        ) : null}
        {dialog === 'transfer' ? <TransferDialog onClose={closeDialog} /> : null}
        {dialog === 'shortcuts' ? <ShortcutsDialog onClose={closeDialog} /> : null}
      </Suspense>
    </>
  )
}
