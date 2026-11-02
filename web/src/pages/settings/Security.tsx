import { useQuery } from '@tanstack/react-query'
import { AlertTriangle, Copy, Download, RotateCcw, ShieldCheck } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { Dialog, DialogContent, DialogFooter } from '@/components/ui/dialog'
import { Field, Input } from '@/components/ui/input'
import { Alert, Skeleton, Table, Td, Th, Tr } from '@/components/ui/misc'
import { QrCode } from '@/components/ft/QrCode'
import { methodsQuery } from '@/auth/api'
import { because } from '@/auth/client'
import { MIN_PASSWORD_CHARS, passwordProblem, useChangePassword } from '@/data/password'
import {
  KIND_LABEL,
  useConfirmFactor,
  useEnrol,
  useForgetFactor,
  useRegenerateRecovery,
  useSecondFactor,
  type Enrolment,
} from '@/data/second-factor'
import { ago, deviceOf, useEndOtherSessions, useEndSession, useSessions } from '@/data/sessions'
import { stamp } from '@/lib/format'
import { H, Row } from './parts'

export function Security() {
  const { data: methods } = useQuery(methodsQuery)
  const offered = methods?.local_password !== false
  const { standing, asking, error } = useSecondFactor(offered)
  const enrol = useEnrol()
  const forget = useForgetFactor()
  const regenerate = useRegenerateRecovery()
  const [codes, setCodes] = useState<string[] | null>(null)
  const [dropping, setDropping] = useState(false)
  const [renewing, setRenewing] = useState(false)
  const live = standing?.live ?? []
  const on = live.includes('app')
  const copyCodes = () => {
    void navigator.clipboard?.writeText((codes ?? []).join('\n'))
    toast.success('Recovery codes copied')
  }
  const downloadCodes = () => {
    const url = URL.createObjectURL(new Blob([`${(codes ?? []).join('\n')}\n`], { type: 'text/plain' }))
    const a = document.createElement('a')
    a.href = url
    a.download = 'ferristrace-recovery-codes.txt'
    a.click()
    URL.revokeObjectURL(url)
    toast.success('Recovery codes downloaded')
  }
  return (
    <>
      <H title="Login & security" sub="How you sign in, and where you are signed in." />
      {offered ? (
      <Card className="overflow-hidden">
        <div className="flex flex-wrap items-center gap-3 border-b border-border-subtle px-4 py-3.5">
          <span className="flex flex-[1_1_260px] flex-col"><h3 className="text-heading">Two-factor authentication</h3><span className="text-caption text-muted-foreground">A second step after your password, asked at every sign-in.</span></span>
          {asking ? <Skeleton className="w-14" /> : on ? <Badge variant="success"><ShieldCheck />On</Badge> : <Badge variant="warning">Off</Badge>}
        </div>
        {error ? (
          <div className="px-4 py-3.5">
            <Alert variant="warning" icon={<AlertTriangle />}>{because(error, 'This server does not answer for second factors yet.')}</Alert>
          </div>
        ) : (
          <>
            <Row title={KIND_LABEL.app} sub="A 6-digit code from an authenticator app on your phone.">
              {on ? (
                <>
                  <Badge variant="success"><ShieldCheck />On</Badge>
                  <Button size="sm" className="text-destructive-text" onClick={() => setDropping(true)}>Turn off</Button>
                </>
              ) : (
                <>
                  <Badge variant="outline">Off</Badge>
                  <Button variant="primary" size="sm" loading={enrol.isPending} disabled={asking} onClick={() => enrol.mutate(undefined, { onError: (failed) => toast.error(because(failed, 'The enrolment could not be started')) })}>Set up</Button>
                </>
              )}
            </Row>
            <Row title="Recovery codes" sub={on ? `${standing?.recovery_left ?? 0} codes left. Each one signs you in once when the app is out of reach.` : 'Available once a two-factor method is on.'}>
              <Button size="sm" disabled={!on || regenerate.isPending} onClick={() => setRenewing(true)}><RotateCcw />Regenerate</Button>
            </Row>
          </>
        )}
      </Card>
      ) : null}
      {offered ? <Password /> : null}
      <Sessions />
      <Dialog open={Boolean(enrol.data)} onOpenChange={(open) => !open && enrol.reset()}>
        {enrol.data ? (
          <Setup
            enrolment={enrol.data}
            onDone={(minted) => {
              enrol.reset()
              if (minted.length > 0) setCodes(minted)
              toast.success('Two-factor authentication is on')
            }}
            onCancel={() => enrol.reset()}
          />
        ) : null}
      </Dialog>
      <Dialog open={dropping} onOpenChange={setDropping}>
        <DialogContent role="alertdialog" title="Turn off the authenticator app?" description="Your password alone will sign you in, and the recovery codes you have stop working.">
          <DialogFooter>
            <Button onClick={() => setDropping(false)}>Cancel</Button>
            <Button
              variant="destructive"
              loading={forget.isPending}
              onClick={() =>
                forget.mutate('app', {
                  onSuccess: () => {
                    setDropping(false)
                    toast.success('Two-factor authentication is off')
                  },
                  onError: (failed) => toast.error(because(failed, 'The factor was not turned off')),
                })
              }
            >
              Turn off
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <Dialog open={renewing} onOpenChange={setRenewing}>
        <DialogContent role="alertdialog" title="Regenerate recovery codes?" description="The codes you have now stop working, and ten new ones take their place. They are shown once.">
          <DialogFooter>
            <Button onClick={() => setRenewing(false)}>Cancel</Button>
            <Button
              variant="primary"
              loading={regenerate.isPending}
              onClick={() =>
                regenerate.mutate(undefined, {
                  onSuccess: (minted) => {
                    setRenewing(false)
                    setCodes(minted.recovery)
                  },
                  onError: (failed) => toast.error(because(failed, 'No new codes were minted')),
                })
              }
            >
              Regenerate codes
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <Dialog open={Boolean(codes)} onOpenChange={(open) => !open && setCodes(null)}>
        <DialogContent size="md" title="Recovery codes" description="Keep them where you can reach them without your phone. Each code works once.">
          <Alert variant="warning" icon={<AlertTriangle />}>This is the only time they are shown. The server keeps only their digests, so a lost set can be replaced but never read back.</Alert>
          <ol className="m-0 grid list-none grid-cols-[repeat(auto-fit,minmax(min(150px,100%),1fr))] gap-x-5 gap-y-1 rounded-md border bg-muted p-3">
            {(codes ?? []).map((code) => <li key={code} className="font-mono text-[12.5px] tracking-[.03em]">{code}</li>)}
          </ol>
          <div className="flex flex-wrap items-center gap-2">
            <Button size="sm" onClick={copyCodes}><Copy />Copy all</Button>
            <Button size="sm" onClick={downloadCodes}><Download />Download .txt</Button>
          </div>
          <DialogFooter><Button variant="primary" onClick={() => setCodes(null)}>I have kept them</Button></DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  )
}

