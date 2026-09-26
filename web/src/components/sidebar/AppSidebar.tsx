import { AlertCircle, ChevronRight, CircleDot, FolderInput, Inbox, Keyboard, Plus, Settings, Star } from 'lucide-react'
import { memo, type ReactNode, useState } from 'react'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupAction,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
} from '@/components/ui/sidebar'
import { useStoredState } from '@/hooks/use-stored-state'
import { cn } from '@/lib/utils'
import type { Sidebar as SidebarData, SidebarFeed, SidebarFolder } from '../../api'
import { useRefresh } from '../../poller'
import { useSettings, useSubscriptionActions } from '../../queries'
import { scopePath, type Scope } from '../../routes'
import type { DialogName } from '../dialogs'
import { FeedIcon } from '../FeedIcon'
import { RemoveDialog, type Removal } from './RemoveDialog'
import { RenameInput } from './RenameInput'
import { RowMenu, UnreadCount } from './RowMenu'
import { useTreeDrag } from './useTreeDrag'

type Props = {
  /** `null` while settings are open, since no list is. */
  scope: Scope | null
  sidebar: SidebarData | undefined
  onNavigate: (scope: Scope) => void
  onOpenSettings: () => void
  onOpenRules: (feed: string) => void
  onOpenDialog: (dialog: DialogName) => void
  /** A feed or folder was renamed, so its URL changed. */
  onRenamed: (from: Scope, to: Scope) => void
}

const dropLine =
  "before:absolute before:inset-x-2 before:-top-px before:h-0.5 before:rounded-full before:bg-signal before:content-['']"

const isActive = (current: Scope | null, candidate: Scope) => current !== null && scopePath(current) === scopePath(candidate)

