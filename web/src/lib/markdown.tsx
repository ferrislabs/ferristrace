import type { ReactNode } from 'react'
import { cn } from '@/lib/utils'

const SAFE_LINK = /^(?:https?:\/\/|mailto:|\/(?!\/))/i

const INLINE = /(`[^`]+`)|(\*\*(?:\\.|[^*\\])+\*\*)|(\[((?:\\.|[^\]\\])*)\]\(([^)\s]*)\))|(_(?:\\.|[^_\\])+_)/g

const ESCAPED = /\\([\\`*_[\]|()])/g

const FENCE = /^\s*```/

const HEADING = /^(#{1,6})\s+(.*)$/

const ITEM = /^\s*([-*]|\d+\.)\s+(.*)$/

const TASK = /^\[([ xX])\]\s*(.*)$/

const RULE = /^\s*\|?\s*:?-{3,}:?\s*(\|\s*:?-{3,}:?\s*)*\|?\s*$/

const CELL_BREAK = /(?<!\\)\|/

const unescaped = (text: string) => text.replace(ESCAPED, '$1')

function inline(text: string, base: string): ReactNode[] {
  const out: ReactNode[] = []
  let last = 0
  let count = 0
  for (const found of text.matchAll(INLINE)) {
    const at = found.index ?? 0
    if (at > last) out.push(unescaped(text.slice(last, at)))
    const key = `${base}-${count++}`
    if (found[1]) {
      out.push(<code key={key} className="rounded-sm bg-muted px-1 font-mono text-[0.92em]">{found[1].slice(1, -1)}</code>)
    } else if (found[2]) {
      out.push(<strong key={key}>{inline(found[2].slice(2, -2), key)}</strong>)
    } else if (found[3] !== undefined) {
      const target = found[5] ?? ''
      const label = inline(found[4] ?? '', key)
      out.push(SAFE_LINK.test(target)
        ? <a key={key} href={target} rel="noreferrer noopener" className="text-accent-text underline">{label}</a>
        : <span key={key}>{label}</span>)
    } else if (found[6]) {
      out.push(<em key={key}>{inline(found[6].slice(1, -1), key)}</em>)
    }
    last = at + found[0].length
  }
  if (last < text.length) out.push(unescaped(text.slice(last)))
  return out
}

const cells = (line: string) => {
  const trimmed = line.trim().replace(/^\|/, '').replace(/(?<!\\)\|$/, '')
  return trimmed.split(CELL_BREAK).map((cell) => cell.trim())
}

const startsBlock = (line: string, next: string | undefined) =>
  FENCE.test(line) || HEADING.test(line) || ITEM.test(line) || /^\s*>/.test(line) || (line.trim().startsWith('|') && next !== undefined && RULE.test(next))

const HEADING_CLASS = ['text-title', 'text-[17px] font-bold', 'text-[15px] font-bold', 'text-control font-bold', 'text-control font-semibold', 'text-control font-semibold']

export function Markdown({ source, className }: { source: string; className?: string }) {
  const lines = source.replace(/\r\n?/g, '\n').split('\n')
  const blocks: ReactNode[] = []
  let at = 0
  while (at < lines.length) {
    const line = lines[at] ?? ''
    const key = `b${at}`
    const next = lines[at + 1]
    if (line.trim() === '') {
      at += 1
    } else if (FENCE.test(line)) {
      const body: string[] = []
      at += 1
      while (at < lines.length && !FENCE.test(lines[at] ?? '')) body.push(lines[at++] ?? '')
      at += 1
      blocks.push(<pre key={key} className="overflow-x-auto rounded-md border bg-muted p-3 font-mono text-caption"><code>{body.join('\n')}</code></pre>)
    } else if (HEADING.test(line)) {
      const found = HEADING.exec(line)
      const level = found?.[1]?.length ?? 1
      const Tag = `h${level}` as 'h1'
      blocks.push(<Tag key={key} className={cn('mt-2', HEADING_CLASS[level - 1])}>{inline(found?.[2] ?? '', key)}</Tag>)
      at += 1
    } else if (line.trim().startsWith('|') && next !== undefined && RULE.test(next)) {
      const head = cells(line)
      const rows: string[][] = []
      at += 2
      while (at < lines.length && (lines[at] ?? '').trim().startsWith('|')) rows.push(cells(lines[at++] ?? ''))
      blocks.push(
        <div key={key} className="overflow-x-auto">
          <table className="w-full border-collapse text-control">
            <thead><tr>{head.map((cell, index) => <th key={index} className="border-b px-2 py-1 text-left font-semibold">{inline(cell, `${key}h${index}`)}</th>)}</tr></thead>
            <tbody>{rows.map((row, r) => <tr key={r}>{row.map((cell, index) => <td key={index} className="border-b px-2 py-1 align-top">{inline(cell, `${key}r${r}c${index}`)}</td>)}</tr>)}</tbody>
          </table>
        </div>,
      )
    } else if (ITEM.test(line)) {
      const ordered = /^\s*\d+\./.test(line)
      const items: ReactNode[] = []
      while (at < lines.length && ITEM.test(lines[at] ?? '')) {
        const text = ITEM.exec(lines[at] ?? '')?.[2] ?? ''
        const task = TASK.exec(text)
        const itemKey = `${key}i${at}`
        items.push(
          task ? (
            <li key={itemKey} className="flex list-none items-start gap-2"><input type="checkbox" checked={task[1] !== ' '} readOnly disabled className="mt-1" /><span>{inline(task[2] ?? '', itemKey)}</span></li>
          ) : (
            <li key={itemKey}>{inline(text, itemKey)}</li>
          ),
        )
        at += 1
      }
      const List = ordered ? 'ol' : 'ul'
      blocks.push(<List key={key} className={cn('m-0 flex flex-col gap-1 pl-5', ordered ? 'list-decimal' : 'list-disc')}>{items}</List>)
    } else if (/^\s*>/.test(line)) {
      const quoted: string[] = []
      while (at < lines.length && /^\s*>/.test(lines[at] ?? '')) quoted.push((lines[at++] ?? '').replace(/^\s*>\s?/, ''))
      blocks.push(<blockquote key={key} className="m-0 border-l-2 pl-3 text-muted-foreground">{inline(quoted.join(' '), key)}</blockquote>)
    } else {
      const paragraph: string[] = []
      while (at < lines.length && (lines[at] ?? '').trim() !== '' && (paragraph.length === 0 || !startsBlock(lines[at] ?? '', lines[at + 1]))) paragraph.push((lines[at++] ?? '').trim())
      blocks.push(<p key={key} className="m-0">{inline(paragraph.join(' '), key)}</p>)
    }
  }
  return <div className={cn('flex flex-col gap-3 text-control', className)}>{blocks}</div>
}