function Setup({ enrolment, onDone, onCancel }: { enrolment: Enrolment; onDone: (recovery: string[]) => void; onCancel: () => void }) {
  const confirm = useConfirmFactor()
  const [code, setCode] = useState('')
  return (
    <DialogContent size="md" title="Set up the authenticator app" description="Scan this with an authenticator app, then enter the 6 digits it shows.">
      <div className="flex flex-wrap items-center gap-4">
        <QrCode value={enrolment.uri} label="Scan this code with an authenticator app" />
        <div className="flex min-w-0 flex-[1_1_200px] flex-col gap-1.5">
          <span className="text-caption text-muted-foreground">Can&apos;t scan? Enter this key by hand:</span>
          <span className="break-all font-mono text-control">{enrolment.key}</span>
          <Button size="sm" className="self-start" onClick={() => { void navigator.clipboard?.writeText(enrolment.key); toast.success('Key copied') }}><Copy />Copy key</Button>
        </div>
      </div>
      <form
        className="flex flex-col gap-4"
        onSubmit={(event) => {
          event.preventDefault()
          confirm.mutate(code, {
            onSuccess: (minted) => onDone(minted.recovery),
            onError: (failed) => toast.error(because(failed, 'That code was refused')),
          })
        }}
      >
        <Field label="Code" htmlFor="tf-c" hint="Six digits from the app.">
          <Input id="tf-c" mono inputMode="numeric" autoComplete="one-time-code" value={code} onChange={(event) => setCode(event.target.value.replace(/\D/g, '').slice(0, 6))} className="h-10 max-w-[180px] tracking-[.2em]" autoFocus />
        </Field>
        <DialogFooter>
          <Button type="button" onClick={onCancel}>Cancel</Button>
          <Button type="submit" variant="primary" loading={confirm.isPending} disabled={code.length !== 6}>Verify and turn on</Button>
        </DialogFooter>
      </form>
    </DialogContent>
  )
}

