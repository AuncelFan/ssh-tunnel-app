import type { Jumper, RuntimeEvent } from './types'

export const duplicateJumper = (jumper: Jumper, suffix: string): Jumper => ({ ...jumper, id: 0, name: `${jumper.name} ${suffix}` })
export const logLevel = (event: RuntimeEvent): 'info' | 'warn' | 'error' => event.kind === 'error' || event.kind === 'action_error' ? 'error' : event.kind === 'host_key' || (event.kind === 'status' && event.message === 'reconnecting') ? 'warn' : 'info'
export function logTime(event: RuntimeEvent, locale: string): string {
  if (!event.timestamp_ms || !Number.isFinite(event.timestamp_ms)) return '--'
  return new Date(event.timestamp_ms).toLocaleTimeString(locale, { hour12: false })
}

const actions: Record<string, string> = {
  save_jumper: 'Save jumper', delete_jumper: 'Delete jumper', import_jumpers: 'Import SSH config',
  save_tunnel: 'Save tunnel', delete_tunnel: 'Delete tunnel', import_command_tunnels: 'Import SSH command',
  save_group: 'Save group', delete_group: 'Delete group', reorder_groups: 'Reorder groups',
  set_traffic_monitor: 'Traffic monitoring', set_auto_run: 'Launch at login', approve_probe_key: 'Confirm server identity',
  confirm_import: 'Import configuration', export_config: 'Export configuration', relocate_config: 'Change directory',
}
export function logMessage(event: RuntimeEvent, t: (key: string) => string): string {
  if (event.kind === 'action' || event.kind === 'action_error') return `${t(actions[event.message] ?? event.message)}: ${t(event.kind === 'action' ? 'Succeeded' : 'Failed')}`
  return event.kind === 'status' ? t(`status.${event.message}`) : event.message
}

export async function copyText(text: string): Promise<void> {
  const focused = document.activeElement
  try { if (navigator.clipboard?.writeText) { await navigator.clipboard.writeText(text); return } } catch { /* Try WebView's user-gesture clipboard fallback. */ }
  const area = document.createElement('textarea')
  area.value = text; area.readOnly = true
  area.style.position = 'fixed'; area.style.opacity = '0'
  // Appending inside the active dialog keeps the fallback inside its focus trap.
  const parent = focused?.closest('dialog') ?? document.body
  parent.appendChild(area)
  try { area.select(); if (!document.execCommand?.('copy')) throw new Error('Clipboard unavailable') }
  finally { area.remove(); if (focused instanceof HTMLElement) focused.focus() }
}
