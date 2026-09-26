import { ArrowLeft, ArrowUpRight, CheckCheck, ChevronDown, Circle, CircleCheck, RefreshCw, Star, WifiOff } from 'lucide-react'
import { type ReactNode, useState } from 'react'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { Separator } from '@/components/ui/separator'
import { SidebarTrigger, useSidebar } from '@/components/ui/sidebar'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { cn } from '@/lib/utils'
import type { Item, Sidebar } from '../api'
import type { Chrome } from '../chrome'
import { scopePath, type Scope } from '../routes'
import { IconAction } from './IconAction'

type Props = {
  chrome: Chrome
  /** Shown in place of the leading icon, e.g. to return from an article to its list. */
  onBack?: () => void
  /** Title centred on the bar, cut off with an ellipsis when space runs out. */
  crumb?: string | null
  /** Page-specific actions, placed before the refresh control. */
  children?: ReactNode
  /** Shown instead of the list switcher, on pages that aren't a list. */
  title?: string
}

export function TopBar({ chrome, onBack, crumb, children, title }: Props) {
  const { state, isMobile } = useSidebar()
  const sidebarHidden = state === 'collapsed' || isMobile

  return (
    <header
      // Only the empty bar drags the desktop window; its buttons still click.
      data-tauri-drag-region
      className={cn(
        'grid h-13 shrink-0 grid-cols-[minmax(max-content,1fr)_minmax(0,auto)_minmax(max-content,1fr)] items-center gap-3 border-b px-3',
        // With the sidebar hidden, the desktop window's traffic lights sit over this bar.
        sidebarHidden && 'traffic-lights:pl-20',
      )}
    >
      <div className="flex min-w-0 items-center gap-2">
        <SidebarTrigger className="size-8 text-muted-foreground" />
        {onBack ? (
          <IconAction label="Back to articles" shortcut="Esc" onClick={onBack}>
            <ArrowLeft />
          </IconAction>
        ) : null}
        {title === undefined ? (
          <ScopeSwitcher scope={chrome.scope} label={chrome.label} sidebar={chrome.sidebar} onNavigate={chrome.onNavigate} />
        ) : (
          <span className="px-1 text-[13.5px] font-medium">{title}</span>
        )}
      </div>

      <span className="truncate text-center text-[13.5px] text-muted-foreground max-md:hidden">{crumb}</span>

      <div className="flex items-center justify-end gap-1">
        {children}
        {chrome.status.offline ? (
          <span className="flex items-center gap-1.5 px-2 text-xs text-muted-foreground" title="feedrsauros can't reach the internet and will retry.">
            <WifiOff className="size-3.5" /> Offline
          </span>
        ) : null}
        <IconAction label={chrome.status.refreshing ? 'Refreshing feeds' : 'Refresh feeds'} shortcut="R" onClick={chrome.onRefresh}>
          <RefreshIcon refreshing={chrome.status.refreshing} />
        </IconAction>
      </div>
    </header>
  )
}

type ListActionsProps = {
  unreadOnly: boolean
  canFilterUnread: boolean
  onUnreadOnlyChange: (unreadOnly: boolean) => void
  canMarkAllRead: boolean
  onMarkAllRead: () => void
}

const toggleItem = 'h-7 rounded-md px-2.5 text-xs data-[state=on]:bg-background data-[state=on]:shadow-xs'

export function ListActions({ unreadOnly, canFilterUnread, onUnreadOnlyChange, canMarkAllRead, onMarkAllRead }: ListActionsProps) {
  return (
    <>
      {canFilterUnread ? (
        <ToggleGroup
          type="single"
          size="sm"
          value={unreadOnly ? 'unread' : 'all'}
          onValueChange={(value) => value && onUnreadOnlyChange(value === 'unread')}
          className="mr-1 rounded-lg bg-secondary p-0.5"
          aria-label="Show"
        >
          <ToggleGroupItem value="all" className={toggleItem}>All</ToggleGroupItem>
          <ToggleGroupItem value="unread" className={toggleItem}>Unread</ToggleGroupItem>
        </ToggleGroup>
      ) : null}
      <IconAction label="Mark all as read" shortcut="⇧A" onClick={onMarkAllRead} disabled={!canMarkAllRead}>
        <CheckCheck />
      </IconAction>
    </>
  )
}

