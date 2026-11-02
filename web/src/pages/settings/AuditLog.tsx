import { useState } from 'react'
import { useOrganisations } from '@/auth/session'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { Field, NativeSelect } from '@/components/ui/input'
import { Table, Td, Th, Tr } from '@/components/ui/misc'
import { AUDIT_ACTIONS, OPERATOR, actorName, useAuditLog, type AuditEntry, type AuditFilter } from '@/data/audit'
import { useMembers } from '@/data/members'
import { stamp } from '@/lib/format'
import { H } from './parts'

function details(entry: AuditEntry) {
  return Object.entries(entry.detail)
    .map(([key, value]) => `${key}: ${value ?? '—'}`)
    .join(' · ')
}

export function AuditLog() {
  const { active } = useOrganisations()
  const mayRead = active?.role === 'owner' || active?.role === 'admin'
  const [filter, setFilter] = useState<AuditFilter>({ action: '', actor: '' })
  const { entries, nothing, more, loadingMore } = useAuditLog(filter, mayRead)
  const { members } = useMembers()

  if (!mayRead) {
    return (
      <>
        <H title="Audit log" sub="Who changed what in the organisation, and when." />
        <Card className="px-4 py-3 text-control text-muted-foreground">Only an owner or an administrator can read the audit log.</Card>
      </>
    )
  }

  return (
    <>
      <H title="Audit log" sub="Every change to the organisation, its projects, teams, members and keys, newest first." />
      <div className="flex flex-wrap gap-3">
        <Field label="Action" htmlFor="al-action" className="min-w-[220px]">
          <NativeSelect id="al-action" value={filter.action} onChange={(e) => setFilter((now) => ({ ...now, action: e.target.value }))}>
            <option value="">Every action</option>
            {AUDIT_ACTIONS.map((action) => <option key={action} value={action}>{action}</option>)}
          </NativeSelect>
        </Field>
        <Field label="Actor" htmlFor="al-actor" className="min-w-[220px]">
          <NativeSelect id="al-actor" value={filter.actor} onChange={(e) => setFilter((now) => ({ ...now, actor: e.target.value }))}>
            <option value="">Everybody</option>
            <option value={OPERATOR}>Operator (command line)</option>
            {(members ?? []).map((member) => <option key={member.account} value={member.account}>{member.email}</option>)}
          </NativeSelect>
        </Field>
      </div>
      <Card className="overflow-x-auto">
        {entries ? (
          <Table>
            <thead><tr><Th className="w-[170px]">Time</Th><Th className="w-[200px]">Actor</Th><Th className="w-[190px]">Action</Th><Th className="w-[220px]">Target</Th><Th>Detail</Th></tr></thead>
            <tbody>
              {entries.length === 0 ? <Tr><Td colSpan={5} className="text-muted-foreground">Nothing has been recorded{filter.action || filter.actor ? ' for this filter' : ' yet'}.</Td></Tr> : null}
              {entries.map((entry) => (
                <Tr key={entry.id}>
                  <Td className="font-mono">{stamp(entry.at_ms)}</Td>
                  <Td><span className="flex items-center gap-2">{actorName(entry)}{entry.actor.kind === 'api_key' ? <Badge variant="outline">key</Badge> : null}{entry.actor.kind === 'agent' ? <Badge variant="accent">agent</Badge> : null}</span></Td>
                  <Td className="font-mono">{entry.action}</Td>
                  <Td><span className="flex items-center gap-2"><Badge variant="outline">{entry.target.kind}</Badge><span className="truncate font-mono text-caption text-muted-foreground">{entry.target.id ?? ''}</span></span></Td>
                  <Td className="max-w-[360px] truncate text-caption text-muted-foreground" title={details(entry)}>{details(entry)}</Td>
                </Tr>
              ))}
            </tbody>
          </Table>
        ) : <div className="px-4 py-3 text-control text-muted-foreground">{nothing}</div>}
      </Card>
      {more ? <div><Button size="sm" disabled={loadingMore} onClick={more}>{loadingMore ? 'Loading…' : 'Load more'}</Button></div> : null}
    </>
  )
}
