import { Building2, Check, ChevronDown, Lock, Pencil, Save, Trash2 } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { because } from '@/auth/client'
import { useAccount, useOrganisations } from '@/auth/session'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogFooter } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Segmented } from '@/components/ui/segmented'
import {
  BUILT_IN, sameDefinition, useChangeView, useDeleteView, useSaveView, useViews,
  type Definition, type SavedView, type Visibility,
} from '@/data/views'
import { cn } from '@/lib/utils'

type Editing = { kind: 'new' } | { kind: 'edit'; view: SavedView }

export function Views({ current, viewKey, onSelect }: { current: Definition; viewKey: string | undefined; onSelect: (key: string | undefined, definition: Definition) => void }) {
  const { views } = useViews()
  const account = useAccount()
  const { active } = useOrganisations()
  const [open, setOpen] = useState(false)
  const [editing, setEditing] = useState<Editing | null>(null)
  const change = useChangeView()
  const remove = useDeleteView()

  const builtIn = BUILT_IN.find((one) => one.key === viewKey)
  const saved = views.find((one) => one.id === viewKey)
  const base = builtIn?.definition ?? saved?.definition
  const dirty = base !== undefined && !sameDefinition(base, current)
  const filtered = current.q.length > 0 || current.o !== 'started'
  const isAdmin = active?.role === 'owner' || active?.role === 'admin'
  const mayChange = (view: SavedView) => view.createdBy === account?.account || (view.visibility === 'org' && isAdmin)
  const name = builtIn?.name ?? saved?.name ?? 'Unsaved'

  const update = () => {
    if (!saved) return
    change.mutate(
      { id: saved.id, name: saved.name, definition: current },
      {
        onSuccess: () => toast.success('View updated', { description: saved.name }),
        onError: (error) => toast.error(because(error, 'The view was not updated')),
      },
    )
  }
  const drop = (view: SavedView) => {
    if (!window.confirm(`Delete the view “${view.name}”?${view.visibility === 'org' ? ' Everyone in the organisation loses it.' : ''}`)) return
    remove.mutate(view.id, {
      onSuccess: () => {
        toast.success('View deleted', { description: view.name })
        if (view.id === viewKey) onSelect(undefined, current)
      },
      onError: (error) => toast.error(because(error, 'The view was not deleted')),
    })
  }
  const mine = views.filter((one) => one.visibility === 'private')
  const shared = views.filter((one) => one.visibility === 'org')

  return (
    <>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <Button size="sm" aria-label={`View: ${name}${dirty ? ', changed' : ''}`}>
            <span className="max-w-[180px] truncate">{name}</span>
            {dirty ? <span className="size-1.5 rounded-full bg-primary" aria-hidden /> : null}
            <ChevronDown className="text-muted-foreground" />
          </Button>
        </PopoverTrigger>
        <PopoverContent align="start" className="w-[320px] p-1">
          <Section title="Built in">
            {BUILT_IN.map((one) => (
              <Row key={one.key} on={one.key === viewKey} onPick={() => { onSelect(one.key, one.definition); setOpen(false) }} hint={one.hint}>{one.name}</Row>
            ))}
          </Section>
          {shared.length > 0 ? (
            <Section title="Shared with the organisation">
              {shared.map((one) => (
                <Row key={one.id} on={one.id === viewKey} icon={<Building2 />} onPick={() => { onSelect(one.id, one.definition); setOpen(false) }}
                  actions={mayChange(one) ? <Actions view={one} onEdit={() => { setOpen(false); setEditing({ kind: 'edit', view: one }) }} onDelete={() => drop(one)} /> : null}>
                  {one.name}
                </Row>
              ))}
            </Section>
          ) : null}
          {mine.length > 0 ? (
            <Section title="Only you">
              {mine.map((one) => (
                <Row key={one.id} on={one.id === viewKey} icon={<Lock />} onPick={() => { onSelect(one.id, one.definition); setOpen(false) }}
                  actions={<Actions view={one} onEdit={() => { setOpen(false); setEditing({ kind: 'edit', view: one }) }} onDelete={() => drop(one)} />}>
                  {one.name}
                </Row>
              ))}
            </Section>
          ) : null}
          {views.length === 0 ? <p className="m-0 px-2 py-2 text-caption text-muted-foreground">Filter the traces, then save the result as a view to come back to it.</p> : null}
        </PopoverContent>
      </Popover>

      {saved && dirty && mayChange(saved) ? (
        <Button size="sm" variant="primary" loading={change.isPending} onClick={update}><Save />Update</Button>
      ) : null}
      {(dirty || (!base && filtered)) ? (
        <Button size="sm" variant={saved && dirty && mayChange(saved) ? 'outline' : 'primary'} onClick={() => setEditing({ kind: 'new' })}><Save />Save as view</Button>
      ) : null}

      {editing ? (
        <ViewDialog
          editing={editing}
          current={current}
          canShare={editing.kind === 'new' || editing.view.createdBy === account?.account}
          onClose={() => setEditing(null)}
          onSaved={(view) => { setEditing(null); onSelect(view.id, view.definition) }}
        />
      ) : null}
    </>
  )
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col py-1">
      <span className="px-2 pb-1 pt-1.5 text-[11px] font-semibold uppercase tracking-[.05em] text-muted-foreground">{title}</span>
      {children}
    </div>
  )
}

