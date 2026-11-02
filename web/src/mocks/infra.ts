export type PodStatus = 'Running' | 'Pending' | 'CrashLoopBackOff' | 'OOMKilled' | 'Completed'
export const CLUSTERS = ['prod-eu-1', 'staging-eu-1']
export const NAMESPACES = ['storefront', 'payments', 'data', 'ingress', 'kube-system']

export const WORKLOADS = [
  { ns: 'storefront', name: 'checkout-api', kind: 'Deployment', ready: '4/4', restarts: 0, cpu: 62, mem: 58, image: 'acme/checkout-api:2.14.1', svc: 'checkout-api', status: 'ok' },
  { ns: 'storefront', name: 'api-gateway', kind: 'Deployment', ready: '6/6', restarts: 0, cpu: 48, mem: 41, image: 'acme/api-gateway:5.2.0', svc: 'api-gateway', status: 'ok' },
  { ns: 'storefront', name: 'inventory', kind: 'Deployment', ready: '3/3', restarts: 1, cpu: 35, mem: 52, image: 'acme/inventory:3.2.0', svc: 'inventory', status: 'ok' },
  { ns: 'payments', name: 'payments', kind: 'Deployment', ready: '3/3', restarts: 0, cpu: 41, mem: 66, image: 'acme/payments:1.9.0', svc: 'payments', status: 'ok' },
  { ns: 'data', name: 'notifier', kind: 'Deployment', ready: '1/2', restarts: 14, cpu: 22, mem: 98, image: 'acme/notifier:0.8.3', svc: 'notifier', status: 'bad' },
  { ns: 'data', name: 'webhook-sender', kind: 'Deployment', ready: '0/1', restarts: 0, cpu: 0, mem: 0, image: 'acme/webhook-sender:1.4.0', svc: 'webhook-sender', status: 'pending' },
  { ns: 'data', name: 'search-reindex', kind: 'CronJob', ready: '—', restarts: 0, cpu: 0, mem: 0, image: 'acme/search:2.0.1', svc: '', status: 'ok' },
  { ns: 'ingress', name: 'ingress-nginx', kind: 'DaemonSet', ready: '6/6', restarts: 0, cpu: 18, mem: 22, image: 'ingress-nginx/controller:v1.11.2', svc: '', status: 'ok' },
  { ns: 'kube-system', name: 'otel-collector', kind: 'DaemonSet', ready: '6/6', restarts: 0, cpu: 9, mem: 31, image: 'otel/opentelemetry-collector-contrib:0.111.0', svc: '', status: 'ok' },
]

export const PODS: { name: string; ns: string; status: PodStatus; node: string; restarts: number; age: string; cpu: string; mem: string; workload: string; reason?: string }[] = [
  { name: 'notifier-7c9d8b6f4-kq2lz', ns: 'data', status: 'CrashLoopBackOff', node: 'node-pool-b-2', restarts: 14, age: '3 h', cpu: '12m', mem: '508Mi / 512Mi', workload: 'notifier', reason: 'OOMKilled · exit code 137 · last at 14:32:04' },
  { name: 'webhook-sender-5f6b7c8d9-x8mpt', ns: 'data', status: 'Pending', node: '—', restarts: 0, age: '23 min', cpu: '—', mem: '—', workload: 'webhook-sender', reason: 'FailedScheduling · 0/6 nodes have 2 CPU free' },
  { name: 'notifier-7c9d8b6f4-p4hvw', ns: 'data', status: 'Running', node: 'node-pool-b-1', restarts: 3, age: '3 h', cpu: '96m', mem: '402Mi / 512Mi', workload: 'notifier' },
  { name: 'checkout-api-6d5f9c7b8-2jx9k', ns: 'storefront', status: 'Running', node: 'node-pool-a-3', restarts: 0, age: '28 min', cpu: '410m', mem: '612Mi / 1Gi', workload: 'checkout-api' },
  { name: 'checkout-api-6d5f9c7b8-7mq4c', ns: 'storefront', status: 'Running', node: 'node-pool-a-1', restarts: 0, age: '28 min', cpu: '388m', mem: '590Mi / 1Gi', workload: 'checkout-api' },
  { name: 'checkout-api-6d5f9c7b8-c81zn', ns: 'storefront', status: 'Running', node: 'node-pool-a-2', restarts: 0, age: '28 min', cpu: '402m', mem: '598Mi / 1Gi', workload: 'checkout-api' },
  { name: 'checkout-api-6d5f9c7b8-vt5ds', ns: 'storefront', status: 'Running', node: 'node-pool-a-3', restarts: 0, age: '28 min', cpu: '395m', mem: '604Mi / 1Gi', workload: 'checkout-api' },
  { name: 'payments-84b7d9c5f-hm2rq', ns: 'payments', status: 'Running', node: 'node-pool-a-1', restarts: 0, age: '3 h', cpu: '220m', mem: '680Mi / 1Gi', workload: 'payments' },
  { name: 'inventory-59c8b7f6d-zq7lw', ns: 'storefront', status: 'Running', node: 'node-pool-a-2', restarts: 1, age: '15 d', cpu: '140m', mem: '266Mi / 512Mi', workload: 'inventory' },
  { name: 'search-reindex-28801440-9hj2k', ns: 'data', status: 'Completed', node: 'node-pool-b-1', restarts: 0, age: '1 h', cpu: '—', mem: '—', workload: 'search-reindex' },
]

