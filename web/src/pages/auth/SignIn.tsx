import { useQuery } from '@tanstack/react-query'
import { TriangleAlert } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Alert } from '@/components/ui/misc'
import { methodsQuery } from '@/auth/api'
import { here, signIn } from '@/auth/oidc'
import { useSearchParam } from '@/lib/router'
import { AuthFrame } from './frame'

export function SignIn() {
  const { data: methods } = useQuery(methodsQuery)
  const back = useSearchParam('redirect')
  const unavailable = methods !== undefined && !methods.oidc
  return (
    <AuthFrame
      title="Sign in"
      intro={back?.startsWith('/') ? 'Your session ended. Sign in to pick up where you were.' : undefined}
    >
      {unavailable ? (
        <Alert variant="warning" icon={<TriangleAlert />}>
          Sign-in is unavailable: this server has no identity provider configured, so nobody can sign in yet.
        </Alert>
      ) : (
        <Button type="button" className="h-10" disabled={!methods?.oidc} onClick={() => void signIn(back ?? here())}>
          Continue with FerrisKey
        </Button>
      )}
    </AuthFrame>
  )
}
