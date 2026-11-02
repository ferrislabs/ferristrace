export function Logo({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0.4 33.8 33.8" fill="none" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <rect x="0.5" y="15" width="11" height="2.6" rx="1.3" fill="#8b93ff" />
      <rect x="2.5" y="19" width="9" height="2.6" rx="1.3" fill="#2fb394" />
      <rect x="4.5" y="23" width="7" height="2.6" rx="1.3" fill="#e8893c" />
      <g stroke="#3730a3" strokeWidth="1.6" className="dark:stroke-[#a5b4fc]">
        <path d="M14.5 25 L12.8 28.5" /><path d="M18 26.2 L17.2 29.5" /><path d="M24 26.2 L24.8 29.5" /><path d="M27.5 25 L29.2 28.5" />
        <path d="M18.6 15 L17.6 10.4" /><path d="M23.4 15 L24.4 10.4" />
        <path d="M13 19 Q 10.6 17.6 10.8 14.2" /><path d="M29 19 Q 31.2 17.4 30.8 14" />
      </g>
      <path d="M10.6 14.8 C 8.6 14.4 8.4 11.2 10.2 10.2 C 11.2 9.6 12.8 10 13 11.2 L 11.3 12 L 13.2 13 C 13 14.4 11.8 15.1 10.6 14.8 Z" fill="#4f46e5" className="dark:fill-[#6c64f2]" />
      <path d="M31.2 14.6 C 33.2 14.2 33.4 11 31.6 10 C 30.6 9.4 29 9.8 28.8 11 L 30.5 11.8 L 28.6 12.8 C 28.8 14.2 30 14.9 31.2 14.6 Z" fill="#4f46e5" className="dark:fill-[#6c64f2]" />
      <ellipse cx="21" cy="20.5" rx="8.6" ry="6.3" fill="#4f46e5" className="dark:fill-[#6c64f2]" />
      <circle cx="17.4" cy="8.6" r="2.7" fill="#ffffff" stroke="#3730a3" strokeWidth="1.1" />
      <circle cx="24.6" cy="8.6" r="2.7" fill="#ffffff" stroke="#3730a3" strokeWidth="1.1" />
      <circle cx="17.9" cy="8.8" r="1.3" fill="#0c0c0e" />
      <circle cx="25.1" cy="8.8" r="1.3" fill="#0c0c0e" />
      <path d="M18.8 21.4 Q 21 23.3 23.2 21.4" stroke="#ffffff" strokeWidth="1.3" />
    </svg>
  )
}
