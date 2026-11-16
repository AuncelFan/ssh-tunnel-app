// Frontend layout coverage with synthetic IPC, separate from native WebView2 acceptance.
import { chromium, expect } from '@playwright/test'
import { preview } from 'vite'
import assert from 'node:assert/strict'
import { mkdir } from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

if (process.platform !== 'win32') throw new Error('Run this layout check on Windows')
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..')
const output = path.join(root, 'target', 'list-layout')
await mkdir(output, { recursive: true })
const server = await preview({ root: path.join(root, 'frontend'), preview: { host: '127.0.0.1', port: 0 } })
let browser
try {
  browser = await chromium.launch({ channel: 'msedge', headless: true })
  for (const locale of ['en', 'zh-CN']) for (const theme of ['dark', 'light']) {
    const context = await browser.newContext()
    try {
      const page = await context.newPage()
      const errors = []
      page.on('pageerror', error => errors.push(String(error)))
      await page.addInitScript(({ locale, theme }) => {
        localStorage.setItem('ssh-tunnel.locale', locale)
        localStorage.setItem('ssh-tunnel.theme', theme)
        const jumpers = [1, 2].map(id => ({ id, name: `Machine ${id}`, host: `machine-${id}.invalid`, port: 22, user: 'fixture', auth_type: 'ssh_agent', key_path: '', password: '', agent_socket_path: '', bypass_host_verification: false, keep_alive_interval_ms: 30000, timeout_ms: 5000, host_key_algorithms: '', notes: '' }))
        const tunnels = ['local', 'remote', 'dynamic'].map((mode, index) => ({ id: index + 1, name: index === 0 ? 'Database / 数据库服务 with a long tunnel name' : mode, group_id: 0, mode, jumper_ids: [1], local_host: '127.0.0.1', local_port: 8080 + index, remote_host: '127.0.0.1', remote_port: 5432 + index, auto_start: false, status: index === 1 ? 'running' : 'stopped', last_error: '', description: '', reconnect_mode: 'fixed', reconnect_interval_seconds: 5, reconnect_max_attempts: 0 }))
        const config = { version: 1, jumpers, tunnels, groups: [{ id: 1, name: 'Services', tunnel_ids: [1, 2, 3] }], auto_run: false, traffic_monitor_enabled: false, trusted_hosts: [] }
        window.__TAURI_INTERNALS__ = {
          transformCallback: () => 1,
          invoke: async command => {
            switch (command) {
              case 'get_state': return structuredClone(config)
              case 'get_ui_locale': return locale
              case 'get_auto_run': return false
              case 'get_pending_trust': case 'take_startup_errors': case 'get_tunnel_logs': return []
              case 'plugin:event|listen': return 1
              case 'plugin:event|unlisten': return
              default: throw new Error(`Unexpected synthetic IPC: ${command}`)
            }
          },
        }
      }, { locale, theme })
      await page.goto(`http://127.0.0.1:${server.httpServer.address().port}`)
      await expect(page.locator('article')).toHaveCount(3)
      const navLabels = locale === 'en' ? ['Tunnels', 'Jumpers', 'Logs', 'Overview', 'Settings'] : ['隧道', '跳板机', '日志', '概览', '设置']
      await expect(page.locator('.page-tabs button')).toHaveText(navLabels)
      for (const width of [1120, 390]) {
        await page.setViewportSize({ width, height: 760 })
        assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true, `${locale}/${theme}/${width}: document overflow`)
        for (const row of await page.locator('article').all()) {
          await expect(row.getByRole('button')).toHaveCount(7)
          await expect(row.getByRole('switch')).toHaveCount(1)
          assert.equal(await row.evaluate(article => {
            const bounds = article.getBoundingClientRect()
            const buttons = [...article.querySelectorAll('button, input')]
            return buttons.every(button => {
              const rect = button.getBoundingClientRect()
              return rect.left >= bounds.left && rect.right <= bounds.right && rect.top >= bounds.top && rect.bottom <= bounds.bottom
            })
          }), true, `${locale}/${theme}/${width}: clipped row action`)
          for (const button of await row.getByRole('button').all()) {
            assert.ok(await button.getAttribute('aria-label'))
            assert.equal(await button.locator('..').getAttribute('title'), await button.getAttribute('aria-label'))
          }
        }
        await page.screenshot({ path: path.join(output, `tunnels-${locale}-${theme}-${width}.png`), fullPage: true })
      }
      const first = page.locator('article').first()
      await first.getByRole('button', { name: locale === 'en' ? 'Duplicate tunnel' : '复制隧道', exact: true }).click()
      const name = page.getByRole('dialog').getByLabel(locale === 'en' ? 'Name' : '名称', { exact: true })
      await expect(name).toHaveValue('Database / 数据库服务 with a long tunnel name')
      await page.getByRole('dialog').getByRole('button', { name: locale === 'en' ? 'Cancel' : '取消', exact: true }).click()
      const clone = locale === 'en' ? 'Clone group' : '克隆整组'
      await page.getByRole('button', { name: clone, exact: true }).click()
      const dialog = page.getByRole('dialog', { name: clone, exact: true })
      await expect(dialog.getByRole('combobox')).toHaveValue('2')
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true, 'Clone dialog overflow')
      await page.screenshot({ path: path.join(output, `clone-${locale}-${theme}-390.png`), fullPage: true })
      await dialog.getByRole('button', { name: locale === 'en' ? 'Cancel' : '取消', exact: true }).click()
      assert.deepEqual(errors, [])
    } finally { await context.close() }
  }
  console.log(`Windows headless layout checks passed: 2 locales × 2 themes × 2 widths; evidence: ${output}`)
} finally {
  await browser?.close()
  await new Promise((resolve, reject) => server.httpServer.close(error => error ? reject(error) : resolve()))
}
