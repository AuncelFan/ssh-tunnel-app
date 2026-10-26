import { useState } from 'react'
import { Modal } from './Modal'
import { useLocale } from '../locale'

export function ConfirmDialog({ title, message, confirm, close, confirmLabel, destructive = true }: { title: string; message: string; confirm: () => Promise<void>; close: () => void; confirmLabel?: string; destructive?: boolean }) {
  const { t } = useLocale()
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  return <Modal title={title} closeLabel={t("Cancel")} onClose={() => { if (!busy) close() }}>
    <p>{message}</p>{error && <p className="error-detail" role="alert">{error}</p>}
    <div className="dialog-actions"><button disabled={busy} onClick={close}>{t("Cancel")}</button><button className={destructive ? 'danger' : 'primary'} disabled={busy} onClick={async () => {
      setBusy(true); try { await confirm(); close() } catch (error) { setError(String(error)) } finally { setBusy(false) }
    }}>{confirmLabel ?? t("Delete")}</button></div>
  </Modal>
}
