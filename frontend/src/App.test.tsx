// @vitest-environment jsdom
import React from 'react'
import { afterEach, expect, test, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import App from './App'
import { api } from './api'
import { newJumper, newTunnel } from './defaults'
import type { Config } from './types'

vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }))
vi.mock('./components/Modal', () => ({ Modal: ({ children }: { children: React.ReactNode }) => <div role="dialog">{children}</div> }))
vi.mock('./api', () => ({ api: {
  getState: vi.fn(), pendingTrust: vi.fn(async () => []),
  takeStartupErrors: vi.fn(async () => []), getAutoRun: vi.fn(async () => false),
  runtime: vi.fn(), logs: vi.fn(async () => []),
  saveTunnel: vi.fn(async () => 8), deleteTunnel: vi.fn(async () => {}),
  setTunnelAutoStart: vi.fn(async () => {}), cloneGroup: vi.fn(), deleteSelection: vi.fn(async () => {}),
  startTunnel: vi.fn(async () => {}), stopTunnel: vi.fn(async () => {}),
  beginProbe: vi.fn(async () => 42), runProbe: vi.fn(async () => ({ success: true, elapsed_ms: 1, latency_ms: null, checks: [], challenge: null })),
  cancelProbe: vi.fn(async () => {}),
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

function setup(status = 'stopped') {
  const config: Config = {
    version: 1, auto_run: false, traffic_monitor_enabled: false, trusted_hosts: [], groups: [],
    jumpers: [{ ...newJumper(), id: 1, name: 'Machine A' }, { ...newJumper(), id: 2, name: 'Machine B' }],
    tunnels: [{ ...newTunnel(), id: 7, name: 'Database', jumper_ids: [1], status }],
  }
  vi.mocked(api.getState).mockResolvedValue(config)
  render(<App />)
  return config
}

test('navigation puts overview immediately before settings and opens tunnels first', async () => {
  setup()
  await screen.findByRole('article')
  const buttons = within(screen.getByRole('navigation', { name: 'Main navigation' })).getAllByRole('button')
  expect(buttons.map(button => button.textContent)).toEqual(['Tunnels', 'Jumpers', 'Logs', 'Overview', 'Settings'])
  expect(buttons[0].getAttribute('aria-current')).toBe('page')
})

test('all tunnel operations are accessible icons in the list and details contain no action controls', async () => {
  setup()
  const row = await screen.findByRole('article')
  for (const label of ['Start', 'Test connection', 'Edit tunnel', 'Duplicate tunnel', 'Delete tunnel', 'Details: Database']) {
    const button = within(row).getByRole('button', { name: label })
    expect(button.querySelector('svg')).toBeTruthy()
    expect(button.textContent).toBe('')
    expect(button.parentElement?.title).toBe(label)
  }
  expect(within(row).getByRole('switch', { name: 'Start automatically: Database' })).toBeTruthy()
  fireEvent.click(within(row).getByRole('button', { name: 'Test connection' }))
  await screen.findByText('Connection test passed')
  expect(api.beginProbe).toHaveBeenCalledWith(expect.objectContaining({ diagnostics: true, tunnel: expect.objectContaining({ id: 7 }) }))
  fireEvent.click(screen.getByRole('button', { name: 'Close test' }))
  fireEvent.click(within(row).getByRole('button', { name: 'Details: Database' }))
  expect(within(screen.getByRole('dialog')).queryByRole('button')).toBeNull()
})

test('duplicating a tunnel opens an independent draft with its original name', async () => {
  const config = setup()
  fireEvent.click(await screen.findByRole('button', { name: 'Duplicate tunnel' }))
  const dialog = screen.getByRole('dialog')
  const name = within(dialog).getByLabelText('Name') as HTMLInputElement
  expect(name.value).toBe('Database')
  fireEvent.change(name, { target: { value: 'Database clone' } })
  expect(config.tunnels[0].name).toBe('Database')
  fireEvent.click(within(dialog).getByRole('button', { name: 'Save tunnel' }))
  await waitFor(() => expect(api.saveTunnel).toHaveBeenCalledWith(expect.objectContaining({ id: 0, name: 'Database clone', jumper_ids: [1], status: 'stopped', last_error: '' })))
})

test('running tunnels keep stop, copy, delete and auto-start available while edit and probes are disabled', async () => {
  const config = setup('running')
  const row = await screen.findByRole('article')
  for (const label of ['Edit tunnel', 'Test connection']) {
    expect((within(row).getByRole('button', { name: label }) as HTMLButtonElement).disabled).toBe(true)
  }
  for (const label of ['Stop', 'Duplicate tunnel', 'Delete tunnel']) {
    expect((within(row).getByRole('button', { name: label }) as HTMLButtonElement).disabled).toBe(false)
  }
  fireEvent.click(within(row).getByRole('switch', { name: 'Start automatically: Database' }))
  await waitFor(() => expect(api.setTunnelAutoStart).toHaveBeenCalledWith(7, true))
  expect(api.saveTunnel).not.toHaveBeenCalled()
  expect(api.stopTunnel).not.toHaveBeenCalled()
  expect(config.tunnels[0].status).toBe('running')
})

test('deleting from the list still requires confirmation', async () => {
  setup()
  fireEvent.click(await screen.findByRole('button', { name: 'Delete tunnel' }))
  expect(api.deleteTunnel).not.toHaveBeenCalled()
  fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Delete' }))
  await waitFor(() => expect(api.deleteTunnel).toHaveBeenCalledWith(7))
})

test('a failed auto-start write leaves the switch unchanged and reports the error', async () => {
  vi.mocked(api.setTunnelAutoStart).mockRejectedValueOnce(new Error('Cannot save preference'))
  setup('running')
  const row = await screen.findByRole('article')
  const toggle = within(row).getByRole('switch', { name: 'Start automatically: Database' }) as HTMLInputElement
  fireEvent.click(toggle)
  await screen.findByRole('alert')
  expect(toggle.checked).toBe(false)
  await waitFor(() => expect(toggle.disabled).toBe(false))
  expect(api.stopTunnel).not.toHaveBeenCalled()
})

test('cloning the selected group sends its complete membership and selects the target machine', async () => {
  const config = setup()
  vi.mocked(api.cloneGroup).mockImplementation(async () => {
    vi.mocked(api.getState).mockResolvedValue({ ...config, tunnels: [...config.tunnels, { ...config.tunnels[0], id: 8, jumper_ids: [2] }] })
    return [8]
  })
  fireEvent.click(await screen.findByRole('button', { name: 'Clone group' }))
  const dialog = screen.getByRole('dialog')
  fireEvent.click(within(dialog).getByRole('button', { name: 'Clone group' }))
  await waitFor(() => expect(api.cloneGroup).toHaveBeenCalledWith({ tunnel_ids: [7], target_jumper_id: 2 }))
  await screen.findByRole('heading', { name: 'Machine B' })
  expect(screen.queryByRole('dialog')).toBeNull()
  expect(api.startTunnel).not.toHaveBeenCalled()
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

test('selected tunnels may be cloned or deleted independently of the group', async () => {
  const config = setup()
  config.tunnels.push({ ...config.tunnels[0], id: 9, name: 'Cache' }, { ...config.tunnels[0], id: 10, name: 'SSH' })
  for (const name of ['Database', 'SSH']) fireEvent.click(await screen.findByRole('checkbox', { name: `Select tunnel: ${name}`}))
  fireEvent.click(screen.getByRole('button', { name: 'Clone selected' }))
  expect(within(screen.getByRole('dialog')).queryByText('Cache')).toBeNull()
  fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Cancel' }))
  fireEvent.click(screen.getByRole('button', { name: 'Delete selected' }))
  expect(api.deleteSelection).not.toHaveBeenCalled()
  fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Delete' }))
  await waitFor(() => expect(api.deleteSelection).toHaveBeenCalledWith([7,10], null))
})

test('jumper deletion previews a later-hop dependency and sends the reviewed IDs', async () => {
  const config = setup()
  config.tunnels[0].jumper_ids = [1,2]
  fireEvent.click(await screen.findByRole('button', { name: 'Jumpers' }))
  fireEvent.click(within(screen.getByText('Machine B').closest('article')!).getByRole('button', { name: 'Delete' }))
  const dialog = screen.getByRole('dialog')
  expect(within(dialog).getByText(/Database \(#7\)/)).toBeTruthy()
  fireEvent.click(within(dialog).getByRole('button', { name: 'Delete' }))
  await waitFor(() => expect(api.deleteSelection).toHaveBeenCalledWith([7],2))
})
