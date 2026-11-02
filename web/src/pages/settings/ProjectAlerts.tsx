import { AlertTriangle, Copy, Pencil, Plus, TriangleAlert } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { because } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { Dialog, DialogContent, DialogFooter } from '@/components/ui/dialog'
import { Field, Input } from '@/components/ui/input'
import { Segmented } from '@/components/ui/segmented'
import { Alert, Table, Td, Th, Tr } from '@/components/ui/misc'
import { isForbidden, isNotConfigured, useChannels, useCreateChannel, useDeleteChannel, useUpdateChannel, type Channel, type ChannelKind, type IssuedChannel } from '@/data/alerts'
import { H } from './parts'

type Editing = { channel: Channel | null } | null

const addresses = (text: string) => text.split(',').map((one) => one.trim()).filter((one) => one.length > 0)

function problemOf(kind: ChannelKind, name: string, target: string): string | null {
  if (!name.trim()) return 'A channel needs a name'
  if (kind === 'email') {
    const found = addresses(target)
    if (found.length === 0) return 'Give at least one address'
    const bad = found.find((one) => !/^[^\s@]+@[^\s@]+$/.test(one))
    return bad ? `"${bad}" is not an address` : null
  }
  try {
    const url = new URL(target.trim())
    return url.protocol === 'https:' || url.protocol === 'http:' ? null : 'The URL must start with https://'
  } catch {
    return 'The URL is not valid'
  }
}

