import { useMutation } from '@tanstack/react-query'
import { MailCheck, TriangleAlert } from 'lucide-react'
import { useState } from 'react'
import { Button } from '@/components/ui/button'
import { Field, Input } from '@/components/ui/input'
import { Alert } from '@/components/ui/misc'
import { requestReset } from '@/auth/api'
import { Link } from '@/lib/router'
import { AuthFrame } from './frame'

export function ForgotPassword() {
  const [email, setEmail] = useState('')
  const ask = useMutation({ mutationFn: () => requestReset(email) })
  return (
    <AuthFrame
      title="Reset your password"
      intro="We send a link that sets a new password. It can be spent once."
      footer={<>A self-hosted server only sends the mail when a relay is configured. Without one, an administrator runs <span className="font-mono">ferristrace reset-password --email you@example.com</span> and hands you the link.</>}
    >
      {ask.isSuccess ? (
        <>
          <Alert variant="success" icon={<MailCheck />} title="Check your inbox">
            If <span className="font-mono">{email}</span> belongs to an account, the link is on its way. The answer is the same either way, so this page never says whether an address exists.
          </Alert>
          <Button asChild>
            <Link to="/sign-in">Back to sign in</Link>
          </Button>
        </>
      ) : (
        <form
          className="flex flex-col gap-4"
          onSubmit={(e) => {
            e.preventDefault()
            ask.mutate()
          }}
        >
          {ask.error ? (
            <Alert variant="destructive" icon={<TriangleAlert />}>
              The server could not take the request. Try again in a moment.
            </Alert>
          ) : null}
          <Field label="Email" htmlFor="fp-email">
            <Input id="fp-email" type="email" required autoFocus autoComplete="email" className="h-10" value={email} onChange={(e) => setEmail(e.target.value)} />
          </Field>
          <Button type="submit" variant="primary" size="lg" loading={ask.isPending} disabled={!email}>
            Send the link
          </Button>
          <Link to="/sign-in" className="text-control">
            Back to sign in
          </Link>
        </form>
      )}
    </AuthFrame>
  )
}
