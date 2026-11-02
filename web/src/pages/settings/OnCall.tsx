import { ArrowDown, ArrowUp, Plus, X } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { because } from '@/auth/client'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { Dialog, DialogContent, DialogFooter } from '@/components/ui/dialog'
import { Field, Input, NativeSelect } from '@/components/ui/input'
import { useMembers, type Member } from '@/data/members'
import {
  LENGTH_MAX_DAYS,
  LENGTH_MIN_DAYS,
  WEEKDAYS,
  draftProblem,
  moved,
  useAddOverride,
  useCreateSchedule,
  useDeleteSchedule,
  useOnCallNow,
  useRemoveOverride,
  useTeamSchedules,
  useUpdateSchedule,
  type Schedule,
  type ScheduleDraft,
  type Weekday,
} from '@/data/oncall'
import type { Team } from '@/data/teams'
import { stamp } from '@/lib/format'

const browserZone = () => Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC'

const today = () => new Date().toISOString().slice(0, 10)

const zones = (): string[] => {
  const listed = (Intl as unknown as { supportedValuesOf?: (key: string) => string[] }).supportedValuesOf
  return listed ? listed('timeZone') : ['UTC']
}

const blank = (): ScheduleDraft => ({
  name: '',
  timezone: browserZone(),
  rotation: [],
  handoff: { weekday: 'monday', time: '09:00' },
  length_days: 7,
  starts_on: today(),
})

const draftOf = (schedule: Schedule): ScheduleDraft => ({
  name: schedule.name,
  timezone: schedule.timezone,
  rotation: schedule.rotation,
  handoff: schedule.handoff,
  length_days: schedule.length_days,
  starts_on: schedule.starts_on,
})

const capitalised = (day: Weekday) => day.charAt(0).toUpperCase() + day.slice(1)

function nameOf(members: Member[] | undefined, account: string) {
  const member = members?.find((one) => one.account === account)
  return member ? member.name ?? member.email : 'a former member'
}

