import { NewOrganisationForm } from '@/components/layout/NewOrganisation'
import { useAccount, useSignOut } from '@/auth/session'
import { AuthFrame } from './frame'

export function Welcome() {
  const account = useAccount()
  const signingOut = useSignOut()
  return (
    <AuthFrame
      title="Create your organisation"
      intro="An organisation holds your projects, your telemetry and the people who see it. Name yours to get started."
      footer={
        <>
          Waiting for an invitation instead? Ask an owner to add {account?.email ?? 'your address'}, then sign in again.{' '}
          <button type="button" className="cursor-pointer bg-transparent p-0 font-semibold text-foreground underline underline-offset-2" onClick={() => signingOut.mutate()}>
            Sign out
          </button>
        </>
      }
    >
      <NewOrganisationForm />
    </AuthFrame>
  )
}
