import { useState } from 'react'
import { api, type DirectoryConflict } from '../api'
import { useLocale } from '../locale'
import { Modal } from './Modal'

export function ConfigDirectory({ onChanged }: { onChanged: () => Promise<void> }) {
  const { t, reloadLocale } = useLocale()
  const [target, setTarget] = useState('')
  const [pending, setPending] = useState<{ target: string | null; conflict: DirectoryConflict } | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [message, setMessage] = useState('')
  async function run(operation: () => Promise<void>) {
    if (busy) return
    setBusy(true); setError(''); setMessage('')
    try { await operation() } catch (error) { setError(String(error)) } finally { setBusy(false) }
  }
  async function prepare(reset: boolean) {
    const selected = reset ? null : target.trim() || await api.chooseConfigDirectory()
    if (!reset && !selected) return
    if (selected) setTarget(selected)
    setPending({ target: selected, conflict: await api.directoryConflicts(selected) })
  }
  async function apply(overwrite: boolean) {
    if (!pending) return
    const warnings = await api.relocateConfig(pending.target, overwrite)
    setPending(null)
    await onChanged()
    await reloadLocale()
    setMessage(warnings.length ? warnings.join('\n') : t('Configuration directory changed.'))
  }
  return <div className="editor-fields">
    <h3>{t('Configuration directory')}</h3>
    <label>{t('Target directory')}<input value={target} disabled={busy} onChange={event => setTarget(event.target.value)} placeholder={t('Leave blank to choose a directory')} spellCheck={false} /></label>
    <div className="actions">
      <button disabled={busy} onClick={() => { void run(() => prepare(false)) }}>{t('Change directory')}</button>
      <button disabled={busy} onClick={() => { void run(() => prepare(true)) }}>{t('Restore default directory')}</button>
      <button disabled={busy} onClick={() => { void run(async () => { await api.revealConfigDirectory() }) }}>{t('Open configuration directory')}</button>
    </div>
    {message && <p role="status">{message}</p>}
    {error && !pending && <p role="alert" className="error-detail">{error}</p>}
    {pending && <Modal title={t('Change configuration directory?')} closeLabel={t('Cancel')} onClose={() => { if (!busy) setPending(null) }}>
      <p className="config-path">{pending.target ?? t('Default configuration directory')}</p>
      <p className="notice">{t('Current tunnels will stop and auto-start tunnels from the selected configuration will start. Original files are retained. Trusted hosts and login startup follow the selected configuration.')}</p>
      {(pending.conflict.config_exists || pending.conflict.locale_exists) && <p>{t('The destination already contains configuration or language settings. Keep them or replace them with the current settings?')}</p>}
      {error && <p role="alert" className="error-detail">{error}</p>}
      <div className="dialog-actions">
        <button disabled={busy} onClick={() => setPending(null)}>{t('Cancel')}</button>
        {(pending.conflict.config_exists || pending.conflict.locale_exists) && <button disabled={busy} onClick={() => { void run(() => apply(false)) }}>{t('Use destination settings')}</button>}
        <button className="primary" disabled={busy} onClick={() => { void run(() => apply(true)) }}>{t('Copy current settings')}</button>
      </div>
    </Modal>}
  </div>
}
