interface Props {
  label: string
  checked: boolean
  disabled?: boolean
  compact?: boolean
  onChange: (checked: boolean) => void
}

export function Toggle({ label, checked, disabled = false, compact = false, onChange }: Props) {
  return <label className={compact ? 'compact-toggle' : 'toggle-row'} title={compact ? label : undefined}><span className={compact ? 'sr-only' : undefined}>{label}</span><input className="toggle" type="checkbox" role="switch" checked={checked} disabled={disabled} onChange={event => onChange(event.target.checked)} /></label>
}