function Password() {
  const change = useChangePassword()
  const [current, setCurrent] = useState('')
  const [chosen, setChosen] = useState('')
  const [again, setAgain] = useState('')
  const problem = chosen.length > 0 ? passwordProblem(chosen) : null
  const apart = again.length > 0 && again !== chosen
  const refused =
    problem ??
    (apart ? 'the two new passwords do not match' : null) ??
    (change.error ? because(change.error, 'The password was not changed') : null)
  const typing = (keep: (value: string) => void) => (event: React.ChangeEvent<HTMLInputElement>) => {
    if (change.error) change.reset()
    keep(event.target.value)
  }
  return (
    <Card className="flex flex-col gap-3 p-4">
      <h3 className="text-heading">Password</h3>
      <form
        className="flex flex-col gap-3"
        onSubmit={(event) => {
          event.preventDefault()
          change.mutate(
            { current, password: chosen },
            {
              onSuccess: () => {
                setCurrent('')
                setChosen('')
                setAgain('')
                toast.success('Password changed', { description: 'Your other sessions were signed out.' })
              },
            },
          )
        }}
      >
        <div className="grid grid-cols-[repeat(auto-fit,minmax(min(240px,100%),1fr))] gap-3">
          <Field label="Current password" htmlFor="pw-c"><Input id="pw-c" type="password" autoComplete="current-password" value={current} onChange={typing(setCurrent)} /></Field>
          <Field label="New password" htmlFor="pw-n" hint={`At least ${MIN_PASSWORD_CHARS} characters.`}><Input id="pw-n" type="password" autoComplete="new-password" aria-invalid={problem ? true : undefined} value={chosen} onChange={typing(setChosen)} /></Field>
          <Field label="Confirm new password" htmlFor="pw-a" hint="The same one again."><Input id="pw-a" type="password" autoComplete="new-password" aria-invalid={apart ? true : undefined} value={again} onChange={typing(setAgain)} /></Field>
        </div>
        {refused ? <span className="text-caption text-destructive-text">{refused}</span> : null}
        <div><Button type="submit" variant="primary" size="sm" loading={change.isPending} disabled={current.length === 0 || chosen.length === 0 || again !== chosen || Boolean(problem)}>Change password</Button></div>
      </form>
    </Card>
  )
}

function Sessions() {
  const { sessions, asking, error } = useSessions()
  const endOne = useEndSession()
  const endOthers = useEndOtherSessions()
  const roll = sessions ?? []
  const others = roll.filter((one) => !one.current).length
  return (
    <Card className="overflow-hidden">
      <div className="flex items-center gap-3 px-4 py-3.5">
        <h3 className="flex-1 text-heading">Sessions</h3>
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
      </div>
      {error ? (
        <div className="px-4 pb-3.5">
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
  )
}
