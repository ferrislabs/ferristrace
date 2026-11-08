import { Plus, X } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { because } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { Button } from '@/components/ui/button'
import { Field, Input, Textarea } from '@/components/ui/input'
import { Segmented } from '@/components/ui/segmented'
import { Switch } from '@/components/ui/switch'
import { DESIGN_FACTS, PLACEHOLDERS, previewDesign, STATUSES, type AlertStatus, type Design, type DesignFact, type DiscordMessage } from '@/data/alerts'

const MAX_FIELDS = 5
const STATE_LABEL: Record<AlertStatus, string> = { firing: 'Firing', ok: 'Resolved', pending: 'Pending', no_data: 'No data' }
const DEFAULT_COLOR: Record<AlertStatus, string> = { firing: '#d92d20', ok: '#12b76a', pending: '#fdb022', no_data: '#98a2b3' }

const COLOR_KEY: Record<AlertStatus, keyof Design['colors']> = { firing: 'firing', ok: 'resolved', pending: 'pending', no_data: 'no_data' }

type Text = 'content' | 'title' | 'description' | 'author' | 'footer'

const ids = (text: string) => text.split(/[\s,]+/).filter((one) => one.length > 0)

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <fieldset className="m-0 flex min-w-0 flex-col gap-3 rounded-md border border-border-subtle p-3">
      <legend className="px-1 text-caption font-semibold uppercase text-muted-foreground">{title}</legend>
      {children}
    </fieldset>
  )
}

function Preview({ message }: { message: DiscordMessage }) {
  const embed = message.embeds?.[0]
  const color = embed?.color !== undefined ? `#${embed.color.toString(16).padStart(6, '0')}` : '#4e5058'
  return (
    <div className="rounded-md bg-[#313338] p-3 font-sans text-[14px] leading-[1.35] text-[#dbdee1]" aria-label="Discord preview">
      <div className="flex gap-3">
        <div className="mt-0.5 size-9 shrink-0 rounded-full bg-[#5865f2]" aria-hidden />
        <div className="min-w-0 flex-1">
          <div className="mb-0.5"><span className="font-semibold text-white">FerrisTrace</span> <span className="ml-1 rounded bg-[#5865f2] px-1 text-[10px] font-semibold uppercase text-white">App</span></div>
          {message.content ? <div className="mb-1 break-words">{message.content}</div> : null}
          {embed ? (
            <div className="flex max-w-[520px] overflow-hidden rounded-[4px] bg-[#2b2d31]" style={{ borderLeft: `4px solid ${color}` }}>
              <div className="flex min-w-0 flex-1 flex-col gap-1.5 p-3">
                {embed.author ? <div className="text-[12px] font-semibold text-white">{embed.author.name}</div> : null}
                {embed.title ? <div className={`break-words font-semibold ${embed.url ? 'text-[#00a8fc]' : 'text-white'}`}>{embed.title}</div> : null}
                {embed.description ? <div className="whitespace-pre-wrap break-words text-[13px]">{embed.description}</div> : null}
                {embed.fields?.length ? (
                  <div className="grid grid-cols-3 gap-x-3 gap-y-2">
                    {embed.fields.map((one, index) => (
                      <div key={`${one.name}-${index}`} className={one.inline ? 'min-w-0' : 'col-span-3 min-w-0'}>
                        <div className="text-[12px] font-semibold text-white">{one.name}</div>
                        <div className="break-words text-[13px]">{one.value}</div>
                      </div>
                    ))}
                  </div>
                ) : null}
                {embed.footer || embed.timestamp ? <div className="text-[11px] text-[#949ba4]">{[embed.footer?.text, embed.timestamp?.slice(0, 16).replace('T', ' ')].filter(Boolean).join(' • ')}</div> : null}
              </div>
              {embed.thumbnail ? <img src={embed.thumbnail.url} alt="" className="m-3 size-16 shrink-0 rounded object-cover" /> : null}
            </div>
          ) : null}
        </div>
      </div>
    </div>
  )
}

