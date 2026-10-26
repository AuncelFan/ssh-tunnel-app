import type { Jumper, Tunnel } from '../types'

export function TunnelMapping({ tunnel, jumpers }: { tunnel: Tunnel; jumpers: Jumper[] }) {
  const chain = tunnel.jumper_ids.map(id => jumpers.find(jumper => jumper.id === id)?.name || `#${id}`).join(' > ')
  const mapping = `${tunnel.local_host}:${tunnel.local_port} ${tunnel.mode === 'remote' ? '\u2190' : '\u2192'} ${tunnel.mode === 'dynamic' ? 'SOCKS5' : `${tunnel.remote_host}:${tunnel.remote_port}`}`
  const label = chain ? `${chain} / ${mapping}` : mapping
  return <code title={label}>{label}</code>
}