/** Memoized: it only depends on the sidebar data and where you are, not on the article list. */
export const AppSidebar = memo(function AppSidebar({ scope, sidebar, onNavigate, onOpenSettings, onOpenRules, onOpenDialog, onRenamed }: Props) {
  const actions = useSubscriptionActions()
  const refresh = useRefresh()
  const aiEnabled = useSettings().data?.ai_enabled ?? false
  const [collapsed, setCollapsed] = useStoredState<string[]>('collapsedFolders', [])
  const [renaming, setRenaming] = useState<string | null>(null)
  const [creatingFolder, setCreatingFolder] = useState(false)
  const [removal, setRemoval] = useState<Removal | null>(null)

  const folders = sidebar?.folders ?? []
  const uncategorized = sidebar?.uncategorized ?? []
  const drag = useTreeDrag({ folders, uncategorized, onMoveFeed: actions.moveFeed, onMoveFolder: actions.moveFolder })

  const toggleFolder = (slug: string) =>
    setCollapsed((slugs) => (slugs.includes(slug) ? slugs.filter((x) => x !== slug) : [...slugs, slug]))

  const remove = (target: Removal) => {
    if (target.kind === 'feed') {
      actions.unsubscribe(target.feed.slug)
      if (isActive(scope, { kind: 'feed', slug: target.feed.slug })) onNavigate({ kind: 'all' })
    } else {
      actions.deleteFolder(target.folder.slug)
      if (isActive(scope, { kind: 'folder', slug: target.folder.slug })) onNavigate({ kind: 'all' })
    }
    setRemoval(null)
  }

  const feedRow = (feed: SidebarFeed, folder: string | null, siblings: SidebarFeed[]) => (
    <FeedRow
      key={feed.slug}
      feed={feed}
      nested={folder !== null}
      active={isActive(scope, { kind: 'feed', slug: feed.slug })}
      renaming={renaming === `feed:${feed.slug}`}
      dropBefore={drag.dropsBefore(`feed:${feed.slug}`)}
      dragProps={drag.feed(feed, folder, siblings)}
      onOpen={() => onNavigate({ kind: 'feed', slug: feed.slug })}
      onRename={() => setRenaming(`feed:${feed.slug}`)}
      onRenamed={(title) => {
        if (title !== undefined) {
          actions.renameFeed(
            { slug: feed.slug, title: title || null },
            { onSuccess: (renamed) => onRenamed({ kind: 'feed', slug: feed.slug }, { kind: 'feed', slug: renamed.slug }) },
          )
        }
        setRenaming(null)
      }}
      onRefresh={() => refresh({ kind: 'feed', slug: feed.slug })}
      onRules={aiEnabled ? () => onOpenRules(feed.slug) : undefined}
      onRemove={() => setRemoval({ kind: 'feed', feed })}
    />
  )

  return (
    <>
      <Sidebar variant="inset" collapsible="offcanvas">
        {/* Empty: spacing above the menu, and room for the desktop window's traffic lights. */}
        <SidebarHeader data-tauri-drag-region className="h-12 traffic-lights:h-13" />

        <SidebarContent>
          <SidebarGroup className="mt-4">
            <SidebarMenu>
              <NavItem active={isActive(scope, { kind: 'all' })} icon={<Inbox />} label="All articles" onClick={() => onNavigate({ kind: 'all' })} />
              <NavItem
                active={isActive(scope, { kind: 'unread' })}
                icon={<CircleDot />}
                label="Unread"
                count={sidebar?.total_unread}
                onClick={() => onNavigate({ kind: 'unread' })}
              />
              <NavItem
                active={isActive(scope, { kind: 'starred' })}
                icon={<Star />}
                label="Starred"
                count={sidebar?.total_starred}
                onClick={() => onNavigate({ kind: 'starred' })}
              />
            </SidebarMenu>
          </SidebarGroup>

          <SidebarGroup className="mt-4">
            <SidebarGroupLabel className="text-[13px] font-medium text-foreground">Subscriptions</SidebarGroupLabel>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <SidebarGroupAction aria-label="Add a feed or folder">
                  <Plus />
                </SidebarGroupAction>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="dark">
                <DropdownMenuItem onSelect={() => onOpenDialog('add')}>Add feed</DropdownMenuItem>
                <DropdownMenuItem onSelect={() => setCreatingFolder(true)}>New folder</DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>

            <SidebarGroupContent>
              <SidebarMenu>
                {creatingFolder ? (
                  <SidebarMenuItem>
                    <RenameInput
                      initial=""
                      label="Folder name"
                      onDone={(name) => {
                        if (name) actions.createFolder(name)
                        setCreatingFolder(false)
                      }}
                    />
                  </SidebarMenuItem>
                ) : null}

                {folders.map((folder) => (
                  // Feed rows are siblings of their folder row, not children, so hover and menu
                  // state on one row never leaks into another through `group` variants.
                  <FolderRows
                    key={folder.slug}
                    folder={folder}
                    open={!collapsed.includes(folder.slug)}
                    active={isActive(scope, { kind: 'folder', slug: folder.slug })}
                    renaming={renaming === `folder:${folder.slug}`}
                    dropBefore={drag.dropsBefore(`folder:${folder.slug}`)}
                    dropInto={drag.dropsInto(`folder:${folder.slug}`)}
                    dragProps={drag.folder(folder)}
                    onOpen={() => onNavigate({ kind: 'folder', slug: folder.slug })}
                    onToggle={() => toggleFolder(folder.slug)}
                    onRename={() => setRenaming(`folder:${folder.slug}`)}
                    onRenamed={(name) => {
                      if (name) {
                        actions.renameFolder(
                          { slug: folder.slug, name },
                          {
                            onSuccess: (renamed) =>
                              onRenamed({ kind: 'folder', slug: folder.slug }, { kind: 'folder', slug: renamed.slug }),
                          },
                        )
                      }
                      setRenaming(null)
                    }}
                    onRefresh={() => refresh({ kind: 'folder', slug: folder.slug })}
                    onRemove={() => setRemoval({ kind: 'folder', folder })}
                  >
                    {folder.feeds.map((feed) => feedRow(feed, folder.slug, folder.feeds))}
                  </FolderRows>
                ))}

                {uncategorized.map((feed) => feedRow(feed, null, uncategorized))}

                {drag.draggingFeed ? (
                  <li
                    className={cn(
                      'mt-2 flex items-center gap-2 rounded-md border border-dashed p-2.5 text-xs text-muted-foreground',
                      drag.overOutOfFolder ? 'border-signal bg-signal/8' : 'border-faint',
                    )}
                    {...drag.outOfFolder}
                  >
                    <FolderInput className="size-3.5" /> Drop here to take it out of its folder
                  </li>
                ) : null}

                {sidebar && folders.length === 0 && uncategorized.length === 0 && !creatingFolder ? (
                  <SidebarMenuItem>
                    <SidebarMenuButton onClick={() => onOpenDialog('add')} className="text-muted-foreground">
                      <Plus />
                      Add your first feed
                    </SidebarMenuButton>
                  </SidebarMenuItem>
                ) : null}
              </SidebarMenu>
            </SidebarGroupContent>
          </SidebarGroup>
        </SidebarContent>

        <SidebarFooter>
          <SidebarMenu>
            <NavItem icon={<FolderInput />} label="Import & export" onClick={() => onOpenDialog('transfer')} />
            <NavItem icon={<Keyboard />} label="Keyboard shortcuts" onClick={() => onOpenDialog('shortcuts')} />
            <NavItem icon={<Settings />} label="Settings" active={scope === null} onClick={onOpenSettings} />
          </SidebarMenu>
        </SidebarFooter>
      </Sidebar>

      <RemoveDialog removal={removal} onCancel={() => setRemoval(null)} onConfirm={remove} />
    </>
  )
})

