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
  // Turning AI on without a saved key opens the key form; nothing is enabled until it's saved.
  const [settingUp, setSettingUp] = useState(false)
  const [replacing, setReplacing] = useState(false)
  const aiId = useId()
  useShortcuts(chrome.shortcuts, chrome.shortcutsEnabled)
  useDocumentTitle('Settings', chrome.sidebar?.total_unread ?? 0)

  const savedKey = settings?.api_key ?? null
  const enabled = settings?.ai_enabled ?? false

  const toggle = (on: boolean) => {
    if (!on) {
      setSettingUp(false)
      if (enabled) setAi.mutate(false)
    } else if (savedKey === null) {
      setSettingUp(true)
    } else {
      setAi.mutate(true)
    }
  }

  const saveKey = (key: string, then?: () => void) =>
    saveApiKey.mutate(key, {
      onSuccess: () => {
        setReplacing(false)
        then?.()
      },
    })

  return (
    <>
      <TopBar chrome={chrome} title="Settings" />
      <div className="flex-1 overflow-y-auto">
        <div className="mx-auto max-w-190 px-8 pt-11 pb-24 max-md:px-5">
          <h1 className="text-[32px] leading-tight font-semibold tracking-[-0.025em]">Settings</h1>

          <section className="mt-10 max-w-lg border-t pt-8">
            <div className="flex items-start justify-between gap-6">
              <div>
                <label htmlFor={aiId} className="text-[17px] font-semibold">
                  AI features
                </label>
                <p className="mt-1 text-[14px] text-muted-foreground">
                  File the sites you add into your folders, and filter each feed down to what you want to read.
                </p>
              </div>
              <Switch
                id={aiId}
                checked={enabled || settingUp}
                disabled={setAi.isPending}
                onCheckedChange={toggle}
                className="mt-1.5"
              />
            </div>

            {settingUp && !enabled ? (
              <div className="mt-5 border-l-2 pl-4">
                <p className="text-[14px] font-medium">Paste your typesafe.ai API key to finish.</p>
                <KeyForm
                  submitLabel="Turn on"
                  pending={saveApiKey.isPending || setAi.isPending}
                  onSubmit={(key) =>
                    saveKey(key, () =>
                      setAi.mutate(true, {
                        onSuccess: () => setSettingUp(false),
                      }),
                    )
                  }
                />
                <KeyNote />
              </div>
            ) : null}

            {enabled && savedKey !== null ? (
              <div className="mt-5 border-l-2 pl-4">
                {replacing ? (
                  <>
                    <p className="text-[14px] font-medium">Paste the new typesafe.ai API key.</p>
                    <KeyForm
                      submitLabel="Save"
                      pending={saveApiKey.isPending}
                      onSubmit={(key) => saveKey(key)}
                      onCancel={() => setReplacing(false)}
                    />
                  </>
                ) : (
                  <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-[14px]">
                    <span className="text-muted-foreground">
                      typesafe.ai key <span className="font-mono text-foreground">••••{savedKey.hint}</span>
                    </span>
                    <Button variant="ghost" size="sm" onClick={() => setReplacing(true)}>
                      Replace
                    </Button>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => removeApiKey.mutate(undefined)}
                      disabled={removeApiKey.isPending}
                    >
                      Remove
                    </Button>
                  </div>
                )}
              </div>
            ) : null}
          </section>
        </div>
      </div>
    </>
  )
}

type KeyFormProps = {
  submitLabel: string
  pending: boolean
  onSubmit: (key: string) => void
  onCancel?: () => void
}

function KeyForm({ submitLabel, pending, onSubmit, onCancel }: KeyFormProps) {
  const [key, setKey] = useState('')
  const submit = (event: FormEvent) => {
    event.preventDefault()
    onSubmit(key.trim())
  }
  return (
    <form onSubmit={submit} className="mt-2 flex gap-2">
      <Input
        type="password"
        autoComplete="off"
        spellCheck={false}
        autoFocus
        aria-label="typesafe.ai API key"
        placeholder="Paste your API key"
        value={key}
        onChange={(event) => setKey(event.target.value)}
      />
      <Button type="submit" disabled={key.trim() === '' || pending}>
        {submitLabel}
      </Button>
      {onCancel ? (
        <Button type="button" variant="ghost" onClick={onCancel}>
          Cancel
        </Button>
      ) : null}
    </form>
  )
}

function KeyNote() {
  return (
    <p className="mt-2 text-[13px] text-muted-foreground">
      Encrypted, and it never leaves this machine.{' '}
      <a href="https://typesafe.ai" target="_blank" rel="noopener noreferrer" className="underline underline-offset-2 hover:text-foreground">
        Get a key
      </a>
    </p>
  )
}
