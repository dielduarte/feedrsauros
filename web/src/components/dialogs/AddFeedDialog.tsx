import { type FormEvent, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import type { Sidebar } from '../../api'
import { useSettings } from '../../queries'
import { panel } from './panel'

const NO_FOLDER = 'none'

type Props = {
  sidebar: Sidebar | undefined
  /** Pre-filled when trying again after a failed add. */
  initialUrl: string
  /** Slug of the folder to preselect. */
  defaultFolder: string | null
  onClose: () => void
  /** The dialog closes straight away; the feed is added in the background. */
  onSubmit: (url: string, folder: string | null) => void
}

export function AddFeedDialog({ sidebar, initialUrl, defaultFolder, onClose, onSubmit }: Props) {
  const [url, setUrl] = useState(initialUrl)
  const [folder, setFolder] = useState(defaultFolder ?? NO_FOLDER)
  // With AI on, leaving the folder unset lets the server pick one.
  const aiEnabled = useSettings().data?.ai_enabled ?? false
  const address = url.trim()

  const submit = (event: FormEvent) => {
    event.preventDefault()
    if (address) onSubmit(address, folder === NO_FOLDER ? null : folder)
  }

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className={panel}>
        <form onSubmit={submit} className="grid gap-4">
          <DialogHeader>
            <DialogTitle>Add a feed</DialogTitle>
            <DialogDescription>Paste a website and feedrsauros finds its feed.</DialogDescription>
          </DialogHeader>

          <Input
            autoFocus
            autoComplete="off"
            data-1p-ignore
            data-lpignore="true"
            aria-label="Site or feed address"
            placeholder="example.com"
            value={url}
            onChange={(e) => setUrl(e.target.value)}
          />

          {sidebar && sidebar.folders.length > 0 ? (
            <Select value={folder} onValueChange={setFolder}>
              <SelectTrigger aria-label="Folder" className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent className="dark">
                <SelectItem value={NO_FOLDER}>{aiEnabled ? 'Choose for me' : 'No folder'}</SelectItem>
                {sidebar.folders.map((f) => (
                  <SelectItem key={f.slug} value={f.slug}>
                    {f.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          ) : null}

          <DialogFooter className="grid grid-cols-2 gap-2 sm:grid-cols-2">
            <DialogClose asChild>
              <Button type="button" variant="secondary">
                Cancel
              </Button>
            </DialogClose>
            <Button type="submit" disabled={!address}>
              Add feed
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
