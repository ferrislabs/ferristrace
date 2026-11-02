import { useQuery } from '@tanstack/react-query'
import { TriangleAlert } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Alert } from '@/components/ui/misc'
import { OIDC_START, methodsQuery } from '@/auth/api'
import { useSearchParam } from '@/lib/router'
import { AuthFrame } from './frame'

export function SignIn() {
  const { data: methods } = useQuery(methodsQuery)
  const back = useSearchParam('redirect')
  const locked = methods !== undefined && !methods.oidc
  return (
    <AuthFrame title="Sign in" intro={back?.startsWith('/') ? 'Your session ended. Sign in to pick up where you were.' : undefined}>
      {locked ? <Alert variant="warning" icon={<TriangleAlert />}>This server has no identity provider configured, so nobody can sign in yet.</Alert> : null}
      {methods?.oidc ? (
        <Button type="button" className="h-10" onClick={() => window.location.assign(OIDC_START)}>
          Continue with single sign-on
        </Button>
      ) : null}
    </AuthFrame>
  )
}
