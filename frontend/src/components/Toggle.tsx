interface Props {
  label: string
  checked: boolean
  disabled?: boolean
  onChange: (checked: boolean) => void
}

export function Toggle({ label, checked, disabled = false, onChange }: Props) {
  return <label className="toggle-row"><span>{label}</span><input className="toggle" type="checkbox" role="switch" checked={checked} disabled={disabled} onChange={event => onChange(event.target.checked)} /></label>
}
