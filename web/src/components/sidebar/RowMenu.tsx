import { MoreHorizontal } from 'lucide-react'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { SidebarMenuAction, SidebarMenuBadge } from '@/components/ui/sidebar'

type Props = {
  label: string
  onRename: () => void
  onRefresh: () => void
  /** Offered only while AI features are on. */
  onRules?: () => void
  destructiveLabel: string
  onDestroy: () => void
}

export function RowMenu({ label, onRename, onRefresh, onRules, destructiveLabel, onDestroy }: Props) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <SidebarMenuAction showOnHover aria-label={`Options for ${label}`}>
          <MoreHorizontal />
        </SidebarMenuAction>
      </DropdownMenuTrigger>
      <DropdownMenuContent side="right" align="start" className="dark">
        <DropdownMenuItem onSelect={onRename}>Rename</DropdownMenuItem>
        <DropdownMenuItem onSelect={onRefresh}>Refresh now</DropdownMenuItem>
        {onRules ? <DropdownMenuItem onSelect={onRules}>AI rules…</DropdownMenuItem> : null}
        <DropdownMenuSeparator />
        <DropdownMenuItem variant="destructive" onSelect={onDestroy}>
          {destructiveLabel}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

/** Unread count that makes room for the row's menu button on hover. */
export function UnreadCount({ value }: { value: number | undefined }) {
  if (!value) return null
  return (
    <SidebarMenuBadge className="font-normal text-faint tabular-nums group-focus-within/menu-item:opacity-0 group-hover/menu-item:opacity-0 group-has-data-[state=open]/menu-item:opacity-0">
      {value}
    </SidebarMenuBadge>
  )
}
