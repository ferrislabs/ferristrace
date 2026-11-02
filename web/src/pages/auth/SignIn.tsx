import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { TriangleAlert } from 'lucide-react'
import { useState } from 'react'
import { Button } from '@/components/ui/button'
import { Field, Input } from '@/components/ui/input'
import { Alert } from '@/components/ui/misc'
import { OIDC_START, accountQuery, methodsQuery, signIn, verifySecondStep, type FactorKind } from '@/auth/api'
import { ApiError } from '@/auth/client'
import { Link, useNavigate, useSearchParam } from '@/lib/router'
import { AuthFrame } from './frame'

const refusal = (error: unknown) => {
  if (!(error instanceof ApiError)) return 'The server could not be reached.'
  if (error.status === 401) return 'That email and password do not match an account.'
  if (error.status === 501) return 'Passwords are turned off on this server. Use single sign-on.'
  if (error.status === 503) return 'The server cannot sign anyone in right now. Try again in a moment.'
  return error.body || 'The server refused the sign-in.'
}

const stepRefusal = (error: unknown) => {
  if (!(error instanceof ApiError)) return 'The server could not be reached.'
  if (error.status === 400) return 'That is neither a six-digit code nor a recovery code.'
  if (error.status === 401) return 'That answer does not match. Try the next code the app shows.'
  if (error.status === 429) return 'Too many answers. Sign in again to get a new challenge.'
  if (error.status === 503) return 'The server cannot finish the sign-in right now. Try again in a moment.'
  return error.body || 'The server refused the answer.'
}

export function SignIn() {
  const { data: methods } = useQuery(methodsQuery)
  const client = useQueryClient()
  const navigate = useNavigate()
  const back = useSearchParam('redirect')
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [asked, setAsked] = useState<FactorKind[] | null>(null)
  const landed = async () => {
    await client.invalidateQueries({ queryKey: accountQuery.queryKey })
    navigate(back?.startsWith('/') ? back : '/', { replace: true })
  }
  const submit = useMutation({
    mutationFn: () => signIn(email, password),
    onSuccess: async (admitted) => {
      if (admitted.kind === 'second-step') {
        setAsked(admitted.offered)
        return
      }
      await landed()
    },
  })
  const verify = useMutation({
    mutationFn: (answer: { code: string } | { recovery: string }) => verifySecondStep(answer),
    onSuccess: landed,
    onError: (failed) => {
      if (failed instanceof ApiError && (failed.status === 429 || failed.status === 401)) {
        setPassword('')
        if (failed.status === 429) setAsked(null)
      }
    },
  })
  const locked = !methods?.local_password && !methods?.oidc
  const refused = submit.error ? refusal(submit.error) : verify.error ? stepRefusal(verify.error) : null
  if (asked) {
    return (
      <AuthFrame title="One more step" intro="Your password is right. Finish with the second factor this account keeps.">
        {refused ? <Alert variant="destructive" icon={<TriangleAlert />}>{refused}</Alert> : null}
        <SecondStep offered={asked} answering={verify.isPending} onAnswer={(answer) => verify.mutate(answer)} onBack={() => setAsked(null)} />
      </AuthFrame>
    )
  }
  return (
    <AuthFrame
      title="Sign in"
      intro={back?.startsWith('/') ? 'Your session ended. Sign in to pick up where you were.' : undefined}
      footer={methods?.local_password ? <>No account yet? An administrator creates one with <span className="font-mono">ferristrace create-account</span>.</> : null}
    >
      {refused ? (
        <Alert variant="destructive" icon={<TriangleAlert />}>
          {refused}
        </Alert>
      ) : null}
      {locked ? <Alert variant="warning" icon={<TriangleAlert />}>This server has neither local passwords nor an identity provider configured, so nobody can sign in yet.</Alert> : null}
      {methods?.oidc ? (
        <Button type="button" className="h-10" onClick={() => window.location.assign(OIDC_START)}>
          Continue with single sign-on
        </Button>
      ) : null}
      {methods?.oidc && methods.local_password ? (
        <div className="flex items-center gap-3 text-caption text-muted-foreground">
          <span className="h-px flex-1 bg-border" />
          or
          <span className="h-px flex-1 bg-border" />
        </div>
      ) : null}
      {methods?.local_password ? (
        <form
          className="flex flex-col gap-4"
          onSubmit={(e) => {
            e.preventDefault()
            submit.mutate()
          }}
        >
          <Field label="Email" htmlFor="si-email">
            <Input id="si-email" type="email" required autoFocus autoComplete="email" className="h-10" value={email} onChange={(e) => setEmail(e.target.value)} />
          </Field>
          <Field label="Password" htmlFor="si-password">
            <Input id="si-password" type="password" required autoComplete="current-password" className="h-10" value={password} onChange={(e) => setPassword(e.target.value)} />
          </Field>
          <Button type="submit" variant="primary" size="lg" loading={submit.isPending} disabled={!email || !password}>
            Sign in
          </Button>
          <Link to="/forgot-password" className="text-control">
            Forgot your password?
          </Link>
        </form>
      ) : null}
    </AuthFrame>
  )
}

function SecondStep({
  offered,
  answering,
  onAnswer,
  onBack,
}: {
  offered: FactorKind[]
  answering: boolean
  onAnswer: (answer: { code: string } | { recovery: string }) => void
  onBack: () => void
}) {
  const [code, setCode] = useState('')
  const [recovery, setRecovery] = useState('')
  const [stuck, setStuck] = useState(false)
  const ready = stuck ? recovery.trim().length > 0 : code.length === 6
  return (
    <form
      className="flex flex-col gap-4"
      onSubmit={(event) => {
        event.preventDefault()
        onAnswer(stuck ? { recovery: recovery.trim() } : { code })
      }}
    >
      {stuck ? (
        <Field label="Recovery code" htmlFor="si-recovery" hint="One of the ten codes you kept when you turned this on.">
          <Input id="si-recovery" mono required autoFocus className="h-10" value={recovery} onChange={(event) => setRecovery(event.target.value)} />
        </Field>
      ) : (
        <Field label="Code" htmlFor="si-code" hint={offered.includes('app') ? 'Six digits from your authenticator app.' : 'Six digits.'}>
          <Input id="si-code" mono inputMode="numeric" required autoFocus autoComplete="one-time-code" className="h-10 tracking-[.3em]" value={code} onChange={(event) => setCode(event.target.value.replace(/\D/g, '').slice(0, 6))} />
        </Field>
      )}
      <Button type="submit" variant="primary" size="lg" loading={answering} disabled={!ready}>
        Finish signing in
      </Button>
      <div className="flex flex-wrap items-center gap-3 text-control">
        <button type="button" className="cursor-pointer text-accent-text underline-offset-4 hover:underline" onClick={() => setStuck(!stuck)}>
          {stuck ? 'Use a code from the app' : 'Use a recovery code instead'}
        </button>
        <span className="flex-1" />
        <button type="button" className="cursor-pointer text-muted-foreground underline-offset-4 hover:underline" onClick={onBack}>
          Start over
        </button>
      </div>
    </form>
  )
}
