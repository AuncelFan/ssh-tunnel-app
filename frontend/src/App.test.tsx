// @vitest-environment jsdom
import React from 'react'
import { afterEach, expect, test, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import App from './App'
import { api } from './api'
import { newJumper, newTunnel } from './defaults'

vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }))
vi.mock('./components/Modal', () => ({ Modal: ({ children }: { children: React.ReactNode }) => <div role="dialog">{children}</div> }))
vi.mock('./api', () => ({ api: {
  getState: vi.fn(), pendingTrust: vi.fn(async () => []),
  takeStartupErrors: vi.fn(async () => []), getAutoRun: vi.fn(async () => false),
  runtime: vi.fn(), logs: vi.fn(async () => []),
} }))
afterEach(() => { cleanup(); vi.clearAllMocks(); localStorage.clear() })

test('the actual tunnel list renders the jumper before the remote port mapping', async () => {
  vi.mocked(api.getState).mockResolvedValue({
    version: 1, auto_run: false, traffic_monitor_enabled: false, trusted_hosts: [], groups: [],
    jumpers: [{ ...newJumper(), id: 1, name: 'pro6000-1' }],
    tunnels: [{ ...newTunnel(), id: 1, name: 'Remote fixture', mode: 'remote', jumper_ids: [1], local_port: 7897, remote_port: 7897 }],
  })
  render(<App />)
  const row = await screen.findByRole('article')
  const label = 'pro6000-1 / 127.0.0.1:7897 \u2190 127.0.0.1:7897'
  expect(within(row).getByText(label).getAttribute('title')).toBe(label)
})

test('overview and tunnel details show rolling-hour volume instead of lifetime volume', async () => {
  vi.mocked(api.getState).mockResolvedValue({
    version: 1, auto_run: false, traffic_monitor_enabled: true, trusted_hosts: [], groups: [], jumpers: [],
    tunnels: [{ ...newTunnel(), id: 1, name: 'Traffic fixture' }],
  })
  vi.mocked(api.runtime).mockResolvedValue([
    { tunnel_id: 1, generation: 1, status: 'stopped', last_error: '', retry_attempt: 0,
      traffic: { uploaded: 999999, downloaded: 888888 }, traffic_last_hour: { uploaded: 123, downloaded: 456 } },
    { tunnel_id: 2, generation: 1, status: 'stopped', last_error: '', retry_attempt: 0,
      traffic: { uploaded: 999999, downloaded: 888888 }, traffic_last_hour: { uploaded: 10, downloaded: 20 } },
  ])
  render(<App />)
  fireEvent.click(await screen.findByRole('button', { name: 'Details: Traffic fixture' }))
  const dialog = screen.getByRole('dialog')
  expect(within(dialog).getByText('Last hour: 123 B')).toBeTruthy()
  expect(within(dialog).getByText('Last hour: 456 B')).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: 'Overview' }))
  expect(screen.getByText('Last hour: 133 B')).toBeTruthy()
  expect(screen.getByText('Last hour: 476 B')).toBeTruthy()
})
