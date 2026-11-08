import { useState } from 'react'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { NativeSelect } from '@/components/ui/input'
import { Segmented } from '@/components/ui/segmented'
import { Switch } from '@/components/ui/switch'
import { Table, Td, Th, Tr } from '@/components/ui/misc'
import { because } from '@/auth/client'
import { useAccount } from '@/auth/session'
import { useNotificationPreferences, useSetNotificationPreferences, type NotificationKind, type NotificationPreferences } from '@/data/notifications'
import { timeZoneNames } from '@/data/orgs'
import { usePreferences, useSetPreferences, type AccountPreferences } from '@/data/profile'
import { useTheme, type ThemePref } from '@/lib/theme'
import { H, Row, Sub } from './parts'

const DEFAULTS: AccountPreferences = { timezone: 'UTC', precision: 'ms', density: 'compact', start_page: 'overview' }

const LABELS: Record<NotificationKind, { title: string; sub: string }> = {
  issue: { title: 'Exception alerts for my teams', sub: 'Rules set per project, e.g. new exception type' },
  service_silent: { title: 'A service stops sending', sub: 'No spans for 10 min' },
  status_page: { title: 'Status page incidents and updates', sub: 'Posted by anyone in the workspace' },
  api_key_expiring: { title: 'Api keys about to expire', sub: '7 days and 1 day before' },
  usage: { title: 'Usage and billing', sub: 'Admins only · 50 %, 80 % and 100 % of included' },
  membership: { title: 'Members join or leave', sub: 'Admins only' },
}

const KIND_ORDER: NotificationKind[] = ['issue', 'service_silent', 'status_page', 'api_key_expiring', 'usage', 'membership']

function Display() {
  const { pref, setPref } = useTheme()
  const { data: kept, isPending } = usePreferences()
  const set = useSetPreferences()
  const [draft, setDraft] = useState<AccountPreferences | null>(null)
  const base = kept ?? DEFAULTS
  const shown = draft ?? base
  const unchanged = draft === null || JSON.stringify(draft) === JSON.stringify(base)
  const zones = timeZoneNames()

  const save = () =>
    set.mutate(shown, {
      onSuccess: () => {
        setDraft(null)
        toast.success('Saved')
      },
      onError: (error) => toast.error(because(error, 'Your preferences were not saved')),
    })

  return (
    <Card className="flex flex-col gap-3.5 overflow-hidden p-0">
      <Row title="Theme" sub="Follows your system unless you pick one."><Segmented label="Theme" value={pref} onChange={(v: ThemePref) => setPref(v)} options={[{ value: 'system', label: 'System' }, { value: 'light', label: 'Light' }, { value: 'dark', label: 'Dark' }]} /></Row>
      <Row title="Time zone" sub="Used for every timestamp. Raw values stay in UTC nanoseconds.">
        <NativeSelect className="w-[240px]" aria-label="Time zone" disabled={isPending} value={shown.timezone} onChange={(e) => setDraft({ ...shown, timezone: e.target.value })}>
          {zones.map((name) => <option key={name} value={name}>{name}</option>)}
        </NativeSelect>
      </Row>
      <Row title="Timestamp precision" sub="In lists. Details always show the full value.">
        <Segmented label="Precision" value={shown.precision} onChange={(v) => setDraft({ ...shown, precision: v })} options={[{ value: 'ms', label: 'ms' }, { value: 'us', label: 'µs' }, { value: 'ns', label: 'ns' }]} />
      </Row>
      <Row title="Row density" sub="Traces, logs and tables.">
        <Segmented label="Density" value={shown.density} onChange={(v) => setDraft({ ...shown, density: v })} options={[{ value: 'compact', label: 'Compact' }, { value: 'comfortable', label: 'Comfortable' }]} />
      </Row>
      <Row title="Start page" sub="What opens after sign-in.">
        <NativeSelect className="w-[200px]" aria-label="Start page" value={shown.start_page} onChange={(e) => setDraft({ ...shown, start_page: e.target.value as AccountPreferences['start_page'] })}>
          <option value="overview">Overview</option>
          <option value="services">Services</option>
          <option value="traces">Traces</option>
        </NativeSelect>
      </Row>
      <div className="px-4 py-3"><Button variant="primary" size="sm" disabled={unchanged || set.isPending} onClick={save}>Save</Button></div>
    </Card>
  )
}

function Notifications() {
  const account = useAccount()
  const { data: kept, isPending } = useNotificationPreferences()
  const set = useSetNotificationPreferences()
  const [draft, setDraft] = useState<NotificationPreferences | null>(null)
  const shown = draft ?? kept
  const unchanged = draft === null || JSON.stringify(draft) === JSON.stringify(kept)

  const channelFor = (kind: NotificationKind) => shown?.channels.find((c) => c.kind === kind)
  const setChannel = (kind: NotificationKind, key: 'in_app' | 'email', value: boolean) => {
    if (!shown) return
    setDraft({ ...shown, channels: shown.channels.map((c) => (c.kind === kind ? { ...c, [key]: value } : c)) })
  }

  const save = () => {
    if (!shown) return
    set.mutate(shown, {
      onSuccess: () => {
        setDraft(null)
        toast.success('Saved')
      },
      onError: (error) => toast.error(because(error, 'Your notification preferences were not saved')),
    })
  }

  return (
    <>
      <Card className="overflow-hidden">
        <Row title="Email" sub={account?.email ?? '—'}>
          <Switch checked={shown?.email_enabled ?? true} disabled={isPending} onCheckedChange={(v) => shown && setDraft({ ...shown, email_enabled: v })} aria-label="Email notifications" />
        </Row>
        <Row title="Weekly digest" sub="Monday 09:00 · errors, slowest services, usage">
          <Switch checked={shown?.weekly_digest ?? false} disabled={isPending} onCheckedChange={(v) => shown && setDraft({ ...shown, weekly_digest: v })} aria-label="Weekly digest" />
        </Row>
      </Card>
      <Card className="overflow-x-auto">
        <Table aria-label="What to notify me about">
          <thead><tr><Th>Notify me when</Th><Th numeric className="w-[90px]">In-app</Th><Th numeric className="w-[90px]">Email</Th></tr></thead>
          <tbody>{KIND_ORDER.map((kind) => {
            const label = LABELS[kind]
            const channel = channelFor(kind)
            return (
              <Tr key={kind}>
                <Td className="whitespace-normal py-2"><div className="flex flex-col gap-0.5"><span className="text-control font-semibold">{label.title}</span><span className="text-caption text-muted-foreground">{label.sub}</span></div></Td>
                <Td numeric><Switch checked={channel?.in_app ?? true} disabled={isPending} onCheckedChange={(v) => setChannel(kind, 'in_app', v)} aria-label={`In-app: ${label.title}`} /></Td>
                <Td numeric><Switch checked={channel?.email ?? true} disabled={isPending} onCheckedChange={(v) => setChannel(kind, 'email', v)} aria-label={`Email: ${label.title}`} /></Td>
              </Tr>
            )
          })}</tbody>
        </Table>
      </Card>
      <div><Button variant="primary" size="sm" disabled={unchanged || set.isPending} onClick={save}>Save</Button></div>
    </>
  )
}

export function Preferences() {
  return (
    <>
      <H title="Preferences" sub="Only for you, on every device." />
      <Sub>Display</Sub>
      <Display />
      <Sub>Notifications</Sub>
      <Notifications />
    </>
  )
}
