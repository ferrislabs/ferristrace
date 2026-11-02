import { AlertTriangle } from 'lucide-react'
import { toast } from 'sonner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { Alert, Skeleton, Table, Td, Th, Tr } from '@/components/ui/misc'
import { because } from '@/auth/client'
import { ago, deviceOf, useEndOtherSessions, useEndSession, useSessions } from '@/data/sessions'
import { stamp } from '@/lib/format'
import { H } from './parts'

export function Security() {
  const { sessions, asking, error } = useSessions()
  const endOne = useEndSession()
  const endOthers = useEndOtherSessions()
  const roll = sessions ?? []
  const others = roll.filter((one) => !one.current).length
  return (
    <>
      <H title="Sessions" sub="Where you are signed in.">
        <Button
          size="sm"
          loading={endOthers.isPending}
          disabled={others === 0}
          onClick={() =>
            endOthers.mutate(undefined, {
              onSuccess: ({ ended }) => toast.success(ended === 1 ? '1 other session signed out' : `${ended} other sessions signed out`),
              onError: (failed) => toast.error(because(failed, 'The other sessions were not signed out')),
            })
          }
        >
          Sign out other sessions
        </Button>
      </H>
      <Card className="overflow-hidden">
        {error ? (
          <div className="px-4 py-3.5">
            <Alert variant="warning" icon={<AlertTriangle />}>{because(error, 'This server does not answer for sessions yet.')}</Alert>
          </div>
        ) : (
          <Table>
            <thead><tr><Th>Device</Th><Th className="w-[140px]">Signed in</Th><Th className="w-[170px]">Last active</Th><Th className="w-[100px]"><span className="sr-only">Actions</span></Th></tr></thead>
            <tbody>
              {asking ? (
                <Tr><Td colSpan={4}><Skeleton className="w-48" /></Td></Tr>
              ) : (
                roll.map((one) => (
                  <Tr key={one.id}>
                    <Td><span className="inline-flex items-center gap-2">{deviceOf(one.from)}{one.current ? <Badge variant="success">This device</Badge> : null}</span></Td>
                    <Td className="font-mono">{stamp(one.created_at_ms).slice(0, 10)}</Td>
                    <Td className="font-mono">{ago(one.last_seen_at_ms)}</Td>
                    <Td numeric>
                      {one.current ? null : (
                        <Button
                          variant="ghost"
                          size="sm"
                          disabled={endOne.isPending}
                          onClick={() => endOne.mutate(one.id, { onError: (failed) => toast.error(because(failed, 'That session was not signed out')) })}
                        >
                          Sign out
                        </Button>
                      )}
                    </Td>
                  </Tr>
                ))
              )}
            </tbody>
          </Table>
        )}
      </Card>
    </>
  )
}