type NavItemProps = {
  icon: ReactNode
  label: string
  onClick: () => void
  active?: boolean
  count?: number
}

function NavItem({ icon, label, onClick, active = false, count }: NavItemProps) {
  return (
    <SidebarMenuItem>
      <SidebarMenuButton isActive={active} onClick={onClick}>
        {icon}
        {label}
      </SidebarMenuButton>
      <UnreadCount value={count} />
    </SidebarMenuItem>
  )
}

type DragProps = ReturnType<ReturnType<typeof useTreeDrag>['feed']>

type FeedRowProps = {
  feed: SidebarFeed
  nested: boolean
  active: boolean
  renaming: boolean
  dropBefore: boolean
  dragProps: DragProps
  onOpen: () => void
  onRename: () => void
  onRenamed: (title: string | undefined) => void
  onRefresh: () => void
  onRules: (() => void) | undefined
  onRemove: () => void
}

function FeedRow({ feed, nested, active, renaming, dropBefore, dragProps, onOpen, onRename, onRenamed, onRefresh, onRules, onRemove }: FeedRowProps) {
  return (
    <SidebarMenuItem className={cn(nested && 'pl-4', dropBefore && dropLine)} draggable={!renaming && !feed.pending} {...dragProps}>
      {feed.pending ? (
        <SidebarMenuButton isActive={active} onClick={onOpen} aria-busy className="text-muted-foreground data-[active=true]:text-foreground">
          <FeedIcon siteUrl={null} />
          <span className="truncate">{feed.title}</span>
        </SidebarMenuButton>
      ) : renaming ? (
        <RenameInput initial={feed.title} label="Feed name" onDone={onRenamed} />
      ) : (
        <>
          <SidebarMenuButton isActive={active} onClick={onOpen} className="text-muted-foreground hover:text-foreground data-[active=true]:text-foreground">
            <FeedIcon siteUrl={feed.site_url} />
            <span className="truncate">{feed.title}</span>
            {feed.last_error ? <AlertCircle className="text-warning" aria-label={`Not updating: ${feed.last_error}`} /> : null}
          </SidebarMenuButton>
          <UnreadCount value={feed.unread} />
          <RowMenu
            label={feed.title}
            onRename={onRename}
            onRefresh={onRefresh}
            onRules={onRules}
            destructiveLabel="Unsubscribe…"
            onDestroy={onRemove}
          />
        </>
      )}
    </SidebarMenuItem>
  )
}

type FolderRowsProps = {
  folder: SidebarFolder
  open: boolean
  active: boolean
  renaming: boolean
  dropBefore: boolean
  dropInto: boolean
  dragProps: DragProps
  onOpen: () => void
  onToggle: () => void
  onRename: () => void
  onRenamed: (name: string | undefined) => void
  onRefresh: () => void
  onRemove: () => void
  /** The folder's feed rows, shown while it's open. */
  children: ReactNode
}

function FolderRows({ folder, open, active, renaming, dropBefore, dropInto, dragProps, onOpen, onToggle, onRename, onRenamed, onRefresh, onRemove, children }: FolderRowsProps) {
  return (
    <>
      <SidebarMenuItem className={cn(dropBefore && dropLine)} draggable={!renaming} {...dragProps}>
        {renaming ? (
          <RenameInput initial={folder.name} label="Folder name" onDone={onRenamed} />
        ) : (
          <>
            <SidebarMenuButton isActive={active} onClick={onOpen} className={cn('pl-8', dropInto && 'bg-signal/12')}>
              <span className="truncate">{folder.name}</span>
            </SidebarMenuButton>
            <button
              type="button"
              aria-expanded={open}
              aria-label={open ? `Collapse ${folder.name}` : `Expand ${folder.name}`}
              onClick={onToggle}
              className="absolute top-1.5 left-1.5 grid size-5 place-items-center rounded-sm text-faint hover:text-foreground"
            >
              <ChevronRight className={cn('size-3.5 transition-transform motion-reduce:transition-none', open && 'rotate-90')} />
            </button>
            <UnreadCount value={folder.unread} />
            <RowMenu
              label={folder.name}
              onRename={onRename}
              onRefresh={onRefresh}
              destructiveLabel="Delete folder…"
              onDestroy={onRemove}
            />
          </>
        )}
      </SidebarMenuItem>
      {open ? children : null}
    </>
  )
}
