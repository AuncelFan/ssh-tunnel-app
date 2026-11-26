import { useState, type FormEvent } from 'react'
import type { GroupClone as Request } from '../api'
import type { Filter, Jumper } from '../types'
import { useLocale } from '../locale'
import { Modal } from './Modal'

export function GroupClone({ source, jumpers, save, close }: { source: Filter; jumpers: Jumper[]; save: (request: Request) => Promise<void>; close: () => void }) {
  const { t } = useLocale()
  const targets = jumpers.filter(jumper => !source.tunnels.every(tunnel => tunnel.jumper_ids[0] === jumper.id) && source.tunnels.every(tunnel => !tunnel.jumper_ids.slice(1).includes(jumper.id)))
  const [target, setTarget] = useState(targets[0]?.id ?? 0)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  async function submit(event: FormEvent) {
    event.preventDefault()
    if (busy || !target || !source.tunnels.length) return
    setBusy(true); setError('')
    try { await save({ tunnel_ids: source.tunnels.map(tunnel => tunnel.id), target_jumper_id: target }); close() }
    catch (error) { setError(String(error)) }
    finally { setBusy(false) }
  }
  return <Modal title={t(source.key === 'selection' ? 'Clone selected' : 'Clone group')} closeLabel={t('Close group clone')} onClose={() => { if (!busy) close() }}>
    <p className="notice">{t('Replace the first jumper in every tunnel. Other hops, names and port mappings are preserved. Clones are stopped with auto-start off to avoid listener conflicts.')}</p>
    <p>{source.name} · {source.tunnels.length} {t('Tunnels')}</p>
    <form onSubmit={submit}><fieldset className="editor-fields" disabled={busy}>
      <label>{t('Target jumper')}<select required value={target || ''} onChange={event => setTarget(Number(event.target.value))}><option value="" disabled>{t('Select a jumper')}</option>{targets.map(jumper => <option key={jumper.id} value={jumper.id}>{jumper.name} ({jumper.user}@{jumper.host}:{jumper.port})</option>)}</select></label>
      {!targets.length && <p className="notice">{t('Add another jumper that does not already occur later in these chains.')}</p>}
      <ul className="clone-tunnels">{source.tunnels.map(tunnel => <li key={tunnel.id}><strong>{tunnel.name}</strong><code>{tunnel.local_host}:{tunnel.local_port} {tunnel.mode === 'dynamic' ? '→ SOCKS5' : `${tunnel.mode === 'remote' ? '←' : '→'} ${tunnel.remote_host}:${tunnel.remote_port}`}</code></li>)}</ul>
      {error && <p role="alert" className="error-detail">{error}</p>}
      <div className="dialog-actions"><button type="button" onClick={close}>{t('Cancel')}</button><button type="submit" className="primary" disabled={!target || !source.tunnels.length}>{t(source.key === 'selection' ? 'Clone selected' : 'Clone group')}</button></div>
    </fieldset></form>
  </Modal>
}