type ReaderActionsProps = {
  item: Item
  words: number
  onToggleStar: () => void
  onToggleRead: () => void
}

export function ReaderActions({ item, words, onToggleStar, onToggleRead }: ReaderActionsProps) {
  return (
    <>
      {words > 0 ? (
        <span className="px-2 text-[13px] whitespace-nowrap text-muted-foreground tabular-nums max-md:hidden">
          {words.toLocaleString('en-US')} words
        </span>
      ) : null}
      <Separator orientation="vertical" className="mr-1 data-[orientation=vertical]:h-4 max-md:hidden" />
      <IconAction label={item.starred_at ? 'Unstar' : 'Star'} shortcut="S" pressed={item.starred_at !== null} onClick={onToggleStar}>
        <Star />
      </IconAction>
      <IconAction label={item.read_at ? 'Mark as unread' : 'Mark as read'} shortcut="M" onClick={onToggleRead}>
        {item.read_at ? <Circle /> : <CircleCheck />}
      </IconAction>
      {item.url ? (
        <IconAction label="Open original" shortcut="V" asChild>
          <a href={item.url} target="_blank" rel="noopener noreferrer">
            <ArrowUpRight />
          </a>
        </IconAction>
      ) : null}
    </>
  )
}

type SwitcherProps = {
  scope: Scope
  label: string
  sidebar: Sidebar | undefined
  onNavigate: (scope: Scope) => void
}

function ScopeSwitcher({ scope, label, sidebar, onNavigate }: SwitcherProps) {
  const current = scopePath(scope)
  const option = (target: Scope, text: string, nested = false) => (
    <DropdownMenuItem
      key={scopePath(target)}
      onSelect={() => onNavigate(target)}
      className={cn(nested && 'pl-5 text-muted-foreground', scopePath(target) === current && 'font-semibold text-foreground')}
    >
      {text}
    </DropdownMenuItem>
  )
  const hasFeeds = !!sidebar && (sidebar.folders.length > 0 || sidebar.uncategorized.length > 0)

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="secondary" size="sm" className="h-7.5 shrink-0 gap-1.5 px-2.5 text-[13.5px] font-normal">
          {label}
          <ChevronDown className="size-3.5" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="dark max-h-[60vh] min-w-60 overflow-y-auto">
        {option({ kind: 'all' }, 'All articles')}
        {option({ kind: 'unread' }, 'Unread')}
        {option({ kind: 'starred' }, 'Starred')}
        {hasFeeds ? <DropdownMenuSeparator /> : null}
        {sidebar?.folders.map((folder) => [
          option({ kind: 'folder', slug: folder.slug }, folder.name),
          ...folder.feeds.map((feed) => option({ kind: 'feed', slug: feed.slug }, feed.title, true)),
        ])}
        {sidebar?.uncategorized.map((feed) => option({ kind: 'feed', slug: feed.slug }, feed.title))}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

/**
 * Spins while refreshing and always finishes the turn it's on, so even an instant refresh shows
 * one full rotation and the icon never stops at an odd angle.
 */
function RefreshIcon({ refreshing }: { refreshing: boolean }) {
  const [spinning, setSpinning] = useState(refreshing)
  if (refreshing && !spinning) setSpinning(true)

  // Animating a wrapper rather than the SVG itself lets the browser composite it on the GPU.
  return (
    <span
      className={cn('inline-grid', spinning && 'animate-spin motion-reduce:[animation-duration:3s]')}
      onAnimationIteration={() => !refreshing && setSpinning(false)}
    >
      <RefreshCw />
    </span>
  )
}
