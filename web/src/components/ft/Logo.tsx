export function Logo({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.75" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <circle cx="17" cy="7" r="3" />
      <path d="M14.8 9.2 4 20M11.5 5.5 4 13M18.5 12.5 11 20" />
    </svg>
  )
}
