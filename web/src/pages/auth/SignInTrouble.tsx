import { TriangleAlert } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Alert } from '@/components/ui/misc'
import { here, signIn } from '@/auth/oidc'
import { AuthFrame } from './frame'

export function SignInStalled() {
  return (
    <AuthFrame title="Sign-in did not finish" intro="FerrisKey sent you back without a session twice in a row, so FerrisTrace stopped redirecting.">
      <Button type="button" className="h-10" onClick={() => void signIn(here())}>
        Try again
      </Button>
    </AuthFrame>
  )
}

export function SignInUnavailable() {
  return (
    <AuthFrame title="Sign-in is unavailable">
      <Alert variant="warning" icon={<TriangleAlert />}>
        This server has no identity provider configured, so nobody can sign in yet.
      </Alert>
    </AuthFrame>
  )
}

export function SignedOut() {
  return (
    <AuthFrame title="You are signed out" intro="Your FerrisKey session has ended on this browser.">
      <Button type="button" className="h-10" onClick={() => void signIn('/')}>
        Sign in again
      </Button>
    </AuthFrame>
  )
}
