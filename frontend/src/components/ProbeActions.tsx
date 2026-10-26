import { useEffect, useRef, useState } from 'react'
import { api, type ProbeReport } from '../api'
import type { Jumper, Tunnel } from '../types'
import { useLocale } from '../locale'
import { Modal } from './Modal'
import { HostTrustDialog } from './HostTrustDialog'

const phases: Record<string, string> = {
  configuration: 'Configuration validation', local_bind: 'Local listening port', ssh_authentication: 'SSH connection and authentication',
  latency: 'SSH round-trip latency', remote_target: 'Remote destination', remote_bind: 'Remote listening port', remote_unbind: 'Remote listener cleanup',
  dynamic_forwarding: 'SOCKS forwarding permission', dns_first_hop: 'First-hop DNS', tcp_first_hop: 'First-hop TCP', private_key_file: 'Private key file',
}
const hints: Record<string, string> = {
  invalid: 'Check the host, port, username and ordered jumper chain.', authentication: 'Verify the SSH username and password or authorized public key.',
  timeout: 'Check network reachability, firewall rules and the configured connection timeout.', cancelled: 'The test was cancelled.',
  host_key: 'Verify the displayed fingerprint through a trusted channel before accepting it.', verification: 'Inspect known_hosts and revocations. Do not bypass a revoked key.',
  key: 'Check the private key path, file format and passphrase.', agent: 'Check that the SSH agent is running and has the required key loaded.',
  refused: 'Check the address, port and whether the destination service is listening.', forwarding_denied: 'Check SSH server forwarding permissions and destination restrictions.',
  transport: 'Inspect the failed stage, destination service, firewall and SSH server policy.',
}
const stageHints: Record<string, string> = {
  local_bind: 'Check whether the local address exists and another application is using the port.',
  dns_first_hop: 'Check the hostname and local DNS settings.', private_key_file: hints.key,
  remote_bind: 'Check remote bind permissions and whether the remote port is already in use.',
  remote_target: 'Check the destination from the final jumper, not only from this computer.',
}

export function ProbeActions({ jumpers, tunnel = null, disabled = false }: { jumpers: Jumper[]; tunnel?: Tunnel | null; disabled?: boolean }) {
  const { t } = useLocale()
  const [show, setShow] = useState(false)
  const [busy, setBusy] = useState(false)
  const [report, setReport] = useState<ProbeReport | null>(null)
  const [error, setError] = useState('')
  const token = useRef<number | null>(null)
  const generation = useRef(0)
  const starting = useRef(false)
  const cancellationRequested = useRef(false)
  useEffect(() => () => {
    generation.current++
    if (token.current !== null) void api.cancelProbe(token.current).catch(() => {})
  }, [])
  async function execute(id: number, sequence: number) {
    const result = await api.runProbe(id)
    if (sequence === generation.current) setReport(result)
  }
  async function start(diagnostics: boolean) {
    if (starting.current) return
    starting.current = true
    cancellationRequested.current = false
    token.current = null
    const sequence = ++generation.current
    setShow(true); setBusy(true); setReport(null); setError('')
    try {
      const id = await api.beginProbe({ jumpers, tunnel, diagnostics })
      if (sequence !== generation.current) { await api.cancelProbe(id); return }
      token.current = id
      if (cancellationRequested.current) { await api.cancelProbe(id); setError(t('The test was cancelled.')); return }
      await execute(id, sequence)
    } catch (error) { if (sequence === generation.current) setError(String(error)) }
    finally { starting.current = false; if (sequence === generation.current) setBusy(false) }
  }
  async function cancel() {
    cancellationRequested.current = true
    if (token.current !== null) await api.cancelProbe(token.current)
  }
  function close() {
    generation.current++; setShow(false); setBusy(false)
    void cancel().catch(() => {})
  }
  async function trust(accept: boolean) {
    if (token.current === null) return
    if (!accept) { await cancel(); setReport(old => old ? { ...old, challenge: null } : old); return }
    const sequence = generation.current
    await api.approveProbeKey(token.current)
    setReport(null); setBusy(true); setError('')
    try { await execute(token.current, sequence) }
    finally { if (sequence === generation.current) setBusy(false) }
  }
  return <>
    <div className="actions"><button type="button" disabled={disabled || busy} onClick={() => { void start(false) }}>{t('Test connection')}</button><button type="button" disabled={disabled || busy} onClick={() => { void start(true) }}>{t('Diagnose connection')}</button></div>
    {show && <Modal title={t('Connection test')} closeLabel={t('Close test')} onClose={close}>
      <p className="notice">{t('Tests do not save drafts. Confirmed host keys are saved. Stop a running tunnel before testing its listening port.')}</p>
      {busy && <p role="status">{t('Testing connection...')}</p>}
      {report && <>
        <h3 role="status">{t(report.success ? 'Connection test passed' : 'Connection test failed')}</h3>
        <p>{t('Elapsed')}: {report.elapsed_ms} ms{report.latency_ms !== null && <> / {t('SSH round-trip latency')}: {report.latency_ms.toFixed(2)} ms</>}</p>
        <div className="log-list">{report.checks.map((check, index) => <div key={index}>
          <p><strong>{t(phases[check.phase] ?? check.phase)}</strong>: {t(check.ok ? 'Passed' : 'Failed')}</p>
          {!check.ok && <><p className="error-detail">{check.detail}</p><p>{t((['transport', 'invalid'].includes(check.code) && stageHints[check.phase]) || hints[check.code] || hints.transport)}</p></>}
        </div>)}</div>
      </>}
      {error && <p role="alert" className="error-detail">{error}</p>}
      <div className="dialog-actions">{busy && <button type="button" onClick={() => { void cancel().catch(error => setError(String(error))) }}>{t('Cancel test')}</button>}<button type="button" onClick={close}>{t('Close test')}</button></div>
    </Modal>}
    {show && report?.challenge && <HostTrustDialog event={{ tunnel_id: 0, generation: token.current ?? 0, kind: 'host_key', message: '', challenge: report.challenge }} respond={(_, accept) => trust(accept)} onError={setError} />}
  </>
}