export const NODES = [
  { name: 'node-pool-a-1', type: 'e2-standard-8', pods: 28, cpu: 71, mem: 68, ready: true, zone: 'europe-west1-b' },
  { name: 'node-pool-a-2', type: 'e2-standard-8', pods: 26, cpu: 64, mem: 62, ready: true, zone: 'europe-west1-c' },
  { name: 'node-pool-a-3', type: 'e2-standard-8', pods: 30, cpu: 82, mem: 74, ready: true, zone: 'europe-west1-d' },
  { name: 'node-pool-b-1', type: 'e2-highmem-4', pods: 19, cpu: 38, mem: 77, ready: true, zone: 'europe-west1-b' },
  { name: 'node-pool-b-2', type: 'e2-highmem-4', pods: 21, cpu: 44, mem: 81, ready: true, zone: 'europe-west1-c' },
  { name: 'node-pool-b-3', type: 'e2-highmem-4', pods: 18, cpu: 29, mem: 58, ready: true, zone: 'europe-west1-d' },
]

export const K8S_EVENTS = [
  ['14:32:04', 'Warning', 'BackOff', 'pod/notifier-7c9d8b6f4-kq2lz', 'Back-off restarting failed container notifier'],
  ['14:32:03', 'Warning', 'OOMKilling', 'pod/notifier-7c9d8b6f4-kq2lz', 'Memory cgroup out of memory: killed process 1 (node), limit 512Mi'],
  ['14:09:12', 'Warning', 'FailedScheduling', 'pod/webhook-sender-5f6b7c8d9-x8mpt', '0/6 nodes are available: 6 Insufficient cpu'],
  ['14:04:41', 'Normal', 'ScalingReplicaSet', 'deployment/checkout-api', 'Scaled up replica set checkout-api-6d5f9c7b8 to 4 (release 2.14.1)'],
  ['14:04:10', 'Normal', 'Pulled', 'pod/checkout-api-6d5f9c7b8-2jx9k', 'Pulled image acme/checkout-api:2.14.1 in 3.2s'],
  ['13:00:00', 'Normal', 'SuccessfulCreate', 'cronjob/search-reindex', 'Created job search-reindex-28801440'],
] as const

export type CStatus = 'running' | 'restarting' | 'exited' | 'unhealthy'
export const DOCKER_HOSTS = ['edge-1 (Docker 27.3)', 'edge-2 (Docker 27.3)', 'build-runner (Docker 26.1)']
export const CONTAINERS: { name: string; image: string; compose: string; status: CStatus; health: string; up: string; cpu: number; mem: string; memPct: number; net: string; restarts: number; ports: string }[] = [
  { name: 'ferristrace-agent', image: 'otel/opentelemetry-collector-contrib:0.111.0', compose: 'observability', status: 'running', health: 'healthy', up: '40 d', cpu: 3, mem: '96 MiB / 512 MiB', memPct: 19, net: '1.2 MB/s ↑', restarts: 0, ports: '4317, 4318' },
  { name: 'edge-proxy', image: 'caddy:2.8', compose: 'edge', status: 'running', health: 'healthy', up: '12 d', cpu: 21, mem: '140 MiB / 1 GiB', memPct: 14, net: '38 MB/s ↓', restarts: 0, ports: '80, 443' },
  { name: 'storefront-ssr', image: 'acme/storefront-web:2.14.1', compose: 'storefront', status: 'running', health: 'healthy', up: '28 min', cpu: 54, mem: '780 MiB / 1 GiB', memPct: 76, net: '6.4 MB/s ↓', restarts: 0, ports: '3000' },
  { name: 'image-resizer', image: 'acme/imgproxy:3.24', compose: 'storefront', status: 'running', health: 'healthy', up: '9 d', cpu: 38, mem: '410 MiB / 2 GiB', memPct: 20, net: '22 MB/s ↑', restarts: 0, ports: '8080' },
  { name: 'redis-cache', image: 'redis:7.4-alpine', compose: 'storefront', status: 'running', health: 'healthy', up: '40 d', cpu: 6, mem: '1.6 GiB / 2 GiB', memPct: 80, net: '9.1 MB/s ↕', restarts: 0, ports: '6379' },
  { name: 'mailer-worker', image: 'acme/notifier:0.8.3', compose: 'data', status: 'restarting', health: 'starting', up: '12 s', cpu: 0, mem: '—', memPct: 0, net: '—', restarts: 9, ports: '—' },
  { name: 'legacy-cron', image: 'acme/cron:1.2.0', compose: 'data', status: 'exited', health: 'exit 1', up: 'exited 2 h ago', cpu: 0, mem: '—', memPct: 0, net: '—', restarts: 0, ports: '—' },
  { name: 'pdf-renderer', image: 'gotenberg/gotenberg:8', compose: 'storefront', status: 'unhealthy', health: 'unhealthy · /health 503', up: '6 d', cpu: 92, mem: '1.9 GiB / 2 GiB', memPct: 95, net: '0.3 MB/s', restarts: 2, ports: '3001' },
]
