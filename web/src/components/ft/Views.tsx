import { Building2, Check, Lock, PanelLeft, Pencil, Save, Trash2, X } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { because } from '@/auth/client'
import { useAccount, useOrganisations } from '@/auth/session'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogFooter } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Segmented } from '@/components/ui/segmented'
import {
  BUILT_IN, sameDefinition, useChangeView, useDeleteView, useSaveView, useViews,
  type Definition, type SavedView, type Visibility,
} from '@/data/views'
import { cn } from '@/lib/utils'

type Editing = { kind: 'new' } | { kind: 'edit'; view: SavedView }

export function useViewsState(current: Definition, viewKey: string | undefined, onSelect: (key: string | undefined, definition: Definition) => void) {
  const { views } = useViews()
  const account = useAccount()
  const { active } = useOrganisations()
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
  const dialog = editing ? (
    <ViewDialog
      editing={editing}
      current={current}
      canShare={editing.kind === 'new' || editing.view.createdBy === account?.account}
      onClose={() => setEditing(null)}
      onSaved={(view) => { setEditing(null); onSelect(view.id, view.definition) }}
    />
  ) : null

  return {
    views, builtIn, saved, dirty, filtered, mayChange, update, drop, dialog, updating: change.isPending,
    name: builtIn?.name ?? saved?.name ?? 'Unsaved',
    edit: (view: SavedView) => setEditing({ kind: 'edit', view }),
    saveAs: () => setEditing({ kind: 'new' }),
  }
}

export type ViewsState = ReturnType<typeof useViewsState>

export function ViewsToggle({ state, on, onToggle }: { state: ViewsState; on: boolean; onToggle: () => void }) {
  const { saved, dirty, filtered, builtIn, mayChange, update, saveAs, updating, name } = state
  const base = builtIn ?? saved
  const mine = saved !== undefined && mayChange(saved)
  return (
    <>
      <Button size="sm" aria-pressed={on} aria-label={`Views, current: ${name}${dirty ? ', changed' : ''}`} onClick={onToggle} className={cn(on && 'bg-muted')}>
        <PanelLeft />
        <span className="max-w-[180px] truncate">{name}</span>
        {dirty ? <span className="size-1.5 rounded-full bg-primary" aria-hidden /> : null}
      </Button>
      {saved && dirty && mine ? <Button size="sm" variant="primary" loading={updating} onClick={update}><Save />Update</Button> : null}
      {dirty || (!base && filtered) ? (
        <Button size="sm" variant={saved && dirty && mine ? 'outline' : 'primary'} onClick={saveAs}><Save />Save as view</Button>
      ) : null}
    </>
  )
}

export function ViewsPanel({ state, viewKey, onSelect, onClose }: { state: ViewsState; viewKey: string | undefined; onSelect: (key: string | undefined, definition: Definition) => void; onClose: () => void }) {
  const [search, setSearch] = useState('')
  const wanted = search.trim().toLowerCase()
  const matches = <T extends { name: string }>(rows: T[]) => rows.filter((row) => row.name.toLowerCase().includes(wanted))
  const builtIn = matches(BUILT_IN)
  const shared = matches(state.views.filter((one) => one.visibility === 'org'))
  const mine = matches(state.views.filter((one) => one.visibility === 'private'))
  const nothing = builtIn.length + shared.length + mine.length === 0
  return (
    <div className="flex flex-col gap-1" role="group" aria-label="All views">
      <div className="flex items-center gap-1.5 pb-1">
        <span className="flex-1 text-control font-semibold">All views</span>
        <Button variant="ghost" size="icon-sm" aria-label="Back to the filters" onClick={onClose}><X /></Button>
      </div>
      <Input value={search} onChange={(event) => setSearch(event.target.value)} placeholder="Search…" aria-label="Search the views" className="h-8" />
      {builtIn.length > 0 ? (
        <Section title="Built in">
          {builtIn.map((one) => (
            <Row key={one.key} on={one.key === viewKey} hint={one.hint} onPick={() => onSelect(one.key, one.definition)}>{one.name}</Row>
          ))}
        </Section>
      ) : null}
      {shared.length > 0 ? (
        <Section title="Shared with the organisation">
          {shared.map((one) => (
            <Row key={one.id} on={one.id === viewKey} icon={<Building2 />} onPick={() => onSelect(one.id, one.definition)}
              actions={state.mayChange(one) ? <Actions view={one} onEdit={() => state.edit(one)} onDelete={() => state.drop(one)} /> : null}>
              {one.name}
            </Row>
          ))}
        </Section>
      ) : null}
      {mine.length > 0 ? (
        <Section title="Created by me">
          {mine.map((one) => (
            <Row key={one.id} on={one.id === viewKey} icon={<Lock />} onPick={() => onSelect(one.id, one.definition)}
              actions={<Actions view={one} onEdit={() => state.edit(one)} onDelete={() => state.drop(one)} />}>
              {one.name}
            </Row>
          ))}
        </Section>
      ) : null}
      {nothing ? <p className="m-0 px-1 py-2 text-caption text-muted-foreground">No view matches.</p> : null}
      {state.views.length === 0 && wanted === '' ? <p className="m-0 px-1 py-2 text-caption text-muted-foreground">Filter the traces, then save the result as a view to come back to it.</p> : null}
    </div>
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
