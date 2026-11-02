import { AlertTriangle, Copy, KeyRound, Plus } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { Dialog, DialogContent, DialogFooter } from '@/components/ui/dialog'
import { Field, Input } from '@/components/ui/input'
import { Alert, Table, Td, Th, Tr } from '@/components/ui/misc'
import { useMock } from '@/data/seam'
import { INGEST_KEYS, QUERY_KEYS } from '@/mocks/mock'
import { cn } from '@/lib/utils'
import { H } from './parts'

export function ProjectApiKeys() {
  const readKeys = useMock(['organisation', 'read-keys'], () => QUERY_KEYS)
  const [keys, setKeys] = useState(useMock(['organisation', 'ingest-keys'], () => INGEST_KEYS))
  const [issue, setIssue] = useState(false)
  const [name, setName] = useState('edge-collector')
  const [secret, setSecret] = useState<string | null>(null)
  const [revoke, setRevoke] = useState<string | null>(null)
  return (
    <>
      <H title="Api keys" sub="Keys for storefront. Each key is shown once, then only its prefix." />
      <div className="mt-1 flex flex-wrap items-center gap-3"><span className="flex flex-1 flex-col"><span className="font-semibold">Ingest keys <span className="font-mono font-normal text-muted-foreground">ft_ik_</span></span><span className="text-control text-muted-foreground">Write only. Used by SDKs and collectors to send signals.</span></span><Button variant="primary" size="sm" onClick={() => setIssue(true)}><KeyRound />Issue ingest key</Button></div>
      <Card className="overflow-x-auto">
        <Table><thead><tr><Th>Name</Th><Th className="w-[200px]">Key</Th><Th className="w-[120px]">Created</Th><Th className="w-[170px]">Last used</Th><Th className="w-[100px]"><span className="sr-only">Actions</span></Th></tr></thead>
          <tbody>{keys.map((k) => (
            <Tr key={k.name}><Td className={cn('font-semibold', k.revoked && 'font-normal text-muted-foreground line-through')}>{k.name}</Td><Td className="font-mono">{k.prefix}<span className="text-faint-foreground">••••••</span></Td><Td className="font-mono">{k.created}</Td><Td className={cn('font-mono', k.used === 'never' && 'text-muted-foreground')}>{k.used}</Td>
              <Td numeric>{k.revoked ? <Badge variant="outline">Revoked</Badge> : <Button variant="ghost" size="sm" className="text-destructive-text" onClick={() => setRevoke(k.name)}>Revoke</Button>}</Td></Tr>
          ))}</tbody></Table>
      </Card>
      <div className="mt-2 flex flex-wrap items-center gap-3"><span className="flex flex-1 flex-col"><span className="font-semibold">Query keys <span className="font-mono font-normal text-muted-foreground">ft_at_</span></span><span className="text-control text-muted-foreground">Read only. Used by scripts and dashboards to query this project.</span></span><Button size="sm"><Plus />Create query key</Button></div>
      <Card className="overflow-x-auto">
        <Table><thead><tr><Th>Name</Th><Th className="w-[200px]">Key</Th><Th className="w-[160px]">Created by</Th><Th className="w-[120px]">Created</Th><Th className="w-[170px]">Last used</Th></tr></thead>
          <tbody>{readKeys.map((k) => <Tr key={k.name}><Td className="font-semibold">{k.name}</Td><Td className="font-mono">{k.prefix}<span className="text-faint-foreground">••••••</span></Td><Td className="text-control">{k.by}</Td><Td className="font-mono">{k.created}</Td><Td className="font-mono">{k.used}</Td></Tr>)}</tbody></Table>
      </Card>
      <Dialog open={issue} onOpenChange={(o) => { setIssue(o); if (!o) setSecret(null) }}>
        <DialogContent size={secret ? 'md' : 'sm'} title={secret ? 'Ingest key for storefront' : 'Issue ingest key'} description={secret ? name : 'The key is shown once after this step.'}>
          {secret ? (
            <>
              <Alert variant="warning" icon={<AlertTriangle />}>Copy this key now. It is shown once and cannot be retrieved later.</Alert>
              <div className="flex gap-2"><Input readOnly mono value={secret} className="bg-muted text-[12px]" aria-label="New ingest key" /><Button onClick={() => { void navigator.clipboard?.writeText(secret); toast.success('Key copied') }}><Copy />Copy</Button></div>
              <pre className="m-0 overflow-x-auto rounded-md border bg-muted px-3 py-2.5 font-mono text-[12px]">export OTEL_EXPORTER_OTLP_HEADERS="authorization=Bearer {secret.slice(0, 11)}…"</pre>
              <DialogFooter><Button variant="primary" onClick={() => { setIssue(false); setSecret(null) }}>Done</Button></DialogFooter>
            </>
          ) : (
            <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); const s = `ft_ik_${Math.random().toString(36).slice(2)}${Math.random().toString(36).slice(2, 10)}`; setSecret(s); setKeys((k) => [{ name, prefix: s.slice(0, 11), created: '2026-10-24', used: 'never', revoked: false }, ...k]) }}>
              <Field label="Name" htmlFor="ik-n" hint="Where the key will live, so you know what breaks when you revoke it."><Input id="ik-n" value={name} onChange={(e) => setName(e.target.value)} autoFocus /></Field>
              <DialogFooter><Button type="button" onClick={() => setIssue(false)}>Cancel</Button><Button variant="primary" type="submit">Issue key</Button></DialogFooter>
            </form>
          )}
        </DialogContent>
      </Dialog>
      <Dialog open={!!revoke} onOpenChange={(o) => !o && setRevoke(null)}>
        <DialogContent role="alertdialog" title={`Revoke ${revoke}?`} description="SDKs sending with this key are rejected from now on. This cannot be undone.">
          <DialogFooter><Button onClick={() => setRevoke(null)}>Cancel</Button><Button variant="destructive" onClick={() => { setKeys((k) => k.map((x) => (x.name === revoke ? { ...x, revoked: true } : x))); setRevoke(null); toast.success('Key revoked') }}>Revoke key</Button></DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  )
}
