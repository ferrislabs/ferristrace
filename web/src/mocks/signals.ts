/* eslint-disable */
export type Col = { label: string; cls: string; sort: 'none' | 'ascending' | 'descending' }
export type Cell = { v: string; cls: string; title: string; warn: boolean; bar?: number; barColor?: string }
export type SignalView = {
  title: string; subtitle: string; group: string[]
  kpis: [string, string, string, string, string][]
  chart: [string, string, string, string, string, string]
  a: (_i: number) => number; lo: number; hi: number; b: (_i: number) => number
  table: [string, string, string]; cols: Col[]; rows: Cell[][]; foot: string; empty: [string, string, string]
}
let seed = 5
const rnd = () => { seed = (seed * 9301 + 49297) % 233280; return seed / 233280 }
export const resetSeed = (s = 5) => { seed = s }
const C = (label: string, cls = '', sort: Col['sort'] = 'none'): Col => ({ label, cls, sort })
const X = (v: string, cls = '', o: { title?: string; warn?: boolean | number; bar?: number; barColor?: string } = {}): Cell => ({ v, cls, title: o.title || '', warn: !!o.warn, bar: o.bar, barColor: o.barColor })
// eslint-disable-next-line @typescript-eslint/no-explicit-any
type R = any[]

export const BACKEND: Record<string, SignalView> = {
      emails: {
        title: 'Emails', subtitle: 'Outgoing mail, from mailer spans', group: ['Template', 'Provider'],
        kpis: [['Sent', '3,904', 'last hour', '', 'muted'], ['Failed', '64', '1.6 % · SMTP 421 from provider', 'warn-text', 'warn-text'], ['Send time p95', '880 ms', 'provider round trip', '', 'muted'], ['Queued now', '12', 'oldest 40 s', '', 'muted']],
        chart: ['Emails sent per minute', 'solid: sent · dashed: failed ×20', '100', '50', '0', 'About 65 emails per minute; failures appear after 14:10'],
        a: (i: number) => 65 + Math.sin(i / 7) * 10 + (rnd() - 0.5) * 8, lo: 0, hi: 100, b: (i: number) => (i >= 38 ? 2.5 : 0.2) * 20 + (rnd() - 0.5) * 4,
        table: ['Templates', '9 templates', 'Filter templates'],
        cols: [C('Template'), C('Provider', 'cw-120'), C('Sent', 'num cw-90'), C('Failed', 'num cw-90', 'descending'), C('Send p95', 'num cw-90'), C('Last sent', 'cw-110')],
        rows: [['order_confirmation', 'smtp · mailgun', '1,210', '41', '940 ms', '14:32:02', true], ['welcome_email', 'smtp · mailgun', '88', '9', '820 ms', '14:31:40', true], ['shipping_update', 'smtp · mailgun', '1,402', '8', '860 ms', '14:32:01'], ['cart_reminder', 'smtp · mailgun', '960', '6', '790 ms', '14:30:00'], ['invoice', 'smtp · mailgun', '244', '0', '1.1 s', '14:12:44']].map((r: R) => [X(r[0], 'mono strong'), X(r[1], 'mono sm muted'), X(r[2], 'num mono'), X(r[3], 'num mono ' + (r[6] ? 'warn-text strong' : ''), { warn: r[6] }), X(r[4], 'num mono'), X(r[5], 'mono sm')]),
        foot: 'FerrisTrace records the template, recipient domain and result, never the address or the body. Recipients show as *@domain.',
        empty: ['No emails yet', 'Emails appear when your mailer sends a span named email.send with ferristrace.email.template. Addresses and bodies are not recorded.', 'span.setAttribute("ferristrace.email.template", "order_confirmation")']
      },
      system: {
        title: 'System metrics', subtitle: 'Hosts and containers, from host and process metrics', group: ['Host', 'Service'],
        kpis: [['Hosts', '6', '4 app · 2 workers', '', 'muted'], ['CPU', '41 %', 'average · peak 78 % on app-3', '', 'muted'], ['Memory', '63 %', '38 of 60 GB', '', 'muted'], ['Disk', '71 %', 'worker-1 at 88 %', 'warn-text', 'warn-text']],
        chart: ['CPU, all hosts', 'solid: average · dashed: busiest host', '100 %', '50 %', '0', 'Average CPU near 40 percent; busiest host between 60 and 80 percent'],
        a: (i: number) => 41 + Math.sin(i / 8) * 6 + (rnd() - 0.5) * 4, lo: 0, hi: 100, b: (i: number) => 68 + Math.sin(i / 5) * 8 + (rnd() - 0.5) * 6,
        table: ['Hosts', 'from system.* and process.* metrics', 'Filter hosts'],
        cols: [C('Host'), C('Services', 'cw-160'), C('CPU', 'cw-140', 'descending'), C('Memory', 'cw-140'), C('Disk', 'cw-140'), C('Load 1m', 'num cw-80'), C('Up', 'cw-90')],
        rows: [['app-3', 'checkout-api, api-gateway', 78, 71, 52, '3.1', '12 d'], ['app-1', 'api-gateway, auth', 46, 62, 49, '1.8', '12 d'], ['app-2', 'api-gateway, inventory', 39, 58, 50, '1.5', '12 d'], ['app-4', 'payments', 33, 66, 47, '1.2', '3 d'], ['worker-1', 'notifier, webhook-sender', 28, 55, 88, '0.9', '40 d'], ['worker-2', 'data jobs', 22, 49, 61, '0.7', '40 d']].map((r: R) => [X(r[0], 'mono strong'), X(r[1], 'sm muted'), X(r[2] + ' %', 'mono small', { bar: r[2], barColor: r[2] >= 75 ? 'bar-w' : 'bar-p' }), X(r[3] + ' %', 'mono small', { bar: r[3], barColor: r[3] >= 85 ? 'bar-w' : 'bar-p' }), X(r[4] + ' %', 'mono small ' + (r[4] >= 85 ? 'warn-text strong' : ''), { bar: r[4], barColor: r[4] >= 85 ? 'bar-w' : 'bar-p', warn: r[4] >= 85 }), X(r[5], 'num mono'), X(r[6], 'mono sm')]),
        foot: 'Needs the hostmetrics receiver of the OpenTelemetry Collector, or the host metrics package of your SDK. Amber from 75 % CPU and 85 % disk.',
        empty: ['No system metrics yet', 'Add the hostmetrics receiver to your OpenTelemetry Collector and point its exporter at FerrisTrace.', 'receivers:\n  hostmetrics:\n    scrapers: [cpu, memory, disk, load, network]']
      }
    }
