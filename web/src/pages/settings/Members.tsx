import { Lock, Plus } from 'lucide-react'
import { useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@/lib/router'
import { toast } from 'sonner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { Dialog, DialogContent, DialogFooter } from '@/components/ui/dialog'
import { Field, Input, NativeSelect } from '@/components/ui/input'
import { Segmented } from '@/components/ui/segmented'
import { Avatar, Table, Td, Th, Tr } from '@/components/ui/misc'
import { because } from '@/auth/client'
import { type Role } from '@/auth/api'
import { useAccount, useOrganisations } from '@/auth/session'
import { mayChange, mayHandOut, theLastOwner, useGrantRole, useInvite, useMembers, useRemoveMember, type Member, type Welcomed } from '@/data/members'
import { initialsOf, nameOf } from '@/data/profile'
import { stamp } from '@/lib/format'
import { H, saved } from './parts'

export function Members() {
  const { active } = useOrganisations()
  const me = useAccount()
  const { members, nothing } = useMembers()
  const grant = useGrantRole()
  const remove = useRemoveMember()
  const client = useQueryClient()
  const navigate = useNavigate()
  const [invite, setInvite] = useState(false)
  const [leaving, setLeaving] = useState(false)
  const mine = active?.role
  const roll = members ?? []
  const administered = roll.filter((m) => m.role !== 'member').length

  const change = (member: Member, role: Role) =>
    grant.mutate({ account: member.account, role }, { onSuccess: saved, onError: (error) => toast.error(because(error, 'The role was not changed')) })

  const drop = (member: Member) =>
    remove.mutate(member.account, {
      onSuccess: () => {
        if (member.account !== me?.account) {
          toast(`${member.email} is no longer a member`)
          return
        }
        setLeaving(false)
        client.clear()
        navigate('/', { replace: true })
      },
      onError: (error) => toast.error(because(error, 'Nobody was removed')),
    })

  return (
    <>
      <H title="Members" sub={members ? `${roll.length} ${roll.length === 1 ? 'member' : 'members'} · ${administered} can administer it` : 'Who belongs to the organisation.'}>
        {mine === 'owner' || mine === 'admin' ? <Button variant="primary" onClick={() => setInvite(true)}><Plus />Invite member</Button> : null}
      </H>
      <Card className="overflow-x-auto">
        <Table aria-label="Members">
          <thead><tr><Th>Member</Th><Th className="w-[160px]">Role</Th><Th className="w-[190px]">Joined</Th><Th className="w-[110px]"><span className="sr-only">Actions</span></Th></tr></thead>
          <tbody>{roll.map((m) => {
            const you = m.account === me?.account
            const last = theLastOwner(roll, m)
            const editable = mayChange(mine, m.role) && !last
            return (
              <Tr key={m.account}>
                <Td className="h-11"><span className="inline-flex items-center gap-2.5"><Avatar initials={initialsOf(nameOf(m))} src={m.photo} /><span className="flex min-w-0 flex-col leading-4"><span className="truncate font-semibold">{nameOf(m)}</span>{m.name ? <span className="truncate text-caption text-muted-foreground">{m.email}</span> : null}</span>{you ? <Badge variant="outline">you</Badge> : null}</span></Td>
                <Td>{editable ? (
                  <NativeSelect aria-label={`Role for ${m.email}`} className="h-7 w-[130px]" value={m.role} disabled={grant.isPending} onChange={(e) => change(m, e.target.value as Role)}>
                    {mayHandOut(mine).map((role) => <option key={role} value={role}>{role}</option>)}
                  </NativeSelect>
                ) : (
                  <span className="inline-flex items-center gap-1.5 text-control text-muted-foreground"><Lock className="size-3" aria-hidden />{m.role}{last ? ' · last owner' : null}</span>
                )}</Td>
                <Td className="font-mono text-control">{stamp(m.joined_at_ms)}</Td>
                <Td numeric>{editable ? <Button variant="ghost" size="sm" className="text-destructive-text" disabled={remove.isPending} onClick={() => (you ? setLeaving(true) : drop(m))}>{you ? 'Leave' : 'Remove'}</Button> : null}</Td>
              </Tr>
            )
          })}</tbody>
        </Table>
        {roll.length === 0 ? <p className="m-0 px-4 py-3 text-control text-muted-foreground">{members ? 'Nobody belongs to this organisation.' : nothing}</p> : null}
      </Card>
      <p className="m-0 text-caption text-muted-foreground">Only an owner hands out the owner role or changes somebody who holds it, and an organisation keeps at least one owner — so the last one stays until it has handed the role on.</p>
      <InviteMember open={invite} onOpenChange={setInvite} mine={mine} />
      <Dialog open={leaving} onOpenChange={setLeaving}>
        <DialogContent role="alertdialog" title={`Leave ${active?.name ?? 'the organisation'}?`} description="You lose every screen of it at once. Somebody who stays has to invite you back.">
          <DialogFooter><Button onClick={() => setLeaving(false)}>Stay</Button><Button variant="destructive" disabled={remove.isPending} onClick={() => { const mineRow = roll.find((m) => m.account === me?.account); if (mineRow) drop(mineRow) }}>Leave</Button></DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  )
}

function InviteMember({ open, onOpenChange, mine }: { open: boolean; onOpenChange: (o: boolean) => void; mine: Role | undefined }) {
  const invite = useInvite()
  const [email, setEmail] = useState('')
  const [role, setRole] = useState<Role>('member')
  const [welcomed, setWelcomed] = useState<Welcomed | null>(null)

  const close = (o: boolean) => {
    onOpenChange(o)
    if (!o) { setWelcomed(null); setEmail(''); setRole('member') }
  }

  const send = (e: React.FormEvent) => {
    e.preventDefault()
    invite.mutate({ email, role }, { onSuccess: setWelcomed, onError: (error) => toast.error(because(error, 'Nobody was invited')) })
  }

  return (
    <Dialog open={open} onOpenChange={close}>
      <DialogContent
        title={welcomed ? `${welcomed.member.email} belongs to the organisation` : 'Invite member'}
        description={welcomed ? `Role: ${welcomed.member.role}. They sign in with single sign-on.` : 'An address that has no account here gets one, and signs in with single sign-on.'}
      >
        {welcomed ? (
          <DialogFooter><Button onClick={() => close(false)}>Done</Button></DialogFooter>
        ) : (
          <form className="flex flex-col gap-4" onSubmit={send}>
            <Field label="Email" htmlFor="iv-e"><Input id="iv-e" type="email" value={email} onChange={(e) => setEmail(e.target.value)} placeholder="somebody@example.com" autoFocus /></Field>
            <Field label="Role"><div><Segmented label="Role" value={role} onChange={(chosen) => setRole(chosen as Role)} options={mayHandOut(mine).map((one) => ({ value: one, label: one }))} /></div></Field>
            <span className="text-caption text-muted-foreground">{role === 'owner' ? 'Answers for the organisation: everything an administrator does, and the owner role itself.' : role === 'admin' ? 'Also manages members, projects, keys and billing.' : 'Reads traces, logs and metrics, and posts incidents.'}</span>
            <DialogFooter><Button type="button" onClick={() => close(false)}>Cancel</Button><Button variant="primary" type="submit" disabled={email.trim().length === 0 || invite.isPending}>Invite</Button></DialogFooter>
          </form>
        )}
      </DialogContent>
    </Dialog>
  )
}
