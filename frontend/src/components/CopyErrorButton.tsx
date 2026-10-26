import { useState } from 'react'
import { copyText } from '../journal'
import { useLocale } from '../locale'

export function CopyErrorButton({ text }: { text: string }) {
  const { t } = useLocale()
  const [copied, setCopied] = useState('')
  const [error, setError] = useState(false)
  const [busy, setBusy] = useState(false)
  return <div className="actions"><button type="button" disabled={busy} onClick={() => {
    setBusy(true); setError(false)
    void copyText(text).then(() => setCopied(text)).catch(() => setError(true)).finally(() => setBusy(false))
  }}>{t(copied === text ? 'Copied' : 'Copy error')}</button>{error && <span role="alert">{t('Clipboard unavailable')}</span>}</div>
}