export function OnCall({ team, mayArrange, onOpenChange }: { team: Team | null; mayArrange: boolean; onOpenChange: (o: boolean) => void }) {
  const { schedules, nothing } = useTeamSchedules(team?.id)
  const [editing, setEditing] = useState<Schedule | 'new' | null>(null)
  if (!team) return null
  return (
    <>
      <Dialog open onOpenChange={onOpenChange}>
        <DialogContent size="lg" title={`On-call for ${team.name}`} description="Who is paged when an alert or an incident asks for this team, and who comes next.">
          <div className="flex flex-col gap-3">
            {(schedules ?? []).map((schedule) => (
              <ScheduleCard key={schedule.id} schedule={schedule} mayArrange={mayArrange} onEdit={() => setEditing(schedule)} />
            ))}
            {schedules && schedules.length === 0 ? <p className="m-0 text-control text-muted-foreground">No schedule yet. A schedule rotates the team's members through on-call shifts.</p> : null}
            {schedules ? null : <p className="m-0 text-control text-muted-foreground">{nothing}</p>}
          </div>
          <DialogFooter>
            {mayArrange ? <Button className="mr-auto" variant="primary" onClick={() => setEditing('new')}><Plus />New schedule</Button> : null}
            <Button onClick={() => onOpenChange(false)}>Done</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <ScheduleEditor team={team} editing={editing} onClose={() => setEditing(null)} />
    </>
  )
}

function NowCard({ schedule }: { schedule: Schedule }) {
  const { now, failed } = useOnCallNow(schedule.id)
  return (
    <div className="flex flex-col gap-0.5 rounded-md bg-muted px-3 py-2">
      <span className="text-caption text-muted-foreground">On call now</span>
      {failed ? <span className="text-control text-destructive-text">Could not ask who is on call.</span> : null}
      {now ? (
        now.account ? (
          <span className="text-control"><span className="font-semibold">{now.account.name ?? now.account.email ?? 'a former member'}</span>{now.until_ms ? <span className="text-muted-foreground"> until {stamp(now.until_ms)}</span> : null}</span>
        ) : <span className="text-control text-muted-foreground">Nobody: the rotation is empty or has not started.</span>
      ) : null}
    </div>
  )
}

function ScheduleCard({ schedule, mayArrange, onEdit }: { schedule: Schedule; mayArrange: boolean; onEdit: () => void }) {
  const { members } = useMembers()
  const remove = useDeleteSchedule()
  const drop = useRemoveOverride()
  const failed = (what: string) => (error: unknown) => toast.error(because(error, what))
  return (
    <Card className="flex flex-col gap-3 p-3">
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-[15px] font-bold">{schedule.name}</span>
        <Badge variant="outline">{schedule.timezone}</Badge>
        <Badge variant="outline">every {schedule.length_days === 1 ? 'day' : `${schedule.length_days} days`}, {capitalised(schedule.handoff.weekday)} {schedule.handoff.time}</Badge>
        <span className="flex-1" />
        {mayArrange ? <Button size="sm" onClick={onEdit}>Edit</Button> : null}
        {mayArrange ? <Button size="sm" variant="ghost" className="text-destructive-text" disabled={remove.isPending} onClick={() => remove.mutate(schedule.id, { onSuccess: () => toast(`Schedule ${schedule.name} deleted`), onError: failed('The schedule was not deleted') })}>Delete</Button> : null}
      </div>
      <NowCard schedule={schedule} />
      <div className="flex flex-col gap-1">
        <span className="text-caption font-semibold text-muted-foreground">Rotation, from {schedule.starts_on}</span>
        {schedule.rotation.length > 0 ? (
          <ol className="m-0 flex list-decimal flex-col gap-0.5 pl-5 text-control">
            {schedule.rotation.map((account) => <li key={account}>{nameOf(members, account)}</li>)}
          </ol>
        ) : <span className="text-control text-muted-foreground">Nobody yet.</span>}
      </div>
      <div className="flex flex-col gap-1">
        <span className="text-caption font-semibold text-muted-foreground">Overrides</span>
        {schedule.overrides.length > 0 ? schedule.overrides.map((cover) => (
          <span key={cover.id} className="flex items-center gap-2 text-control">
            <span className="font-semibold">{nameOf(members, cover.account)}</span>
            <span className="font-mono text-caption text-muted-foreground">{stamp(cover.from_ms)} → {stamp(cover.to_ms)}</span>
            {mayArrange ? <Button size="icon-sm" variant="ghost" aria-label="Remove the override" disabled={drop.isPending} onClick={() => drop.mutate({ schedule: schedule.id, id: cover.id }, { onError: failed('The override was not removed') })}><X /></Button> : null}
          </span>
        )) : <span className="text-control text-muted-foreground">None coming.</span>}
        {mayArrange ? <NewOverride schedule={schedule} members={members ?? []} /> : null}
      </div>
    </Card>
  )
}

function NewOverride({ schedule, members }: { schedule: Schedule; members: Member[] }) {
  const add = useAddOverride()
  const [account, setAccount] = useState('')
  const [from, setFrom] = useState('')
  const [to, setTo] = useState('')
  const fromMs = Date.parse(from)
  const toMs = Date.parse(to)
  const ready = account !== '' && Number.isFinite(fromMs) && Number.isFinite(toMs) && fromMs < toMs
  return (
    <form
      className="flex flex-wrap items-end gap-2"
      onSubmit={(e) => {
        e.preventDefault()
        add.mutate({ schedule: schedule.id, account, from_ms: fromMs, to_ms: toMs }, {
          onSuccess: () => { setAccount(''); setFrom(''); setTo(''); toast.success('Override added') },
          onError: (error) => toast.error(because(error, 'The override was not added')),
        })
      }}
    >
      <Field label="Person" htmlFor={`ov-p-${schedule.id}`} className="flex-[1_1_160px]">
        <NativeSelect id={`ov-p-${schedule.id}`} value={account} onChange={(e) => setAccount(e.target.value)}>
          <option value="">Choose someone</option>
          {members.map((one) => <option key={one.account} value={one.account}>{one.name ?? one.email}</option>)}
        </NativeSelect>
      </Field>
      <Field label="From" htmlFor={`ov-f-${schedule.id}`}><Input id={`ov-f-${schedule.id}`} type="datetime-local" value={from} onChange={(e) => setFrom(e.target.value)} /></Field>
      <Field label="To" htmlFor={`ov-t-${schedule.id}`}><Input id={`ov-t-${schedule.id}`} type="datetime-local" value={to} onChange={(e) => setTo(e.target.value)} /></Field>
      <Button type="submit" size="sm" className="h-8" disabled={!ready || add.isPending}>Add override</Button>
    </form>
  )
}

function ScheduleEditor({ team, editing, onClose }: { team: Team; editing: Schedule | 'new' | null; onClose: () => void }) {
  const [draft, setDraft] = useState<ScheduleDraft | null>(null)
  const [opened, setOpened] = useState<Schedule | 'new' | null>(null)
  const { members } = useMembers()
  const create = useCreateSchedule()
  const update = useUpdateSchedule()
  if (editing !== opened) {
    setOpened(editing)
    setDraft(editing === null ? null : editing === 'new' ? blank() : draftOf(editing))
  }
  if (editing === null || draft === null) return null
  const set = (change: Partial<ScheduleDraft>) => setDraft({ ...draft, ...change })
  const problem = draftProblem(draft)
  const outside = team.members.filter((account) => !draft.rotation.includes(account))
  const pending = create.isPending || update.isPending
  const save = () => {
    const done = { onSuccess: () => { toast.success('Schedule saved'); onClose() }, onError: (error: unknown) => toast.error(because(error, 'The schedule was not saved')) }
    if (editing === 'new') create.mutate({ team: team.id, draft }, done)
    else update.mutate({ id: editing.id, change: draft }, done)
  }
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent size="md" title={editing === 'new' ? 'New schedule' : `Edit ${editing.name}`} description="The rotation hands the pager to the next person at each handoff.">
        <form className="flex flex-col gap-3" onSubmit={(e) => { e.preventDefault(); save() }}>
          <div className="flex flex-wrap gap-3">
            <Field label="Name" htmlFor="sc-n" className="flex-[1_1_180px]"><Input id="sc-n" value={draft.name} onChange={(e) => set({ name: e.target.value })} autoFocus /></Field>
            <Field label="Timezone" htmlFor="sc-z" className="flex-[1_1_180px]">
              <Input id="sc-z" list="sc-zones" mono value={draft.timezone} onChange={(e) => set({ timezone: e.target.value })} />
              <datalist id="sc-zones">{zones().map((zone) => <option key={zone} value={zone} />)}</datalist>
            </Field>
          </div>
          <div className="flex flex-wrap gap-3">
            <Field label="Handoff day" htmlFor="sc-d">
              <NativeSelect id="sc-d" value={draft.handoff.weekday} onChange={(e) => set({ handoff: { ...draft.handoff, weekday: e.target.value as Weekday } })}>
                {WEEKDAYS.map((day) => <option key={day} value={day}>{capitalised(day)}</option>)}
              </NativeSelect>
            </Field>
            <Field label="Handoff time" htmlFor="sc-t"><Input id="sc-t" type="time" value={draft.handoff.time} onChange={(e) => set({ handoff: { ...draft.handoff, time: e.target.value } })} /></Field>
            <Field label="Shift length (days)" htmlFor="sc-l" className="w-36"><Input id="sc-l" type="number" min={LENGTH_MIN_DAYS} max={LENGTH_MAX_DAYS} value={draft.length_days} onChange={(e) => set({ length_days: Number(e.target.value) })} /></Field>
            <Field label="Starts on" htmlFor="sc-s"><Input id="sc-s" type="date" value={draft.starts_on} onChange={(e) => set({ starts_on: e.target.value })} /></Field>
          </div>
          <Field label="Rotation" hint="In order. The first person takes the first shift after the start date.">
            <Card className="divide-y divide-border-subtle p-0">
              {draft.rotation.map((account, at) => (
                <div key={account} className="flex items-center gap-2 px-3 py-1.5 text-control">
                  <span className="w-5 font-mono text-caption text-muted-foreground">{at + 1}</span>
                  <span className="flex-1 font-semibold">{nameOf(members, account)}</span>
                  <Button type="button" size="icon-sm" variant="ghost" aria-label="Move up" disabled={at === 0} onClick={() => set({ rotation: moved(draft.rotation, at, -1) })}><ArrowUp /></Button>
                  <Button type="button" size="icon-sm" variant="ghost" aria-label="Move down" disabled={at === draft.rotation.length - 1} onClick={() => set({ rotation: moved(draft.rotation, at, 1) })}><ArrowDown /></Button>
                  <Button type="button" size="icon-sm" variant="ghost" aria-label="Take off the rotation" onClick={() => set({ rotation: draft.rotation.filter((one) => one !== account) })}><X /></Button>
                </div>
              ))}
              {draft.rotation.length === 0 ? <p className="m-0 px-3 py-2 text-control text-muted-foreground">Nobody yet.</p> : null}
            </Card>
          </Field>
          {outside.length > 0 ? (
            <div className="flex flex-wrap gap-1.5">
              {outside.map((account) => <Button key={account} type="button" size="sm" onClick={() => set({ rotation: [...draft.rotation, account] })}><Plus />{nameOf(members, account)}</Button>)}
            </div>
          ) : team.members.length === 0 ? <span className="text-caption text-muted-foreground">The team has no member yet. Seat people on it first.</span> : null}
          {problem && draft.name.length > 0 ? <span className="text-caption text-destructive-text">{problem}</span> : null}
          <DialogFooter>
            <Button type="button" onClick={onClose}>Cancel</Button>
            <Button type="submit" variant="primary" disabled={Boolean(problem) || pending}>Save schedule</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
