// @vitest-environment jsdom
import React from 'react'
import { afterEach, expect, test } from 'vitest'
import { cleanup, render, screen } from '@testing-library/react'
import { newJumper, newTunnel } from '../defaults'
import { TunnelMapping } from './TunnelMapping'

afterEach(cleanup)
test('prefixes the mapping with the jumper and exposes the full text on hover', () => {
  render(<TunnelMapping tunnel={{ ...newTunnel(), jumper_ids: [1], local_port: 7897, remote_port: 7897 }} jumpers={[{ ...newJumper(), id: 1, name: 'pro6000-1' }]} />)
  const label = 'pro6000-1 / 127.0.0.1:7897 \u2192 127.0.0.1:7897'
  expect(screen.getByText(label).getAttribute('title')).toBe(label)
})
test('keeps hop order, identifies missing jumpers and preserves remote direction', () => {
  render(<TunnelMapping tunnel={{ ...newTunnel(), mode: 'remote', jumper_ids: [2, 1, 3] }} jumpers={[{ ...newJumper(), id: 1, name: 'destination' }, { ...newJumper(), id: 2, name: 'gateway' }]} />)
  expect(screen.getByText('gateway > destination > #3 / 127.0.0.1:8080 \u2190 127.0.0.1:80')).toBeTruthy()
})
test('keeps SOCKS mappings without adding an empty prefix for direct tunnels', () => {
  render(<TunnelMapping tunnel={{ ...newTunnel(), mode: 'dynamic' }} jumpers={[]} />)
  expect(screen.getByText('127.0.0.1:8080 \u2192 SOCKS5')).toBeTruthy()
})
