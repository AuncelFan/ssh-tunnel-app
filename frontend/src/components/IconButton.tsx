import type { ButtonHTMLAttributes, ReactNode } from 'react'

export type IconName = 'start' | 'stop' | 'test' | 'edit' | 'duplicate' | 'delete' | 'details' | 'clone'

const paths: Record<IconName, ReactNode> = {
  start: <path d="m8 5 11 7-11 7Z" />,
  stop: <rect x="6" y="6" width="12" height="12" rx="1" />,
  test: <><path d="m13 2-8 12h6l-1 8 9-13h-6Z" /></>,
  edit: <><path d="m15 5 4 4M4 20l4-1L20 7a3 3 0 0 0-4-4L4 15Z" /></>,
  duplicate: <><rect x="8" y="8" width="12" height="13" rx="2" /><path d="M16 8V3H3v13h5" /></>,
  delete: <><path d="M3 6h18M9 6V3h6v3M6 6l1 15h10l1-15M10 10v7m4-7v7" /></>,
  details: <><circle cx="12" cy="12" r="9" /><path d="M12 11v6m0-10v1" /></>,
  clone: <><rect x="3" y="3" width="12" height="12" rx="2" /><path d="M9 15v6h12V9h-6M6 7h6M6 11h4m8 2v5m-3-2h5" /></>,
}

export function IconButton({ icon, label, className = '', ...props }: Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'children'> & { icon: IconName; label: string }) {
  return <span className="icon-button-wrap" title={label}><button {...props} type={props.type ?? 'button'} className={`icon-button ${className}`} aria-label={label}>
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{paths[icon]}</svg>
  </button></span>
}
