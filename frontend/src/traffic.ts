import type { TunnelRuntime } from './types'

export interface Rates { uploaded: number; downloaded: number }
export function rates(previous: TunnelRuntime[], next: TunnelRuntime[], seconds: number): Record<number, Rates> {
  const result: Record<number, Rates> = {}
  for (const current of next) {
    const old = previous.find(item => item.tunnel_id === current.tunnel_id)
    result[current.tunnel_id] = { uploaded: old && seconds > 0 ? Math.max(0, current.traffic.uploaded - old.traffic.uploaded) / seconds : 0, downloaded: old && seconds > 0 ? Math.max(0, current.traffic.downloaded - old.traffic.downloaded) / seconds : 0 }
  }
  return result
}
export function bytes(value: number): string {
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB']
  let unit = 0
  while (value >= 1024 && unit < units.length - 1) { value /= 1024; unit++ }
  return `${value.toFixed(unit ? 1 : 0)} ${units[unit]}`
}
