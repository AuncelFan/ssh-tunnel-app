import { useEffect, useState } from 'react'
import { api, type SSHConfigPreview, type BatchImport, type SSHConfigCandidate } from '../api'
import type { Jumper } from '../types'
import { useLocale } from '../locale'
import { Modal } from './Modal'
import { JumperEditor } from './ConnectionEditor'
import { ConfirmDialog } from './ConfirmDialog'

export function SSHConfigImport({ jumpers, save, saveMany, close }: { jumpers: Jumper[]; save: (jumper: Jumper) => Promise<void>; saveMany: (jumpers: Jumper[]) => Promise<BatchImport>; close: () => void }) {
  const { t } = useLocale()
  const [path, setPath] = useState('')
  const [initializing, setInitializing] = useState(true)
  const [preview, setPreview] = useState<SSHConfigPreview | null>(null)
  const [editing, setEditing] = useState<Jumper | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [selected, setSelected] = useState<string[]>([])
  const [confirming, setConfirming] = useState(false)
  const [result, setResult] = useState<BatchImport | null>(null)
  const exists = (candidate: SSHConfigCandidate) => jumpers.some(jumper => jumper.host.trim().toLowerCase() === candidate.jumper.host.trim().toLowerCase() && jumper.port === candidate.jumper.port && jumper.user.trim() === candidate.jumper.user.trim())
  const eligible = (candidate: SSHConfigCandidate) => !exists(candidate) && !!candidate.jumper.user.trim() && !(candidate.warnings.includes('agent_disabled') && candidate.jumper.auth_type === 'ssh_agent')
  const selection = preview?.candidates.filter(candidate => selected.includes(candidate.alias) && eligible(candidate)) ?? []
  useEffect(() => {
    let disposed = false
    void api.defaultSSHConfigPath().then(value => { if (!disposed) setPath(value) }).catch(error => { if (!disposed) setError(String(error)) }).finally(() => { if (!disposed) setInitializing(false) })
    return () => { disposed = true }
  }, [])
  async function load() {
    if (busy || initializing) return
    setBusy(true); setError(''); setPreview(null); setSelected([]); setResult(null)
    try { setPreview(await api.loadSSHConfig(path.trim())) } catch (error) { setError(String(error)) } finally { setBusy(false) }
  }
  return <><Modal title={t('Import SSH config')} closeLabel={t('Close importer')} onClose={() => { if (!busy && !editing && !confirming) close() }}>
    <form onSubmit={event => { event.preventDefault(); void load() }} className="editor-fields"><label>{t('SSH config path')}<input required disabled={busy || initializing} value={path} onChange={event => { setPath(event.target.value); setPreview(null) }} spellCheck={false} /></label><button disabled={busy || initializing} className="standalone-button" type="submit">{t('Load aliases')}</button></form>
    <p className="notice">{t('Review each alias before saving. ProxyJump chains must be assembled in the tunnel editor; commands are never executed.')}</p>
    {error && <p className="error-detail" role="alert">{error}</p>}
    {result && <p role="status">{t('Imported')}: {result.imported.length}; {t('Duplicates skipped')}: {result.skipped}</p>}
    {!!preview?.candidates.length && <div className="actions"><button disabled={busy} onClick={() => setSelected(preview.candidates.filter(eligible).map(candidate => candidate.alias))}>{t('Select all')}</button><button disabled={busy} onClick={() => setSelected([])}>{t('Clear selection')}</button><button disabled={busy || !selection.length} onClick={() => setConfirming(true)}>{t('Import selected')} ({selection.length})</button></div>}
    {preview?.warnings.map(warning => <p className="notice" key={warning}>{t(`ssh-import.${warning}`)}</p>)}
    {preview && !preview.candidates.length && <p>{t('No explicit host aliases found.')}</p>}
    {preview?.candidates.map(candidate => {
      const existing = exists(candidate)
      return <section className="panel" key={candidate.alias}><div className="list-heading"><label className="import-selection"><input type="checkbox" disabled={busy || !eligible(candidate)} checked={eligible(candidate) && selected.includes(candidate.alias)} onChange={event => setSelected(old => event.target.checked ? [...old, candidate.alias] : old.filter(alias => alias !== candidate.alias))} /><strong>{candidate.alias}</strong></label><button className="standalone-button" disabled={existing || busy} onClick={() => setEditing({ ...candidate.jumper, id: 0 })}>{t(existing ? 'Already imported' : 'Review and import')}</button></div><code className="config-path">{candidate.jumper.user || '?'}@{candidate.jumper.host}:{candidate.jumper.port}</code>{candidate.proxy_jump && <p className="config-path">ProxyJump: {candidate.proxy_jump}</p>}{candidate.warnings.map(warning => <p className="notice" key={warning}>{t(`ssh-import.${warning}`)}</p>)}</section>
    })}
  </Modal>{editing && <JumperEditor initial={editing} save={save} close={() => setEditing(null)} />}{confirming && <ConfirmDialog destructive={false} confirmLabel={t('Import selected')} title={t('Import selected aliases?')} message={`${t('Review the warnings before importing. Existing connections are kept and duplicate endpoints are skipped.')} (${selection.length})`} close={() => setConfirming(false)} confirm={async () => {
    setBusy(true)
    try { const result = await saveMany(selection.map(candidate => ({ ...candidate.jumper, id: 0 }))); setResult(result); setSelected([]) } finally { setBusy(false) }
  }} />}</>
}
