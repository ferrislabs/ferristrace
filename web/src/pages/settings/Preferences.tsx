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
import { timeZoneNames } from '@/data/orgs'
import { usePreferences, useSetPreferences, type AccountPreferences } from '@/data/profile'
import { useTheme, type ThemePref } from '@/lib/theme'
import { H, Row, Sub, saved } from './parts'

const DEFAULTS: AccountPreferences = { timezone: 'UTC', precision: 'ms', density: 'compact', start_page: 'overview' }

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

export function Preferences() {
  const account = useAccount()
  const [rows, setRows] = useState([
    ['Exception alerts for my teams', 'Rules set per project, e.g. new exception type', true, true],
    ['A service stops sending', 'No spans for 10 min', true, true],
    ['Status page incidents and updates', 'Posted by anyone in the workspace', true, false],
    ['Api keys about to expire', '7 days and 1 day before', true, true],
    ['Usage and billing', 'Admins only · 50 %, 80 % and 100 % of included', true, true],
    ['Members join or leave', 'Admins only', true, false],
  ] as [string, string, boolean, boolean][])
  const flip = (i: number, j: 2 | 3) => setRows((r) => r.map((x, k) => (k === i ? (Object.assign([...x], { [j]: !x[j] }) as typeof x) : x)))
  return (
    <>
      <H title="Preferences" sub="Only for you, on every device." />
      <Sub>Display</Sub>
      <Display />
      <Sub>Notifications</Sub>
      <Card className="overflow-hidden">
        <Row title="Email" sub={account?.email ?? '—'}><Switch defaultChecked aria-label="Email notifications" /></Row>
        <Row title="Weekly digest" sub="Monday 09:00 · errors, slowest services, usage"><Switch aria-label="Weekly digest" /></Row>
      </Card>
      <Card className="overflow-x-auto">
        <Table aria-label="What to notify me about">
          <thead><tr><Th>Notify me when</Th><Th numeric className="w-[90px]">In-app</Th><Th numeric className="w-[90px]">Email</Th></tr></thead>
          <tbody>{rows.map((r, i) => (
            <Tr key={r[0]}><Td className="whitespace-normal py-2"><div className="flex flex-col gap-0.5"><span className="text-control font-semibold">{r[0]}</span><span className="text-caption text-muted-foreground">{r[1]}</span></div></Td>
              <Td numeric><Switch checked={r[2]} onCheckedChange={() => flip(i, 2)} aria-label={`In-app: ${r[0]}`} /></Td><Td numeric><Switch checked={r[3]} onCheckedChange={() => flip(i, 3)} aria-label={`Email: ${r[0]}`} /></Td></Tr>
          ))}</tbody>
        </Table>
      </Card>
      <div><Button variant="primary" size="sm" onClick={saved}>Save</Button></div>
    </>
  )
}