function Row({ on, icon, hint, actions, onPick, children }: { on: boolean; icon?: React.ReactNode; hint?: string; actions?: React.ReactNode; onPick: () => void; children: React.ReactNode }) {
  return (
    <div className={cn('group flex h-8 items-center rounded-sm hover:bg-muted focus-within:bg-muted', on && 'bg-accent-subtle')}>
      <button type="button" onClick={onPick} aria-current={on || undefined} className="flex h-full min-w-0 flex-1 cursor-pointer items-center gap-2 px-2 text-left text-control focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring [&>svg]:size-3.5 [&>svg]:text-muted-foreground">
        {icon}
        <span className="truncate">{children}</span>
        {hint ? <span className="ml-auto truncate text-caption text-muted-foreground">{hint}</span> : null}
        {on ? <Check className="ml-auto !text-accent-text" aria-hidden /> : null}
      </button>
      {actions}
    </div>
  )
}

function Actions({ view, onEdit, onDelete }: { view: SavedView; onEdit: () => void; onDelete: () => void }) {
  return (
    <span className="flex shrink-0 pr-1 opacity-0 group-focus-within:opacity-100 group-hover:opacity-100">
      <Button variant="ghost" size="icon-sm" aria-label={`Edit ${view.name}`} onClick={onEdit}><Pencil /></Button>
      <Button variant="ghost" size="icon-sm" aria-label={`Delete ${view.name}`} onClick={onDelete}><Trash2 /></Button>
    </span>
  )
}

function ViewDialog({ editing, current, canShare, onClose, onSaved }: { editing: Editing; current: Definition; canShare: boolean; onClose: () => void; onSaved: (view: SavedView) => void }) {
  const edit = editing.kind === 'edit' ? editing.view : undefined
  const [name, setName] = useState(edit?.name ?? '')
  const [visibility, setVisibility] = useState<Visibility>(edit?.visibility ?? 'private')
  const save = useSaveView()
  const change = useChangeView()
  const busy = save.isPending || change.isPending
  const submit = () => {
    const trimmed = name.trim()
    if (!trimmed) return
    const failed = (error: unknown) => toast.error(because(error, 'The view was not saved'))
    if (edit) {
      change.mutate(
        { id: edit.id, name: trimmed, definition: edit.definition, visibility: canShare && visibility !== edit.visibility ? visibility : undefined },
        { onSuccess: onSaved, onError: failed },
      )
    } else {
      save.mutate({ name: trimmed, visibility, definition: current }, { onSuccess: (view) => { toast.success('View saved', { description: view.name }); onSaved(view) }, onError: failed })
    }
  }
  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent title={edit ? 'Edit view' : 'Save as view'} description={edit ? undefined : 'Keeps the filters and the order you see now.'}>
        <form className="flex flex-col gap-3" onSubmit={(event) => { event.preventDefault(); submit() }}>
          <label className="flex flex-col gap-1 text-caption font-semibold text-muted-foreground">
            Name
            <Input autoFocus value={name} maxLength={80} onChange={(event) => setName(event.target.value)} placeholder="Errors on checkout" aria-label="View name" />
          </label>
          {canShare ? (
            <div className="flex flex-col gap-1 text-caption font-semibold text-muted-foreground">
              Who can see it
              <Segmented label="Who can see it" value={visibility} onChange={setVisibility} options={[{ value: 'private', label: 'Only me' }, { value: 'org', label: 'The organisation' }]} />
            </div>
          ) : null}
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={onClose}>Cancel</Button>
            <Button type="submit" variant="primary" loading={busy} disabled={name.trim() === ''}>{edit ? 'Save' : 'Save view'}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
