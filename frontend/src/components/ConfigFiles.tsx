import { useEffect, useState } from 'react'
import { api, type ImportPreview } from '../api'
import { useLocale } from '../locale'
import { Modal } from './Modal'
import { ConfigDirectory } from './ConfigDirectory'

export function ConfigFiles({ onImported }: { onImported: () => Promise<void> }) {
  const { t } = useLocale()
  const [activePath, setActivePath] = useState('')
  const [path, setPath] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [message, setMessage] = useState('')
  const [preview, setPreview] = useState<ImportPreview | null>(null)
  const [exportPath, setExportPath] = useState('')
  useEffect(() => {
    let disposed = false
    void api.configPath().then(value => { if (!disposed) setActivePath(value) }).catch(error => { if (!disposed) setError(String(error)) })
    return () => { disposed = true }
  }, [])
  async function run(operation: () => Promise<void>) {
    if (busy) return
    setBusy(true); setError(''); setMessage('')
    try { await operation() } catch (error) { setError(String(error)) } finally { setBusy(false) }
  }
  async function choose(save: boolean) {
    const selected = path.trim() || await api.chooseConfigFile(save)
    if (!selected) return
    setPath(selected)
    if (save) setExportPath(selected)
    else setPreview(await api.previewImport(selected))
  }
  async function close() {
    if (busy) return
    await run(async () => { await api.cancelImport(); setPreview(null); setExportPath('') })
  }
  return <section className="panel editor-fields">
    <h2>{t('Configuration')}</h2>
    <label>{t('Active configuration')}<code className="config-path">{activePath}</code></label>
    <ConfigDirectory onChanged={async () => { setActivePath(await api.configPath()); await onImported() }} />
    <label>{t('Configuration file')}<input value={path} disabled={busy} onChange={event => setPath(event.target.value)} placeholder={t('Leave blank to choose a file')} spellCheck={false} /></label>
    <p className="notice">{t('Configuration files contain credentials and trusted host keys. Keep exports private and import only trusted files.')}</p>
    <div className="actions"><button disabled={busy} onClick={() => { void run(() => choose(false)) }}>{t('Import configuration')}</button><button disabled={busy} onClick={() => { void run(() => choose(true)) }}>{t('Export configuration')}</button></div>
    {message && <p role="status">{message}</p>}
    {error && !preview && !exportPath && <p role="alert" className="error-detail">{error}</p>}
    {(preview || exportPath) && <Modal title={t(preview ? 'Confirm import' : 'Confirm export')} closeLabel={t('Cancel')} onClose={() => { void close() }}>
      <p className="config-path">{path}</p>
      <p className="notice">{t(preview ? 'This replaces the active configuration, stops current tunnels and starts imported auto-start tunnels. A backup is kept. Imported trust and bypass settings also take effect.' : 'The export includes credentials and trusted host keys. An existing destination file will be replaced with a backup.')}</p>
      {preview && <dl className="detail-properties">{[['Jumpers', preview.jumpers], ['Tunnels', preview.tunnels], ['Custom groups', preview.groups], ['Trusted host keys', preview.trusted_hosts], ['Jumpers bypassing verification', preview.bypass_jumpers], ['Auto-start', preview.auto_start]].map(([label, count]) => <div key={label} style={{ display: 'contents' }}><dt>{t(String(label))}</dt><dd>{count}</dd></div>)}</dl>}
      {error && <p role="alert" className="error-detail">{error}</p>}
      {preview && <p>{t('Launch at login')}: {t(preview.auto_run ? 'Enabled' : 'Disabled')}</p>}
      <div className="dialog-actions"><button disabled={busy} onClick={() => { void close() }}>{t('Cancel')}</button><button disabled={busy} className="primary" onClick={() => { void run(async () => {
        if (preview) {
          const warnings = await api.confirmImport(preview.token)
          setPreview(null); await onImported()
          setMessage(warnings.length ? warnings.join('\n') : t('Configuration imported.'))
        } else {
          await api.exportConfig(exportPath); setExportPath(''); setMessage(t('Configuration exported.'))
        }
      }) }}>{t(preview ? 'Replace configuration' : 'Export configuration')}</button></div>
    </Modal>}
  </section>
}
