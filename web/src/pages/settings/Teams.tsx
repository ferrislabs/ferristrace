import { Plus } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { because } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { Dialog, DialogContent, DialogFooter } from '@/components/ui/dialog'
import { Field, Input } from '@/components/ui/input'
import { Table, Td, Th, Tr } from '@/components/ui/misc'
import { useMembers } from '@/data/members'
import { useProjects } from '@/data/projects'
import {
  initialsOf,
  teamNameProblem,
  useCreateTeam,
  useDisbandTeam,
  useOwnTeam,
  useRenameTeam,
  useSeatTeam,
  useTeams,
  type Team,
} from '@/data/teams'
import { OnCall } from './OnCall'
import { H } from './parts'

export function Teams() {
  const { active } = useOrganisations()
  const { teams, nothing } = useTeams()
  const [adding, setAdding] = useState(false)
  const [managing, setManaging] = useState<Team | null>(null)
  const [paging, setPaging] = useState<Team | null>(null)
  const mayArrange = active?.role === 'owner' || active?.role === 'admin'
  const roll = teams ?? []

  const open = managing && roll.find((team) => team.id === managing.id)
  const onCall = paging && roll.find((team) => team.id === paging.id)

  return (
    <>
      <H title="Teams" sub="A team groups members, owns projects and keeps its on-call schedules.">
        {mayArrange ? <Button variant="primary" size="sm" onClick={() => setAdding(true)}><Plus />New team</Button> : null}
      </H>
      <Card className="overflow-x-auto">
        <Table aria-label="Teams">
          <thead><tr><Th>Team</Th><Th numeric className="w-[100px]">Members</Th><Th className="w-[260px]">Projects</Th><Th className="w-[180px]"><span className="sr-only">Actions</span></Th></tr></thead>
          <tbody>{roll.map((t) => (
            <Tr key={t.id}>
              <Td className="h-11"><span className="inline-flex items-center gap-2.5"><span className="inline-flex size-7 items-center justify-center rounded-md bg-muted text-[11px] font-bold text-muted-foreground" aria-hidden>{initialsOf(t.name)}</span><span className="font-semibold">{t.name}</span></span></Td>
              <Td numeric>{t.members.length}</Td>
              <Td className="text-control">{t.projects.length > 0 ? t.projects.map((p) => p.name).join(', ') : <span className="text-muted-foreground">No project yet</span>}</Td>
              <Td numeric><Button variant="ghost" size="sm" onClick={() => setPaging(t)}>On-call</Button>{mayArrange ? <Button variant="ghost" size="sm" onClick={() => setManaging(t)}>Manage</Button> : null}</Td>
            </Tr>
          ))}</tbody>
        </Table>
        {roll.length === 0 ? <p className="m-0 px-4 py-3 text-control text-muted-foreground">{teams ? 'No team yet. One groups members and owns projects.' : nothing}</p> : null}
      </Card>
      <p className="m-0 text-caption text-muted-foreground">A project has one owner team or none, so handing it to a team takes it off the team that held it. Leaving the organisation takes you off its teams.</p>
      <NewTeam open={adding} onOpenChange={setAdding} />
      <ManageTeam team={open ?? null} onOpenChange={(o) => !o && setManaging(null)} />
      <OnCall team={onCall ?? null} mayArrange={mayArrange} onOpenChange={(o) => !o && setPaging(null)} />
    </>
  )
}

