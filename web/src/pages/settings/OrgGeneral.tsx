import { Copy, Upload } from 'lucide-react'
import { useRef, useState } from 'react'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { Field, Input } from '@/components/ui/input'
import { Avatar } from '@/components/ui/misc'
import { because } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { effectiveRetention, logoProblem, orgNameProblem, retentionProblem, useOrgCard, usePutOrgLogo, useRemoveOrgLogo, useRenameOrg, useSetOrgRetention, type OrgCard } from '@/data/orgs'
import { initialsOf } from '@/data/profile'
import { stamp } from '@/lib/format'
import { cn } from '@/lib/utils'
import { ConfirmDelete, Danger, H } from './parts'

function Identity({ card }: { card: OrgCard }) {
  const rows: [string, React.ReactNode][] = [
    ['Organisation id', <span className="inline-flex items-center gap-2 font-mono">{card.id}<Button variant="ghost" size="icon-sm" aria-label="Copy id" onClick={() => { void navigator.clipboard?.writeText(card.id); toast.success('Copied') }}><Copy /></Button></span>],
    ['Created', <span className="font-mono">{stamp(card.created_at_ms)}</span>],
    ['Plan', card.plan],
    ['Version', <span className="font-mono">{card.version}</span>],
  ]
  return (
    <Card><dl className="m-0 grid grid-cols-[200px_minmax(0,1fr)] text-control">
      {rows.map(([k, v], i, a) => (
        <div key={k} className="contents"><dt className={cn('px-4 py-3 text-muted-foreground', i < a.length - 1 && 'border-b border-border-subtle')}>{k}</dt><dd className={cn('m-0 px-4 py-2.5', i < a.length - 1 && 'border-b border-border-subtle')}>{v}</dd></div>
      ))}
    </dl></Card>
  )
}

function Mark({ card, mayChange }: { card: OrgCard | undefined; mayChange: boolean }) {
  const picker = useRef<HTMLInputElement>(null)
  const put = usePutOrgLogo()
  const drop = useRemoveOrgLogo()
  const busy = put.isPending || drop.isPending

  const pick = (event: React.ChangeEvent<HTMLInputElement>) => {
    const chosen = event.target.files?.[0]
    event.target.value = ''
    if (!chosen) return
    const problem = logoProblem(chosen)
    if (problem) {
      toast.error(problem)
      return
    }
    put.mutate(chosen, {
      onSuccess: () => toast.success('Saved'),
      onError: (error) => toast.error(because(error, 'The logo was not kept')),
    })
  }

  const remove = () =>
    drop.mutate(undefined, {
      onSuccess: () => toast.success('The initials are back'),
      onError: (error) => toast.error(because(error, 'The logo was not removed')),
    })

  return (
    <div className="flex flex-wrap items-center gap-3">
      <Avatar initials={initialsOf(card?.name ?? '?')} src={card?.logo} className="size-14 rounded-lg text-[20px]" />
      <span className="flex min-w-0 flex-1 flex-col leading-5">
        <span className="truncate text-heading">{card?.name ?? '—'}</span>
        <span className="truncate text-caption text-muted-foreground">{mayChange ? 'Shown wherever the organisation is named. Without one, its initials stand in.' : 'Only an owner or an administrator can change this.'}</span>
      </span>
      <span className="flex flex-wrap items-center gap-2">
        <input ref={picker} type="file" accept="image/png,image/jpeg,image/webp" className="hidden" onChange={pick} />
        <Button size="sm" disabled={!card || !mayChange || busy} onClick={() => picker.current?.click()}><Upload />{card?.logo ? 'Replace logo' : 'Upload logo'}</Button>
        {card?.logo ? <Button size="sm" variant="ghost" disabled={!mayChange || busy} onClick={remove}>Remove</Button> : null}
        <span className="text-caption text-muted-foreground">PNG, JPEG or WebP, up to 1 MB.</span>
      </span>
    </div>
  )
}

