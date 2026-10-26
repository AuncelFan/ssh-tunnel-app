import { useState, type FormEvent } from 'react'
import { Modal } from './Modal'
import { Toggle } from './Toggle'
import type { Jumper, Tunnel } from '../types'
import { useLocale } from '../locale'
import { newJumper } from '../defaults'
import { ProbeActions } from './ProbeActions'

interface Shared { close: () => void }
export function JumperEditor({ initial, save, close }: Shared & { initial: Jumper; save: (value: Jumper) => Promise<void> }) {
  const { t } = useLocale()
  const [value, setValue] = useState(initial)
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  const set = <K extends keyof Jumper>(key: K, next: Jumper[K]) => setValue(old => ({ ...old, [key]: next }))
  async function submit(event: FormEvent) {
    event.preventDefault(); if (busy) return
    setBusy(true); setError('')
    try { await save(value); close() } catch (error) { setError(String(error)) } finally { setBusy(false) }
  }
  return <Modal title={t(value.id ? 'Edit jumper' : 'New jumper')} closeLabel={t("Close editor")} onClose={() => { if (!busy) close() }}>
    <form onSubmit={submit}><fieldset disabled={busy} className="editor-fields">
      <label>{t("Name")}<input required value={value.name} onChange={e => set('name', e.target.value)} /></label>
      <div className="form-row"><label>{t("Host")}<input required value={value.host} onChange={e => set('host', e.target.value)} spellCheck={false} /></label><label>{t("SSH port")}<input type="number" min="1" max="65535" required value={value.port} onChange={e => set('port', Number(e.target.value))} /></label></div>
      <div className="form-row"><label>{t("User")}<input required value={value.user} onChange={e => set('user', e.target.value)} autoComplete="username" /></label><label>{t("Authentication")}<select value={value.auth_type} onChange={e => set('auth_type', e.target.value as Jumper['auth_type'])}><option value="ssh_key">{t("Private key")}</option><option value="password">{t("Password")}</option><option value="ssh_agent">{t("SSH agent")}</option></select></label></div>
      {value.auth_type === 'ssh_key' && <label>{t("Private key path")}<input required value={value.key_path} onChange={e => set('key_path', e.target.value)} spellCheck={false} placeholder="~/.ssh/id_ed25519" /></label>}
      {value.auth_type !== 'ssh_agent' && <label>{t(value.auth_type === 'password' ? 'Password' : 'Key passphrase')}<input type="password" required={value.auth_type === 'password'} value={value.password} onChange={e => set('password', e.target.value)} autoComplete="new-password" /></label>}
      {value.auth_type === 'ssh_agent' && <label>{t("Agent socket / named pipe")}<input value={value.agent_socket_path} onChange={e => set('agent_socket_path', e.target.value)} placeholder={t("System default")} spellCheck={false} /></label>}
      <div className="form-row"><label>{t("Connection timeout (ms)")}<input type="number" min="0" max="86400000" value={value.timeout_ms} onChange={e => set('timeout_ms', Number(e.target.value))} /></label><label>{t("Keepalive interval (ms)")}<input type="number" min="0" max="86400000" value={value.keep_alive_interval_ms} onChange={e => set('keep_alive_interval_ms', Number(e.target.value))} /></label></div>
      <label>{t("Host-key algorithms")}<input value={value.host_key_algorithms} onChange={e => set('host_key_algorithms', e.target.value)} placeholder={t("System default")} /></label>
      <Toggle label={t("Skip host verification (unsafe)")} checked={value.bypass_host_verification} onChange={next => set('bypass_host_verification', next)} />
      {value.bypass_host_verification && <p className="error-detail">{t("Server identity will not be checked. Prefer confirming the server fingerprint when prompted.")}</p>}
      <label>{t("Notes")}<textarea rows={3} value={value.notes} onChange={e => set('notes', e.target.value)} /></label>
      {error && <p role="alert" className="error-detail">{error}</p>}
      <ProbeActions jumpers={[value]} disabled={busy} />
      <div className="dialog-actions"><button type="button" onClick={close}>{t("Cancel")}</button><button type="submit" className="primary">{t("Save jumper")}</button></div>
    </fieldset></form>
  </Modal>
}

