import { expect, test } from 'vitest'
import { filters, selectedFilter } from './groups'
import type { Config, Tunnel } from './types'

const tunnel = (id: number, host: number): Tunnel => ({ id, name: `t${id}`, jumper_ids: [host], group_id: 0 } as Tunnel)
const config = {
  jumpers: [{ id: 1, name: 'same' }, { id: 2, name: 'same' }],
  tunnels: [tunnel(1, 1), tunnel(2, 2)],
  groups: [{ id: 1, name: 'A', tunnel_ids: [1] }, { id: 2, name: 'B', tunnel_ids: [1, 2] }],
} as Config

test('custom membership overlaps without changing derived jumper groups', () => {
  const items = filters(config, true, 'All')
  expect(items.map(i => i.tunnels.map(t => t.id))).toEqual([[1, 2], [1], [2], [1], [1, 2]])
  expect(new Set(items.map(i => i.key)).size).toBe(5)
})
test('removed or hidden selection falls back to All', () => {
  const items = filters(config, false, 'All')
  expect(items).toHaveLength(3)
  expect(selectedFilter(items, 'jumper-1').key).toBe('all')
  expect(selectedFilter(items, 'all').tunnels).toHaveLength(2)
})
