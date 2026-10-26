import { useCallback, useEffect, useRef, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import { api } from './api'
import { filters, selectedFilter } from './groups'
import { HostTrustDialog } from './components/HostTrustDialog'
import { Modal } from './components/Modal'
import { JumperEditor, TunnelEditor } from './components/ConnectionEditor'
import { GroupEditor } from './components/GroupEditor'
import { ConfirmDialog } from './components/ConfirmDialog'
import { ConfigFiles } from './components/ConfigFiles'
import { SSHConfigImport } from './components/SSHConfigImport'
import { CommandImport } from './components/CommandImport'
import { Toggle } from './components/Toggle'
import { ProbeActions } from './components/ProbeActions'
import { LogsPage } from './components/LogsPage'
import { CopyErrorButton } from './components/CopyErrorButton'
import { TunnelMapping } from './components/TunnelMapping'
import { duplicateJumper, logTime } from './journal'
import { newJumper, newTunnel } from './defaults'
import { bytes, rates, type Rates } from './traffic'
import { useLocale } from './locale'
import type { Config, RuntimeEvent, TunnelRuntime, Jumper, Tunnel } from './types'

const active = (status: string) => !['stopped', 'error'].includes(status)

export default function App() {
  const { t, locale, setLocale } = useLocale()
  const [config, setConfig] = useState<Config | null>(null)
  const [error, setError] = useState('')
  const [selected, setSelected] = useState('all')
  const [pending, setPending] = useState<RuntimeEvent[]>([])
  const [runtime, setRuntime] = useState<TunnelRuntime[]>([])
  const [details, setDetails] = useState<number | null>(null)
  const [logs, setLogs] = useState<RuntimeEvent[]>([])
  const [busy, setBusy] = useState(false)
  const [autoRun, setAutoRun] = useState<boolean | null>(null)
  const [page, setPage] = useState('Tunnels')
  const [theme, setTheme] = useState(() => localStorage.getItem('ssh-tunnel.theme') === 'light' ? 'light' : 'dark')
  useEffect(() => { localStorage.setItem('ssh-tunnel.theme', theme); document.documentElement.dataset.theme = theme }, [theme])
  const [jumperEditor, setJumperEditor] = useState<Jumper | null>(null)
  const [tunnelEditor, setTunnelEditor] = useState<Tunnel | null>(null)
  const [groupEditor, setGroupEditor] = useState(false)
  const [sshImporter, setSSHImporter] = useState(false)
  const [commandImporter, setCommandImporter] = useState(false)
  const [confirmation, setConfirmation] = useState<{ title: string; message: string; confirm: () => Promise<void> } | null>(null)
  const [showJumpers, setShowJumpers] = useState(() => localStorage.getItem('ssh-tunnel.tunnel-groups.show-jumper-groups') !== '0')
  const [speeds, setSpeeds] = useState<Record<number, Rates>>({})
  const sample = useRef<{ time: number; values: TunnelRuntime[] }>({ time: 0, values: [] })
  const refreshSequence = useRef(0)
  const refresh = useCallback(async () => {
    const sequence = ++refreshSequence.current
    const [next, trust, startupErrors, loginEnabled] = await Promise.all([api.getState(), api.pendingTrust(), api.takeStartupErrors(), api.getAutoRun().catch(error => { setError(String(error)); return null })])
    const states = next.traffic_monitor_enabled ? await api.runtime() : []
    if (startupErrors.length) setError(startupErrors.join('\n'))
    if (sequence === refreshSequence.current) {
      setConfig(next); setPending(trust); setRuntime(states)
      setAutoRun(loginEnabled)
      const now = performance.now()
      if (!next.traffic_monitor_enabled) { setSpeeds({}); sample.current = { time: now, values: [] } }
      else if (now - sample.current.time >= 500) { setSpeeds(rates(sample.current.values, states, (now - sample.current.time) / 1000)); sample.current = { time: now, values: states } }
    }
  }, [])
  useEffect(() => {
    let disposed = false
    const removers: (() => void)[] = []
    void refresh().catch(error => setError(String(error)))
    for (const name of ['tunnel-event', 'runtime-resync']) {
      void listen<RuntimeEvent>(name, event => {
        if (disposed) return
        if (name === 'tunnel-event' && event.payload.kind === 'error') setError(event.payload.message)
        void refresh().catch(error => setError(String(error)))
      }).then(remove => { if (disposed) remove(); else removers.push(remove) }).catch(error => { if (!disposed) setError(String(error)) })
    }
    const timer = setInterval(() => { if (!disposed) void refresh().catch(() => {}) }, 1000)
    return () => { disposed = true; clearInterval(timer); removers.forEach(remove => remove()) }
  }, [refresh])
  useEffect(() => {
    if (details === null) return
    let disposed = false
    const update = () => { void api.logs(details).then(next => { if (!disposed) setLogs(next) }).catch(error => setError(String(error))) }
    setLogs([]); update()
    const timer = setInterval(update, 1000)
    return () => { disposed = true; clearInterval(timer) }
  }, [details])
  async function action(run: () => Promise<unknown>) {
    if (busy) return
    setBusy(true)
    try { await run(); await refresh() } catch (error) { setError(String(error)) } finally { setBusy(false) }
  }
  async function batch(tunnels: Tunnel[], start: boolean) {
    const results = await Promise.allSettled(tunnels.map(tunnel => start ? api.startTunnel(tunnel.id) : api.stopTunnel(tunnel.id)))
    const failures = results.flatMap((result, index) => result.status === 'rejected' ? [`${tunnels[index].name}: ${String(result.reason)}`] : [])
    if (failures.length) throw new Error(failures.join('\n'))
  }
  const items = config ? filters(config, showJumpers, t('All')) : []
  const current = items.length ? selectedFilter(items, selected) : null
  const detail = config?.tunnels.find(tunnel => tunnel.id === details)
  const traffic = runtime.find(state => state.tunnel_id === details)?.traffic_last_hour
  const total = runtime.reduce((sum, state) => ({ uploaded: sum.uploaded + state.traffic_last_hour.uploaded, downloaded: sum.downloaded + state.traffic_last_hour.downloaded }), { uploaded: 0, downloaded: 0 })
  const totalSpeed = Object.values(speeds).reduce((sum, speed) => ({ uploaded: sum.uploaded + speed.uploaded, downloaded: sum.downloaded + speed.downloaded }), { uploaded: 0, downloaded: 0 })
  return <div className="shell">
    <header><strong>SSH-TUNNEL <small>0.1.0</small></strong><div className="page-tabs" role="navigation" aria-label={t("Main navigation")}>{['Overview', 'Tunnels', 'Jumpers', 'Logs', 'Settings'].map(item => <button key={item} aria-current={page === item ? 'page' : undefined} onClick={() => setPage(item)}>{t(item)}</button>)}</div></header>
    {!config && !error && <p role="status">{t("Loading configuration...")}</p>}
    {config && page === 'Tunnels' && <><div className="page-toolbar"><h2>{t("Tunnels")}</h2><div className="actions"><button onClick={() => setGroupEditor(true)}>{t("Manage groups")}</button><button onClick={() => setCommandImporter(true)}>{t("Import SSH command")}</button><button className="primary" onClick={() => setTunnelEditor(newTunnel())}>{t("New tunnel")}</button></div></div><main><nav aria-label={t("Tunnel groups")}>{items.map(item => <button key={item.key} aria-pressed={current?.key === item.key} onClick={() => setSelected(item.key)}><span title={item.name}>{item.name}</span><b>{item.tunnels.length}</b></button>)}</nav>
      <section><div className="list-heading"><h1>{current?.name}</h1><div className="actions"><button disabled={busy || !current?.tunnels.length} onClick={() => { void action(() => batch(current!.tunnels.filter(t => !active(t.status)), true)) }}>{t("Start group")}</button><button disabled={busy || !current?.tunnels.length} onClick={() => { void action(() => batch(current!.tunnels, false)) }}>{t("Stop group")}</button></div></div>
        {current?.tunnels.map(tunnel => <article key={tunnel.id}><div className="card-main"><strong><mark>{tunnel.mode === 'local' ? 'L' : tunnel.mode === 'remote' ? 'R' : 'S'}</mark> {tunnel.name}<small className={`status status-${tunnel.status}`}>{t(`status.${tunnel.status}`)}</small></strong><TunnelMapping tunnel={tunnel} jumpers={config.jumpers} /></div><div className="actions"><button disabled={busy} onClick={() => { void action(() => active(tunnel.status) ? api.stopTunnel(tunnel.id) : api.startTunnel(tunnel.id)) }}>{t(active(tunnel.status) ? 'Stop' : 'Start')}</button><button onClick={() => setDetails(tunnel.id)} aria-label={`${t("Details")}: ${tunnel.name}`}>{t("Details")}</button></div></article>)}
        {!current?.tunnels.length && <div className="empty-state">{t("No tunnels in this group.")}</div>}
      </section></main></>}
    {config && page === 'Overview' && <div className="page-content"><h1>{t("Overview")}</h1><div className="summary-grid"><div><span>{t("Total tunnels")}</span><strong>{config.tunnels.length}</strong></div><div><span>{t("Auto-start")}</span><strong>{config.tunnels.filter(t => t.auto_start).length}</strong></div><div><span>{t("Running")}</span><strong>{config.tunnels.filter(t => t.status === 'running').length}</strong></div></div><section className="panel"><h2>{t("Total traffic")}</h2>{config?.traffic_monitor_enabled ? <div className="traffic-grid"><div><span>{t("Upload")}</span><strong>{bytes(totalSpeed.uploaded)}/s</strong><small>{t("Last hour")}: {bytes(total.uploaded)}</small></div><div><span>{t("Download")}</span><strong>{bytes(totalSpeed.downloaded)}/s</strong><small>{t("Last hour")}: {bytes(total.downloaded)}</small></div></div> : <p className="notice">{t("Traffic monitoring is disabled.")}</p>}</section></div>}
    {config && page === 'Jumpers' && <div className="page-content"><div className="list-heading"><h1>{t("Jumpers")}</h1><div className="actions"><button onClick={() => setSSHImporter(true)}>{t("Import SSH config")}</button><button className="primary" onClick={() => setJumperEditor(newJumper())}>{t("New jumper")}</button></div></div>{config.jumpers.map(jumper => <article key={jumper.id}><div className="card-main"><strong>{jumper.name}</strong><code>{jumper.user}@{jumper.host}:{jumper.port}</code></div><div className="actions"><button aria-label={`${t("Duplicate jumper")}: ${jumper.name}`} onClick={() => setJumperEditor(duplicateJumper(jumper, t("Copy suffix")))}>{t("Duplicate")}</button><button onClick={() => setJumperEditor(jumper)}>{t("Edit")}</button><button onClick={() => setConfirmation({ title: t('Delete jumper?'), message: jumper.name, confirm: async () => { await api.deleteJumper(jumper.id); await refresh() } })}>{t("Delete")}</button></div></article>)}{!config.jumpers.length && <div className="empty-state">{t("Add a jumper to create your first tunnel.")}</div>}</div>}
    {config && page === 'Logs' && <LogsPage />}
    {config && page === 'Settings' && <div className="page-content"><div className="list-heading"><h1>{t("Settings")}</h1><button className="standalone-button" onClick={() => { void action(() => api.quit()) }}>{t("Quit application")}</button></div><section className="panel editor-fields">
      <div className="form-row"><label>{t('Language')}<select value={locale} onChange={event => { void action(() => setLocale(event.target.value === 'zh-CN' ? 'zh-CN' : 'en')) }}><option value="en">English</option><option value="zh-CN">{'\u7b80\u4f53\u4e2d\u6587'}</option></select></label><label>{t('Theme')}<select value={theme} onChange={event => setTheme(event.target.value)}><option value="dark">{t('Dark')}</option><option value="light">{t('Light')}</option></select></label></div>
      <Toggle label={t("Launch at login")} checked={config.auto_run} disabled={busy || autoRun === null} onChange={enabled => { void action(() => api.setAutoRun(enabled)) }} />
      {autoRun !== null && autoRun !== config.auto_run && <p className="notice">{t("The system startup state differs from this preference. Check Windows Task Manager or save the preference again.")}</p>}
      <Toggle label={t("Traffic monitoring")} checked={config.traffic_monitor_enabled} disabled={busy} onChange={enabled => { void action(() => api.setTrafficMonitor(enabled)) }} />
      <Toggle label={t("Show default jumper groups")} checked={showJumpers} onChange={value => { localStorage.setItem('ssh-tunnel.tunnel-groups.show-jumper-groups', value ? '1' : '0'); setShowJumpers(value); setSelected('all') }} /><p className="notice">{t("Default groups follow the first jumper. Custom groups remain available.")}</p></section><ConfigFiles onImported={async () => { setSelected('all'); setDetails(null); setLogs([]); sample.current = { time: 0, values: [] }; await refresh() }} /></div>}
    {detail && <Modal title={detail.name} closeLabel={t("Close details")} closeOnBackdrop onClose={() => setDetails(null)}>
      <p>{detail.description || t('No description')}</p>
      <ProbeActions jumpers={detail.jumper_ids.map(id => config?.jumpers.find(jumper => jumper.id === id)).filter((jumper): jumper is Jumper => !!jumper)} tunnel={detail} disabled={active(detail.status)} />
      <div className="detail-actions actions"><button disabled={active(detail.status)} onClick={() => { setTunnelEditor(detail); setDetails(null) }}>{t("Edit tunnel")}</button><button onClick={() => { setTunnelEditor({ ...detail, id: 0, name: `${detail.name} ${t("Copy suffix")}`, status: 'stopped', last_error: '' }); setDetails(null) }}>{t("Duplicate")}</button><button className="danger" onClick={() => setConfirmation({ title: t('Delete tunnel?'), message: detail.name, confirm: async () => { await api.deleteTunnel(detail.id); setDetails(null); await refresh() } })}>{t("Delete")}</button></div>
      <dl className="detail-properties"><dt>{t("Type")}</dt><dd>{t(`mode.${detail.mode}`)}</dd><dt>{t("Local endpoint")}</dt><dd>{detail.local_host}:{detail.local_port}</dd><dt>{t("Remote endpoint")}</dt><dd>{detail.mode === 'dynamic' ? 'SOCKS5' : `${detail.remote_host}:${detail.remote_port}`}</dd><dt>{t("Jumper chain")}</dt><dd>{detail.jumper_ids.map(id => config?.jumpers.find(j => j.id === id)?.name ?? `#${id}`).join(' > ')}</dd><dt>{t("Reconnect policy")}</dt><dd>{t(`policy.${detail.reconnect_mode || "legacy"}`)}</dd></dl>
      {config?.traffic_monitor_enabled ? <div className="traffic-grid"><div><span>{t("Upload")}</span><strong>{bytes(speeds[detail.id]?.uploaded ?? 0)}/s</strong><small>{t("Last hour")}: {bytes(traffic?.uploaded ?? 0)}</small></div><div><span>{t("Download")}</span><strong>{bytes(speeds[detail.id]?.downloaded ?? 0)}/s</strong><small>{t("Last hour")}: {bytes(traffic?.downloaded ?? 0)}</small></div></div> : <p className="notice">{t("Traffic monitoring is disabled.")}</p>}
      {detail.last_error && <><p className="error-detail">{detail.last_error}</p><CopyErrorButton text={detail.last_error} /></>}
      <h3>{t("Tunnel log")}</h3><div className="log-list">{logs.map((log, index) => <p key={index}><time>{logTime(log, locale)}</time><span>{t(`event.${log.kind}`)}</span> {log.kind === "status" ? t(`status.${log.message}`) : log.message}</p>)}</div>
    </Modal>}
    {jumperEditor && <JumperEditor initial={jumperEditor} close={() => setJumperEditor(null)} save={async value => { await api.saveJumper(value); await refresh() }} />}
    {commandImporter && config && <CommandImport jumpers={config.jumpers} tunnels={config.tunnels} close={() => setCommandImporter(false)} save={async request => { const result = await api.importCommandTunnels(request); setSelected('all'); await refresh(); return result }} />}
    {sshImporter && config && <SSHConfigImport saveMany={async jumpers => { const result = await api.importJumpers(jumpers); await refresh(); return result }} jumpers={config.jumpers} close={() => setSSHImporter(false)} save={async jumper => { await api.saveJumper(jumper); await refresh() }} />}
    {tunnelEditor && config && <TunnelEditor createJumper={async jumper => { const id = await api.saveJumper(jumper); await refresh(); return id }} initial={tunnelEditor} jumpers={config.jumpers} close={() => setTunnelEditor(null)} save={async value => { await api.saveTunnel(value); await refresh() }} />}
    {groupEditor && config && <GroupEditor reorder={async ids => { await api.reorderGroups(ids); await refresh() }} groups={config.groups} tunnels={config.tunnels} save={async group => { await api.saveGroup(group); await refresh() }} remove={async id => { await api.deleteGroup(id); await refresh(); setSelected('all') }} close={() => setGroupEditor(false)} />}
    {confirmation && <ConfirmDialog {...confirmation} close={() => setConfirmation(null)} />}
    {pending[0] && <HostTrustDialog key={`${pending[0].tunnel_id}-${pending[0].generation}`} event={pending[0]} onError={setError} respond={async (event, accept) => { await api.resolveHostKey(event.tunnel_id, event.generation, accept); await refresh() }} />}
    {error && <aside className="toast" role="alert"><span>{error}</span><button onClick={() => setError('')} aria-label={t("Dismiss error")}>{'\u00d7'}</button></aside>}
  </div>
}
