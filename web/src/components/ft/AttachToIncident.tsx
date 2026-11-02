import { Paperclip } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { because } from '@/auth/client'
import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { useAttachToIncident, useIncidents, type AttachmentDraft } from '@/data/incidents'
import { Link } from '@/lib/router'

export function AttachToIncident({ attachment }: { attachment: AttachmentDraft | null }) {
  const [open, setOpen] = useState(false)
  const { incidents, failed } = useIncidents('open')
  const attach = useAttachToIncident()
  const pick = (id: string, title: string) => {
    if (!attachment) return
    attach.mutate({ id, attachment }, {
      onSuccess: () => { setOpen(false); toast.success('Attached', { description: title }) },
      onError: (error) => toast.error(because(error, 'Nothing was attached')),
    })
  }
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button size="sm" disabled={!attachment}><Paperclip />Attach to incident</Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="flex w-[300px] flex-col gap-1 p-2">
        <span className="px-2 pb-1 text-caption font-semibold text-muted-foreground">Attach the current view to</span>
        {failed ? (
          <span className="px-2 py-1 text-control text-muted-foreground">{because(failed, 'The open incidents could not be read.')}</span>
        ) : !incidents ? (
          <span className="px-2 py-1 text-control text-muted-foreground">Reading the open incidents…</span>
        ) : incidents.length === 0 ? (
          <span className="px-2 py-1 text-control text-muted-foreground">No incident is open. <Link to="/incidents">Declare one</Link> first.</span>
        ) : (
          <ul role="menu" aria-label="Open incidents" className="m-0 flex list-none flex-col gap-0.5 p-0">
            {incidents.map((one) => (
              <li key={one.id} role="none">
                <button type="button" role="menuitem" disabled={attach.isPending} onClick={() => pick(one.id, one.title)} className="flex w-full cursor-pointer flex-col items-start gap-0.5 rounded-sm border-0 bg-transparent px-2 py-1.5 text-left hover:bg-muted focus-visible:outline-2 focus-visible:outline-ring disabled:cursor-default disabled:opacity-60">
                  <span className="text-control font-semibold">{one.title}</span>
                  <span className="text-caption text-muted-foreground">{one.severity.toUpperCase()} · {one.status}</span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </PopoverContent>
    </Popover>
  )
}
