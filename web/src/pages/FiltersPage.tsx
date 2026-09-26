import { type FormEvent, useId, useState } from 'react'
import { toast } from 'sonner'
import { Link } from 'wouter'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import { useDocumentTitle } from '@/hooks/use-document-title'
import { useShortcuts } from '@/hooks/use-shortcuts'
import type { Filters } from '../api'
import type { Chrome } from '../chrome'
import { DinoLoader } from '../components/DinoLoader'
import { TopBar } from '../components/TopBar'
import { useFilters, useSaveFilters, useSettings } from '../queries'
import { SETTINGS_PATH } from '../routes'

type Props = { chrome: Chrome; feed: string }

export function FiltersPage({ chrome, feed }: Props) {
  const title = chrome.lookup.feed(feed)?.title ?? feed
  const { data: filters, isError } = useFilters(feed)
  const aiEnabled = useSettings().data?.ai_enabled ?? false
  useShortcuts(chrome.shortcuts, chrome.shortcutsEnabled)
  useDocumentTitle(`Filters for ${title}`, chrome.sidebar?.total_unread ?? 0)

  return (
    <>
      <TopBar chrome={chrome} title="Filters" />
      <div className="flex-1 overflow-y-auto">
        <div className="mx-auto max-w-190 px-8 pt-11 pb-24 max-md:px-5">
          <h1 className="text-[32px] leading-tight font-semibold tracking-[-0.025em]">Filters for {title}</h1>
          <p className="mt-2 max-w-[62ch] text-[15px] text-muted-foreground">
            Say in your own words what you want from {title}. feedrsauros reads each article and keeps out what doesn&rsquo;t
            fit. Nothing is deleted: change your filters and every article is checked again. Starred articles always
            stay.
          </p>
          {aiEnabled ? null : (
            <p className="mt-6 rounded-lg bg-secondary px-4 py-3 text-[14px]">
              AI features are off, so these filters aren&rsquo;t applied.{' '}
              <Link href={SETTINGS_PATH} className="font-medium underline underline-offset-2">
                Turn them on in Settings
              </Link>
              .
            </p>
          )}
          {isError ? (
            <p className="mt-10 text-[15px]">This feed no longer exists.</p>
          ) : filters === undefined ? (
            <DinoLoader label="Loading the filters…" />
          ) : (
            // Keyed by feed so moving to another feed's filters starts from its saved text.
            <FiltersForm key={feed} feed={feed} saved={filters} aiEnabled={aiEnabled} />
          )}
        </div>
      </div>
    </>
  )
}

type FormProps = { feed: string; saved: Filters; aiEnabled: boolean }

function FiltersForm({ feed, saved, aiEnabled }: FormProps) {
  const [wanted, setWanted] = useState(saved.wanted ?? '')
  const [unwanted, setUnwanted] = useState(saved.unwanted ?? '')
  const save = useSaveFilters(feed)
  const changed = wanted.trim() !== (saved.wanted ?? '') || unwanted.trim() !== (saved.unwanted ?? '')

  const submit = (event: FormEvent) => {
    event.preventDefault()
    save.mutate(
      { wanted: wanted.trim() || null, unwanted: unwanted.trim() || null },
      { onSuccess: () => toast(aiEnabled ? 'Filters saved. Checking every article again.' : 'Filters saved') },
    )
  }

  return (
    <form onSubmit={submit} className="mt-10 flex flex-col gap-8">
      <FilterField
        label="What do you want to see?"
        hint="Leave it empty to see everything."
        placeholder="Deep dives on performance, databases and Rust"
        value={wanted}
        onChange={setWanted}
      />
      <FilterField
        label="What don't you want to see?"
        hint="Anything that fits this is hidden, even if it matches what you want."
        placeholder="Hiring announcements, event recaps and product promos"
        value={unwanted}
        onChange={setUnwanted}
      />
      <div>
        <Button type="submit" disabled={!changed || save.isPending}>
          Save
        </Button>
      </div>
    </form>
  )
}

type FieldProps = {
  label: string
  hint: string
  placeholder: string
  value: string
  onChange: (value: string) => void
}

function FilterField({ label, hint, placeholder, value, onChange }: FieldProps) {
  const id = useId()
  const hintId = useId()
  return (
    <section className="flex flex-col gap-2">
      <label htmlFor={id} className="text-[17px] font-semibold">
        {label}
      </label>
      <p id={hintId} className="text-[14px] text-muted-foreground">
        {hint}
      </p>
      <Textarea
        id={id}
        aria-describedby={hintId}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder={placeholder}
        className="min-h-24 max-w-2xl text-[15px] md:text-[15px]"
      />
    </section>
  )
}