export function DiscordDesigner({ design, onChange }: { design: Design; onChange: (next: Design) => void }) {
  const { active } = useOrganisations()
  const [state, setState] = useState<AlertStatus>('firing')
  const [message, setMessage] = useState<DiscordMessage | null>(null)
  const [problem, setProblem] = useState<string | null>(null)
  const focused = useRef<Text | null>(null)

  useEffect(() => {
    if (!active) return
    let current = true
    const timer = setTimeout(() => {
      previewDesign(active.org, design, state).then(
        (shown) => { if (current) { setMessage(shown); setProblem(null) } },
        (error) => { if (current) setProblem(because(error, 'The preview could not be drawn')) },
      )
    }, 300)
    return () => { current = false; clearTimeout(timer) }
  }, [active, design, state])

  const text = (key: Text) => design[key] ?? ''
  const setText = (key: Text, value: string) => onChange({ ...design, [key]: value === '' ? null : value })
  const insert = (name: string) => {
    const key = focused.current ?? 'title'
    setText(key, `${text(key)}{${name}}`)
  }
  const color = (key: AlertStatus) => design.colors[COLOR_KEY[key]] ?? DEFAULT_COLOR[key]
  const setColor = (key: AlertStatus, value: string | null) => onChange({ ...design, colors: { ...design.colors, [COLOR_KEY[key]]: value } })
  const setFact = (key: DesignFact, value: boolean) => onChange({ ...design, shown: { ...design.shown, [key]: value } })
  const setField = (index: number, patch: Partial<Design['fields'][number]>) =>
    onChange({ ...design, fields: design.fields.map((one, at) => (at === index ? { ...one, ...patch } : one)) })

  return (
    <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
      <div className="flex min-w-0 flex-col gap-3">
        <Section title="Text">
          <Field label="Message text" htmlFor="dd-content" hint="Shown above the embed. Leave empty for none."><Input id="dd-content" value={text('content')} onFocus={() => { focused.current = 'content' }} onChange={(e) => setText('content', e.target.value)} /></Field>
          <Field label="Title" htmlFor="dd-title" hint="Empty keeps “{state}: {alert}”."><Input id="dd-title" value={text('title')} onFocus={() => { focused.current = 'title' }} onChange={(e) => setText('title', e.target.value)} placeholder="{state}: {alert}" /></Field>
          <Field label="Description" htmlFor="dd-description"><Textarea id="dd-description" rows={3} value={text('description')} onFocus={() => { focused.current = 'description' }} onChange={(e) => setText('description', e.target.value)} placeholder="{value} errors on {label.service}" /></Field>
          <div className="grid grid-cols-2 gap-3">
            <Field label="Author" htmlFor="dd-author"><Input id="dd-author" value={text('author')} onFocus={() => { focused.current = 'author' }} onChange={(e) => setText('author', e.target.value)} /></Field>
            <Field label="Footer" htmlFor="dd-footer"><Input id="dd-footer" value={text('footer')} onFocus={() => { focused.current = 'footer' }} onChange={(e) => setText('footer', e.target.value)} /></Field>
          </div>
          <div className="flex flex-wrap items-center gap-1.5" aria-label="Placeholders">
            <span className="text-caption text-muted-foreground">Insert</span>
            {PLACEHOLDERS.map((name) => <button key={name} type="button" onClick={() => insert(name)} className="cursor-pointer rounded border border-border-subtle bg-muted px-1.5 py-0.5 font-mono text-[11.5px] text-foreground hover:bg-accent">{`{${name}}`}</button>)}
          </div>
        </Section>
        <Section title="Look">
          <div className="grid grid-cols-2 gap-3">
            {STATUSES.map((one) => (
              <Field key={one} label={`Colour · ${STATE_LABEL[one]}`} htmlFor={`dd-color-${one}`}>
                <span className="flex items-center gap-2">
                  <input id={`dd-color-${one}`} type="color" value={color(one)} onChange={(e) => setColor(one, e.target.value)} className="h-8 w-10 cursor-pointer rounded border border-input bg-background p-0.5" />
                  <span className="font-mono text-caption text-muted-foreground">{color(one)}</span>
                  {design.colors[COLOR_KEY[one]] ? <button type="button" onClick={() => setColor(one, null)} className="cursor-pointer text-caption text-muted-foreground underline">reset</button> : null}
                </span>
              </Field>
            ))}
          </div>
          <Field label="Thumbnail" htmlFor="dd-thumb" hint="An https image URL. Empty for none."><Input id="dd-thumb" mono value={design.thumbnail ?? ''} onChange={(e) => onChange({ ...design, thumbnail: e.target.value === '' ? null : e.target.value })} placeholder="https://cdn.example.com/logo.png" /></Field>
        </Section>
        <Section title="Facts">
          <div className="grid grid-cols-2 gap-x-3 gap-y-1">
            {DESIGN_FACTS.map((one) => <Switch key={one.key} label={one.label} checked={design.shown[one.key]} onCheckedChange={(on) => setFact(one.key, on)} />)}
          </div>
        </Section>
        <Section title="Custom fields">
          {design.fields.map((one, index) => (
            <div key={index} className="grid grid-cols-[1fr_1fr_auto_auto] items-center gap-2">
              <Input aria-label={`Field ${index + 1} name`} value={one.name} onChange={(e) => setField(index, { name: e.target.value })} placeholder="Name" />
              <Input aria-label={`Field ${index + 1} value`} value={one.value} onChange={(e) => setField(index, { value: e.target.value })} placeholder="{label.service}" />
              <Switch label="Inline" checked={one.inline} onCheckedChange={(on) => setField(index, { inline: on })} />
              <Button type="button" variant="ghost" size="sm" aria-label={`Remove field ${index + 1}`} onClick={() => onChange({ ...design, fields: design.fields.filter((_, at) => at !== index) })}><X /></Button>
            </div>
          ))}
          {design.fields.length < MAX_FIELDS ? <div><Button type="button" size="sm" onClick={() => onChange({ ...design, fields: [...design.fields, { name: '', value: '', inline: true }] })}><Plus />Add a field</Button></div> : null}
        </Section>
        <Section title="Ping when it fires">
          <Field label="Role ids" htmlFor="dd-roles" hint="Discord ids, separated by spaces or commas. Pings only on firing, never @everyone."><Input id="dd-roles" mono value={design.ping_roles.join(', ')} onChange={(e) => onChange({ ...design, ping_roles: ids(e.target.value) })} /></Field>
          <Field label="User ids" htmlFor="dd-users"><Input id="dd-users" mono value={design.ping_users.join(', ')} onChange={(e) => onChange({ ...design, ping_users: ids(e.target.value) })} /></Field>
        </Section>
      </div>
      <div className="flex min-w-0 flex-col gap-3 lg:sticky lg:top-0 lg:self-start">
        <div className="flex items-center justify-between gap-2">
          <h3 className="m-0 text-control font-semibold">Preview</h3>
          <Segmented label="Preview state" value={state} onChange={setState} options={STATUSES.map((one) => ({ value: one, label: STATE_LABEL[one] }))} />
        </div>
        {message ? <Preview message={message} /> : <div className="rounded-md bg-muted p-3 text-control text-muted-foreground">Drawing the preview…</div>}
        {problem ? <span className="text-caption text-destructive-text">{problem}</span> : null}
        <span className="text-caption text-muted-foreground">Drawn by the server with a sample alert, so it matches what Discord receives. The link to the alert needs a base URL on the server.</span>
      </div>
    </div>
  )
}