export function TunnelEditor({ initial, jumpers, save, createJumper, close }: Shared & { initial: Tunnel; jumpers: Jumper[]; save: (value: Tunnel) => Promise<void>; createJumper: (value: Jumper) => Promise<number> }) {
  const { t } = useLocale()
  const [value, setValue] = useState({ ...initial, jumper_ids: [...initial.jumper_ids] })
  const [inlineJumper, setInlineJumper] = useState(false)
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  const set = <K extends keyof Tunnel>(key: K, next: Tunnel[K]) => setValue(old => ({ ...old, [key]: next }))
  function move(index: number, delta: number) {
    const next = [...value.jumper_ids]; [next[index], next[index + delta]] = [next[index + delta], next[index]]; set('jumper_ids', next)
  }
  async function submit(event: FormEvent) {
    event.preventDefault(); if (busy) return
    if (!value.jumper_ids.length) { setError(t('Select at least one jumper.')); return }
    setBusy(true); setError('')
    try { await save(value); close() } catch (error) { setError(String(error)) } finally { setBusy(false) }
  }
  return <><Modal title={t(value.id ? 'Edit tunnel' : 'New tunnel')} closeLabel={t("Close editor")} onClose={() => { if (!busy && !inlineJumper) close() }}>
    <form onSubmit={submit}><fieldset disabled={busy} className="editor-fields">
      <div className="form-row"><label>{t("Name")}<input required value={value.name} onChange={e => set('name', e.target.value)} /></label><label>{t("Type")}<select value={value.mode} onChange={e => set('mode', e.target.value as Tunnel['mode'])}><option value="local">{t("L - Local")}</option><option value="remote">{t("R - Remote")}</option><option value="dynamic">{t("S - SOCKS5")}</option></select></label></div>
      <div className="endpoint-grid"><label>{t("Local host")}<input required value={value.local_host} onChange={e => set('local_host', e.target.value)} /></label><label>{t("Local port")}<input type="number" required min="1" max="65535" value={value.local_port} onChange={e => set('local_port', Number(e.target.value))} /></label>
        {value.mode !== 'dynamic' && <><label>{t("Remote host")}<input required value={value.remote_host} onChange={e => set('remote_host', e.target.value)} /></label><label>{t("Remote port")}<input type="number" required min="1" max="65535" value={value.remote_port} onChange={e => set('remote_port', Number(e.target.value))} /></label></>}
      </div>
      <div><h3>{t("Jumper chain")}</h3><p className="notice">{t("Connections follow this order. The first jumper determines the default group.")}</p>
        <ol className="chain-list">{value.jumper_ids.map((id, index) => <li key={id}><span>{jumpers.find(j => j.id === id)?.name ?? `#${id}`}</span><div className="actions"><button type="button" disabled={index === 0} aria-label={`${t("Move up")} ${index + 1}`} onClick={() => move(index, -1)}>{'\u2191'}</button><button type="button" disabled={index === value.jumper_ids.length - 1} aria-label={`${t("Move down")} ${index + 1}`} onClick={() => move(index, 1)}>{'\u2193'}</button><button type="button" onClick={() => set('jumper_ids', value.jumper_ids.filter(member => member !== id))}>{t("Remove")}</button></div></li>)}</ol>
        <label>{t("Add jumper")}<select value="" onChange={e => { if (e.target.value) set('jumper_ids', [...value.jumper_ids, Number(e.target.value)]) }}><option value="">{t("Select a jumper")}</option>{jumpers.filter(j => !value.jumper_ids.includes(j.id)).map(j => <option value={j.id} key={j.id}>{j.name} ({j.host}:{j.port})</option>)}</select></label>
        <button type="button" className="standalone-button" onClick={() => setInlineJumper(true)}>{t('New jumper')}</button>
      </div>
      <Toggle label={t("Start automatically")} checked={value.auto_start} onChange={next => set('auto_start', next)} />
      <label>{t("Reconnect policy")}<select value={value.reconnect_mode} onChange={e => set('reconnect_mode', e.target.value)}><option value="disabled">{t("Disabled")}</option><option value="fixed">{t("Fixed interval")}</option><option value="exponential">{t("Exponential backoff")}</option><option value="legacy">{t("Legacy")}</option></select></label>
      {['fixed', 'exponential'].includes(value.reconnect_mode) && <div className="form-row"><label>{t("Retry interval (seconds)")}<input type="number" min="1" max="86400" required value={value.reconnect_interval_seconds} onChange={e => set('reconnect_interval_seconds', Number(e.target.value))} /></label><label>{t("Maximum retries (0 = unlimited)")}<input type="number" min="0" max="1000000" required value={value.reconnect_max_attempts} onChange={e => set('reconnect_max_attempts', Number(e.target.value))} /></label></div>}
      <label>{t("Description")}<textarea rows={3} value={value.description} onChange={e => set('description', e.target.value)} /></label>
      {error && <p role="alert" className="error-detail">{error}</p>}
      <ProbeActions jumpers={value.jumper_ids.map(id => jumpers.find(jumper => jumper.id === id)).filter((jumper): jumper is Jumper => !!jumper)} tunnel={value} disabled={busy} />
      <div className="dialog-actions"><button type="button" onClick={close}>{t("Cancel")}</button><button type="submit" className="primary">{t("Save tunnel")}</button></div>
    </fieldset></form>
  </Modal>{inlineJumper && <JumperEditor initial={newJumper()} close={() => setInlineJumper(false)} save={async jumper => {
    const id = await createJumper(jumper)
    setValue(old => ({ ...old, jumper_ids: [...old.jumper_ids, id] }))
  }} />}</>
}