export function ProjectAlerts() {
  const { active } = useOrganisations()
  const { channels, failed, nothing } = useChannels()
  const create = useCreateChannel()
  const update = useUpdateChannel()
  const remove = useDeleteChannel()
  const [editing, setEditing] = useState<Editing>(null)
  const [kind, setKind] = useState<ChannelKind>('email')
  const [name, setName] = useState('')
  const [target, setTarget] = useState('')
  const [issued, setIssued] = useState<IssuedChannel | null>(null)
  const [deleting, setDeleting] = useState<Channel | null>(null)
  const mayChange = active?.role === 'owner' || active?.role === 'admin'
  const editingExisting = editing?.channel ?? null
  const shape = editingExisting?.kind ?? kind
  const problem = problemOf(shape, name, target)

  const close = () => {
    setEditing(null)
    setIssued(null)
    setName('')
    setTarget('')
    setKind('email')
  }
  const begin = (channel: Channel | null) => {
    setEditing({ channel })
    setName(channel?.name ?? '')
    setTarget(channel?.target ?? '')
    setKind(channel?.kind ?? 'email')
  }
  const normal = shape === 'email' ? addresses(target).join(', ') : target.trim()

  const submit = () => {
    if (problem) return
    if (editingExisting) {
      update.mutate({ id: editingExisting.id, name: name.trim(), target: normal }, { onSuccess: () => { toast.success('Channel saved'); close() }, onError: (error) => toast.error(because(error, 'The channel was not saved')) })
      return
    }
    create.mutate({ name: name.trim(), kind, target: normal }, {
      onSuccess: (made) => {
        if (made.secret) setIssued(made)
        else { toast.success('Channel created'); close() }
      },
      onError: (error) => toast.error(because(error, 'The channel was not created')),
    })
  }

  return (
    <>
      <H title="Alert channels" sub="Where an alert goes: email addresses or a signed webhook. Rules pick their channels in the rule editor.">
        {mayChange ? <Button variant="primary" size="sm" onClick={() => begin(null)}><Plus />New channel</Button> : null}
      </H>
      {channels && !mayChange ? <span className="text-control text-muted-foreground">Read only. Only an owner or an administrator can change a channel.</span> : null}
      {isNotConfigured(failed) ? (
        <Alert variant="warning" icon={<TriangleAlert />} title="Alerting is not configured">This server does not run the alert evaluator. Ask the operator to enable it.</Alert>
      ) : isForbidden(failed) ? (
        <Alert variant="warning" icon={<TriangleAlert />} title="You may not read alert channels">Your role in this organisation does not allow it.</Alert>
      ) : (
        <Card className="overflow-x-auto">
          {channels ? (
            <Table aria-label="Alert channels">
              <thead><tr><Th>Name</Th><Th className="w-[100px]">Kind</Th><Th>Target</Th>{mayChange ? <Th className="w-[160px]"><span className="sr-only">Actions</span></Th> : null}</tr></thead>
              <tbody>
                {channels.length === 0 ? <Tr><Td colSpan={mayChange ? 4 : 3} className="text-muted-foreground">No channel yet. A rule with no channel changes state without telling anyone.</Td></Tr> : null}
                {channels.map((one) => (
                  <Tr key={one.id}>
                    <Td className="font-semibold">{one.name}</Td>
                    <Td><Badge variant="outline">{one.kind}</Badge></Td>
                    <Td className="whitespace-normal break-all font-mono text-control">{one.target}{one.has_secret ? <Badge variant="neutral" className="ml-2">signed</Badge> : null}</Td>
                    {mayChange ? <Td numeric><span className="inline-flex gap-1"><Button variant="ghost" size="sm" onClick={() => begin(one)}><Pencil />Edit</Button><Button variant="ghost" size="sm" className="text-destructive-text" onClick={() => setDeleting(one)}>Delete</Button></span></Td> : null}
                  </Tr>
                ))}
              </tbody>
            </Table>
          ) : <div className="px-4 py-3 text-control text-muted-foreground">{nothing}</div>}
        </Card>
      )}

      <Dialog open={editing !== null} onOpenChange={(o) => !o && close()}>
        <DialogContent size="md" title={issued ? `Secret for ${issued.name}` : editingExisting ? 'Edit channel' : 'New channel'} description={issued ? 'Signs every delivery to this webhook.' : undefined}>
          {issued?.secret ? (
            <>
              <Alert variant="warning" icon={<AlertTriangle />}>Copy this secret now. It is shown once and cannot be retrieved later.</Alert>
              <div className="flex gap-2"><Input readOnly mono value={issued.secret} className="bg-muted text-[12px]" aria-label="Webhook secret" /><Button onClick={() => { void navigator.clipboard?.writeText(issued.secret ?? ''); toast.success('Secret copied') }}><Copy />Copy</Button></div>
              <p className="m-0 text-control text-muted-foreground">Each request carries an <span className="font-mono">X-FerrisTrace-Signature: sha256=&lt;hex&gt;</span> header, the HMAC-SHA256 of the request body under this secret. Recompute it on your side and compare before trusting the payload.</p>
              <DialogFooter><Button variant="primary" onClick={close}>Done</Button></DialogFooter>
            </>
          ) : (
            <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); submit() }}>
              {editingExisting ? null : <Field label="Kind"><div><Segmented label="Kind" value={kind} onChange={setKind} options={[{ value: 'email', label: 'Email' }, { value: 'webhook', label: 'Webhook' }]} /></div></Field>}
              <Field label="Name" htmlFor="ch-n"><Input id="ch-n" value={name} onChange={(e) => setName(e.target.value)} autoFocus /></Field>
              <Field label={shape === 'email' ? 'Addresses' : 'URL'} htmlFor="ch-t" hint={shape === 'email' ? 'Comma separated.' : 'Private, loopback and link-local addresses are refused unless the deployment allows them.'}>
                <Input id="ch-t" mono value={target} placeholder={shape === 'email' ? 'oncall@example.com, team@example.com' : 'https://hooks.example.com/ferristrace'} onChange={(e) => setTarget(e.target.value)} />
              </Field>
              {problem && (name || target) ? <span className="text-caption text-destructive-text">{problem}</span> : null}
              <DialogFooter><Button type="button" onClick={close}>Cancel</Button><Button variant="primary" type="submit" disabled={problem !== null || create.isPending || update.isPending}>{editingExisting ? 'Save channel' : 'Create channel'}</Button></DialogFooter>
            </form>
          )}
        </DialogContent>
      </Dialog>

      <Dialog open={deleting !== null} onOpenChange={(o) => !o && setDeleting(null)}>
        <DialogContent role="alertdialog" title={`Delete ${deleting?.name ?? 'this channel'}?`} description="Rules that notify it stop notifying it. This cannot be undone.">
          <DialogFooter>
            <Button onClick={() => setDeleting(null)}>Cancel</Button>
            <Button
              variant="destructive"
              disabled={remove.isPending}
              onClick={() => {
                if (!deleting) return
                remove.mutate(deleting.id, { onSuccess: () => { setDeleting(null); toast.success('Channel deleted') }, onError: (error) => toast.error(because(error, 'The channel was not deleted')) })
              }}
            >Delete channel</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  )
}
