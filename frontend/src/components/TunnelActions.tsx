import { IconButton } from './IconButton'
import { ProbeActions } from './ProbeActions'
import { Toggle } from './Toggle'
import { useLocale } from '../locale'
import type { Jumper, Tunnel } from '../types'

export function TunnelActions({ tunnel, jumpers, busy, running, toggle, edit, duplicate, remove, details, autoStart }: {
  tunnel: Tunnel; jumpers: Jumper[]; busy: boolean; running: boolean
  toggle: () => void; edit: () => void; duplicate: () => void; remove: () => void; details: () => void; autoStart: (enabled: boolean) => void
}) {
  const { t } = useLocale()
  return <div className="actions tunnel-actions" role="group" aria-label={`${t('Tunnel actions')}: ${tunnel.name}`}>
    <IconButton icon={running ? 'stop' : 'start'} label={t(running ? 'Stop' : 'Start')} disabled={busy} onClick={toggle} />
    <ProbeActions iconOnly jumpers={tunnel.jumper_ids.map(id => jumpers.find(jumper => jumper.id === id)).filter((jumper): jumper is Jumper => !!jumper)} tunnel={tunnel} disabled={busy || running} />
    <IconButton icon="edit" label={t('Edit tunnel')} disabled={busy || running} onClick={edit} />
    <IconButton icon="duplicate" label={t('Duplicate tunnel')} disabled={busy} onClick={duplicate} />
    <IconButton icon="delete" label={t('Delete tunnel')} className="danger" disabled={busy} onClick={remove} />
    <IconButton icon="details" label={`${t('Details')}: ${tunnel.name}`} onClick={details} />
    <Toggle compact label={`${t('Start automatically')}: ${tunnel.name}`} checked={tunnel.auto_start} disabled={busy} onChange={autoStart} />
  </div>
}
