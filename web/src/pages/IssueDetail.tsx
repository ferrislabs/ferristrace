import { AlertTriangle, CircleCheck, EyeOff, RotateCcw, Waypoints } from 'lucide-react'
import { useState } from 'react'
import { Link, useParams } from '@/lib/router'
import { toast } from 'sonner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardHeader, CardTitle } from '@/components/ui/card'
import { NativeSelect } from '@/components/ui/input'
import { Alert, Skeleton, Table, Td, Th, Tr } from '@/components/ui/misc'
import { Kpi, ServiceDot } from '@/components/ft/bits'
import { because } from '@/auth/client'
import { useMembers } from '@/data/members'
import { nameOf } from '@/data/profile'
import {
  RANGES,
  shortPrint,
  useDecide,
  useIssue,
  useOccurrences,
  type Issue,
  type IssueStatus,
} from '@/data/issues'
import { stamp } from '@/lib/format'
import { useOwners } from './Issues'

const A_DAY = 24 * 60 * 60 * 1000

export function IssueDetail() {
  const { id } = useParams()
  const [rangeMs, setRangeMs] = useState(A_DAY)
  const { issue, nothing, failed } = useIssue(id, rangeMs)

  if (!issue) {
    return (
      <>
        <nav aria-label="Breadcrumb" className="text-control">
          <Link to="/issues">Issues</Link>
          <span className="text-faint-foreground"> / </span>
          <span className="font-mono">{id ? shortPrint(id) : '—'}</span>
        </nav>
        {failed ? (
          <Alert variant="warning" icon={<AlertTriangle />}>
            {because(failed, nothing)}
          </Alert>
        ) : (
          <Skeleton className="w-64" />
        )}
      </>
    )
  }
  return <Known issue={issue} rangeMs={rangeMs} onRange={setRangeMs} />
}

