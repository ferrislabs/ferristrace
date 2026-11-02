import { useState } from 'react'
import { Button } from '@/components/ui/button'
import { DialogFooter } from '@/components/ui/dialog'
import { Field, Input } from '@/components/ui/input'
import { because } from '@/auth/client'
import { orgNameProblem, useCreateOrg } from '@/data/orgs'

export function NewOrganisationForm({ onCancel, onCreated }: { onCancel?: () => void; onCreated?: () => void }) {
  const create = useCreateOrg()
  const [name, setName] = useState('')
  const problem = name === '' ? null : orgNameProblem(name)
  const ready = name !== '' && !problem
  const submit = (event: React.FormEvent) => {
    event.preventDefault()
    if (ready && !create.isPending) create.mutate(name, { onSuccess: () => onCreated?.() })
  }
  return (
    <form className="flex flex-col gap-3" onSubmit={submit}>
      <Field label="Organisation name" htmlFor="no-name" hint="You become its owner. It starts with one project, default.">
        <Input id="no-name" value={name} autoFocus aria-invalid={problem ? true : undefined} onChange={(e) => setName(e.target.value)} />
      </Field>
      {problem ? <span role="alert" className="text-caption text-destructive-text">{problem}</span> : null}
      {create.isError ? <span role="alert" className="text-caption text-destructive-text">{because(create.error, 'The organisation was not created.')}</span> : null}
      <DialogFooter>
        {onCancel ? <Button type="button" onClick={onCancel}>Cancel</Button> : null}
        <Button type="submit" variant="primary" loading={create.isPending} disabled={!ready}>Create</Button>
      </DialogFooter>
    </form>
  )
}
