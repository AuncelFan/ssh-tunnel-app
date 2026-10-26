import { expect, test } from 'vitest'
import { bytes, rates } from './traffic'
import type { TunnelRuntime } from './types'
const sample = (id: number, uploaded: number, downloaded: number) => ({ tunnel_id: id, traffic: { uploaded, downloaded } } as TunnelRuntime)
test('traffic rates use real elapsed time and stay isolated by ID', () => {
  expect(rates([sample(1, 100, 200)], [sample(1, 300, 500), sample(2, 999, 999)], 2)).toEqual({ 1: { uploaded: 100, downloaded: 150 }, 2: { uploaded: 0, downloaded: 0 } })
  expect(rates([sample(1, 999, 999)], [sample(1, 0, 0)], 1)[1]).toEqual({ uploaded: 0, downloaded: 0 })
  expect(bytes(1024)).toBe('1.0 KiB')
  expect(bytes(0)).toBe('0 B')
})
test('expiring hourly volume does not affect the live rate', () => {
  const previous = { ...sample(1, 1000, 2000), traffic_last_hour: { uploaded: 900, downloaded: 1800 } }
  const next = { ...sample(1, 1100, 2200), traffic_last_hour: { uploaded: 100, downloaded: 200 } }
  expect(rates([previous], [next], 1)[1]).toEqual({ uploaded: 100, downloaded: 200 })
})