function NewTeam({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const create = useCreateTeam()
  const [name, setName] = useState('')
  const problem = name.length === 0 ? null : teamNameProblem(name)

  const close = (o: boolean) => {
    onOpenChange(o)
    if (!o) setName('')
  }

  return (
    <Dialog open={open} onOpenChange={close}>
      <DialogContent title="New team" description="Who is on it and what it owns come next.">
        <form
          className="flex flex-col gap-4"
          onSubmit={(e) => {
            e.preventDefault()
            create.mutate(name, {
              onSuccess: () => { close(false); toast.success(`Team ${name.trim()} created`) },
              onError: (error) => toast.error(because(error, 'The team was not created')),
            })
          }}
        >
          <Field label="Name" htmlFor="nt-n" hint="Shown wherever the team owns something."><Input id="nt-n" value={name} aria-invalid={problem ? true : undefined} onChange={(e) => setName(e.target.value)} autoFocus /></Field>
          {problem ? <span className="text-caption text-destructive-text">{problem}</span> : null}
          <DialogFooter><Button type="button" onClick={() => close(false)}>Cancel</Button><Button variant="primary" type="submit" disabled={name.trim().length === 0 || Boolean(problem) || create.isPending}>Create team</Button></DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

function ManageTeam({ team, onOpenChange }: { team: Team | null; onOpenChange: (o: boolean) => void }) {
  const { members } = useMembers()
  const projects = useProjects()
  const rename = useRenameTeam()
  const seat = useSeatTeam()
  const own = useOwnTeam()
  const disband = useDisbandTeam()
  const [typed, setTyped] = useState<string | null>(null)

  if (!team) return null
  const name = typed ?? team.name
  const problem = typed === null ? null : teamNameProblem(typed)
  const unchanged = name.trim() === team.name
  const owned = new Set(team.projects.map((p) => p.id))

  const failed = (what: string) => (error: unknown) => toast.error(because(error, what))

  const toggleMember = (account: string) =>
    seat.mutate(
      { team: team.id, members: team.members.includes(account) ? team.members.filter((one) => one !== account) : [...team.members, account] },
      { onError: failed('The team was not changed') },
    )

  const toggleProject = (project: string) =>
    own.mutate(
      { team: team.id, projects: owned.has(project) ? [...owned].filter((one) => one !== project) : [...owned, project] },
      { onError: failed('The projects were not changed') },
    )

  return (
    <Dialog open onOpenChange={(o) => { onOpenChange(o); if (!o) setTyped(null) }}>
      <DialogContent size="md" title={`Manage ${team.name}`} description="Who is on the team, and which projects it owns.">
        <Field label="Name" htmlFor="mt-n">
          <div className="flex gap-2">
            <Input id="mt-n" value={name} aria-invalid={problem ? true : undefined} onChange={(e) => setTyped(e.target.value)} />
            <Button size="sm" className="h-8" variant="primary" disabled={unchanged || Boolean(problem) || rename.isPending} onClick={() => rename.mutate({ team: team.id, name }, { onSuccess: () => { setTyped(null); toast.success('Saved') }, onError: failed('The team was not renamed') })}>Save</Button>
          </div>
        </Field>
        {problem ? <span className="text-caption text-destructive-text">{problem}</span> : null}

        <Picker label="Members" empty="Nobody belongs to this organisation yet.">
          {(members ?? []).map((m) => (
            <label key={m.account} className="flex cursor-pointer items-center gap-2.5 px-3 py-2 text-control">
              <input type="checkbox" className="size-4 accent-[var(--primary)]" checked={team.members.includes(m.account)} disabled={seat.isPending} onChange={() => toggleMember(m.account)} />
              <span className="flex-1 font-semibold">{m.email}</span>
              <span className="text-caption text-muted-foreground">{m.role}</span>
            </label>
          ))}
        </Picker>

        <Picker label="Projects it owns" empty="This organisation holds no project yet.">
          {projects.map((p) => (
            <label key={p.id} className="flex cursor-pointer items-center gap-2.5 px-3 py-2 text-control">
              <input type="checkbox" className="size-4 accent-[var(--primary)]" checked={owned.has(p.id)} disabled={own.isPending} onChange={() => toggleProject(p.id)} />
              <span className="flex-1 font-mono">{p.name}</span>
            </label>
          ))}
        </Picker>

        <DialogFooter>
          <Button className="mr-auto text-destructive-text" disabled={disband.isPending} onClick={() => disband.mutate(team.id, { onSuccess: () => { onOpenChange(false); toast(`Team ${team.name} disbanded`, { description: 'Its projects stayed, with no owner team.' }) }, onError: failed('The team was not disbanded') })}>Disband team</Button>
          <Button onClick={() => onOpenChange(false)}>Done</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function Picker({ label, empty, children }: { label: string; empty: string; children: React.ReactNode[] }) {
  return (
    <Field label={label}>
      <Card className="max-h-[180px] divide-y divide-border-subtle overflow-y-auto p-0">
        {children.length > 0 ? children : <p className="m-0 px-3 py-2 text-control text-muted-foreground">{empty}</p>}
      </Card>
    </Field>
  )
}
