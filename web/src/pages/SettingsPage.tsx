import { type FormEvent, useId, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import { useDocumentTitle } from '@/hooks/use-document-title'
import { useShortcuts } from '@/hooks/use-shortcuts'
import type { Chrome } from '../chrome'
import { TopBar } from '../components/TopBar'
import { useSettings, useSettingsActions } from '../queries'

export function SettingsPage({ chrome }: { chrome: Chrome }) {
  const { data: settings } = useSettings()
  const { saveApiKey, removeApiKey, setAi } = useSettingsActions()
  const [replacing, setReplacing] = useState(false)
  const [key, setKey] = useState('')
  const keyId = useId()
  const aiId = useId()
  useShortcuts(chrome.shortcuts, chrome.shortcutsEnabled)
  useDocumentTitle('Settings', chrome.sidebar?.total_unread ?? 0)

  const save = (event: FormEvent) => {
    event.preventDefault()
    saveApiKey.mutate(key, {
      onSuccess: () => {
        setKey('')
        setReplacing(false)
      },
    })
  }
  const savedKey = settings?.api_key ?? null
  const editing = savedKey === null || replacing

  return (
    <>
      <TopBar chrome={chrome} title="Settings" />
      <div className="flex-1 overflow-y-auto">
        <div className="mx-auto max-w-190 px-8 pt-11 pb-24 max-md:px-5">
          <h1 className="text-[32px] leading-tight font-semibold tracking-[-0.025em]">Settings</h1>

          <section className="mt-10 border-t pt-8" aria-labelledby="ai-heading">
            <h2 id="ai-heading" className="text-[17px] font-semibold">AI</h2>
            <p className="mt-1 max-w-[60ch] text-[14px] text-muted-foreground">
              feedrsauros can use Jev, a model by{' '}
              <a href="https://typesafe.ai" target="_blank" rel="noopener noreferrer" className="underline underline-offset-2 hover:text-foreground">
                TypeSafe
              </a>
              , to file the sites you add into your folders. Your API key is encrypted and stays on this machine.
            </p>

            <div className="mt-6">
              <label htmlFor={keyId} className="text-[14px] font-medium">
                TypeSafe API key
              </label>
              {editing ? (
                <form onSubmit={save} className="mt-2 flex max-w-md gap-2">
                  <Input
                    id={keyId}
                    type="password"
                    autoComplete="off"
                    spellCheck={false}
                    placeholder="Paste your API key"
                    value={key}
                    onChange={(event) => setKey(event.target.value)}
                  />
                  <Button type="submit" disabled={key.trim() === '' || saveApiKey.isPending}>
                    Save
                  </Button>
                  {replacing ? (
                    <Button type="button" variant="ghost" onClick={() => setReplacing(false)}>
                      Cancel
                    </Button>
                  ) : null}
                </form>
              ) : (
                <div className="mt-2 flex items-center gap-2">
                  <span id={keyId} className="text-[14px] text-muted-foreground">
                    Saved, ending in <span className="font-mono text-foreground">{savedKey.hint}</span>
                  </span>
                  <Button variant="secondary" size="sm" onClick={() => setReplacing(true)}>
                    Replace
                  </Button>
                  <Button variant="ghost" size="sm" onClick={() => removeApiKey.mutate(undefined)} disabled={removeApiKey.isPending}>
                    Remove
                  </Button>
                </div>
              )}
            </div>

            <div className="mt-8 flex max-w-lg items-start justify-between gap-6">
              <div>
                <label htmlFor={aiId} className="text-[14px] font-medium">
                  File new sites into folders
                </label>
                <p className="mt-1 text-[14px] text-muted-foreground">
                  {savedKey === null
                    ? 'Save an API key to turn this on.'
                    : "When you add a site without choosing a folder, Jev puts it in the one that fits best. If you have no folders, nothing is sent."}
                </p>
              </div>
              <Switch
                id={aiId}
                checked={settings?.ai_enabled ?? false}
                disabled={savedKey === null || setAi.isPending}
                onCheckedChange={(enabled) => setAi.mutate(enabled)}
                className="mt-1"
              />
            </div>
          </section>
        </div>
      </div>
    </>
  )
}
