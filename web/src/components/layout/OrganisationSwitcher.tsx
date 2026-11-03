import { Building2, Check, ChevronDown, Plus } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent } from '@/components/ui/dialog'
import { because } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { useSwitchOrg } from '@/data/orgs'
import { NewOrganisationForm } from './NewOrganisation'

const orgLabel = (name: string | undefined) => name ?? 'no organisation'

export function OrganisationSwitcher() {
  const { list, active } = useOrganisations()
  const change = useSwitchOrg()
  const [open, setOpen] = useState(false)
  const [creating, setCreating] = useState(false)
  const choose = (org: string) => {
    setOpen(false)
    if (org === active?.org) return
    change.mutate(org, { onError: (error) => toast.error(because(error, 'The organisation was not switched')) })
  }
  return (
    <>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <Button variant="ghost" size="sm" aria-haspopup="listbox" aria-label={`Organisation: ${orgLabel(active?.name)}`} loading={change.isPending} className="min-w-0 max-sm:px-1.5"><Building2 /><span className="max-w-[96px] truncate sm:max-w-none">{orgLabel(active?.name)}</span><ChevronDown className="text-muted-foreground max-sm:hidden" /></Button>
        </PopoverTrigger>
        <PopoverContent align="start" className="w-[300px] p-1">
          {list.map((m) => (
            <button key={m.org} type="button" onClick={() => choose(m.org)} className="flex h-[30px] w-full cursor-pointer items-center gap-2 rounded-sm px-2 text-left text-control hover:bg-muted">
              <span className="flex-1 truncate">{orgLabel(m.name)}</span>
              <Badge variant="outline">{m.role}</Badge>
              {m.org === active?.org ? <Check className="size-3.5 text-accent-text" aria-label="Active" /> : null}
            </button>
          ))}
          {list.length > 0 ? <div className="my-1 border-t" role="separator" /> : null}
          <button type="button" onClick={() => { setOpen(false); setCreating(true) }} className="flex h-[30px] w-full cursor-pointer items-center gap-2 rounded-sm px-2 text-left text-control hover:bg-muted">
            <Plus className="size-3.5" aria-hidden /><span>New organisation</span>
          </button>
        </PopoverContent>
      </Popover>
      <Dialog open={creating} onOpenChange={setCreating}>
        <DialogContent title="New organisation" description="Create an organisation and switch to it.">
          <NewOrganisationForm onCancel={() => setCreating(false)} onCreated={() => setCreating(false)} />
        </DialogContent>
      </Dialog>
    </>
  )
}