function Retention({ card, isOwner }: { card: OrgCard; isOwner: boolean }) {
  const set = useSetOrgRetention()
  const [typed, setTyped] = useState<string | null>(null)
  const shown = typed ?? String(card.retention_days ?? effectiveRetention(card))
  const problem = typed === null ? null : retentionProblem(typed, card.retention_cap_days)
  const unchanged = typed === null || Number(typed) === card.retention_days

  const apply = (days: number | null) =>
    set.mutate(days, {
      onSuccess: () => {
        setTyped(null)
        toast.success('Saved')
      },
      onError: (error) => toast.error(because(error, 'The retention was not changed')),
    })

  return (
    <Card className="flex flex-col gap-3.5 p-4">
      <Field
        label="Retention"
        htmlFor="og-r"
        hint={`Spans are kept ${effectiveRetention(card)} days${card.retention_days === null ? ' (the deployment default)' : ''}. The deployment default is ${card.default_retention_days} days${card.retention_cap_days === null ? '' : `; your plan keeps spans at most ${card.retention_cap_days} days`}. A change applies to spans ingested afterwards.`}
      >
        <div className="flex max-w-[420px] items-center gap-2">
          <Input id="og-r" mono type="number" min={1} max={card.retention_cap_days ?? 3650} value={shown} disabled={!isOwner} aria-invalid={problem ? true : undefined} onChange={(e) => setTyped(e.target.value)} />
          <span className="text-control text-muted-foreground">days</span>
          <Button size="sm" className="h-8" variant="primary" disabled={!isOwner || unchanged || Boolean(problem) || set.isPending} onClick={() => apply(Number(typed))}>Save</Button>
          <Button size="sm" className="h-8" variant="ghost" disabled={!isOwner || card.retention_days === null || set.isPending} onClick={() => apply(null)}>Use the deployment default</Button>
        </div>
      </Field>
      {problem ? <span className="text-caption text-destructive-text">{problem}</span> : null}
      {isOwner ? null : <span className="text-caption text-muted-foreground">Only an owner can change this.</span>}
    </Card>
  )
}

export function OrgGeneral() {
  const { active } = useOrganisations()
  const { card, nothing } = useOrgCard()
  const rename = useRenameOrg()
  const [typed, setTyped] = useState<string | null>(null)
  const [del, setDel] = useState(false)
  const name = typed ?? card?.name ?? ''
  const problem = typed === null ? null : orgNameProblem(typed)
  const mayChange = active?.role === 'owner' || active?.role === 'admin'
  const unchanged = name.trim() === (card?.name ?? '')

  const save = () =>
    rename.mutate(name, {
      onSuccess: () => {
        setTyped(null)
        toast.success('Saved')
      },
      onError: (error) => toast.error(because(error, 'The organisation was not renamed')),
    })

  return (
    <>
      <H title="General" sub={card ? `The organisation: ${card.name}.` : 'The organisation.'} />
      <Card className="flex flex-col gap-3.5 p-4">
        <Mark card={card} mayChange={mayChange} />
        <Field label="Organisation name" htmlFor="og-n" hint={mayChange ? 'Shown in the top bar and on invitations.' : 'Only an owner or an administrator can change this.'}>
          <div className="flex max-w-[420px] gap-2">
            <Input id="og-n" mono value={name} disabled={!card || !mayChange} aria-invalid={problem ? true : undefined} onChange={(e) => setTyped(e.target.value)} />
            <Button size="sm" className="h-8" variant="primary" disabled={!card || !mayChange || unchanged || Boolean(problem) || rename.isPending} onClick={save}>Save</Button>
          </div>
        </Field>
        {problem ? <span className="text-caption text-destructive-text">{problem}</span> : null}
      </Card>
      {card ? (
        <>
          <Identity card={card} />
          <Retention card={card} isOwner={active?.role === 'owner'} />
          <Danger title={`Delete ${card.name}`} text="Deletes every project, signal, member and key of the organisation. Export first if you need anything." action="Delete organisation" onClick={() => setDel(true)} />
          <ConfirmDelete open={del} onOpenChange={setDel} name={card.name} what="organisation" body="Every project, signal, member, team, key and alert is deleted. This cannot be undone." />
        </>
      ) : (
        <Card className="px-4 py-3 text-control text-muted-foreground">{nothing}</Card>
      )}
    </>
  )
}
