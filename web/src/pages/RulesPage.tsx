import { Plus, X } from 'lucide-react'
import { type FormEvent, useRef, useState } from 'react'
import { toast } from 'sonner'
import { Link } from 'wouter'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useDocumentTitle } from '@/hooks/use-document-title'
import { useShortcuts } from '@/hooks/use-shortcuts'
import type { Rule } from '../api'
import type { Chrome } from '../chrome'
import { DinoLoader } from '../components/DinoLoader'
import { IconAction } from '../components/IconAction'
import { TopBar } from '../components/TopBar'
import { useRules, useSaveRules, useSettings } from '../queries'
import { SETTINGS_PATH } from '../routes'

type Props = { chrome: Chrome; feed: string }

export function RulesPage({ chrome, feed }: Props) {
  const title = chrome.lookup.feed(feed)?.title ?? feed
  const { data: rules, isError } = useRules(feed)
  const aiEnabled = useSettings().data?.ai_enabled ?? false
  useShortcuts(chrome.shortcuts, chrome.shortcutsEnabled)
  useDocumentTitle(`Rules for ${title}`, chrome.sidebar?.total_unread ?? 0)

  return (
    <>
      <TopBar chrome={chrome} title="Rules" />
      <div className="flex-1 overflow-y-auto">
        <div className="mx-auto max-w-190 px-8 pt-11 pb-24 max-md:px-5">
          <h1 className="text-[32px] leading-tight font-semibold tracking-[-0.025em]">Rules for {title}</h1>
          <p className="mt-2 max-w-[62ch] text-[15px] text-muted-foreground">
            Jev reads each new article from {title} and applies these rules before it reaches your list. When you
            change the rules, Jev goes through every article again, hidden ones too, and brings back any the new
            rules let through. Nothing is deleted, and starred articles always stay.
          </p>
          {aiEnabled ? null : (
            <p className="mt-6 rounded-lg bg-secondary px-4 py-3 text-[14px]">
              AI features are off, so these rules aren&rsquo;t applied.{' '}
              <Link href={SETTINGS_PATH} className="font-medium underline underline-offset-2">
                Turn them on in Settings
              </Link>
              .
            </p>
          )}
          {isError ? (
            <p className="mt-10 text-[15px]">This feed no longer exists.</p>
          ) : rules === undefined ? (
            <DinoLoader label="Loading the rules…" />
          ) : (
            // Keyed by feed so moving to another feed's rules starts from its saved list.
            <RulesEditor key={feed} feed={feed} saved={rules} aiEnabled={aiEnabled} />
          )}
        </div>
      </div>
    </>
  )
}

/** A rule being edited, with a key that stays put while rows are added and removed. */
type Draft = Rule & { key: number }

function RulesEditor({ feed, saved, aiEnabled }: { feed: string; saved: Rule[]; aiEnabled: boolean }) {
  const [rules, setRules] = useState<Draft[]>(() => saved.map((rule, key) => ({ ...rule, key })))
  const nextKey = useRef(saved.length)
  const addRule = () => setRules((current) => [...current, { condition: '', action: 'hide', key: nextKey.current++ }])
  const save = useSaveRules(feed)
  const update = (key: number, change: Partial<Rule>) =>
    setRules((current) => current.map((rule) => (rule.key === key ? { ...rule, ...change } : rule)))
  const complete = rules.every((rule) => rule.condition.trim() !== '')
  const changed = JSON.stringify(rules.map(({ condition, action }) => ({ condition, action }))) !== JSON.stringify(saved)

  const submit = (event: FormEvent) => {
    event.preventDefault()
    save.mutate(
      rules.map(({ condition, action }) => ({ condition: condition.trim(), action })),
      { onSuccess: () => toast(aiEnabled ? 'Rules saved. Jev is checking the articles already here.' : 'Rules saved') },
    )
  }

  return (
    <form onSubmit={submit} className="mt-10">
      {rules.length === 0 ? (
        <p className="text-[15px] text-muted-foreground">No rules yet, so every new article reaches your list.</p>
      ) : (
        <ol className="flex flex-col gap-3">
          {rules.map((rule) => (
            <li key={rule.key} className="flex items-center gap-2 max-sm:flex-wrap">
              <Select value={rule.action} onValueChange={(action: Rule['action']) => update(rule.key, { action })}>
                <SelectTrigger className="w-32 shrink-0" aria-label="What happens">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent className="dark">
                  <SelectItem value="hide">Hide</SelectItem>
                  <SelectItem value="keep_only">Only keep</SelectItem>
                </SelectContent>
              </Select>
              <span className="shrink-0 text-[14px] text-muted-foreground">articles about</span>
              <Input
                value={rule.condition}
                onChange={(event) => update(rule.key, { condition: event.target.value })}
                placeholder="soccer news"
                aria-label="Which articles"
                className="min-w-0 flex-1"
              />
              <IconAction label="Remove rule" onClick={() => setRules((current) => current.filter((r) => r.key !== rule.key))}>
                <X />
              </IconAction>
            </li>
          ))}
        </ol>
      )}

      <div className="mt-5 flex items-center gap-2">
        <Button type="button" variant="secondary" onClick={addRule}>
          <Plus /> Add rule
        </Button>
        <Button type="submit" disabled={!changed || !complete || save.isPending}>
          Save
        </Button>
      </div>

      <p className="mt-8 max-w-[62ch] text-[13.5px] text-muted-foreground">
        <strong className="font-medium text-foreground">Hide</strong> keeps out the articles that match.{' '}
        <strong className="font-medium text-foreground">Only keep</strong> keeps out everything that matches none of
        your Only keep rules. Hide wins when both apply.
      </p>
    </form>
  )
}
