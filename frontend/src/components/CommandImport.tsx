import { useState } from 'react'
import { api, type CommandPreview, type TunnelBatchImport } from '../api'
import type { Jumper, Tunnel } from '../types'
import { useLocale } from '../locale'
import { Modal } from './Modal'
import { JumperEditor } from './ConnectionEditor'
import { ConfirmDialog } from './ConfirmDialog'

const signature = (t: Tunnel) => JSON.stringify([t.mode, t.local_host.toLowerCase(), t.local_port, t.mode === 'dynamic' ? '' : t.remote_host.toLowerCase(), t.mode === 'dynamic' ? 0 : t.remote_port])

export function CommandImport({ jumpers, tunnels, close, save }: { jumpers: Jumper[]; tunnels: Tunnel[]; close: () => void; save: (request: { jumpers: Jumper[]; tunnels: Tunnel[] }) => Promise<TunnelBatchImport> }) {
  const { t } = useLocale()
  const [command, setCommand] = useState('')
  const [preview, setPreview] = useState<CommandPreview | null>(null)
  const [hops, setHops] = useState<Jumper[]>([])
  const [drafts, setDrafts] = useState<Tunnel[]>([])
  const [selected, setSelected] = useState<number[]>([])
  const [editing, setEditing] = useState<number | null>(null)
  const [confirming, setConfirming] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [result, setResult] = useState<TunnelBatchImport | null>(null)
  const seen = new Set(tunnels.map(signature))
  const duplicates = drafts.map(tunnel => { const key = signature(tunnel); const duplicate = seen.has(key); seen.add(key); return duplicate })
  const selectedTunnels = drafts.filter((_, index) => selected.includes(index) && !duplicates[index])
  const ready = selectedTunnels.length > 0 && selectedTunnels.every(tunnel => tunnel.name.trim()) && hops.length > 0 && hops.every(hop => hop.user.trim() && hop.host.trim() && (hop.auth_type !== 'ssh_key' || hop.key_path.trim()) && (hop.auth_type !== 'password' || hop.password))
  async function parse() {
    if (busy) return
    setBusy(true); setError(''); setPreview(null); setResult(null)
    try {
      const parsed = await api.parseSSHCommand(command)
      setPreview(parsed); setDrafts(parsed.tunnels); setSelected(parsed.tunnels.map((_, index) => index))
      setHops(parsed.jumpers.map(hop => jumpers.find(saved => saved.host.toLowerCase() === hop.host.toLowerCase() && saved.user === hop.user && saved.port === hop.port) ?? hop))
    } catch (error) { setError(String(error)) } finally { setBusy(false) }
  }
  return <><Modal title={t('Import SSH command')} closeLabel={t('Close importer')} onClose={() => { if (!busy && editing === null && !confirming) close() }}>
    <form className="editor-fields" onSubmit={event => { event.preventDefault(); void parse() }}><label>{t('SSH command')}<textarea rows={4} required value={command} disabled={busy} onChange={event => { setCommand(event.target.value); setPreview(null); setResult(null) }} spellCheck={false} /></label><button disabled={busy} className="standalone-button" type="submit">{t('Parse command')}</button></form>
    <p className="notice">{t('The command is only parsed, never executed. Review authentication and mappings before importing. Tunnels are saved stopped.')}</p>
    {error && <p role="alert" className="error-detail">{error}</p>}
    {result && <p role="status">{t('Imported')}: {result.imported.length}; {t('Duplicates skipped')}: {result.skipped}</p>}
    {preview && <><h3>{t('Jumper chain')}</h3>{preview.warnings.map(warning => <p className="notice" key={warning}>{t(`ssh-import.${warning}`)}</p>)}
      {hops.map((hop, index) => <section className="panel editor-fields" key={index}><label>{t('Jumper')} {index + 1}<select value={hop.id || ''} disabled={busy} onChange={event => setHops(old => old.map((item, i) => i === index ? (event.target.value ? jumpers.find(j => j.id === Number(event.target.value))! : preview.jumpers[index]) : item))}><option value="">{t('New jumper')}</option>{jumpers.map(jumper => <option key={jumper.id} value={jumper.id}>{jumper.name} ({jumper.host}:{jumper.port})</option>)}</select></label><code className="config-path">{hop.user || '?'}@{hop.host}:{hop.port}</code>{!hop.id && <button className="standalone-button" disabled={busy} onClick={() => setEditing(index)}>{t('Review authentication')}</button>}</section>)}
      <h3>{t('Port mappings')}</h3>{drafts.map((tunnel, index) => <section className="panel editor-fields" key={index}><label className="import-selection"><input type="checkbox" checked={!duplicates[index] && selected.includes(index)} disabled={busy || duplicates[index]} onChange={event => setSelected(old => event.target.checked ? [...old, index] : old.filter(i => i !== index))} /><span>{t('Select mapping')} {index + 1} ({tunnel.mode === 'local' ? 'L' : tunnel.mode === 'remote' ? 'R' : 'S'})</span></label><label>{t('Tunnel name')} {index + 1}<input value={tunnel.name} disabled={busy} onChange={event => setDrafts(old => old.map((item, i) => i === index ? { ...item, name: event.target.value } : item))} /></label><code className="config-path">{tunnel.local_host}:{tunnel.local_port} {tunnel.mode === 'remote' ? '<-' : '->'} {tunnel.mode === 'dynamic' ? 'SOCKS5' : `${tunnel.remote_host}:${tunnel.remote_port}`}</code>{duplicates[index] && <p className="notice">{t('Duplicate mapping')}</p>}</section>)}
      {!ready && <p className="notice">{t('Select a new mapping and complete authentication for every hop.')}</p>}
      <div className="dialog-actions"><button disabled={busy || !ready} className="primary" onClick={() => setConfirming(true)}>{t('Import selected')} ({selectedTunnels.length})</button></div>
    </>}
  </Modal>{editing !== null && <JumperEditor initial={hops[editing]} close={() => setEditing(null)} save={async value => { setHops(old => old.map((item, index) => index === editing ? value : item)) }} />}
    {confirming && <ConfirmDialog title={t('Import selected tunnels?')} message={t('New jumpers and selected tunnels are saved together. Existing jumpers are reused without changing credentials.')} confirmLabel={t('Import selected')} destructive={false} close={() => setConfirming(false)} confirm={async () => {
      setBusy(true)
      try { setResult(await save({ jumpers: hops, tunnels: selectedTunnels })); setPreview(null) } finally { setBusy(false) }
    }} />}
  </>
}
