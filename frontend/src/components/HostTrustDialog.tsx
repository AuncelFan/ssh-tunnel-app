import { useState } from 'react'
import type { RuntimeEvent } from '../types'
import { Modal } from './Modal'
import { useLocale } from '../locale'

export function HostTrustDialog({ event, respond, onError }: {
  event: RuntimeEvent; respond: (event: RuntimeEvent, accept: boolean) => Promise<void>; onError: (error: string) => void
}) {
  const { t } = useLocale()
  const [busy, setBusy] = useState(false)
  const [failure, setFailure] = useState('')
  const challenge = event.challenge!
  const changed = Boolean(challenge.previous_fingerprint)
  async function decide(accept: boolean) {
    if (busy) return
    setBusy(true); setFailure('')
    try { await respond(event, accept) } catch (error) { setFailure(String(error)); onError(String(error)) } finally { setBusy(false) }
  }
  return <Modal title={t(changed ? 'Server identity changed' : 'Trust this server?')} closeLabel={t("Cancel connection")} onClose={() => { void decide(false) }}>
    <p><strong>{challenge.host}:{challenge.port}</strong></p>
    <p>{t(changed ? 'The received key differs from the saved key. This can mean a server reinstall or an intercepted connection.' : 'This server key has not been trusted yet.')} {t('Verify its SHA256 fingerprint through a separate trusted channel before continuing.')}</p>
    {changed && <div className="fingerprint"><span>{t("Previously trusted")}</span><code>{challenge.previous_fingerprint}</code></div>}
    <div className="fingerprint"><span>{t("Received fingerprint")}</span><code>{challenge.fingerprint}</code></div>
    <p className="notice">{t("Only this endpoint and key will be trusted. SSH host verification remains enabled.")}</p>
    {failure && <p className="error-detail" role="alert">{failure}</p>}
    <div className="dialog-actions"><button disabled={busy} onClick={() => { void decide(false) }}>{t("Cancel connection")}</button><button className="primary" disabled={busy} onClick={() => { void decide(true) }}>{t(changed ? 'Replace saved key and reconnect' : 'Trust key and connect')}</button></div>
  </Modal>
}