function Known({
  issue,
  rangeMs,
  onRange,
}: {
  issue: Issue
  rangeMs: number
  onRange: (ms: number) => void
}) {
  const { members } = useMembers()
  const owner = useOwners()
  const decide = useDecide()
  const { occurrences, asking } = useOccurrences(issue, rangeMs)
  const latest = occurrences[0]

  const set = (said: { status?: IssueStatus; owner?: string | null }, told: string) =>
    decide.mutate(
      { issue, ...said },
      {
        onSuccess: () => toast.success(told),
        onError: (refused) => toast.error(because(refused, 'Nothing was changed')),
      },
    )

  return (
    <>
      <nav aria-label="Breadcrumb" className="text-control">
        <Link to="/issues">Issues</Link>
        <span className="text-faint-foreground"> / </span>
        <span className="font-mono">{shortPrint(issue.fingerprint)}</span>
      </nav>
      <div className="flex flex-wrap items-start gap-3">
        <div className="flex min-w-0 flex-[1_1_480px] flex-col gap-1">
          <div className="flex flex-wrap items-center gap-2">
            <h1 className="font-mono text-title text-destructive-text">
              {issue.exception_type || 'Exception'}
            </h1>
            {issue.regressed ? <Badge variant="warning">Regressed</Badge> : null}
            {issue.status === 'resolved' ? (
              <Badge variant="success">
                <CircleCheck />
                Resolved
              </Badge>
            ) : issue.status === 'ignored' ? (
              <Badge variant="outline">
                <EyeOff />
                Ignored
              </Badge>
            ) : (
              <Badge variant="destructive">Unresolved</Badge>
            )}
          </div>
          <span className="text-body">{issue.message}</span>
          <span className="flex flex-wrap items-center gap-x-2 text-caption text-muted-foreground">
            <span className="inline-flex items-center gap-1">
              <ServiceDot name={issue.service} />
              {issue.service}
            </span>
            <span aria-hidden>·</span>
            <span className="font-mono">{issue.fingerprint}</span>
          </span>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          {issue.status === 'unresolved' ? (
            <>
              <Button
                variant="primary"
                size="sm"
                loading={decide.isPending}
                onClick={() => set({ status: 'resolved' }, 'Issue resolved')}
              >
                <CircleCheck />
                Resolve
              </Button>
              <Button size="sm" onClick={() => set({ status: 'ignored' }, 'Issue ignored')}>
                <EyeOff />
                Ignore
              </Button>
            </>
          ) : (
            <Button size="sm" onClick={() => set({ status: 'unresolved' }, 'Issue reopened')}>
              <RotateCcw />
              Reopen
            </Button>
          )}
          <NativeSelect
            aria-label="Owner"
            className="h-7 w-[180px]"
            value={issue.owner ?? ''}
            onChange={(event) =>
              set(
                { owner: event.target.value || null },
                event.target.value ? 'Owner set' : 'Owner cleared',
              )
            }
          >
            <option value="">Unassigned</option>
            {(members ?? []).map((one) => (
              <option key={one.account} value={one.account}>
                {nameOf(one)}
              </option>
            ))}
          </NativeSelect>
          <NativeSelect
            aria-label="Window"
            className="h-7 w-[150px]"
            value={String(rangeMs)}
            onChange={(event) => onRange(Number(event.target.value))}
          >
            {RANGES.map(([label, ms]) => (
              <option key={label} value={ms}>
                Last {label}
              </option>
            ))}
          </NativeSelect>
        </div>
      </div>
      <div className="grid grid-cols-[repeat(auto-fit,minmax(160px,1fr))] gap-3">
        <Kpi label="Events" value={issue.events.toLocaleString('en-US')} sub="in this window" />
        <Kpi label="First seen" value={stamp(issue.first_seen_at_ms)} />
        <Kpi label="Last seen" value={stamp(issue.last_seen_at_ms)} />
        <Kpi
          label="Owner"
          value={owner(issue.owner) ?? 'Unassigned'}
          sub={issue.decided_at_ms ? `decided ${stamp(issue.decided_at_ms)}` : 'never decided'}
        />
      </div>
      {latest?.stacktrace ? (
        <Card className="overflow-hidden">
          <CardHeader>
            <CardTitle className="flex-1">Stack trace</CardTitle>
            <span className="text-caption text-muted-foreground">as the sdk sent it</span>
          </CardHeader>
          <div className="border-b border-border-subtle bg-destructive-subtle px-4 py-2.5 font-mono text-control text-destructive-text">
            {latest.exception_type}: {latest.message}
          </div>
          <pre className="m-0 overflow-x-auto bg-sidebar p-4 font-mono text-[12px] leading-5">
            {latest.stacktrace}
          </pre>
        </Card>
      ) : null}
      <Card className="overflow-x-auto">
        <CardHeader>
          <CardTitle className="flex-1">Occurrences</CardTitle>
          <span className="text-caption text-muted-foreground">most recent first</span>
        </CardHeader>
        <Table aria-label="Occurrences">
          <thead>
            <tr>
              <Th className="w-[170px]">Time</Th>
              <Th className="w-[180px]">Trace</Th>
              <Th>Message</Th>
              <Th className="w-[110px]">
                <span className="sr-only">Links</span>
              </Th>
            </tr>
          </thead>
          <tbody>
            {asking ? (
              <Tr>
                <Td colSpan={4}>
                  <Skeleton className="w-48" />
                </Td>
              </Tr>
            ) : occurrences.length === 0 ? (
              <Tr>
                <Td colSpan={4} className="text-muted-foreground">
                  No occurrence in this window.
                </Td>
              </Tr>
            ) : (
              occurrences.map((seen) => (
                <Tr key={`${seen.trace_id}-${seen.span_id}-${seen.at_ms}`}>
                  <Td className="font-mono">{stamp(seen.at_ms)}</Td>
                  <Td>
                    <Link to={`/traces/${seen.trace_id}`} className="font-mono">
                      {shortPrint(seen.trace_id)}…{seen.trace_id.slice(-4)}
                    </Link>
                  </Td>
                  <Td className="truncate">{seen.message}</Td>
                  <Td numeric>
                    <Button asChild variant="ghost" size="sm">
                      <Link to={`/traces/${seen.trace_id}`}>
                        <Waypoints />
                        Trace
                      </Link>
                    </Button>
                  </Td>
                </Tr>
              ))
            )}
          </tbody>
        </Table>
      </Card>
      <p className="m-0 text-caption text-muted-foreground">
        The stack trace is the string the sdk recorded on the exception. FerrisTrace holds no source
        and no source maps, so there is no code to show around a frame. Resolving an issue keeps its
        owner; an occurrence after that marks it Regressed without reopening it.
      </p>
    </>
  )
}
