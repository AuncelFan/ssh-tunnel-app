import { useEffect, useState } from 'react'
import { api } from '../api'
import { logLevel, logMessage, logTime } from '../journal'
import { useLocale } from '../locale'
import type { RuntimeEvent } from '../types'

export function LogsPage() {
  const { t, locale } = useLocale()
  const [entries, setEntries] = useState<RuntimeEvent[]>([])
  const [level, setLevel] = useState('all')
  const [error, setError] = useState('')
  useEffect(() => {
    let disposed = false
    let sequence = 0
    const update = async () => {
      const current = ++sequence
      try { const next = await api.activityLogs(); if (!disposed && current === sequence) { setEntries(next); setError('') } }
      catch (error) { if (!disposed && current === sequence) setError(String(error)) }
    }
    void update()
    const timer = setInterval(() => { void update() }, 1000)
    return () => { disposed = true; clearInterval(timer) }
  }, [])
  const shown = entries.filter(entry => level === 'all' || logLevel(entry) === level)
  return <div className="page-content"><div className="list-heading"><h1>{t('Logs')}</h1><div className="actions" aria-label={t('Log level')}>{['all', 'info', 'warn', 'error'].map(value => <button key={value} aria-pressed={level === value} onClick={() => setLevel(value)}>{t({ all: 'All', info: 'Info', warn: 'Warnings', error: 'Errors' }[value]!)}</button>)}</div></div>
    <p className="notice">{t('The latest 500 events are kept in memory. Switching configuration clears this history.')}</p>
    {error && <p role="alert" className="error-detail">{error}</p>}
    <section className="panel" role="log" aria-live="off" aria-label={t('Activity log')}>{shown.map((entry, index) => <div className="journal-entry" key={`${entry.timestamp_ms}-${index}`}>
      <div className="journal-meta"><time>{logTime(entry, locale)}</time><span className={logLevel(entry) === 'error' ? 'error-detail' : ''}>{t({ info: 'Info', warn: 'Warnings', error: 'Errors' }[logLevel(entry)])}</span><span>{entry.tunnel_id ? `${t('Tunnel')} #${entry.tunnel_id}` : t('Application')}</span></div>
      <p>{logMessage(entry, t)}</p>
    </div>)}{!shown.length && <p>{t('No log entries.')}</p>}</section>
  </div>
}
